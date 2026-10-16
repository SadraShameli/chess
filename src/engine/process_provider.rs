use std::future::Future;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

use crate::engine::package::selector::EnginePackageSelected;

#[derive(Debug, Error)]
pub enum EngineProcessError {
    #[error("Engine process I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("An engine pipe was not created")]
    MissingPipe,
    #[error("The engine closed its {0:?} pipe unexpectedly")]
    Closed(EngineProcessEnumPipe),
    #[error("An engine line exceeds the 64 KiB limit")]
    OversizedLine,
    #[error("The engine sent invalid UTF-8: {0}")]
    InvalidText(#[from] std::string::FromUtf8Error),
    #[error("The engine command contains a line break or exceeds its bound")]
    InvalidCommand,
    #[error("The engine pipe task failed: {0}")]
    ReaderTask(#[from] tokio::task::JoinError),
    #[error("The engine did not shut down within its deadline")]
    ShutdownTimeout,
    #[error("Engine shutdown exceeded its absolute deadline: {0}")]
    ShutdownDeadline(#[source] tokio::time::error::Elapsed),
    #[error("Engine shutdown previously failed: {0}")]
    ShutdownFailed(#[source] Arc<Self>),
    #[error("The engine exited unsuccessfully: {0}")]
    Exit(ExitStatus),
    #[error("Engine shutdown failed ({operation}); pipe cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
}

impl EngineProcessError {
    fn combine(operation: Result<(), Self>, cleanup: Result<(), Self>) -> Result<(), Self> {
        match (operation, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(operation), Err(cleanup)) => Err(Self::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineProcessEnumPipe {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub struct EngineProcessLine {
    pipe: EngineProcessEnumPipe,
    text: String,
}

impl EngineProcessLine {
    pub fn new(pipe: EngineProcessEnumPipe, text: String) -> Result<Self, EngineProcessError> {
        if text.len() > EngineProcessLimits::LINE_BYTES {
            return Err(EngineProcessError::OversizedLine);
        }
        if text.contains(['\n', '\r']) {
            return Err(EngineProcessError::InvalidCommand);
        }
        Ok(Self { pipe, text })
    }

    pub fn pipe(&self) -> EngineProcessEnumPipe {
        self.pipe
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

pub struct EngineProcessLimits;

impl EngineProcessLimits {
    pub const LINE_BYTES: usize = 64 * 1024;
    pub const PENDING_LINES: usize = 256;
    pub const SHUTDOWN: Duration = Duration::from_secs(3);
    const GRACEFUL_SHUTDOWN: Duration = Duration::from_secs(2);
}

pub trait EngineProcess: Send {
    fn send(
        &mut self,
        command: &str,
    ) -> impl Future<Output = Result<(), EngineProcessError>> + Send;
    fn receive(
        &mut self,
    ) -> impl Future<Output = Result<EngineProcessLine, EngineProcessError>> + Send;
    fn shutdown(&mut self) -> impl Future<Output = Result<(), EngineProcessError>> + Send;
    fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> impl Future<Output = Result<(), EngineProcessError>> + Send;
    fn is_shutdown_complete(&self) -> bool;
    fn terminate(
        &mut self,
        deadline: Instant,
    ) -> impl Future<Output = Result<(), EngineProcessError>> + Send;
}

pub trait EngineProcessProvider {
    type Process: EngineProcess;

    fn spawn(&self, package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError>;
}

pub struct EngineNativeProcessProvider;

impl EngineProcessProvider for EngineNativeProcessProvider {
    type Process = EngineNativeProcess;

    fn spawn(&self, package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        let mut child = Command::new(package.executable())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let input = child.stdin.take().ok_or(EngineProcessError::MissingPipe)?;
        let stdout = child.stdout.take().ok_or(EngineProcessError::MissingPipe)?;
        let stderr = child.stderr.take().ok_or(EngineProcessError::MissingPipe)?;
        let (sender, lines) = mpsc::channel(EngineProcessLimits::PENDING_LINES);
        let readers = EngineProcessReaders::new([
            EngineProcessReader::spawn(stdout, EngineProcessEnumPipe::Stdout, sender.clone()),
            EngineProcessReader::spawn(stderr, EngineProcessEnumPipe::Stderr, sender),
        ]);
        Ok(EngineNativeProcess {
            child,
            input,
            lines,
            readers,
            shutdown: EngineProcessEnumShutdown::Running,
            status: None,
            quit_failure: None,
        })
    }
}

pub struct EngineNativeProcess {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Result<EngineProcessLine, EngineProcessError>>,
    readers: EngineProcessReaders,
    shutdown: EngineProcessEnumShutdown,
    status: Option<ExitStatus>,
    quit_failure: Option<Arc<EngineProcessError>>,
}

#[derive(Clone, Copy)]
enum EngineProcessEnumShutdown {
    Running,
    SendingQuit,
    Waiting { graceful: Instant },
    Forced,
    Reaped,
}

impl EngineProcess for EngineNativeProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        if command.len() > EngineProcessLimits::LINE_BYTES || command.contains(['\r', '\n']) {
            return Err(EngineProcessError::InvalidCommand);
        }
        self.input.write_all(command.as_bytes()).await?;
        self.input.write_all(b"\n").await?;
        self.input.flush().await?;
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        self.lines
            .recv()
            .await
            .ok_or(EngineProcessError::Closed(EngineProcessEnumPipe::Stdout))?
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await
    }

    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.lines.close();
        let operation = self.shutdown_process(deadline).await;
        let cleanup = self.readers.join(deadline).await;
        EngineProcessError::combine(operation, cleanup)
    }

    fn is_shutdown_complete(&self) -> bool {
        self.status.is_some() && self.readers.is_complete()
    }

    async fn terminate(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.lines.close();
        if self.status.is_none() {
            self.shutdown = EngineProcessEnumShutdown::Forced;
        }
        let operation = self.shutdown_process(deadline).await;
        let cleanup = self.readers.join(deadline).await;
        EngineProcessError::combine(operation, cleanup)
    }
}

impl EngineNativeProcess {
    async fn shutdown_process(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        match self.shutdown {
            EngineProcessEnumShutdown::Running => {
                let graceful =
                    deadline.min(Instant::now() + EngineProcessLimits::GRACEFUL_SHUTDOWN);
                self.shutdown = EngineProcessEnumShutdown::SendingQuit;
                match timeout_at(graceful, self.send("quit")).await {
                    Ok(Ok(())) => self.shutdown = EngineProcessEnumShutdown::Waiting { graceful },
                    Ok(Err(error)) => {
                        self.quit_failure = Some(Arc::new(error));
                        self.shutdown = EngineProcessEnumShutdown::Forced;
                    }
                    Err(error) => {
                        self.quit_failure =
                            Some(Arc::new(EngineProcessError::ShutdownDeadline(error)));
                        self.shutdown = EngineProcessEnumShutdown::Forced;
                    }
                }
            }
            EngineProcessEnumShutdown::SendingQuit => {
                self.shutdown = EngineProcessEnumShutdown::Forced;
            }
            EngineProcessEnumShutdown::Waiting { .. }
            | EngineProcessEnumShutdown::Forced
            | EngineProcessEnumShutdown::Reaped => {}
        }
        if let EngineProcessEnumShutdown::Waiting { graceful } = self.shutdown {
            match timeout_at(deadline.min(graceful), self.child.wait()).await {
                Ok(status) => self.reaped(status?),
                Err(error) => {
                    self.quit_failure = Some(Arc::new(EngineProcessError::ShutdownDeadline(error)));
                    self.shutdown = EngineProcessEnumShutdown::Forced;
                }
            }
        }
        if matches!(self.shutdown, EngineProcessEnumShutdown::Forced) {
            match self.child.try_wait()? {
                Some(status) => self.reaped(status),
                None => {
                    self.child.start_kill()?;
                    let status = timeout_at(deadline, self.child.wait())
                        .await
                        .map_err(EngineProcessError::ShutdownDeadline)??;
                    self.reaped(status);
                }
            }
        }
        match &self.quit_failure {
            Some(error) => Err(EngineProcessError::ShutdownFailed(Arc::clone(error))),
            None => Ok(()),
        }
    }

    fn reaped(&mut self, status: ExitStatus) {
        if !status.success()
            && !matches!(self.shutdown, EngineProcessEnumShutdown::Forced)
            && self.quit_failure.is_none()
        {
            self.quit_failure = Some(Arc::new(EngineProcessError::Exit(status)));
        }
        self.status = Some(status);
        self.shutdown = EngineProcessEnumShutdown::Reaped;
    }
}

struct EngineProcessReaders {
    workers: [Option<JoinHandle<()>>; 2],
    failures: [Option<Arc<EngineProcessError>>; 2],
    aborted: [bool; 2],
}

impl EngineProcessReaders {
    fn new(workers: [JoinHandle<()>; 2]) -> Self {
        Self {
            workers: workers.map(Some),
            failures: [None, None],
            aborted: [false, false],
        }
    }

    fn is_complete(&self) -> bool {
        self.workers.iter().all(Option::is_none)
    }

    async fn join(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        let mut outcome = Ok(());
        for (index, worker) in self.workers.iter_mut().enumerate() {
            if let Some(reader) = worker {
                match timeout_at(deadline, &mut *reader).await {
                    Ok(result) => {
                        *worker = None;
                        if let Err(error) = result
                            && !(self.aborted[index] && error.is_cancelled())
                        {
                            self.failures[index] =
                                Some(Arc::new(EngineProcessError::ReaderTask(error)));
                        }
                    }
                    Err(error) => {
                        reader.abort();
                        self.aborted[index] = true;
                        outcome = EngineProcessError::combine(
                            outcome,
                            Err(EngineProcessError::ShutdownDeadline(error)),
                        );
                    }
                }
            }
        }
        for error in self.failures.iter().flatten() {
            outcome = EngineProcessError::combine(
                outcome,
                Err(EngineProcessError::ShutdownFailed(Arc::clone(error))),
            );
        }
        outcome
    }
}

impl Drop for EngineProcessReaders {
    fn drop(&mut self) {
        for reader in self.workers.iter().flatten() {
            reader.abort();
        }
    }
}

struct EngineProcessReader;

impl EngineProcessReader {
    fn spawn(
        input: impl tokio::io::AsyncRead + Unpin + Send + 'static,
        pipe: EngineProcessEnumPipe,
        sender: mpsc::Sender<Result<EngineProcessLine, EngineProcessError>>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut input = BufReader::new(input);
            loop {
                let line = Self::read_line(&mut input).await.and_then(|text| {
                    text.ok_or(EngineProcessError::Closed(pipe))
                        .and_then(|text| EngineProcessLine::new(pipe, text))
                });
                let failed = line.is_err();
                if sender.send(line).await.is_err() || failed {
                    return;
                }
            }
        })
    }

    async fn read_line(
        input: &mut (impl AsyncBufRead + Unpin),
    ) -> Result<Option<String>, EngineProcessError> {
        let mut bytes = Vec::new();
        loop {
            let buffer = input.fill_buf().await?;
            if buffer.is_empty() {
                return if bytes.is_empty() {
                    Ok(None)
                } else {
                    Err(EngineProcessError::Closed(EngineProcessEnumPipe::Stdout))
                };
            }
            let newline = buffer.iter().position(|byte| *byte == b'\n');
            let count = newline.map_or(buffer.len(), |index| index + 1);
            if bytes.len() + count > EngineProcessLimits::LINE_BYTES {
                return Err(EngineProcessError::OversizedLine);
            }
            bytes.extend_from_slice(&buffer[..count]);
            input.consume(count);
            if newline.is_some() {
                bytes.pop();
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
                return Ok(Some(String::from_utf8(bytes)?));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context, Waker};
    use std::time::Duration;

    use crate::engine::process_provider::{
        EngineProcessError, EngineProcessLimits, EngineProcessReader, EngineProcessReaders,
    };
    use tokio::io::BufReader;
    use tokio::sync::oneshot;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn cancelled_reader_join_retains_only_unobserved_handles()
    -> Result<(), Box<dyn std::error::Error>> {
        let (finish, completion) = oneshot::channel();
        let mut readers = EngineProcessReaders::new([
            tokio::spawn(async {}),
            tokio::spawn(async move {
                assert!(completion.await.is_ok());
            }),
        ]);
        tokio::task::yield_now().await;
        let deadline = Instant::now() + Duration::from_secs(3);
        {
            let joining = readers.join(deadline);
            let mut joining = std::pin::pin!(joining);
            assert!(
                joining
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
        }
        assert!(readers.workers[0].is_none());
        assert!(readers.workers[1].is_some());
        assert!(!readers.is_complete());
        finish
            .send(())
            .map_err(|()| "The retained reader was dropped")?;
        readers.join(deadline).await?;
        assert!(readers.is_complete());
        readers.join(deadline).await?;
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn reader_deadline_retains_aborted_handles_until_join_is_observed()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut readers = EngineProcessReaders::new([
            tokio::spawn(std::future::pending()),
            tokio::spawn(std::future::pending()),
        ]);
        assert!(
            readers
                .join(Instant::now() + Duration::from_secs(1))
                .await
                .is_err()
        );
        assert!(!readers.is_complete());
        readers
            .join(Instant::now() + Duration::from_secs(3))
            .await?;
        assert!(readers.is_complete());
        readers.join(Instant::now()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn pipe_reader_bounds_unterminated_output_and_validates_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let oversized = vec![b'x'; EngineProcessLimits::LINE_BYTES + 1];
        assert!(matches!(
            EngineProcessReader::read_line(&mut BufReader::new(&oversized[..])).await,
            Err(EngineProcessError::OversizedLine)
        ));
        assert!(matches!(
            EngineProcessReader::read_line(&mut BufReader::new(&b"unterminated"[..])).await,
            Err(EngineProcessError::Closed(_))
        ));
        assert!(matches!(
            EngineProcessReader::read_line(&mut BufReader::new(&b"\xff\n"[..])).await,
            Err(EngineProcessError::InvalidText(_))
        ));
        let mut reader = BufReader::new(&b"first\r\nsecond\n"[..]);
        assert_eq!(
            EngineProcessReader::read_line(&mut reader)
                .await?
                .as_deref(),
            Some("first")
        );
        assert_eq!(
            EngineProcessReader::read_line(&mut reader)
                .await?
                .as_deref(),
            Some("second")
        );
        assert_eq!(EngineProcessReader::read_line(&mut reader).await?, None);
        Ok(())
    }
}
