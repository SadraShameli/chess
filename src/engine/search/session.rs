use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::time::{Instant, sleep_until};

use crate::board::r#move::BoardMove;
use crate::capacity::lease::CapacityLease;
use crate::engine::analysis::{EngineAnalysis, EngineAnalysisLine};
use crate::engine::process_provider::{EngineProcess, EngineProcessEnumPipe, EngineProcessLimits};
use crate::engine::search::enums::{EngineSearchEnumOutput, EngineSearchEnumState};
use crate::engine::search::identity::{EngineSearchContext, EngineSearchTag};
use crate::engine::search::limit::EngineSearchLimit;
use crate::engine::search::position::EngineSearchPosition;
use crate::engine::search::stop::EngineSearchStop;
use crate::engine::session::{EngineSession, EngineSessionError};

#[derive(Debug, Clone, Error)]
pub enum EngineSearchSessionError {
    #[error("The analysis session has unfinished search ownership or an unobserved result")]
    Busy,
    #[error("The analysis request requires a complete admitted resource configuration")]
    ResourcesRequired,
    #[error("Configured engine threads {threads} do not match the owned CPU allocation {workers}")]
    ThreadLeaseMismatch { threads: u32, workers: usize },
    #[error("The search request belongs to another search, revision or process generation")]
    StaleSearch,
    #[error("Search setup was interrupted; stop or shut down its retained owner")]
    SetupInterrupted,
    #[error("The terminal search result was already delivered")]
    CompletionObserved,
    #[error("The analysis candidate rank exceeds the configured root candidate limit")]
    CandidateRank,
    #[error("The analysis publication deadline exceeds the runtime clock range")]
    Deadline,
    #[error("The engine analysis operation failed: {0}")]
    Engine(#[source] Arc<EngineSessionError>),
    #[error("Analysis operation failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Arc<Self>,
        cleanup: Arc<EngineSessionError>,
    },
}

pub struct EngineSearchSessionRequest {
    context: EngineSearchContext,
    position: EngineSearchPosition,
    limit: EngineSearchLimit,
    lease: CapacityLease,
}

impl EngineSearchSessionRequest {
    pub fn new(
        context: EngineSearchContext,
        position: EngineSearchPosition,
        limit: EngineSearchLimit,
        lease: CapacityLease,
    ) -> Self {
        Self {
            context,
            position,
            limit,
            lease,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EngineSearchSessionMetadata {
    tag: EngineSearchTag,
    position: EngineSearchPosition,
    limit: EngineSearchLimit,
    started_at: Instant,
}

impl EngineSearchSessionMetadata {
    pub const fn tag(&self) -> EngineSearchTag {
        self.tag
    }
    pub const fn position(&self) -> &EngineSearchPosition {
        &self.position
    }
    pub const fn limit(&self) -> EngineSearchLimit {
        self.limit
    }
    pub const fn started_at(&self) -> Instant {
        self.started_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineSearchSessionEnumCancellation {
    NotRequested,
    StopRequested(EngineSearchStop),
    ShutdownRequested,
}

#[derive(Debug, Clone)]
pub struct EngineSearchSessionSnapshot {
    metadata: EngineSearchSessionMetadata,
    cancellation: EngineSearchSessionEnumCancellation,
    lines: Vec<Arc<EngineAnalysisLine>>,
    latest_update: Option<Arc<EngineAnalysisLine>>,
}

impl EngineSearchSessionSnapshot {
    pub const fn metadata(&self) -> &EngineSearchSessionMetadata {
        &self.metadata
    }
    pub const fn cancellation(&self) -> EngineSearchSessionEnumCancellation {
        self.cancellation
    }
    pub fn lines(&self) -> &[Arc<EngineAnalysisLine>] {
        &self.lines
    }
    pub fn latest_update(&self) -> Option<&EngineAnalysisLine> {
        self.latest_update.as_deref()
    }
}

#[derive(Debug, Clone)]
pub enum EngineSearchSessionEnumCompletion {
    BestMove(BoardMove),
    Stopped,
    Cancelled,
    Failed(Arc<EngineSearchSessionError>),
}

#[derive(Debug, Clone)]
pub struct EngineSearchSessionTerminal {
    snapshot: EngineSearchSessionSnapshot,
    completion: EngineSearchSessionEnumCompletion,
}

impl EngineSearchSessionTerminal {
    pub const fn snapshot(&self) -> &EngineSearchSessionSnapshot {
        &self.snapshot
    }
    pub const fn completion(&self) -> &EngineSearchSessionEnumCompletion {
        &self.completion
    }
}

#[derive(Debug)]
pub enum EngineSearchSessionEnumEvent {
    Snapshot(EngineSearchSessionSnapshot),
    Readiness {
        tag: EngineSearchTag,
    },
    Diagnostic {
        tag: EngineSearchTag,
        pipe: EngineProcessEnumPipe,
        text: String,
    },
    Finished(EngineSearchSessionTerminal),
}

struct EngineSearchSessionAnalysis {
    candidates: BTreeMap<NonZeroU32, Arc<EngineAnalysisLine>>,
    latest: Option<Arc<EngineAnalysisLine>>,
    limit: NonZeroU32,
    next_publication: Instant,
    dirty: bool,
}

impl EngineSearchSessionAnalysis {
    const CADENCE: Duration = Duration::from_millis(50);
    fn new(limit: NonZeroU32, started_at: Instant) -> Result<Self, EngineSearchSessionError> {
        Ok(Self {
            candidates: BTreeMap::new(),
            latest: None,
            limit,
            next_publication: started_at
                .checked_add(Self::CADENCE)
                .ok_or(EngineSearchSessionError::Deadline)?,
            dirty: false,
        })
    }

    fn accept(&mut self, analysis: EngineAnalysis) -> Result<(), EngineSearchSessionError> {
        let Some(line) = analysis.into_line() else {
            return Ok(());
        };
        let line = Arc::new(line);
        if line.score().is_some() && line.variation().is_some() {
            let rank = line.rank().unwrap_or(NonZeroU32::MIN);
            if rank > self.limit {
                return Err(EngineSearchSessionError::CandidateRank);
            }
            self.candidates.insert(rank, Arc::clone(&line));
        }
        self.latest = Some(line);
        self.dirty = true;
        Ok(())
    }

    fn snapshot(
        &self,
        metadata: &EngineSearchSessionMetadata,
        cancellation: EngineSearchSessionEnumCancellation,
    ) -> EngineSearchSessionSnapshot {
        EngineSearchSessionSnapshot {
            metadata: metadata.clone(),
            cancellation,
            lines: self.candidates.values().map(Arc::clone).collect(),
            latest_update: self.latest.as_ref().map(Arc::clone),
        }
    }

    fn publication_deadline(&self) -> Option<Instant> {
        self.dirty.then_some(self.next_publication)
    }

    fn published(&mut self) -> Result<(), EngineSearchSessionError> {
        self.next_publication = Instant::now()
            .checked_add(Self::CADENCE)
            .ok_or(EngineSearchSessionError::Deadline)?;
        self.dirty = false;
        Ok(())
    }
}

pub struct EngineSearchSession<P: EngineProcess> {
    engine: EngineSession<P>,
    lease: Option<CapacityLease>,
    metadata: Option<EngineSearchSessionMetadata>,
    analysis: Option<EngineSearchSessionAnalysis>,
    cancellation: EngineSearchSessionEnumCancellation,
    terminal: Option<EngineSearchSessionTerminal>,
    terminal_delivered: bool,
    pending_signal: Option<EngineSearchSessionEnumEvent>,
}

impl<P: EngineProcess> EngineSearchSession<P> {
    pub const CADENCE: Duration = EngineSearchSessionAnalysis::CADENCE;
    const MESSAGE_BATCH: usize = 64;

    pub fn new(engine: EngineSession<P>) -> Self {
        Self {
            engine,
            lease: None,
            metadata: None,
            analysis: None,
            cancellation: EngineSearchSessionEnumCancellation::NotRequested,
            terminal: None,
            terminal_delivered: false,
            pending_signal: None,
        }
    }

    pub const fn engine(&self) -> &EngineSession<P> {
        &self.engine
    }
    pub const fn metadata(&self) -> Option<&EngineSearchSessionMetadata> {
        self.metadata.as_ref()
    }
    pub const fn terminal(&self) -> Option<&EngineSearchSessionTerminal> {
        self.terminal.as_ref()
    }
    pub const fn cancellation(&self) -> EngineSearchSessionEnumCancellation {
        self.cancellation
    }
    pub fn allocation(&self) -> Option<usize> {
        self.lease.as_ref().map(CapacityLease::workers)
    }

    pub async fn start(
        &mut self,
        request: EngineSearchSessionRequest,
    ) -> Result<EngineSearchTag, EngineSearchSessionError> {
        if self.lease.is_some()
            || self.engine.state() != EngineSearchEnumState::Idle
            || self.terminal.is_some() && !self.terminal_delivered
        {
            return Err(EngineSearchSessionError::Busy);
        }
        let resources = self
            .engine
            .resources()
            .ok_or(EngineSearchSessionError::ResourcesRequired)?;
        if usize::try_from(resources.threads()).ok() != Some(request.lease.workers()) {
            return Err(EngineSearchSessionError::ThreadLeaseMismatch {
                threads: resources.threads(),
                workers: request.lease.workers(),
            });
        }
        let count = request.position.current().legal_moves().len();
        if count == 0 {
            return Err(EngineSearchSessionError::Engine(Arc::new(
                EngineSessionError::TerminalPosition,
            )));
        }
        let limit = self
            .engine
            .applied_options()
            .ok_or(EngineSearchSessionError::ResourcesRequired)?
            .multi_pv()
            .get();
        let count =
            u32::try_from(count).map_err(|_conversion| EngineSearchSessionError::CandidateRank)?;
        let limit =
            NonZeroU32::new(limit.min(count)).ok_or(EngineSearchSessionError::CandidateRank)?;
        let started_at = Instant::now();
        let analysis = EngineSearchSessionAnalysis::new(limit, started_at)?;
        let tag = EngineSearchTag::fresh(self.engine.generation(), request.context);
        self.metadata = Some(EngineSearchSessionMetadata {
            tag,
            position: request.position.clone(),
            limit: request.limit,
            started_at,
        });
        self.lease = Some(request.lease);
        self.analysis = Some(analysis);
        self.cancellation = EngineSearchSessionEnumCancellation::NotRequested;
        self.terminal = None;
        self.terminal_delivered = false;
        self.pending_signal = None;
        match self
            .engine
            .start_search_tagged(tag, request.position, request.limit)
            .await
        {
            Ok(tag) => Ok(tag),
            Err(error) => {
                let error = EngineSearchSessionError::Engine(Arc::new(error));
                self.finish(EngineSearchSessionEnumCompletion::Failed(Arc::new(
                    error.clone(),
                )))?;
                Err(error)
            }
        }
    }

    fn validate(&self, tag: EngineSearchTag) -> Result<(), EngineSearchSessionError> {
        if self
            .metadata
            .as_ref()
            .is_none_or(|metadata| metadata.tag != tag)
        {
            return Err(EngineSearchSessionError::StaleSearch);
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<EngineSearchSessionSnapshot, EngineSearchSessionError> {
        Ok(self
            .analysis
            .as_ref()
            .ok_or(EngineSearchSessionError::SetupInterrupted)?
            .snapshot(
                self.metadata
                    .as_ref()
                    .ok_or(EngineSearchSessionError::StaleSearch)?,
                self.cancellation,
            ))
    }

    fn release_observed_capacity(&mut self) {
        if self.engine.is_shutdown_complete() || self.engine.state() == EngineSearchEnumState::Idle
        {
            self.lease = None;
        }
    }

    fn finish(
        &mut self,
        completion: EngineSearchSessionEnumCompletion,
    ) -> Result<EngineSearchSessionTerminal, EngineSearchSessionError> {
        self.release_observed_capacity();
        let terminal = EngineSearchSessionTerminal {
            snapshot: self.snapshot()?,
            completion,
        };
        self.terminal = Some(terminal.clone());
        self.terminal_delivered = false;
        Ok(terminal)
    }

    fn deliver(&mut self, terminal: EngineSearchSessionTerminal) -> EngineSearchSessionEnumEvent {
        self.terminal_delivered = true;
        EngineSearchSessionEnumEvent::Finished(terminal)
    }

    fn interrupted_failure(
        &mut self,
    ) -> Result<Option<EngineSearchSessionTerminal>, EngineSearchSessionError> {
        self.engine
            .take_pending_failure()
            .map(|error| {
                self.finish(EngineSearchSessionEnumCompletion::Failed(Arc::new(
                    EngineSearchSessionError::Engine(Arc::new(error)),
                )))
            })
            .transpose()
    }

    fn due_snapshot(
        &mut self,
    ) -> Result<Option<EngineSearchSessionSnapshot>, EngineSearchSessionError> {
        if self
            .analysis
            .as_ref()
            .and_then(EngineSearchSessionAnalysis::publication_deadline)
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            let snapshot = self.snapshot()?;
            self.analysis
                .as_mut()
                .ok_or(EngineSearchSessionError::SetupInterrupted)?
                .published()?;
            Ok(Some(snapshot))
        } else {
            Ok(None)
        }
    }

    fn deliver_signal(
        &mut self,
        signal: EngineSearchSessionEnumEvent,
    ) -> Result<EngineSearchSessionEnumEvent, EngineSearchSessionError> {
        if let Some(snapshot) = self.due_snapshot()? {
            self.pending_signal = Some(signal);
            Ok(EngineSearchSessionEnumEvent::Snapshot(snapshot))
        } else {
            Ok(signal)
        }
    }

    async fn wait_publication(deadline: Option<Instant>) {
        match deadline {
            Some(deadline) => sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    }

    pub async fn next_event(
        &mut self,
        expected: EngineSearchTag,
    ) -> Result<EngineSearchSessionEnumEvent, EngineSearchSessionError> {
        self.validate(expected)?;
        if let Some(terminal) = &self.terminal {
            if self.terminal_delivered {
                return Err(EngineSearchSessionError::CompletionObserved);
            }
            return Ok(self.deliver(terminal.clone()));
        }
        if let Some(terminal) = self.interrupted_failure()? {
            return Ok(self.deliver(terminal));
        }
        if let Some(signal) = self.pending_signal.take() {
            return Ok(signal);
        }
        if self.engine.state() == EngineSearchEnumState::Preparing {
            return Err(EngineSearchSessionError::SetupInterrupted);
        }
        let mut messages = 0;
        loop {
            let deadline = self
                .analysis
                .as_ref()
                .and_then(EngineSearchSessionAnalysis::publication_deadline);
            let event = tokio::select! {
                biased;
                event = self.engine.next_event() => Some(event),
                () = Self::wait_publication(deadline) => None,
            };
            if let Some(event) = event {
                match event {
                    Ok(event) => {
                        self.validate(event.tag())?;
                        match event.into_output() {
                            EngineSearchEnumOutput::Info(analysis) => {
                                if analysis.line().is_none() {
                                    return self.deliver_signal(
                                        EngineSearchSessionEnumEvent::Diagnostic {
                                            tag: expected,
                                            pipe: EngineProcessEnumPipe::Stdout,
                                            text: analysis.into_text(),
                                        },
                                    );
                                }
                                if let Err(operation) = self
                                    .analysis
                                    .as_mut()
                                    .ok_or(EngineSearchSessionError::SetupInterrupted)?
                                    .accept(analysis)
                                {
                                    let operation = Arc::new(operation);
                                    self.finish(EngineSearchSessionEnumCompletion::Failed(
                                        Arc::clone(&operation),
                                    ))?;
                                    let cleanup = self
                                        .engine
                                        .shutdown_retained(
                                            Instant::now() + EngineProcessLimits::SHUTDOWN,
                                        )
                                        .await;
                                    let error = match cleanup {
                                        Ok(()) => (*operation).clone(),
                                        Err(cleanup) => EngineSearchSessionError::Cleanup {
                                            operation,
                                            cleanup: Arc::new(cleanup),
                                        },
                                    };
                                    let terminal = self.finish(
                                        EngineSearchSessionEnumCompletion::Failed(Arc::new(error)),
                                    )?;
                                    return Ok(self.deliver(terminal));
                                }
                            }
                            EngineSearchEnumOutput::Readiness => {
                                return self.deliver_signal(
                                    EngineSearchSessionEnumEvent::Readiness { tag: expected },
                                );
                            }
                            EngineSearchEnumOutput::Diagnostic { pipe, text } => {
                                return self.deliver_signal(
                                    EngineSearchSessionEnumEvent::Diagnostic {
                                        tag: expected,
                                        pipe,
                                        text,
                                    },
                                );
                            }
                            EngineSearchEnumOutput::BestMove(movement) => {
                                let terminal = self.finish(
                                    EngineSearchSessionEnumCompletion::BestMove(movement),
                                )?;
                                return Ok(self.deliver(terminal));
                            }
                            EngineSearchEnumOutput::Stopped => {
                                let terminal =
                                    self.finish(EngineSearchSessionEnumCompletion::Stopped)?;
                                return Ok(self.deliver(terminal));
                            }
                        }
                    }
                    Err(error) => {
                        let terminal = self.finish(EngineSearchSessionEnumCompletion::Failed(
                            Arc::new(EngineSearchSessionError::Engine(Arc::new(error))),
                        ))?;
                        return Ok(self.deliver(terminal));
                    }
                }
            } else if let Some(terminal) = self.interrupted_failure()? {
                return Ok(self.deliver(terminal));
            }
            if let Some(snapshot) = self.due_snapshot()? {
                return Ok(EngineSearchSessionEnumEvent::Snapshot(snapshot));
            }
            messages += 1;
            if messages == Self::MESSAGE_BATCH {
                tokio::task::yield_now().await;
                messages = 0;
            }
        }
    }

    pub async fn stop(
        &mut self,
        request: EngineSearchStop,
    ) -> Result<EngineSearchSessionTerminal, EngineSearchSessionError> {
        self.validate(request.tag())?;
        if let Some(terminal) = &self.terminal {
            let terminal = terminal.clone();
            self.terminal_delivered = true;
            return Ok(terminal);
        }
        let request = match self.cancellation {
            EngineSearchSessionEnumCancellation::StopRequested(existing)
                if existing.deadline() < request.deadline() =>
            {
                existing
            }
            EngineSearchSessionEnumCancellation::NotRequested
            | EngineSearchSessionEnumCancellation::StopRequested(_)
            | EngineSearchSessionEnumCancellation::ShutdownRequested => request,
        };
        self.cancellation = EngineSearchSessionEnumCancellation::StopRequested(request);
        if self.engine.state() == EngineSearchEnumState::Preparing {
            self.shutdown_retained(request.termination_deadline())
                .await?;
            return self
                .terminal
                .clone()
                .ok_or(EngineSearchSessionError::SetupInterrupted);
        }
        if let Some(terminal) = self.interrupted_failure()? {
            self.terminal_delivered = true;
            return Ok(terminal);
        }
        if let Err(error) = self.engine.request_stop(request).await {
            let terminal = self.finish(EngineSearchSessionEnumCompletion::Failed(Arc::new(
                EngineSearchSessionError::Engine(Arc::new(error)),
            )))?;
            self.terminal_delivered = true;
            return Ok(terminal);
        }
        loop {
            match self.next_event(request.tag()).await? {
                EngineSearchSessionEnumEvent::Finished(terminal) => return Ok(terminal),
                EngineSearchSessionEnumEvent::Snapshot(_)
                | EngineSearchSessionEnumEvent::Readiness { .. }
                | EngineSearchSessionEnumEvent::Diagnostic { .. } => {}
            }
        }
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), EngineSearchSessionError> {
        if self.cancellation == EngineSearchSessionEnumCancellation::NotRequested {
            self.cancellation = EngineSearchSessionEnumCancellation::ShutdownRequested;
        }
        let deadline = match self.cancellation {
            EngineSearchSessionEnumCancellation::StopRequested(request) if self.lease.is_some() => {
                deadline.min(request.termination_deadline())
            }
            EngineSearchSessionEnumCancellation::NotRequested
            | EngineSearchSessionEnumCancellation::StopRequested(_)
            | EngineSearchSessionEnumCancellation::ShutdownRequested => deadline,
        };
        if self.metadata.is_some() {
            self.interrupted_failure()?;
        }
        let cleanup = self.engine.shutdown_retained(deadline).await;
        self.release_observed_capacity();
        match cleanup {
            Ok(()) => {
                if self.metadata.is_some() && self.terminal.is_none() {
                    self.finish(EngineSearchSessionEnumCompletion::Cancelled)?;
                }
                Ok(())
            }
            Err(cleanup) => {
                let cleanup = Arc::new(cleanup);
                let error = match self
                    .terminal
                    .as_ref()
                    .map(EngineSearchSessionTerminal::completion)
                {
                    Some(EngineSearchSessionEnumCompletion::Failed(operation)) => {
                        EngineSearchSessionError::Cleanup {
                            operation: Arc::clone(operation),
                            cleanup,
                        }
                    }
                    Some(
                        EngineSearchSessionEnumCompletion::BestMove(_)
                        | EngineSearchSessionEnumCompletion::Stopped
                        | EngineSearchSessionEnumCompletion::Cancelled,
                    )
                    | None => EngineSearchSessionError::Engine(cleanup),
                };
                if self.metadata.is_some()
                    && self.terminal.as_ref().is_none_or(|terminal| {
                        matches!(
                            terminal.completion(),
                            EngineSearchSessionEnumCompletion::Failed(_)
                        )
                    })
                {
                    self.finish(EngineSearchSessionEnumCompletion::Failed(Arc::new(
                        error.clone(),
                    )))?;
                }
                Err(error)
            }
        }
    }
}
