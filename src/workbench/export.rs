use std::error::Error;

use serde::{Serialize, Serializer};
use thiserror::Error;
use uuid::Uuid;

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::board::position::BoardPosition;
use crate::clock::duration::{ClockDuration, ClockDurationError};
use crate::engine::capabilities::EngineIdentity;
use crate::engine::diagnostics::EngineDiagnostics;
use crate::engine::options::{EngineOptionsApplied, EngineOptionsEnumTablebases};
use crate::engine::package::manifest::{
    EnginePackageEnumProfile, EnginePackageEnumTarget, EnginePackageManifest,
};
use crate::engine::process_provider::EngineProcessEnumPipe;
use crate::engine::search::position::EngineSearchPosition;
use crate::package::digest::{PackageDigest, PackageDigestError};
use crate::workbench::enums::{
    WorkbenchEnumCancellation, WorkbenchEnumCompletion, WorkbenchEnumJobState,
    WorkbenchEnumOutcome, WorkbenchEnumParameters, WorkbenchEnumResult,
};
use crate::workbench::job::{WorkbenchJob, WorkbenchJobFailure, WorkbenchJobId};

#[derive(Debug, Error)]
pub enum WorkbenchExportError {
    #[error("The {0:?} workbench job has no final exportable outcome")]
    Active(WorkbenchEnumJobState),
    #[error("The captured child provenance belongs to another workbench job or package")]
    ContextMismatch,
    #[error(
        "Export metadata must contain a bounded nonempty application version and non-nil export identity"
    )]
    Metadata,
    #[error(transparent)]
    Time(#[from] ClockDurationError),
    #[error("Cannot encode the diagnostic export: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error(transparent)]
    Digest(#[from] PackageDigestError),
}

#[derive(Debug, Error)]
pub enum WorkbenchExportWriteError<E: Error + 'static> {
    #[error(transparent)]
    Export(#[from] WorkbenchExportError),
    #[error("The diagnostic export was cancelled before commit")]
    Cancelled,
    #[error("The diagnostic export destination failed: {0}")]
    Destination(#[source] E),
}

pub trait WorkbenchExportPort {
    type Error: Error + 'static;

    fn is_cancelled(&self) -> bool;
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
    fn commit(&mut self) -> Result<(), Self::Error>;
    fn abort(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WorkbenchExportReceipt {
    bytes: WorkbenchExportDecimal,
    sha256: PackageDigest,
}

impl WorkbenchExportReceipt {
    pub const fn bytes(self) -> u64 {
        self.bytes.0
    }
    pub const fn sha256(self) -> PackageDigest {
        self.sha256
    }
}

#[derive(Debug, Default, Serialize)]
pub struct WorkbenchExportMetadata {
    export_id: Option<Uuid>,
    created_unix_milliseconds: Option<WorkbenchExportDecimal>,
    application_version: Option<Box<str>>,
}

impl WorkbenchExportMetadata {
    pub fn new(
        export_id: Uuid,
        created_unix_milliseconds: u64,
        application_version: String,
    ) -> Result<Self, WorkbenchExportError> {
        if export_id.is_nil()
            || application_version.trim().is_empty()
            || application_version.len() > 256
            || application_version.chars().any(char::is_control)
        {
            return Err(WorkbenchExportError::Metadata);
        }
        Ok(Self {
            export_id: Some(export_id),
            created_unix_milliseconds: Some(WorkbenchExportDecimal(created_unix_milliseconds)),
            application_version: Some(application_version.into_boxed_str()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkbenchExportDecimal(u64);

impl Serialize for WorkbenchExportDecimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum WorkbenchExportEnumObservation {
    Unobserved,
    Observed {
        identity: WorkbenchExportIdentity,
        configuration: WorkbenchExportConfiguration,
    },
}

#[derive(Debug, Clone, Serialize)]
struct WorkbenchExportIdentity {
    name: Box<str>,
    author: Box<str>,
    capability_version: u32,
}

#[derive(Debug, Clone, Serialize)]
struct WorkbenchExportConfiguration {
    resources: Option<WorkbenchExportResources>,
    variant: BoardEnumVariant,
    minimal: bool,
    multi_pv: u32,
    tablebases: WorkbenchExportEnumTablebases,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct WorkbenchExportResources {
    threads: u32,
    hash_mib: u32,
    move_overhead_milliseconds: WorkbenchExportDecimal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum WorkbenchExportEnumTablebases {
    Unconfigured,
    Loaded { engine_path: Box<str>, pieces: u32 },
    Unavailable { engine_path: Box<str> },
}

#[derive(Debug, Clone)]
pub struct WorkbenchExportContext {
    job_id: WorkbenchJobId,
    package_sha256: PackageDigest,
    observation: WorkbenchExportEnumObservation,
}

impl WorkbenchExportContext {
    pub fn unobserved(job: &WorkbenchJob) -> Self {
        Self {
            job_id: job.id(),
            package_sha256: job.package().digest(),
            observation: WorkbenchExportEnumObservation::Unobserved,
        }
    }

    pub fn observed(
        job: &WorkbenchJob,
        identity: &EngineIdentity,
        applied: &EngineOptionsApplied,
    ) -> Self {
        let tablebases = match applied.tablebases() {
            EngineOptionsEnumTablebases::Unconfigured => {
                WorkbenchExportEnumTablebases::Unconfigured
            }
            EngineOptionsEnumTablebases::Loaded { path, pieces } => {
                WorkbenchExportEnumTablebases::Loaded {
                    engine_path: path.as_str().into(),
                    pieces: pieces.get(),
                }
            }
            EngineOptionsEnumTablebases::Unavailable { path } => {
                WorkbenchExportEnumTablebases::Unavailable {
                    engine_path: path.as_str().into(),
                }
            }
        };
        Self {
            job_id: job.id(),
            package_sha256: job.package().digest(),
            observation: WorkbenchExportEnumObservation::Observed {
                identity: WorkbenchExportIdentity {
                    name: identity.name().into(),
                    author: identity.author().into(),
                    capability_version: identity.capabilities().version().get(),
                },
                configuration: WorkbenchExportConfiguration {
                    resources: applied
                        .resources()
                        .map(|resources| WorkbenchExportResources {
                            threads: resources.threads(),
                            hash_mib: resources.hash_mib(),
                            move_overhead_milliseconds: WorkbenchExportDecimal(
                                resources.move_overhead().as_millis(),
                            ),
                        }),
                    variant: applied.variant(),
                    minimal: applied.is_minimal(),
                    multi_pv: applied.multi_pv().get(),
                    tablebases,
                },
            },
        }
    }
}

#[derive(Serialize)]
struct WorkbenchExportProvenance {
    engine_version: &'static str,
    source_repository: &'static str,
    source_tag: &'static str,
    source_commit: &'static str,
    model_name: &'static str,
    model_bytes: WorkbenchExportDecimal,
    model_sha256: &'static str,
    target: EnginePackageEnumTarget,
    profile: EnginePackageEnumProfile,
    package_sha256: PackageDigest,
}

#[derive(Serialize)]
struct WorkbenchExportPosition {
    variant: BoardEnumVariant,
    initial_fen: String,
    moves_uci: Vec<String>,
    current_fen: String,
}

impl WorkbenchExportPosition {
    fn capture(position: &EngineSearchPosition) -> Self {
        let variant = position.initial().variant();
        Self {
            variant,
            initial_fen: position.initial().fen(),
            moves_uci: position
                .history()
                .iter()
                .map(|movement| movement.uci(variant))
                .collect(),
            current_fen: position.current().fen(),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum WorkbenchExportEnumParameters {
    Inspector {
        position: WorkbenchExportPosition,
    },
    RawEvaluation {
        position: WorkbenchExportPosition,
    },
    Compiler,
    Perft {
        position: WorkbenchExportPosition,
        depth_plies: u32,
    },
    Bench {
        hash_mib: u32,
        threads: u32,
        depth_plies: u32,
        positions: u32,
    },
}

impl WorkbenchExportEnumParameters {
    fn capture(parameters: &WorkbenchEnumParameters) -> Self {
        match parameters {
            WorkbenchEnumParameters::Inspector(position) => Self::Inspector {
                position: WorkbenchExportPosition::capture(position),
            },
            WorkbenchEnumParameters::RawEvaluation(position) => Self::RawEvaluation {
                position: WorkbenchExportPosition::capture(position),
            },
            WorkbenchEnumParameters::Compiler => Self::Compiler,
            WorkbenchEnumParameters::Perft(parameters) => Self::Perft {
                position: WorkbenchExportPosition::capture(parameters.position()),
                depth_plies: parameters.depth().value(),
            },
            WorkbenchEnumParameters::Bench(parameters) => Self::Bench {
                hash_mib: parameters.hash_mib(),
                threads: parameters.threads(),
                depth_plies: parameters.depth(),
                positions: crate::workbench::parameters::WorkbenchBenchParameters::POSITIONS,
            },
        }
    }
}

#[derive(Serialize)]
struct WorkbenchExportRoot {
    move_uci: String,
    nodes: WorkbenchExportDecimal,
}

#[derive(Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum WorkbenchExportEnumResult<'a> {
    Inspector {
        fen: String,
        variant: BoardEnumVariant,
        turn: BoardEnumColor,
        legal_moves_uci: Vec<String>,
    },
    RawEvaluation {
        raw_white_nnue: i32,
    },
    Compiler {
        compiler: &'a str,
        target: EnginePackageEnumTarget,
        target_features: &'a str,
    },
    Perft {
        fen: String,
        variant: BoardEnumVariant,
        depth_plies: u32,
        roots: Vec<WorkbenchExportRoot>,
        nodes: WorkbenchExportDecimal,
        elapsed_nanoseconds: WorkbenchExportDecimal,
        nodes_per_second: WorkbenchExportDecimal,
    },
    Bench {
        positions: u32,
        nodes: WorkbenchExportDecimal,
        elapsed_nanoseconds: WorkbenchExportDecimal,
        nodes_per_second: WorkbenchExportDecimal,
    },
}

impl<'a> WorkbenchExportEnumResult<'a> {
    fn capture(result: &'a WorkbenchEnumResult) -> Self {
        match result {
            WorkbenchEnumResult::Inspector(position) => Self::inspector(position),
            WorkbenchEnumResult::RawEvaluation(evaluation) => Self::RawEvaluation {
                raw_white_nnue: evaluation.white_value(),
            },
            WorkbenchEnumResult::Compiler(result) => Self::Compiler {
                compiler: result.compiler(),
                target: result.target(),
                target_features: result.target_features(),
            },
            WorkbenchEnumResult::Perft(result) => Self::Perft {
                fen: result.position().fen(),
                variant: result.position().variant(),
                depth_plies: result.depth().value(),
                roots: result
                    .roots()
                    .iter()
                    .map(|root| WorkbenchExportRoot {
                        move_uci: root.movement().uci(result.position().variant()),
                        nodes: WorkbenchExportDecimal(root.nodes()),
                    })
                    .collect(),
                nodes: WorkbenchExportDecimal(result.nodes()),
                elapsed_nanoseconds: WorkbenchExportDecimal(result.elapsed().as_nanos()),
                nodes_per_second: WorkbenchExportDecimal(result.nodes_per_second()),
            },
            WorkbenchEnumResult::Bench(result) => Self::Bench {
                positions: result.positions(),
                nodes: WorkbenchExportDecimal(result.nodes()),
                elapsed_nanoseconds: WorkbenchExportDecimal(result.elapsed().as_nanos()),
                nodes_per_second: WorkbenchExportDecimal(result.nodes_per_second()),
            },
        }
    }

    fn inspector(position: &BoardPosition) -> Self {
        Self::Inspector {
            fen: position.fen(),
            variant: position.variant(),
            turn: position.turn(),
            legal_moves_uci: position
                .legal_moves()
                .iter()
                .map(|movement| movement.uci(position.variant()))
                .collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkbenchExportEnumFailureKind {
    Session,
    Process,
    Analysis,
    Result,
    InvalidResponse,
    UnexpectedTermination,
    Cleanup,
    SessionCleanup,
}

#[derive(Serialize)]
struct WorkbenchExportFailure {
    kind: WorkbenchExportEnumFailureKind,
    message: String,
    sources: Vec<String>,
    operation: Option<Box<WorkbenchExportFailure>>,
    cleanup: Option<String>,
}

impl WorkbenchExportFailure {
    fn capture(failure: &WorkbenchJobFailure) -> Self {
        let (kind, operation, cleanup) = match failure {
            WorkbenchJobFailure::Session(_) => {
                (WorkbenchExportEnumFailureKind::Session, None, None)
            }
            WorkbenchJobFailure::Process(_) => {
                (WorkbenchExportEnumFailureKind::Process, None, None)
            }
            WorkbenchJobFailure::Analysis(_) => {
                (WorkbenchExportEnumFailureKind::Analysis, None, None)
            }
            WorkbenchJobFailure::Result(_) => (WorkbenchExportEnumFailureKind::Result, None, None),
            WorkbenchJobFailure::InvalidResponse(_) => {
                (WorkbenchExportEnumFailureKind::InvalidResponse, None, None)
            }
            WorkbenchJobFailure::UnexpectedTermination => (
                WorkbenchExportEnumFailureKind::UnexpectedTermination,
                None,
                None,
            ),
            WorkbenchJobFailure::Cleanup { operation, cleanup } => (
                WorkbenchExportEnumFailureKind::Cleanup,
                Some(Box::new(Self::capture(operation))),
                Some(cleanup.to_string()),
            ),
            WorkbenchJobFailure::SessionCleanup { operation, cleanup } => (
                WorkbenchExportEnumFailureKind::SessionCleanup,
                Some(Box::new(Self::capture(operation))),
                Some(cleanup.to_string()),
            ),
        };
        let mut sources = Vec::new();
        let mut source = failure.source();
        while let Some(error) = source {
            sources.push(error.to_string());
            source = error.source();
        }
        Self {
            kind,
            message: failure.to_string(),
            sources,
            operation,
            cleanup,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkbenchExportEnumCancellation {
    User,
    Deadline,
}

#[derive(Serialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
enum WorkbenchExportEnumCompletion<'a> {
    Result(WorkbenchExportEnumResult<'a>),
    Failed(WorkbenchExportFailure),
    Terminated,
}

impl<'a> WorkbenchExportEnumCompletion<'a> {
    fn capture(completion: &'a WorkbenchEnumCompletion) -> Self {
        match completion {
            WorkbenchEnumCompletion::Result(result) => {
                Self::Result(WorkbenchExportEnumResult::capture(result))
            }
            WorkbenchEnumCompletion::Failed(failure) => {
                Self::Failed(WorkbenchExportFailure::capture(failure))
            }
            WorkbenchEnumCompletion::Terminated => Self::Terminated,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum WorkbenchExportEnumOutcome<'a> {
    Succeeded {
        result: WorkbenchExportEnumResult<'a>,
    },
    Failed {
        failure: WorkbenchExportFailure,
    },
    Cancelled {
        reason: WorkbenchExportEnumCancellation,
        requested_after_nanoseconds: WorkbenchExportDecimal,
        completion: Option<WorkbenchExportEnumCompletion<'a>>,
    },
}

impl<'a> WorkbenchExportEnumOutcome<'a> {
    fn capture(job: &'a WorkbenchJob) -> Result<Self, WorkbenchExportError> {
        Ok(
            match job
                .outcome()
                .ok_or_else(|| WorkbenchExportError::Active(job.state()))?
            {
                WorkbenchEnumOutcome::Succeeded(result) => Self::Succeeded {
                    result: WorkbenchExportEnumResult::capture(result),
                },
                WorkbenchEnumOutcome::Failed(failure) => Self::Failed {
                    failure: WorkbenchExportFailure::capture(failure),
                },
                WorkbenchEnumOutcome::Cancelled {
                    cancellation,
                    completion,
                } => Self::Cancelled {
                    reason: match cancellation.reason() {
                        WorkbenchEnumCancellation::User => WorkbenchExportEnumCancellation::User,
                        WorkbenchEnumCancellation::Deadline => {
                            WorkbenchExportEnumCancellation::Deadline
                        }
                    },
                    requested_after_nanoseconds: WorkbenchExportDecimal(
                        cancellation
                            .requested_at()
                            .elapsed_since(job.created_at())?
                            .as_nanos(),
                    ),
                    completion: completion
                        .as_ref()
                        .map(WorkbenchExportEnumCompletion::capture),
                },
            },
        )
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkbenchExportEnumPipe {
    Stdout,
    Stderr,
}

#[derive(Serialize)]
struct WorkbenchExportDiagnostic<'a> {
    pipe: WorkbenchExportEnumPipe,
    text: &'a str,
}

#[derive(Serialize)]
struct WorkbenchExportDiagnostics<'a> {
    maximum_owned_bytes: usize,
    maximum_entries: usize,
    evicted_lines: WorkbenchExportDecimal,
    evicted_bytes: WorkbenchExportDecimal,
    entries: Vec<WorkbenchExportDiagnostic<'a>>,
}

#[derive(Serialize)]
pub struct WorkbenchExport<'a> {
    provenance: WorkbenchExportProvenance,
    child: &'a WorkbenchExportEnumObservation,
    parameters: WorkbenchExportEnumParameters,
    timeout_nanoseconds: WorkbenchExportDecimal,
    elapsed_nanoseconds: WorkbenchExportDecimal,
    progress_completed: u32,
    progress_total: u32,
    outcome: WorkbenchExportEnumOutcome<'a>,
    diagnostics: WorkbenchExportDiagnostics<'a>,
    #[serde(skip)]
    job_id: Uuid,
}

#[derive(Serialize)]
struct WorkbenchExportDocument<'a, 'b> {
    schema_version: u32,
    content: &'a WorkbenchExport<'b>,
    observation: WorkbenchExportDocumentObservation<'a>,
}

#[derive(Serialize)]
struct WorkbenchExportDocumentObservation<'a> {
    job_id: Uuid,
    supplied: &'a WorkbenchExportMetadata,
}

impl<'a> WorkbenchExport<'a> {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn capture(
        job: &'a WorkbenchJob,
        context: &'a WorkbenchExportContext,
    ) -> Result<Self, WorkbenchExportError> {
        let outcome = WorkbenchExportEnumOutcome::capture(job)?;
        if context.job_id != job.id() || context.package_sha256 != job.package().digest() {
            return Err(WorkbenchExportError::ContextMismatch);
        }
        Ok(Self {
            provenance: WorkbenchExportProvenance {
                engine_version: EnginePackageManifest::VERSION,
                source_repository: EnginePackageManifest::REPOSITORY,
                source_tag: EnginePackageManifest::TAG,
                source_commit: EnginePackageManifest::COMMIT,
                model_name: EnginePackageManifest::NETWORK_NAME,
                model_bytes: WorkbenchExportDecimal(EnginePackageManifest::NETWORK_BYTES),
                model_sha256: EnginePackageManifest::NETWORK_SHA256,
                target: job.package().target(),
                profile: job.package().profile(),
                package_sha256: job.package().digest(),
            },
            child: &context.observation,
            parameters: WorkbenchExportEnumParameters::capture(job.parameters()),
            timeout_nanoseconds: WorkbenchExportDecimal(
                ClockDuration::try_from(job.timeout().duration())?.as_nanos(),
            ),
            elapsed_nanoseconds: WorkbenchExportDecimal(
                job.observed_at()
                    .elapsed_since(job.created_at())?
                    .as_nanos(),
            ),
            progress_completed: job.progress().completed(),
            progress_total: job.progress().total(),
            outcome,
            diagnostics: WorkbenchExportDiagnostics {
                maximum_owned_bytes: EngineDiagnostics::MAX_OWNED_BYTES,
                maximum_entries: EngineDiagnostics::MAX_ENTRIES,
                evicted_lines: WorkbenchExportDecimal(job.diagnostics().evicted_lines()),
                evicted_bytes: WorkbenchExportDecimal(job.diagnostics().evicted_bytes()),
                entries: job
                    .diagnostics()
                    .entries()
                    .map(|entry| WorkbenchExportDiagnostic {
                        pipe: match entry.pipe() {
                            EngineProcessEnumPipe::Stdout => WorkbenchExportEnumPipe::Stdout,
                            EngineProcessEnumPipe::Stderr => WorkbenchExportEnumPipe::Stderr,
                        },
                        text: entry.text(),
                    })
                    .collect(),
            },
            job_id: job.id().value(),
        })
    }

    pub fn diagnostic_lines(&self) -> usize {
        self.diagnostics.entries.len()
    }
    pub const fn evicted_lines(&self) -> u64 {
        self.diagnostics.evicted_lines.0
    }
    pub const fn evicted_bytes(&self) -> u64 {
        self.diagnostics.evicted_bytes.0
    }

    pub fn encode_content(&self) -> Result<Vec<u8>, WorkbenchExportError> {
        Ok(serde_json::to_vec_pretty(self)?)
    }

    pub fn encode(
        &self,
        metadata: &WorkbenchExportMetadata,
    ) -> Result<Vec<u8>, WorkbenchExportError> {
        Ok(serde_json::to_vec_pretty(&WorkbenchExportDocument {
            schema_version: Self::SCHEMA_VERSION,
            content: self,
            observation: WorkbenchExportDocumentObservation {
                job_id: self.job_id,
                supplied: metadata,
            },
        })?)
    }

    pub fn prepare(
        &self,
        metadata: &WorkbenchExportMetadata,
    ) -> Result<WorkbenchExportEncoded, WorkbenchExportError> {
        let bytes = self.encode(metadata)?;
        let sha256 = PackageDigest::read(bytes.as_slice())?;
        let length = u64::try_from(bytes.len()).map_err(ClockDurationError::from)?;
        Ok(WorkbenchExportEncoded {
            bytes,
            receipt: WorkbenchExportReceipt {
                bytes: WorkbenchExportDecimal(length),
                sha256,
            },
        })
    }

    pub fn write<P: WorkbenchExportPort>(
        &self,
        metadata: &WorkbenchExportMetadata,
        port: &mut P,
    ) -> Result<WorkbenchExportReceipt, WorkbenchExportWriteError<P::Error>> {
        if port.is_cancelled() {
            port.abort();
            return Err(WorkbenchExportWriteError::Cancelled);
        }
        match self.prepare(metadata) {
            Ok(encoded) => encoded.write(port),
            Err(error) => {
                port.abort();
                Err(error.into())
            }
        }
    }
}

pub struct WorkbenchExportEncoded {
    bytes: Vec<u8>,
    receipt: WorkbenchExportReceipt,
}

impl WorkbenchExportEncoded {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub const fn sha256(&self) -> PackageDigest {
        self.receipt.sha256
    }
    pub const fn length(&self) -> u64 {
        self.receipt.bytes.0
    }

    pub fn write<P: WorkbenchExportPort>(
        &self,
        port: &mut P,
    ) -> Result<WorkbenchExportReceipt, WorkbenchExportWriteError<P::Error>> {
        let result = self.write_staged(port);
        if result.is_err() {
            port.abort();
        }
        result
    }

    fn write_staged<P: WorkbenchExportPort>(
        &self,
        port: &mut P,
    ) -> Result<WorkbenchExportReceipt, WorkbenchExportWriteError<P::Error>> {
        if port.is_cancelled() {
            return Err(WorkbenchExportWriteError::Cancelled);
        }
        port.write(&self.bytes)
            .map_err(WorkbenchExportWriteError::Destination)?;
        if port.is_cancelled() {
            return Err(WorkbenchExportWriteError::Cancelled);
        }
        port.commit()
            .map_err(WorkbenchExportWriteError::Destination)?;
        Ok(self.receipt)
    }
}
