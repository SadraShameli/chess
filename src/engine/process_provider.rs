use std::future::Future;
use std::process::{ExitStatus, Stdio};
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
    #[error("The engine exited unsuccessfully: {0}")]
    Exit(ExitStatus),
    #[error("Engine shutdown failed ({operation}); pipe cleanup also failed ({cleanup})")]
    Cleanup {
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
        let readers = [
            EngineProcessReader::spawn(stdout, EngineProcessEnumPipe::Stdout, sender.clone()),
            EngineProcessReader::spawn(stderr, EngineProcessEnumPipe::Stderr, sender),
        ];
        Ok(EngineNativeProcess {
            child,
            input,
            lines,
            readers,
        })
    }
}

pub struct EngineNativeProcess {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Result<EngineProcessLine, EngineProcessError>>,
    readers: [JoinHandle<()>; 2],
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
        let now = Instant::now();
        let deadline = now + EngineProcessLimits::SHUTDOWN;
        self.lines.close();
        let operation = self
            .shutdown_process(now + EngineProcessLimits::GRACEFUL_SHUTDOWN, deadline)
            .await;
        let cleanup = self.join_readers(deadline).await;
        EngineProcessError::combine(operation, cleanup)
    }

    async fn terminate(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.lines.close();
        let operation = async {
            self.child.start_kill()?;
            timeout_at(deadline, self.child.wait())
                .await
                .map_err(|_elapsed| EngineProcessError::ShutdownTimeout)??;
            Ok(())
        }
        .await;
        let cleanup = self.join_readers(deadline).await;
        EngineProcessError::combine(operation, cleanup)
    }
}

impl EngineNativeProcess {
    async fn shutdown_process(
        &mut self,
        graceful: Instant,
        deadline: Instant,
    ) -> Result<(), EngineProcessError> {
        let quit = timeout_at(graceful, self.send("quit")).await;
        let status = match timeout_at(graceful, self.child.wait()).await {
            Ok(status) => status?,
            Err(_elapsed) => {
                self.child.start_kill()?;
                timeout_at(deadline, self.child.wait())
                    .await
                    .map_err(|_elapsed| EngineProcessError::ShutdownTimeout)??;
                return Err(EngineProcessError::ShutdownTimeout);
            }
        };
        if !status.success() {
            return Err(EngineProcessError::Exit(status));
        }
        match quit {
            Ok(result) => result,
            Err(_elapsed) => Err(EngineProcessError::ShutdownTimeout),
        }
    }

    async fn join_readers(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        let mut failed = None;
        for reader in &mut self.readers {
            match timeout_at(deadline, &mut *reader).await {
                Ok(result) => {
                    if let Err(error) = result {
                        failed = Some(EngineProcessError::ReaderTask(error));
                    }
                }
                Err(_elapsed) => {
                    reader.abort();
                    if let Err(error) = reader.await
                        && !error.is_cancelled()
                    {
                        failed = Some(EngineProcessError::ReaderTask(error));
                    }
                    if failed.is_none() {
                        failed = Some(EngineProcessError::ShutdownTimeout);
                    }
                }
            }
        }
        match failed {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for EngineNativeProcess {
    fn drop(&mut self) {
        for reader in &self.readers {
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
    use crate::engine::process_provider::{
        EngineProcessError, EngineProcessLimits, EngineProcessReader,
    };
    use tokio::io::BufReader;

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
