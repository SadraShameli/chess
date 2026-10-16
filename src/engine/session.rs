use std::time::Duration;

use thiserror::Error;
use tokio::time::{Instant, timeout, timeout_at};

use crate::board::enums::BoardEnumVariant;
use crate::board::r#move::BoardMoveError;

use crate::engine::capabilities::{EngineCapabilities, EngineCapabilitiesError, EngineIdentity};
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLimits,
    EngineProcessProvider,
};
use crate::engine::search::enums::{EngineSearchEnumOutput, EngineSearchEnumState};
use crate::engine::search::event::EngineSearchEvent;
use crate::engine::search::identity::{
    EngineSearchContext, EngineSearchGeneration, EngineSearchTag,
};
use crate::engine::search::limit::EngineSearchLimit;
use crate::engine::search::position::EngineSearchPosition;
use crate::engine::search::stop::EngineSearchStop;

#[derive(Debug, Error)]
pub enum EngineSessionError {
    #[error(transparent)]
    Process(#[from] EngineProcessError),
    #[error(transparent)]
    Capabilities(#[from] EngineCapabilitiesError),
    #[error("Engine {0} handshake exceeded five seconds")]
    Timeout(&'static str),
    #[error("The engine sent an unexpected startup response")]
    UnexpectedResponse,
    #[error("Engine startup failed ({startup}); cleanup also failed ({cleanup})")]
    Cleanup {
        startup: Box<Self>,
        cleanup: EngineProcessError,
    },
    #[error("The engine cannot perform this operation while {0:?}")]
    State(EngineSearchEnumState),
    #[error("A terminal position must be adjudicated before starting an engine search")]
    TerminalPosition,
    #[error("The engine stop exceeded its deadline from application receipt")]
    StopDeadline,
    #[error("Engine stop delivery was interrupted and its process cannot be reused")]
    AmbiguousStopDelivery,
    #[error("The stop request belongs to another search or process generation")]
    StaleSearch,
    #[error("The engine search sent an unsupported or malformed response")]
    SearchResponse,
    #[error("The engine search returned an invalid move: {0}")]
    SearchMove(#[from] BoardMoveError),
    #[error("Engine operation failed ({operation}); forced cleanup also failed ({cleanup})")]
    OperationCleanup {
        operation: Box<Self>,
        cleanup: EngineProcessError,
    },
}

struct EngineSessionSearch {
    tag: EngineSearchTag,
    position: EngineSearchPosition,
}

enum EngineSessionEnumStopDelivery {
    Pending,
    Complete,
}

enum EngineSessionEnumLifecycle {
    Idle,
    Preparing,
    Searching(EngineSessionSearch),
    Stopping {
        active: EngineSessionSearch,
        request: EngineSearchStop,
        delivery: EngineSessionEnumStopDelivery,
    },
    Failed,
}

pub struct EngineSession<P: EngineProcess> {
    process: Option<P>,
    identity: EngineIdentity,
    package: EnginePackageSelected,
    generation: EngineSearchGeneration,
    variant: BoardEnumVariant,
    lifecycle: EngineSessionEnumLifecycle,
}

impl<P: EngineProcess> EngineSession<P> {
    pub const HANDSHAKE: Duration = Duration::from_secs(5);

    pub async fn start(
        provider: &impl EngineProcessProvider<Process = P>,
        package: EnginePackageSelected,
    ) -> Result<Self, EngineSessionError> {
        let mut process = provider.spawn(&package)?;
        let result = Self::handshake(&mut process).await;
        match result {
            Ok(identity) => Ok(Self {
                process: Some(process),
                identity,
                package,
                generation: EngineSearchGeneration::fresh(),
                variant: BoardEnumVariant::Standard,
                lifecycle: EngineSessionEnumLifecycle::Idle,
            }),
            Err(startup) => match process.shutdown().await {
                Ok(()) => Err(startup),
                Err(cleanup) => Err(EngineSessionError::Cleanup {
                    startup: Box::new(startup),
                    cleanup,
                }),
            },
        }
    }

    pub fn identity(&self) -> &EngineIdentity {
        &self.identity
    }

    pub fn package(&self) -> &EnginePackageSelected {
        &self.package
    }

    pub fn generation(&self) -> EngineSearchGeneration {
        self.generation
    }

    pub fn state(&self) -> EngineSearchEnumState {
        match self.lifecycle {
            EngineSessionEnumLifecycle::Idle => EngineSearchEnumState::Idle,
            EngineSessionEnumLifecycle::Preparing => EngineSearchEnumState::Preparing,
            EngineSessionEnumLifecycle::Searching(_) => EngineSearchEnumState::Searching,
            EngineSessionEnumLifecycle::Stopping { .. } => EngineSearchEnumState::Stopping,
            EngineSessionEnumLifecycle::Failed => EngineSearchEnumState::Failed,
        }
    }

    pub async fn readiness(&mut self) -> Result<(), EngineSessionError> {
        self.require_idle()?;
        self.lifecycle = EngineSessionEnumLifecycle::Preparing;
        let result = timeout(Self::HANDSHAKE, Self::ready(self.process_mut()?))
            .await
            .map_err(|_elapsed| EngineSessionError::Timeout("readiness"))
            .and_then(|result| result);
        self.finish_preparation(result).await
    }

    pub async fn new_game(&mut self, variant: BoardEnumVariant) -> Result<(), EngineSessionError> {
        self.require_idle()?;
        self.lifecycle = EngineSessionEnumLifecycle::Preparing;
        let result = timeout(Self::HANDSHAKE, async {
            self.process_mut()?.send("ucinewgame").await?;
            self.configure_variant(variant).await
        })
        .await
        .map_err(|_elapsed| EngineSessionError::Timeout("new game"))
        .and_then(|result| result);
        self.finish_preparation(result).await
    }

    pub async fn start_search(
        &mut self,
        context: EngineSearchContext,
        position: EngineSearchPosition,
        limit: EngineSearchLimit,
    ) -> Result<EngineSearchTag, EngineSessionError> {
        self.require_idle()?;
        if position.current().legal_moves().is_empty() {
            return Err(EngineSessionError::TerminalPosition);
        }
        self.lifecycle = EngineSessionEnumLifecycle::Preparing;
        let result = timeout(Self::HANDSHAKE, async {
            if self.variant != position.current().variant() {
                self.configure_variant(position.current().variant()).await?;
            }
            self.process_mut()?.send(position.command()).await?;
            self.process_mut()?.send(&limit.command()).await?;
            Ok(())
        })
        .await
        .map_err(|_elapsed| EngineSessionError::Timeout("search setup"))
        .and_then(|result| result);
        if let Err(error) = result {
            return Err(self
                .fail(error, Instant::now() + EngineProcessLimits::SHUTDOWN)
                .await);
        }
        let tag = EngineSearchTag::fresh(self.generation, context);
        self.lifecycle =
            EngineSessionEnumLifecycle::Searching(EngineSessionSearch { tag, position });
        Ok(tag)
    }

    pub async fn next_event(&mut self) -> Result<EngineSearchEvent, EngineSessionError> {
        self.active()?;
        let stop = self.stop_request();
        match self.receive_event().await {
            Ok(event) => Ok(event),
            Err(error) => {
                let termination_deadline = stop.map_or_else(
                    || Instant::now() + EngineProcessLimits::SHUTDOWN,
                    EngineSearchStop::termination_deadline,
                );
                Err(self.fail(error, termination_deadline).await)
            }
        }
    }

    pub async fn stop(
        &mut self,
        request: EngineSearchStop,
    ) -> Result<EngineSearchTag, EngineSessionError> {
        if self.active()?.tag != request.tag() {
            return Err(EngineSessionError::StaleSearch);
        }
        let previous = std::mem::replace(&mut self.lifecycle, EngineSessionEnumLifecycle::Failed);
        let send_stop = match previous {
            EngineSessionEnumLifecycle::Searching(active) => {
                self.lifecycle = EngineSessionEnumLifecycle::Stopping {
                    active,
                    request,
                    delivery: EngineSessionEnumStopDelivery::Pending,
                };
                true
            }
            EngineSessionEnumLifecycle::Stopping {
                active,
                request: existing,
                delivery,
            } => {
                let request = if request.deadline() < existing.deadline() {
                    request
                } else {
                    existing
                };
                self.lifecycle = EngineSessionEnumLifecycle::Stopping {
                    active,
                    request,
                    delivery,
                };
                false
            }
            EngineSessionEnumLifecycle::Idle
            | EngineSessionEnumLifecycle::Preparing
            | EngineSessionEnumLifecycle::Failed => {
                return Err(EngineSessionError::State(self.state()));
            }
        };
        let request = self
            .stop_request()
            .ok_or(EngineSessionError::State(self.state()))?;
        if request.has_expired(Instant::now()) {
            return Err(self
                .fail(
                    EngineSessionError::StopDeadline,
                    request.termination_deadline(),
                )
                .await);
        }
        if send_stop {
            let sent = timeout_at(request.deadline(), self.process_mut()?.send("stop"))
                .await
                .map_err(|_elapsed| EngineSessionError::StopDeadline)
                .and_then(|result| result.map_err(EngineSessionError::Process));
            if let Err(error) = sent {
                return Err(self.fail(error, request.termination_deadline()).await);
            }
            let state = self.state();
            let EngineSessionEnumLifecycle::Stopping { delivery, .. } = &mut self.lifecycle else {
                return Err(EngineSessionError::State(state));
            };
            *delivery = EngineSessionEnumStopDelivery::Complete;
        }
        loop {
            let event = self.next_event().await?;
            if matches!(event.output(), EngineSearchEnumOutput::Stopped) {
                return Ok(event.tag());
            }
        }
    }

    pub async fn shutdown(mut self) -> Result<(), EngineSessionError> {
        let state = self.state();
        let interrupted = self.has_pending_stop_delivery();
        let deadline = Instant::now() + EngineProcessLimits::SHUTDOWN;
        let deadline = self.stop_request().map_or(deadline, |request| {
            deadline.min(request.termination_deadline())
        });
        let Some(mut process) = self.process.take() else {
            return Ok(());
        };
        if interrupted
            || matches!(
                state,
                EngineSearchEnumState::Failed | EngineSearchEnumState::Preparing
            )
        {
            process.terminate(deadline).await?;
            return Ok(());
        }
        let result = timeout_at(deadline, async {
            if matches!(
                state,
                EngineSearchEnumState::Searching | EngineSearchEnumState::Stopping
            ) {
                process.send("stop").await?;
            }
            process.shutdown().await
        })
        .await;
        match result {
            Ok(result) => result.map_err(EngineSessionError::Process),
            Err(_elapsed) => {
                let operation = EngineSessionError::Process(EngineProcessError::ShutdownTimeout);
                match process.terminate(deadline).await {
                    Ok(()) => Err(operation),
                    Err(cleanup) => Err(EngineSessionError::OperationCleanup {
                        operation: Box::new(operation),
                        cleanup,
                    }),
                }
            }
        }
    }

    fn require_idle(&self) -> Result<(), EngineSessionError> {
        if matches!(self.lifecycle, EngineSessionEnumLifecycle::Idle) {
            Ok(())
        } else {
            Err(EngineSessionError::State(self.state()))
        }
    }

    fn process_mut(&mut self) -> Result<&mut P, EngineSessionError> {
        self.process
            .as_mut()
            .ok_or(EngineSessionError::State(EngineSearchEnumState::Failed))
    }

    fn active(&self) -> Result<&EngineSessionSearch, EngineSessionError> {
        match &self.lifecycle {
            EngineSessionEnumLifecycle::Searching(active)
            | EngineSessionEnumLifecycle::Stopping { active, .. } => Ok(active),
            EngineSessionEnumLifecycle::Idle
            | EngineSessionEnumLifecycle::Preparing
            | EngineSessionEnumLifecycle::Failed => Err(EngineSessionError::State(self.state())),
        }
    }

    fn stop_request(&self) -> Option<EngineSearchStop> {
        match self.lifecycle {
            EngineSessionEnumLifecycle::Stopping { request, .. } => Some(request),
            EngineSessionEnumLifecycle::Idle
            | EngineSessionEnumLifecycle::Preparing
            | EngineSessionEnumLifecycle::Searching(_)
            | EngineSessionEnumLifecycle::Failed => None,
        }
    }

    fn has_pending_stop_delivery(&self) -> bool {
        matches!(
            self.lifecycle,
            EngineSessionEnumLifecycle::Stopping {
                delivery: EngineSessionEnumStopDelivery::Pending,
                ..
            }
        )
    }

    async fn configure_variant(
        &mut self,
        variant: BoardEnumVariant,
    ) -> Result<(), EngineSessionError> {
        let value = match variant {
            BoardEnumVariant::Standard => "false",
            BoardEnumVariant::Chess960 => "true",
        };
        self.process_mut()?
            .send(&format!("setoption name UCI_Chess960 value {value}"))
            .await?;
        Self::ready(self.process_mut()?).await?;
        self.variant = variant;
        Ok(())
    }

    async fn finish_preparation(
        &mut self,
        result: Result<(), EngineSessionError>,
    ) -> Result<(), EngineSessionError> {
        match result {
            Ok(()) => {
                self.lifecycle = EngineSessionEnumLifecycle::Idle;
                Ok(())
            }
            Err(error) => Err(self
                .fail(error, Instant::now() + EngineProcessLimits::SHUTDOWN)
                .await),
        }
    }

    async fn fail(
        &mut self,
        operation: EngineSessionError,
        deadline: Instant,
    ) -> EngineSessionError {
        self.lifecycle = EngineSessionEnumLifecycle::Failed;
        if let Some(process) = &mut self.process {
            if let Err(cleanup) = process.terminate(deadline).await {
                return EngineSessionError::OperationCleanup {
                    operation: Box::new(operation),
                    cleanup,
                };
            }
            self.process = None;
        }
        operation
    }

    async fn receive_event(&mut self) -> Result<EngineSearchEvent, EngineSessionError> {
        if self.has_pending_stop_delivery() {
            return Err(EngineSessionError::AmbiguousStopDelivery);
        }
        let line = if let Some(request) = self.stop_request() {
            if request.has_expired(Instant::now()) {
                return Err(EngineSessionError::StopDeadline);
            }
            timeout_at(request.deadline(), self.process_mut()?.receive())
                .await
                .map_err(|_elapsed| EngineSessionError::StopDeadline)??
        } else {
            self.process_mut()?.receive().await?
        };
        if self
            .stop_request()
            .is_some_and(|request| request.has_expired(Instant::now()))
        {
            return Err(EngineSessionError::StopDeadline);
        }
        let active = self.active()?;
        let tag = active.tag;
        let text = line.text();
        let output = if line.pipe() == EngineProcessEnumPipe::Stderr {
            EngineSearchEnumOutput::Diagnostic {
                pipe: line.pipe(),
                text: text.to_owned(),
            }
        } else if text == "readyok" {
            EngineSearchEnumOutput::Readiness
        } else if text.starts_with("info ") {
            EngineSearchEnumOutput::Info(text.to_owned())
        } else if let Some(value) = text.strip_prefix("bestmove ") {
            if value.contains(char::is_whitespace) {
                return Err(EngineSessionError::SearchResponse);
            }
            let movement = active.position.current().parse_uci(value)?;
            let stopped = matches!(self.lifecycle, EngineSessionEnumLifecycle::Stopping { .. });
            self.lifecycle = EngineSessionEnumLifecycle::Idle;
            if stopped {
                EngineSearchEnumOutput::Stopped
            } else {
                EngineSearchEnumOutput::BestMove(movement)
            }
        } else {
            return Err(EngineSessionError::SearchResponse);
        };
        Ok(EngineSearchEvent::new(tag, output))
    }

    async fn handshake(process: &mut P) -> Result<EngineIdentity, EngineSessionError> {
        let identity = timeout(Self::HANDSHAKE, Self::identify(process))
            .await
            .map_err(|_elapsed| EngineSessionError::Timeout("uci"))??;
        timeout(Self::HANDSHAKE, Self::ready(process))
            .await
            .map_err(|_elapsed| EngineSessionError::Timeout("readiness"))??;
        Ok(identity)
    }

    async fn identify(process: &mut P) -> Result<EngineIdentity, EngineSessionError> {
        process.send("uci").await?;
        let mut name = None;
        let mut author = None;
        let mut capabilities = EngineCapabilities::default();
        loop {
            let line = process.receive().await?;
            if line.pipe() == EngineProcessEnumPipe::Stderr {
                continue;
            }
            let text = line.text();
            if let Some(value) = text.strip_prefix("id name ") {
                if name.replace(value.to_owned()).is_some() {
                    return Err(EngineSessionError::UnexpectedResponse);
                }
            } else if let Some(value) = text.strip_prefix("id author ") {
                if author.replace(value.to_owned()).is_some() {
                    return Err(EngineSessionError::UnexpectedResponse);
                }
            } else if text.starts_with("option ") {
                capabilities.discover(text)?;
            } else if text == "uciok" {
                return Ok(EngineIdentity::new(
                    name.ok_or(EngineCapabilitiesError::IdentityMismatch)?,
                    author.ok_or(EngineCapabilitiesError::IdentityMismatch)?,
                    capabilities,
                )?);
            } else {
                return Err(EngineSessionError::UnexpectedResponse);
            }
        }
    }

    async fn ready(process: &mut P) -> Result<(), EngineSessionError> {
        process.send("isready").await?;
        loop {
            let line = process.receive().await?;
            if line.pipe() == EngineProcessEnumPipe::Stderr {
                continue;
            }
            if line.text() == "readyok" {
                return Ok(());
            }
            return Err(EngineSessionError::UnexpectedResponse);
        }
    }
}
