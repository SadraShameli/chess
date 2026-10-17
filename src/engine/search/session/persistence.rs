use std::error::Error;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::engine::capabilities::persistence::{
    EnginePersistenceError, EngineRecordJson, EngineRecordList, EngineRecordText,
};
use crate::engine::process_provider::{EngineProcessEnumPipe, EngineProcessError};
use crate::engine::search::enums::EngineSearchEnumState;
use crate::engine::search::session::EngineSearchSessionError;
use crate::engine::session::EngineSessionError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineSearchSessionFailureEnumCategory {
    Busy,
    ResourcesRequired,
    ThreadLeaseMismatch,
    StaleSearch,
    SetupInterrupted,
    CompletionObserved,
    CandidateRank,
    Deadline,
    Engine,
    Cleanup,
}

impl EngineSearchSessionFailureEnumCategory {
    fn capture(error: &EngineSearchSessionError) -> Self {
        match error {
            EngineSearchSessionError::Busy => Self::Busy,
            EngineSearchSessionError::ResourcesRequired => Self::ResourcesRequired,
            EngineSearchSessionError::ThreadLeaseMismatch { .. } => Self::ThreadLeaseMismatch,
            EngineSearchSessionError::StaleSearch => Self::StaleSearch,
            EngineSearchSessionError::SetupInterrupted => Self::SetupInterrupted,
            EngineSearchSessionError::CompletionObserved => Self::CompletionObserved,
            EngineSearchSessionError::CandidateRank => Self::CandidateRank,
            EngineSearchSessionError::Deadline => Self::Deadline,
            EngineSearchSessionError::Engine(_) => Self::Engine,
            EngineSearchSessionError::Cleanup { .. } => Self::Cleanup,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineSearchSessionFailureEnumEngine {
    Options,
    Analysis,
    Process,
    Capabilities,
    ResourceConfig,
    Timeout,
    UnexpectedResponse,
    Cleanup,
    State,
    TerminalPosition,
    StopDeadline,
    AmbiguousStopDelivery,
    StaleSearch,
    SearchResponse,
    SearchMove,
    OperationCleanup,
    ShutdownUnobserved,
    ShutdownFailed,
    Diagnosed,
    StartupUnavailable,
}

impl EngineSearchSessionFailureEnumEngine {
    fn capture(error: &EngineSessionError) -> Self {
        match error {
            EngineSessionError::Options(_) => Self::Options,
            EngineSessionError::Analysis(_) => Self::Analysis,
            EngineSessionError::Process(_) => Self::Process,
            EngineSessionError::Capabilities(_) => Self::Capabilities,
            EngineSessionError::ResourceConfig(_) => Self::ResourceConfig,
            EngineSessionError::Timeout(_) => Self::Timeout,
            EngineSessionError::UnexpectedResponse => Self::UnexpectedResponse,
            EngineSessionError::Cleanup { .. } => Self::Cleanup,
            EngineSessionError::State(_) => Self::State,
            EngineSessionError::TerminalPosition => Self::TerminalPosition,
            EngineSessionError::StopDeadline => Self::StopDeadline,
            EngineSessionError::AmbiguousStopDelivery => Self::AmbiguousStopDelivery,
            EngineSessionError::StaleSearch => Self::StaleSearch,
            EngineSessionError::SearchResponse => Self::SearchResponse,
            EngineSessionError::SearchMove(_) => Self::SearchMove,
            EngineSessionError::OperationCleanup { .. } => Self::OperationCleanup,
            EngineSessionError::ShutdownUnobserved => Self::ShutdownUnobserved,
            EngineSessionError::ShutdownFailed(_) => Self::ShutdownFailed,
            EngineSessionError::Diagnosed { .. } => Self::Diagnosed,
            EngineSessionError::StartupUnavailable => Self::StartupUnavailable,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "EngineProcessEnumPipe", rename_all = "snake_case")]
enum EngineSearchSessionFailureEnumPipe {
    Stdout,
    Stderr,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "EngineSearchEnumState", rename_all = "snake_case")]
enum EngineSearchSessionFailureEnumState {
    Idle,
    Preparing,
    Searching,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum EngineSearchSessionFailureEnumProcess {
    Io {
        raw_os_error: Option<i32>,
    },
    MissingPipe,
    Closed {
        #[serde(with = "EngineSearchSessionFailureEnumPipe")]
        pipe: EngineProcessEnumPipe,
    },
    OversizedLine,
    InvalidText {
        valid_up_to: u64,
        error_len: Option<u64>,
    },
    InvalidCommand,
    ReaderTaskCancelled,
    ReaderTaskPanicked,
    ShutdownTimeout,
    ShutdownDeadline,
    ShutdownFailed,
    Exit {
        code: Option<i32>,
    },
    Cleanup,
}

impl EngineSearchSessionFailureEnumProcess {
    fn validate(&self) -> Result<(), EnginePersistenceError> {
        let valid = match self {
            Self::InvalidText { error_len, .. } => {
                error_len.is_none_or(|length| (1..=3).contains(&length))
            }
            Self::Io { .. }
            | Self::MissingPipe
            | Self::Closed { .. }
            | Self::OversizedLine
            | Self::InvalidCommand
            | Self::ReaderTaskCancelled
            | Self::ReaderTaskPanicked
            | Self::ShutdownTimeout
            | Self::ShutdownDeadline
            | Self::ShutdownFailed
            | Self::Exit { .. }
            | Self::Cleanup => true,
        };
        if !valid {
            return Err(EnginePersistenceError::Context);
        }
        Ok(())
    }

    fn capture(error: &EngineProcessError) -> Result<Self, EnginePersistenceError> {
        Ok(match error {
            EngineProcessError::Io(error) => Self::Io {
                raw_os_error: error.raw_os_error(),
            },
            EngineProcessError::MissingPipe => Self::MissingPipe,
            EngineProcessError::Closed(pipe) => Self::Closed { pipe: *pipe },
            EngineProcessError::OversizedLine => Self::OversizedLine,
            EngineProcessError::InvalidText(error) => Self::InvalidText {
                valid_up_to: u64::try_from(error.utf8_error().valid_up_to())?,
                error_len: error
                    .utf8_error()
                    .error_len()
                    .map(u64::try_from)
                    .transpose()?,
            },
            EngineProcessError::InvalidCommand => Self::InvalidCommand,
            EngineProcessError::ReaderTask(error) => {
                if error.is_cancelled() {
                    Self::ReaderTaskCancelled
                } else if error.is_panic() {
                    Self::ReaderTaskPanicked
                } else {
                    return Err(EnginePersistenceError::Context);
                }
            }
            EngineProcessError::ShutdownTimeout => Self::ShutdownTimeout,
            EngineProcessError::ShutdownDeadline(_) => Self::ShutdownDeadline,
            EngineProcessError::ShutdownFailed(_) => Self::ShutdownFailed,
            EngineProcessError::Exit(status) => Self::Exit {
                code: status.code(),
            },
            EngineProcessError::Cleanup { .. } => Self::Cleanup,
        })
    }
}

impl Serialize for EngineSearchSessionFailureEnumProcess {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for EngineSearchSessionFailureEnumProcess {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let process = Self::deserialize(deserializer)?;
        process.validate().map_err(serde::de::Error::custom)?;
        Ok(process)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct EngineSearchSessionFailureEngineContext {
    category: EngineSearchSessionFailureEnumEngine,
    process: Option<EngineSearchSessionFailureEnumProcess>,
    phase: Option<EngineRecordText<256>>,
    #[serde(with = "EngineSearchSessionFailureOptionalState")]
    state: Option<EngineSearchEnumState>,
}

struct EngineSearchSessionFailureOptionalState;

impl EngineSearchSessionFailureOptionalState {
    fn serialize<S: Serializer>(
        state: &Option<EngineSearchEnumState>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct EngineSearchSessionFailureStateRef(
            #[serde(with = "EngineSearchSessionFailureEnumState")] EngineSearchEnumState,
        );
        state
            .map(EngineSearchSessionFailureStateRef)
            .serialize(serializer)
    }

    fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<EngineSearchEnumState>, D::Error> {
        #[derive(Deserialize)]
        struct EngineSearchSessionFailureState(
            #[serde(with = "EngineSearchSessionFailureEnumState")] EngineSearchEnumState,
        );
        Option::<EngineSearchSessionFailureState>::deserialize(deserializer)
            .map(|state| state.map(|state| state.0))
    }
}

impl EngineSearchSessionFailureEngineContext {
    fn capture(error: &EngineSessionError) -> Result<Self, EnginePersistenceError> {
        let mut context = Self {
            category: EngineSearchSessionFailureEnumEngine::capture(error),
            process: None,
            phase: None,
            state: None,
        };
        let mut current = error;
        for _depth in 0..EngineSearchSessionFailureRecord::MAX_SOURCES {
            match current {
                EngineSessionError::Process(process) => {
                    context.process =
                        Some(EngineSearchSessionFailureEnumProcess::capture(process)?);
                    return Ok(context);
                }
                EngineSessionError::Timeout(phase) => {
                    context.phase = Some(EngineRecordText::new(phase)?);
                    return Ok(context);
                }
                EngineSessionError::State(state) => {
                    context.state = Some(*state);
                    return Ok(context);
                }
                EngineSessionError::Cleanup { startup, .. } => current = startup,
                EngineSessionError::OperationCleanup { operation, .. }
                | EngineSessionError::Diagnosed { operation, .. } => current = operation,
                EngineSessionError::ShutdownFailed(operation) => current = operation,
                EngineSessionError::Options(_)
                | EngineSessionError::Analysis(_)
                | EngineSessionError::Capabilities(_)
                | EngineSessionError::ResourceConfig(_)
                | EngineSessionError::UnexpectedResponse
                | EngineSessionError::TerminalPosition
                | EngineSessionError::StopDeadline
                | EngineSessionError::AmbiguousStopDelivery
                | EngineSessionError::StaleSearch
                | EngineSessionError::SearchResponse
                | EngineSessionError::SearchMove(_)
                | EngineSessionError::ShutdownUnobserved
                | EngineSessionError::StartupUnavailable => return Ok(context),
            }
        }
        Err(EnginePersistenceError::Bounds)
    }

    fn validate(&self) -> Result<(), EnginePersistenceError> {
        if let Some(process) = &self.process {
            process.validate()?;
        }
        let valid = match self.category {
            EngineSearchSessionFailureEnumEngine::Process => {
                self.process.is_some() && self.phase.is_none() && self.state.is_none()
            }
            EngineSearchSessionFailureEnumEngine::Timeout => {
                self.process.is_none() && self.phase.is_some() && self.state.is_none()
            }
            EngineSearchSessionFailureEnumEngine::State => {
                self.process.is_none() && self.phase.is_none() && self.state.is_some()
            }
            EngineSearchSessionFailureEnumEngine::Cleanup
            | EngineSearchSessionFailureEnumEngine::OperationCleanup
            | EngineSearchSessionFailureEnumEngine::ShutdownFailed
            | EngineSearchSessionFailureEnumEngine::Diagnosed => {
                usize::from(self.process.is_some())
                    + usize::from(self.phase.is_some())
                    + usize::from(self.state.is_some())
                    <= 1
            }
            EngineSearchSessionFailureEnumEngine::Options
            | EngineSearchSessionFailureEnumEngine::Analysis
            | EngineSearchSessionFailureEnumEngine::Capabilities
            | EngineSearchSessionFailureEnumEngine::ResourceConfig
            | EngineSearchSessionFailureEnumEngine::UnexpectedResponse
            | EngineSearchSessionFailureEnumEngine::TerminalPosition
            | EngineSearchSessionFailureEnumEngine::StopDeadline
            | EngineSearchSessionFailureEnumEngine::AmbiguousStopDelivery
            | EngineSearchSessionFailureEnumEngine::StaleSearch
            | EngineSearchSessionFailureEnumEngine::SearchResponse
            | EngineSearchSessionFailureEnumEngine::SearchMove
            | EngineSearchSessionFailureEnumEngine::ShutdownUnobserved
            | EngineSearchSessionFailureEnumEngine::StartupUnavailable => {
                self.process.is_none() && self.phase.is_none() && self.state.is_none()
            }
        };
        if !valid {
            return Err(EnginePersistenceError::Context);
        }
        Ok(())
    }

    pub const fn category(&self) -> EngineSearchSessionFailureEnumEngine {
        self.category
    }
    pub const fn process(&self) -> Option<EngineSearchSessionFailureEnumProcess> {
        self.process
    }
    pub fn phase(&self) -> Option<&str> {
        self.phase.as_ref().map(EngineRecordText::as_str)
    }
    pub const fn state(&self) -> Option<EngineSearchEnumState> {
        self.state
    }
}

impl Serialize for EngineSearchSessionFailureEngineContext {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for EngineSearchSessionFailureEngineContext {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let context = Self::deserialize(deserializer)?;
        context.validate().map_err(serde::de::Error::custom)?;
        Ok(context)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineSearchSessionFailureThreadLease {
    threads: u32,
    workers: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineSearchSessionFailureRecordData {
    category: EngineSearchSessionFailureEnumCategory,
    message: EngineRecordText<{ EngineSearchSessionFailureRecord::MAX_MESSAGE_BYTES }>,
    sources: EngineRecordList<
        EngineRecordText<{ EngineSearchSessionFailureRecord::MAX_MESSAGE_BYTES }>,
        { EngineSearchSessionFailureRecord::MAX_SOURCES },
    >,
    thread_lease: Option<EngineSearchSessionFailureThreadLease>,
    engine: Option<EngineSearchSessionFailureEngineContext>,
    operation_category: Option<EngineSearchSessionFailureEnumCategory>,
    cleanup: Option<EngineSearchSessionFailureEngineContext>,
    cleanup_message:
        Option<EngineRecordText<{ EngineSearchSessionFailureRecord::MAX_MESSAGE_BYTES }>>,
    cleanup_sources: EngineRecordList<
        EngineRecordText<{ EngineSearchSessionFailureRecord::MAX_MESSAGE_BYTES }>,
        { EngineSearchSessionFailureRecord::MAX_SOURCES },
    >,
}

impl EngineSearchSessionFailureRecordData {
    fn validate(&self) -> Result<(), EnginePersistenceError> {
        if self.message.as_str().is_empty()
            || self
                .cleanup_message
                .as_ref()
                .is_some_and(|message| message.as_str().is_empty())
        {
            return Err(EnginePersistenceError::Context);
        }
        if let Some(engine) = &self.engine {
            engine.validate()?;
        }
        if let Some(cleanup) = &self.cleanup {
            cleanup.validate()?;
        }
        let valid = match self.category {
            EngineSearchSessionFailureEnumCategory::ThreadLeaseMismatch => {
                self.thread_lease.is_some_and(|lease| {
                    lease.threads > 0
                        && lease.workers > 0
                        && u64::from(lease.threads) != lease.workers
                }) && self.engine.is_none()
                    && self.cleanup.is_none()
                    && self.operation_category.is_none()
                    && self.cleanup_message.is_none()
            }
            EngineSearchSessionFailureEnumCategory::Engine => {
                self.thread_lease.is_none()
                    && self.engine.is_some()
                    && self.cleanup.is_none()
                    && self.operation_category.is_none()
                    && self.cleanup_message.is_none()
            }
            EngineSearchSessionFailureEnumCategory::Cleanup => {
                self.thread_lease.is_none()
                    && self.engine.is_none()
                    && self.cleanup.is_some()
                    && self.operation_category.is_some()
                    && self.cleanup_message.is_some()
            }
            EngineSearchSessionFailureEnumCategory::Busy
            | EngineSearchSessionFailureEnumCategory::ResourcesRequired
            | EngineSearchSessionFailureEnumCategory::StaleSearch
            | EngineSearchSessionFailureEnumCategory::SetupInterrupted
            | EngineSearchSessionFailureEnumCategory::CompletionObserved
            | EngineSearchSessionFailureEnumCategory::CandidateRank
            | EngineSearchSessionFailureEnumCategory::Deadline => {
                self.thread_lease.is_none()
                    && self.engine.is_none()
                    && self.cleanup.is_none()
                    && self.operation_category.is_none()
                    && self.cleanup_message.is_none()
            }
        };
        if !valid {
            return Err(EnginePersistenceError::Context);
        }
        if self.category != EngineSearchSessionFailureEnumCategory::Cleanup
            && !self.cleanup_sources.as_slice().is_empty()
        {
            return Err(EnginePersistenceError::Context);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSearchSessionFailureRecord {
    data: EngineSearchSessionFailureRecordData,
}

impl EngineSearchSessionFailureRecord {
    pub const MAX_BYTES: usize = 1024 * 1024;
    pub const MAX_MESSAGE_BYTES: usize = 4096;
    pub const MAX_SOURCES: usize = 16;

    pub fn capture(error: &EngineSearchSessionError) -> Result<Self, EnginePersistenceError> {
        let mut data = EngineSearchSessionFailureRecordData {
            category: EngineSearchSessionFailureEnumCategory::capture(error),
            message: EngineRecordText::new("")?,
            sources: Self::source_chain(error)?,
            thread_lease: None,
            engine: None,
            operation_category: None,
            cleanup: None,
            cleanup_message: None,
            cleanup_sources: EngineRecordList::new(),
        };
        match error {
            EngineSearchSessionError::ThreadLeaseMismatch { threads, workers } => {
                data.thread_lease = Some(EngineSearchSessionFailureThreadLease {
                    threads: *threads,
                    workers: u64::try_from(*workers)?,
                })
            }
            EngineSearchSessionError::Engine(engine) => {
                data.engine = Some(EngineSearchSessionFailureEngineContext::capture(engine)?)
            }
            EngineSearchSessionError::Cleanup { operation, cleanup } => {
                data.operation_category =
                    Some(EngineSearchSessionFailureEnumCategory::capture(operation));
                data.cleanup = Some(EngineSearchSessionFailureEngineContext::capture(cleanup)?);
                data.cleanup_sources = Self::source_chain(cleanup.as_ref())?;
                data.cleanup_message = Some(EngineRecordText::display(cleanup)?);
            }
            EngineSearchSessionError::Busy
            | EngineSearchSessionError::ResourcesRequired
            | EngineSearchSessionError::StaleSearch
            | EngineSearchSessionError::SetupInterrupted
            | EngineSearchSessionError::CompletionObserved
            | EngineSearchSessionError::CandidateRank
            | EngineSearchSessionError::Deadline => {}
        }
        data.message = EngineRecordText::display(error)?;
        data.validate()?;
        Ok(Self { data })
    }

    fn source_chain(
        error: &impl Error,
    ) -> Result<
        EngineRecordList<EngineRecordText<{ Self::MAX_MESSAGE_BYTES }>, { Self::MAX_SOURCES }>,
        EnginePersistenceError,
    > {
        let mut causes = EngineRecordList::<_, { Self::MAX_SOURCES }>::new();
        let mut source = error.source();
        while let Some(cause) = source {
            causes.push(cause)?;
            source = cause.source();
        }
        let mut sources = EngineRecordList::new();
        for cause in causes.as_slice() {
            sources.push(EngineRecordText::display(cause)?)?;
        }
        Ok(sources)
    }

    pub const fn category(&self) -> EngineSearchSessionFailureEnumCategory {
        self.data.category
    }
    pub fn message(&self) -> &str {
        self.data.message.as_str()
    }
    pub fn sources(&self) -> impl ExactSizeIterator<Item = &str> {
        self.data
            .sources
            .as_slice()
            .iter()
            .map(EngineRecordText::as_str)
    }
    pub fn thread_lease(&self) -> Option<(u32, u64)> {
        self.data
            .thread_lease
            .map(|lease| (lease.threads, lease.workers))
    }
    pub const fn engine_context(&self) -> Option<&EngineSearchSessionFailureEngineContext> {
        self.data.engine.as_ref()
    }
    pub const fn operation_category(&self) -> Option<EngineSearchSessionFailureEnumCategory> {
        self.data.operation_category
    }
    pub const fn cleanup_context(&self) -> Option<&EngineSearchSessionFailureEngineContext> {
        self.data.cleanup.as_ref()
    }
    pub fn cleanup_message(&self) -> Option<&str> {
        self.data
            .cleanup_message
            .as_ref()
            .map(EngineRecordText::as_str)
    }
    pub fn cleanup_sources(&self) -> impl ExactSizeIterator<Item = &str> {
        self.data
            .cleanup_sources
            .as_slice()
            .iter()
            .map(EngineRecordText::as_str)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, EnginePersistenceError> {
        EngineRecordJson::parse::<Self, { Self::MAX_BYTES }>(bytes)
    }
}

impl Serialize for EngineSearchSessionFailureRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EngineRecordJson::serialize(&self.data, serializer)
    }
}

impl<'de> Deserialize<'de> for EngineSearchSessionFailureRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let record = Self {
            data: EngineRecordJson::deserialize::<
                D,
                EngineSearchSessionFailureRecordData,
                { Self::MAX_BYTES },
            >(deserializer)?,
        };
        record.data.validate().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}
