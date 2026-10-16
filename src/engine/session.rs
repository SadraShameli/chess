use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::time::{Instant, timeout, timeout_at};

use crate::board::enums::BoardEnumVariant;
use crate::board::r#move::BoardMoveError;

use crate::engine::analysis::{EngineAnalysis, EngineAnalysisError};
use crate::engine::capabilities::{EngineCapabilities, EngineCapabilitiesError, EngineIdentity};
use crate::engine::enums::EngineEnumOption;
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLimits,
    EngineProcessProvider,
};
use crate::engine::resource_config::EngineResourceConfig;
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
    Analysis(#[from] EngineAnalysisError),
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
        #[source]
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
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error("The engine reported successful shutdown without observing all owned resources")]
    ShutdownUnobserved,
    #[error("Engine session shutdown previously failed: {0}")]
    ShutdownFailed(#[source] Arc<Self>),
    #[error("Engine initialization already started or its process was transferred or closed")]
    StartupUnavailable,
}

impl EngineSessionError {
    fn shutdown_result(failure: Option<&Arc<Self>>) -> Result<(), Self> {
        match failure {
            Some(error) => Err(Self::ShutdownFailed(Arc::clone(error))),
            None => Ok(()),
        }
    }
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

#[derive(Clone, Copy)]
enum EngineSessionEnumShutdown {
    Running,
    SendingStop,
    Graceful,
    Forced,
    Complete,
}

enum EngineSessionStartupEnumState {
    Ready,
    Initializing,
    Closing,
    Transferred,
}

struct EngineSessionStartupProcess<P: EngineProcess> {
    process: P,
    package: EnginePackageSelected,
}

pub struct EngineSessionStartup<P: EngineProcess> {
    resources: Option<EngineSessionStartupProcess<P>>,
    state: EngineSessionStartupEnumState,
    shutdown_failure: Option<Arc<EngineSessionError>>,
}

impl<P: EngineProcess> EngineSessionStartup<P> {
    pub fn new(
        provider: &impl EngineProcessProvider<Process = P>,
        package: EnginePackageSelected,
    ) -> Result<Self, EngineSessionError> {
        let process = provider.spawn(&package)?;
        Ok(Self {
            resources: Some(EngineSessionStartupProcess { process, package }),
            state: EngineSessionStartupEnumState::Ready,
            shutdown_failure: None,
        })
    }

    pub async fn initialize(&mut self) -> Result<EngineSession<P>, EngineSessionError> {
        if !matches!(self.state, EngineSessionStartupEnumState::Ready) {
            return Err(EngineSessionError::StartupUnavailable);
        }
        self.state = EngineSessionStartupEnumState::Initializing;
        let identity = EngineSession::<P>::handshake(self.process_mut()?).await?;
        let resources = self
            .resources
            .take()
            .ok_or(EngineSessionError::StartupUnavailable)?;
        self.state = EngineSessionStartupEnumState::Transferred;
        Ok(EngineSession {
            process: Some(resources.process),
            identity,
            package: resources.package,
            generation: EngineSearchGeneration::fresh(),
            variant: BoardEnumVariant::Standard,
            resources: None,
            lifecycle: EngineSessionEnumLifecycle::Idle,
            shutdown: EngineSessionEnumShutdown::Running,
            shutdown_failure: None,
        })
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.resources
            .as_ref()
            .is_none_or(|resources| resources.process.is_shutdown_complete())
    }

    pub async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineSessionError> {
        if self.resources.is_none() {
            return EngineSessionError::shutdown_result(self.shutdown_failure.as_ref());
        }
        let graceful = matches!(
            self.state,
            EngineSessionStartupEnumState::Ready | EngineSessionStartupEnumState::Initializing
        );
        self.state = EngineSessionStartupEnumState::Closing;
        let result =
            EngineSession::<P>::shutdown_process(self.process_mut()?, graceful, deadline).await;
        if self.is_shutdown_complete() {
            self.resources = None;
            self.shutdown_failure = result.err().map(Arc::new);
            EngineSessionError::shutdown_result(self.shutdown_failure.as_ref())
        } else {
            result
        }
    }

    fn process_mut(&mut self) -> Result<&mut P, EngineSessionError> {
        self.resources
            .as_mut()
            .map(|resources| &mut resources.process)
            .ok_or(EngineSessionError::StartupUnavailable)
    }
}

pub struct EngineSession<P: EngineProcess> {
    process: Option<P>,
    identity: EngineIdentity,
    package: EnginePackageSelected,
    generation: EngineSearchGeneration,
    variant: BoardEnumVariant,
    resources: Option<EngineResourceConfig>,
    lifecycle: EngineSessionEnumLifecycle,
    shutdown: EngineSessionEnumShutdown,
    shutdown_failure: Option<Arc<EngineSessionError>>,
}

impl<P: EngineProcess> EngineSession<P> {
    pub const HANDSHAKE: Duration = Duration::from_secs(5);

    pub async fn start(
        provider: &impl EngineProcessProvider<Process = P>,
        package: EnginePackageSelected,
    ) -> Result<Self, EngineSessionError> {
        let mut owner = EngineSessionStartup::new(provider, package)?;
        match owner.initialize().await {
            Ok(session) => Ok(session),
            Err(startup) => match owner.process_mut()?.shutdown().await {
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

    pub fn resources(&self) -> Option<EngineResourceConfig> {
        self.resources
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

    pub async fn configure_resources(
        &mut self,
        config: EngineResourceConfig,
    ) -> Result<(), EngineSessionError> {
        self.require_idle()?;
        self.lifecycle = EngineSessionEnumLifecycle::Preparing;
        self.resources = None;
        let result = timeout(Self::HANDSHAKE, async {
            for (option, value) in config.settings() {
                self.set_acknowledged_option(
                    &option.set_value_command(value),
                    &option.acknowledgment(value),
                )
                .await?;
            }
            Self::ready(self.process_mut()?).await
        })
        .await
        .map_err(|_elapsed| EngineSessionError::Timeout("resource configuration"))
        .and_then(|result| result);
        self.finish_preparation(result).await?;
        self.resources = Some(config);
        Ok(())
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
        self.shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.process
            .as_ref()
            .is_none_or(EngineProcess::is_shutdown_complete)
    }

    pub async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineSessionError> {
        if self.process.is_none() {
            return EngineSessionError::shutdown_result(self.shutdown_failure.as_ref());
        }
        let deadline = self.stop_request().map_or(deadline, |request| {
            deadline.min(request.termination_deadline())
        });
        let send_stop = if matches!(self.shutdown, EngineSessionEnumShutdown::Running) {
            let state = self.state();
            self.shutdown = if self.has_pending_stop_delivery()
                || matches!(
                    state,
                    EngineSearchEnumState::Failed | EngineSearchEnumState::Preparing
                ) {
                EngineSessionEnumShutdown::Forced
            } else if matches!(
                state,
                EngineSearchEnumState::Searching | EngineSearchEnumState::Stopping
            ) {
                EngineSessionEnumShutdown::SendingStop
            } else {
                EngineSessionEnumShutdown::Graceful
            };
            matches!(self.shutdown, EngineSessionEnumShutdown::SendingStop)
        } else {
            if matches!(self.shutdown, EngineSessionEnumShutdown::SendingStop) {
                self.shutdown = EngineSessionEnumShutdown::Forced;
            }
            false
        };
        self.lifecycle = EngineSessionEnumLifecycle::Failed;
        let result = async {
            if send_stop {
                timeout_at(deadline, self.process_mut()?.send("stop"))
                    .await
                    .map_err(EngineProcessError::ShutdownDeadline)??;
                self.shutdown = EngineSessionEnumShutdown::Graceful;
            }
            let graceful = matches!(self.shutdown, EngineSessionEnumShutdown::Graceful);
            Self::shutdown_process(self.process_mut()?, graceful, deadline).await
        }
        .await;
        if self.is_shutdown_complete() {
            self.process = None;
            self.shutdown = EngineSessionEnumShutdown::Complete;
            if let Err(error) = result {
                self.shutdown_failure = Some(Arc::new(error));
            }
            EngineSessionError::shutdown_result(self.shutdown_failure.as_ref())
        } else {
            self.shutdown = EngineSessionEnumShutdown::Forced;
            result
        }
    }

    async fn shutdown_process(
        process: &mut P,
        graceful: bool,
        deadline: Instant,
    ) -> Result<(), EngineSessionError> {
        let result = if graceful {
            timeout_at(deadline, process.shutdown_retained(deadline)).await
        } else {
            timeout_at(deadline, process.terminate(deadline)).await
        };
        result.map_err(EngineProcessError::ShutdownDeadline)??;
        if !process.is_shutdown_complete() {
            return Err(EngineSessionError::ShutdownUnobserved);
        }
        Ok(())
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
        let option = EngineEnumOption::Chess960;
        self.set_acknowledged_option(
            &option.set_value_command(value),
            &option.acknowledgment(value),
        )
        .await?;
        Self::ready(self.process_mut()?).await?;
        self.variant = variant;
        Ok(())
    }

    async fn set_acknowledged_option(
        &mut self,
        command: &str,
        acknowledgment: &str,
    ) -> Result<(), EngineSessionError> {
        self.process_mut()?.send(command).await?;
        loop {
            let line = self.process_mut()?.receive().await?;
            if line.pipe() == EngineProcessEnumPipe::Stderr {
                continue;
            }
            if line.text() != acknowledgment {
                return Err(EngineSessionError::UnexpectedResponse);
            }
            return Ok(());
        }
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
        match self.shutdown_retained(deadline).await {
            Ok(()) => operation,
            Err(cleanup) => EngineSessionError::OperationCleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            },
        }
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
            EngineSearchEnumOutput::Info(EngineAnalysis::parse(text, active.position.current())?)
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
