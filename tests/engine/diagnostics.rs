use std::collections::VecDeque;
use std::error::Error;
use std::future::Future;
use std::task::{Context, Waker};
use strum::IntoEnumIterator;

use chess::board::position::BoardPosition;
use chess::clock::duration::ClockDuration;
use chess::engine::capabilities::EngineIdentity;
use chess::engine::diagnostics::EngineDiagnostics;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLimits,
    EngineProcessLine, EngineProcessProvider,
};
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::enums::EngineSearchEnumState;
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::stop::EngineSearchStop;
use chess::engine::session::{EngineSession, EngineSessionError, EngineSessionStartup};
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_package_selection::EnginePackageFixture;

#[derive(Clone, Copy)]
enum EngineDiagnosticsTestEnumHandshake {
    Ready,
    Invalid,
    Pending,
}

struct EngineDiagnosticsTestProvider {
    handshake: EngineDiagnosticsTestEnumHandshake,
    cleanup_failure: bool,
    cleanup_pending: bool,
    search_pending: bool,
}

impl EngineDiagnosticsTestProvider {
    fn ready() -> Self {
        Self {
            handshake: EngineDiagnosticsTestEnumHandshake::Ready,
            cleanup_failure: false,
            cleanup_pending: false,
            search_pending: false,
        }
    }
}

impl EngineProcessProvider for EngineDiagnosticsTestProvider {
    type Process = EngineDiagnosticsTestProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        Ok(EngineDiagnosticsTestProcess {
            handshake: self.handshake,
            lines: VecDeque::new(),
            complete: false,
            cleanup_failure: self.cleanup_failure,
            cleanup_pending: self.cleanup_pending,
            search_pending: self.search_pending,
        })
    }
}

struct EngineDiagnosticsTestProcess {
    handshake: EngineDiagnosticsTestEnumHandshake,
    lines: VecDeque<EngineProcessLine>,
    complete: bool,
    cleanup_failure: bool,
    cleanup_pending: bool,
    search_pending: bool,
}

impl EngineDiagnosticsTestProcess {
    fn emit(&mut self, pipe: EngineProcessEnumPipe, text: &str) -> Result<(), EngineProcessError> {
        self.lines
            .push_back(EngineProcessLine::new(pipe, text.to_owned())?);
        Ok(())
    }

    async fn finish(&mut self) -> Result<(), EngineProcessError> {
        if std::mem::take(&mut self.cleanup_pending) {
            std::future::pending::<()>().await;
        }
        self.complete = true;
        if self.cleanup_failure {
            Err(std::io::Error::other("controlled diagnostic cleanup failure").into())
        } else {
            Ok(())
        }
    }
}

impl EngineProcess for EngineDiagnosticsTestProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        match command {
            "uci" => {
                self.emit(EngineProcessEnumPipe::Stderr, "startup context λ")?;
                match self.handshake {
                    EngineDiagnosticsTestEnumHandshake::Pending => {}
                    EngineDiagnosticsTestEnumHandshake::Invalid => {
                        self.emit(EngineProcessEnumPipe::Stdout, "invalid handshake")?;
                    }
                    EngineDiagnosticsTestEnumHandshake::Ready => {
                        self.emit(EngineProcessEnumPipe::Stdout, "id name Reckless 0.9.0")?;
                        self.emit(
                            EngineProcessEnumPipe::Stdout,
                            &format!("id author {}", EngineIdentity::AUTHOR),
                        )?;
                        for option in EngineEnumOption::iter() {
                            self.emit(
                                EngineProcessEnumPipe::Stdout,
                                &format!(
                                    "option name {} type {}",
                                    option.name(),
                                    option.descriptor()
                                ),
                            )?;
                        }
                        self.emit(EngineProcessEnumPipe::Stdout, "uciok")?;
                    }
                }
            }
            "isready" => {
                self.emit(EngineProcessEnumPipe::Stderr, "readiness context")?;
                self.emit(EngineProcessEnumPipe::Stdout, "readyok")?;
            }
            command if command.starts_with("setoption ") => {
                self.emit(EngineProcessEnumPipe::Stderr, "configuration context")?;
                for (name, value, suffix) in [
                    ("Threads", 1, ""),
                    ("Hash", 16, " MB"),
                    ("MoveOverhead", 0, " ms"),
                ] {
                    if command == format!("setoption name {name} value {value}") {
                        self.emit(
                            EngineProcessEnumPipe::Stdout,
                            &format!("info string set {name} to {value}{suffix}"),
                        )?;
                    }
                }
            }
            command if command.starts_with("go ") => {
                self.emit(EngineProcessEnumPipe::Stderr, "search context")?;
                if !self.search_pending {
                    self.emit(EngineProcessEnumPipe::Stdout, "info score cp broken")?;
                }
            }
            _command => {}
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        match self.lines.pop_front() {
            Some(line) => Ok(line),
            None => std::future::pending().await,
        }
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.finish().await
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.finish().await
    }

    fn is_shutdown_complete(&self) -> bool {
        self.complete
    }

    async fn terminate(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.finish().await
    }
}

#[test]
fn diagnostic_tail_bounds_owned_payload_and_entries_for_long_empty_and_tiny_floods()
-> Result<(), Box<dyn Error>> {
    for text in [
        "界".repeat(EngineProcessLimits::LINE_BYTES / 3),
        String::new(),
        "x".to_owned(),
    ] {
        let line = EngineProcessLine::new(EngineProcessEnumPipe::Stderr, text)?;
        let mut diagnostics = EngineDiagnostics::default();
        for _index in 0..EngineDiagnostics::MAX_ENTRIES * 3 {
            diagnostics.record(&line);
            assert!(diagnostics.owned_bytes() <= EngineDiagnostics::MAX_OWNED_BYTES);
            assert!(diagnostics.entries().len() <= EngineDiagnostics::MAX_ENTRIES);
        }
        assert!(diagnostics.evicted_lines() > 0);
        assert_eq!(
            diagnostics.entries().next_back().map(|entry| entry.text()),
            Some(line.text())
        );
        if !line.text().is_empty() {
            assert!(diagnostics.evicted_bytes() > 0);
        }
    }
    Ok(())
}

#[test]
fn diagnostic_tail_preserves_observed_cross_pipe_order_latest_lines_and_unicode()
-> Result<(), Box<dyn Error>> {
    let mut diagnostics = EngineDiagnostics::default();
    let large = EngineProcessLine::new(
        EngineProcessEnumPipe::Stdout,
        "界".repeat(EngineProcessLimits::LINE_BYTES / 3),
    )?;
    for _index in 0..32 {
        diagnostics.record(&large);
    }
    let expected = [
        (EngineProcessEnumPipe::Stderr, "é λ 🦀"),
        (EngineProcessEnumPipe::Stdout, ""),
        (EngineProcessEnumPipe::Stderr, "latest 界"),
    ];
    for (pipe, text) in expected {
        diagnostics.record(&EngineProcessLine::new(pipe, text.to_owned())?);
    }
    let entries: Vec<_> = diagnostics
        .entries()
        .map(|entry| (entry.pipe(), entry.text()))
        .collect();
    assert!(entries.ends_with(&expected));
    assert!(diagnostics.evicted_lines() > 0);
    assert!(
        diagnostics
            .entries()
            .all(|entry| std::str::from_utf8(entry.text().as_bytes()).is_ok())
    );
    assert!(!format!("{diagnostics:?}").contains("latest"));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn diagnostic_startup_failure_and_cleanup_preserve_ordered_stderr_and_rejected_stdout()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineDiagnosticsTestProvider {
        handshake: EngineDiagnosticsTestEnumHandshake::Invalid,
        ..EngineDiagnosticsTestProvider::ready()
    };
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    assert!(matches!(
        startup.initialize().await,
        Err(EngineSessionError::UnexpectedResponse)
    ));
    startup
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    assert!(startup.is_shutdown_complete());
    let diagnostics = startup.take_diagnostics();
    assert_eq!(
        diagnostics
            .entries()
            .map(|entry| (entry.pipe(), entry.text()))
            .collect::<Vec<_>>(),
        [
            (EngineProcessEnumPipe::Stderr, "startup context λ"),
            (EngineProcessEnumPipe::Stdout, "invalid handshake")
        ]
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn diagnostic_startup_transfer_readiness_and_configuration_keep_one_tail()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineDiagnosticsTestProvider::ready();
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    let mut session = startup.initialize().await?;
    assert_eq!(
        session
            .diagnostics()
            .entries()
            .next()
            .map(|entry| entry.text()),
        Some("startup context λ")
    );
    assert_eq!(startup.diagnostics().entries().len(), 0);
    session.readiness().await?;
    session
        .configure_resources(EngineResourceConfig::new(1, 16, ClockDuration::ZERO)?)
        .await?;
    session
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    assert_eq!(
        session
            .diagnostics()
            .entries()
            .filter(|entry| entry.text() == "configuration context")
            .count(),
        3
    );
    assert_eq!(
        session
            .diagnostics()
            .entries()
            .filter(|entry| entry.text() == "readiness context")
            .count(),
        3
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn diagnostic_malformed_search_and_cleanup_failure_keep_both_typed_causes_and_tail()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineDiagnosticsTestProvider {
        cleanup_failure: true,
        ..EngineDiagnosticsTestProvider::ready()
    };
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    let mut session = startup.initialize().await?;
    session
        .start_search(
            EngineSearchContext::new(
                EngineSearchGameId::try_from(Uuid::from_u128(1))?,
                EngineSearchRevision::new(1),
            ),
            EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
            EngineSearchLimit::depth(1)?,
        )
        .await?;
    session.next_event().await?;
    let error = session
        .next_event()
        .await
        .err()
        .ok_or("Malformed output was accepted")?;
    assert!(
        matches!(error, EngineSessionError::OperationCleanup { operation, cleanup } if matches!(*operation, EngineSessionError::Analysis(_)) && matches!(*cleanup, EngineSessionError::ShutdownFailed(_)))
    );
    assert_eq!(session.state(), EngineSearchEnumState::Failed);
    assert!(session.is_shutdown_complete());
    assert!(session.shutdown_retained(Instant::now()).await.is_err());
    let diagnostics = session.take_diagnostics();
    assert_eq!(
        diagnostics.entries().next_back().map(|entry| entry.text()),
        Some("info score cp broken")
    );
    assert!(
        diagnostics
            .entries()
            .any(|entry| entry.text() == "search context")
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn diagnostic_cancelled_handshake_and_cleanup_keep_received_tail()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineDiagnosticsTestProvider {
        handshake: EngineDiagnosticsTestEnumHandshake::Pending,
        cleanup_pending: true,
        ..EngineDiagnosticsTestProvider::ready()
    };
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    {
        let mut future = std::pin::pin!(startup.initialize());
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert_eq!(
        startup
            .diagnostics()
            .entries()
            .next_back()
            .map(|entry| entry.text()),
        Some("startup context λ")
    );
    let deadline = Instant::now() + EngineProcessLimits::SHUTDOWN;
    {
        let mut future = std::pin::pin!(startup.shutdown_retained(deadline));
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert!(!startup.is_shutdown_complete());
    startup.shutdown_retained(deadline).await?;
    assert!(startup.is_shutdown_complete());
    assert_eq!(
        startup
            .diagnostics()
            .entries()
            .next_back()
            .map(|entry| entry.text()),
        Some("startup context λ")
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn diagnostic_consuming_start_failure_owns_tail_and_both_typed_causes()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineDiagnosticsTestProvider {
        handshake: EngineDiagnosticsTestEnumHandshake::Invalid,
        cleanup_failure: true,
        ..EngineDiagnosticsTestProvider::ready()
    };
    let error = EngineSession::start(&provider, fixture.select()?)
        .await
        .err()
        .ok_or("Invalid startup succeeded")?;
    assert!(
        matches!(error.operation(), EngineSessionError::Cleanup { startup, cleanup }
        if matches!(**startup, EngineSessionError::UnexpectedResponse) && matches!(cleanup, EngineProcessError::Io(_)))
    );
    let diagnostics = error
        .diagnostics()
        .ok_or("Startup error lost diagnostic ownership")?;
    assert_eq!(
        diagnostics.entries().next_back().map(|entry| entry.text()),
        Some("invalid handshake")
    );
    assert!(error.source().is_some());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn diagnostic_startup_timeout_retains_stderr_through_observed_shutdown()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineDiagnosticsTestProvider {
        handshake: EngineDiagnosticsTestEnumHandshake::Pending,
        ..EngineDiagnosticsTestProvider::ready()
    };
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    assert!(matches!(
        startup.initialize().await,
        Err(EngineSessionError::Timeout("uci"))
    ));
    startup
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    assert!(startup.is_shutdown_complete());
    assert_eq!(
        startup
            .diagnostics()
            .entries()
            .next_back()
            .map(|entry| entry.text()),
        Some("startup context λ")
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn diagnostic_stop_timeout_and_cancelled_wait_keep_search_context()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    for cancel in [false, true] {
        let provider = EngineDiagnosticsTestProvider {
            search_pending: true,
            ..EngineDiagnosticsTestProvider::ready()
        };
        let mut session = EngineSession::start(&provider, fixture.select()?).await?;
        let tag = session
            .start_search(
                EngineSearchContext::new(
                    EngineSearchGameId::try_from(Uuid::from_u128(1))?,
                    EngineSearchRevision::new(1),
                ),
                EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
                EngineSearchLimit::infinite(),
            )
            .await?;
        let request = EngineSearchStop::received_at(tag, Instant::now())?;
        if cancel {
            {
                let mut future = std::pin::pin!(session.stop(request));
                assert!(
                    future
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending()
                );
            }
            assert_eq!(session.state(), EngineSearchEnumState::Stopping);
            session
                .shutdown_retained(request.termination_deadline())
                .await?;
        } else {
            assert!(matches!(
                session.stop(request).await,
                Err(EngineSessionError::StopDeadline)
            ));
        }
        assert!(session.is_shutdown_complete());
        assert_eq!(
            session
                .diagnostics()
                .entries()
                .next_back()
                .map(|entry| entry.text()),
            Some("search context")
        );
    }
    Ok(())
}
