use strum::IntoEnumIterator;
use std::error::Error;
use std::num::NonZeroUsize;

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::engine::analysis::EngineAnalysisStaticEvaluation;
use chess::engine::capabilities::EngineCapabilities;
use chess::engine::diagnostics::EngineDiagnostics;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::manifest::EnginePackageManifest;
use chess::engine::process_provider::{
    EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
};
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::session::EngineSessionError;
use chess::storage::atomic_file::{
    StorageAtomicFileError, StorageAtomicFileStage, StorageAtomicFileTarget,
};
use chess::workbench::enums::{
    WorkbenchEnumCompletion, WorkbenchEnumOperation, WorkbenchEnumParameters, WorkbenchEnumResult,
};
use chess::workbench::export::{
    WorkbenchExport, WorkbenchExportContext, WorkbenchExportError, WorkbenchExportMetadata,
    WorkbenchExportPort,
};
use chess::workbench::job::{
    WorkbenchJob, WorkbenchJobDeadline, WorkbenchJobFailure, WorkbenchJobId,
};
use chess::workbench::parameters::{
    WorkbenchBenchParameters, WorkbenchPerftDepth, WorkbenchPerftParameters,
};
use chess::workbench::result::{
    WorkbenchBenchResult, WorkbenchCompilerResult, WorkbenchPerftResult, WorkbenchPerftRoot,
};
use uuid::Uuid;

use crate::engine_package_selection::EnginePackageFixture;

struct WorkbenchExportFixture {
    package: EnginePackageFixture,
    budget: CapacityBudget,
}

impl WorkbenchExportFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            package: EnginePackageFixture::new()?,
            budget: CapacityBudget::new(NonZeroUsize::MIN)?,
        })
    }

    fn capabilities() -> Result<EngineCapabilities, Box<dyn Error>> {
        let mut capabilities = EngineCapabilities::default();
        for option in EngineEnumOption::iter() {
            capabilities.discover(&format!(
                "option name {} type {}",
                option.name(),
                option.descriptor()
            ))?;
        }
        capabilities.validate()?;
        Ok(capabilities)
    }

    fn job(&self, operation: WorkbenchEnumOperation) -> Result<WorkbenchJob, Box<dyn Error>> {
        let position = EngineSearchPosition::new(BoardPosition::standard(), ["e2e4", "e7e5"])?;
        let parameters = match operation {
            WorkbenchEnumOperation::Inspector => WorkbenchEnumParameters::Inspector(position),
            WorkbenchEnumOperation::RawEvaluation => {
                WorkbenchEnumParameters::RawEvaluation(position)
            }
            WorkbenchEnumOperation::Compiler => WorkbenchEnumParameters::Compiler,
            WorkbenchEnumOperation::Perft => WorkbenchEnumParameters::Perft(
                WorkbenchPerftParameters::new(position, WorkbenchPerftDepth::new(2)?),
            ),
            WorkbenchEnumOperation::Bench => WorkbenchEnumParameters::Bench(
                WorkbenchBenchParameters::new(&Self::capabilities()?, None, None, None)?,
            ),
        };
        Ok(WorkbenchJob::new(
            WorkbenchJobId::try_from(Uuid::from_u128(9))?,
            parameters,
            self.package.select()?,
            WorkbenchJobDeadline::default(),
            ClockInstant::from_nanos(0),
        )?)
    }

    fn start(&self, job: &mut WorkbenchJob) -> Result<(), Box<dyn Error>> {
        job.admit(
            job.id(),
            self.budget.try_lease(job.parameters().workers())?,
            ClockInstant::from_nanos(1),
        )?;
        job.start(job.id(), ClockInstant::from_nanos(2))?;
        Ok(())
    }

    fn complete(&self, job: &mut WorkbenchJob) -> Result<(), Box<dyn Error>> {
        let result = match job.parameters() {
            WorkbenchEnumParameters::Inspector(position) => {
                WorkbenchEnumResult::Inspector(position.current().clone())
            }
            WorkbenchEnumParameters::RawEvaluation(_) => WorkbenchEnumResult::RawEvaluation(
                EngineAnalysisStaticEvaluation::parse("-2147483648")?,
            ),
            WorkbenchEnumParameters::Compiler => {
                WorkbenchEnumResult::Compiler(WorkbenchCompilerResult::new(
                    "fixture compiler".to_owned(),
                    job.package().target(),
                    "neon".to_owned(),
                )?)
            }
            WorkbenchEnumParameters::Perft(parameters) => {
                let roots: Vec<_> = parameters
                    .position()
                    .current()
                    .legal_moves()
                    .into_iter()
                    .enumerate()
                    .map(|(index, movement)| {
                        WorkbenchPerftRoot::new(movement, if index == 0 { Self::LARGE } else { 0 })
                    })
                    .collect();
                WorkbenchEnumResult::Perft(WorkbenchPerftResult::new(
                    parameters,
                    roots,
                    Self::LARGE,
                    ClockDuration::from_nanos(1_234_567),
                    u64::MAX,
                )?)
            }
            WorkbenchEnumParameters::Bench(_) => {
                WorkbenchEnumResult::Bench(WorkbenchBenchResult::new(
                    WorkbenchBenchParameters::POSITIONS,
                    Self::LARGE,
                    ClockDuration::from_nanos(1_234_567),
                    u64::MAX,
                )?)
            }
        };
        job.finish_reaped(
            job.id(),
            WorkbenchEnumCompletion::Result(result),
            ClockInstant::from_nanos(9),
        )?;
        Ok(())
    }

    const LARGE: u64 = 9_007_199_254_740_993;

    fn encode(job: &WorkbenchJob) -> Result<Vec<u8>, Box<dyn Error>> {
        let context = WorkbenchExportContext::unobserved(job);
        Ok(WorkbenchExport::capture(job, &context)?.encode(&WorkbenchExportMetadata::default())?)
    }
}

#[test]
fn every_registered_result_exports_complete_deterministic_typed_content()
-> Result<(), Box<dyn Error>> {
    assert!(!WorkbenchEnumOperation::ALL.is_empty());
    let fixture = WorkbenchExportFixture::new()?;
    for operation in WorkbenchEnumOperation::ALL {
        let mut job = fixture.job(operation)?;
        fixture.start(&mut job)?;
        fixture.complete(&mut job)?;
        let bytes = WorkbenchExportFixture::encode(&job)?;
        assert_eq!(bytes, WorkbenchExportFixture::encode(&job)?);
        let text = String::from_utf8(bytes)?;
        assert!(text.contains(EnginePackageManifest::COMMIT));
        assert!(text.contains(EnginePackageManifest::NETWORK_SHA256));
        assert!(text.contains(&job.package().digest().to_string()));
        assert!(!text.contains(&job.package().executable().display().to_string()));
        assert!(text.contains("\"status\": \"succeeded\""));
        match operation {
            WorkbenchEnumOperation::Inspector => {
                assert!(text.contains("e2e4"));
                assert!(text.contains("initial_fen"));
                assert!(text.contains("legal_moves_uci"));
            }
            WorkbenchEnumOperation::RawEvaluation => {
                assert!(text.contains("raw_white_nnue"));
                assert!(text.contains("-2147483648"));
                assert!(!text.contains("centipawn"));
            }
            WorkbenchEnumOperation::Compiler => assert!(text.contains("fixture compiler")),
            WorkbenchEnumOperation::Perft | WorkbenchEnumOperation::Bench => {
                assert!(text.contains("\"9007199254740993\""));
                assert!(text.contains("\"18446744073709551615\""));
                assert!(text.contains("\"elapsed_nanoseconds\": \"1234567\""));
                if operation == WorkbenchEnumOperation::Perft {
                    assert!(text.contains("\"roots\""));
                } else {
                    assert!(text.contains("\"hash_mib\": 16"));
                    assert!(text.contains("\"threads\": 1"));
                    assert!(text.contains("\"depth_plies\": 12"));
                    assert!(text.contains("\"positions\": 45"));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn diagnostics_preserve_order_escaping_all_retained_lines_and_eviction_counts()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExportFixture::new()?;
    let mut job = fixture.job(WorkbenchEnumOperation::Compiler)?;
    fixture.start(&mut job)?;
    for index in 0..EngineDiagnostics::MAX_ENTRIES + 2 {
        job.record(
            job.id(),
            &EngineProcessLine::new(
                if index % 2 == 0 {
                    EngineProcessEnumPipe::Stdout
                } else {
                    EngineProcessEnumPipe::Stderr
                },
                format!("line {index}: <script>\"x\"</script>"),
            )?,
            ClockInstant::from_nanos(3),
        )?;
    }
    fixture.complete(&mut job)?;
    let context = WorkbenchExportContext::unobserved(&job);
    let export = WorkbenchExport::capture(&job, &context)?;
    assert_eq!(export.diagnostic_lines(), EngineDiagnostics::MAX_ENTRIES);
    assert_eq!(export.evicted_lines(), 2);
    let text = String::from_utf8(export.encode(&WorkbenchExportMetadata::default())?)?;
    assert!(
        text.find("line 2:").ok_or("Missing first retained line")?
            < text
                .find("line 4097:")
                .ok_or("Missing final retained line")?
    );
    assert!(!text.contains("line 0:"));
    assert!(text.contains("\\\"x\\\""));
    assert!(text.contains("\"evicted_lines\": \"2\""));
    assert!(text.contains("\"stderr\""));
    assert!(text.contains("\"stdout\""));
    assert!(text.contains(&format!(
        "\"evicted_bytes\": \"{}\"",
        job.diagnostics().evicted_bytes()
    )));
    Ok(())
}

#[test]
fn terminal_failures_cancellation_and_late_results_keep_their_outcomes()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExportFixture::new()?;
    let mut failed = fixture.job(WorkbenchEnumOperation::Compiler)?;
    failed.fail_before_start(
        failed.id(),
        WorkbenchJobFailure::Process(EngineProcessError::Io(std::io::Error::other(
            "source failure",
        ))),
        ClockInstant::from_nanos(1),
    )?;
    let text = String::from_utf8(WorkbenchExportFixture::encode(&failed)?)?;
    assert!(text.contains("\"status\": \"failed\""));
    assert!(text.contains("source failure"));
    let mut session_failed = fixture.job(WorkbenchEnumOperation::Compiler)?;
    session_failed.fail_before_start(
        session_failed.id(),
        WorkbenchJobFailure::Session(EngineSessionError::UnexpectedResponse),
        ClockInstant::from_nanos(1),
    )?;
    let text = String::from_utf8(WorkbenchExportFixture::encode(&session_failed)?)?;
    assert!(text.contains("\"kind\": \"session\""));
    assert!(text.contains("unexpected startup response"));
    let mut cleanup_failed = fixture.job(WorkbenchEnumOperation::Compiler)?;
    cleanup_failed.fail_before_start(
        cleanup_failed.id(),
        WorkbenchJobFailure::SessionCleanup {
            operation: Box::new(WorkbenchJobFailure::Session(
                EngineSessionError::UnexpectedResponse,
            )),
            cleanup: Box::new(EngineSessionError::ShutdownUnobserved),
        },
        ClockInstant::from_nanos(1),
    )?;
    let text = String::from_utf8(WorkbenchExportFixture::encode(&cleanup_failed)?)?;
    assert!(text.contains("\"kind\": \"session_cleanup\""));
    assert!(text.contains("\"kind\": \"session\""));
    assert!(text.contains("without observing all owned resources"));
    let mut cancelled = fixture.job(WorkbenchEnumOperation::Compiler)?;
    fixture.start(&mut cancelled)?;
    cancelled.cancel(cancelled.id(), ClockInstant::from_nanos(3))?;
    fixture.complete(&mut cancelled)?;
    let text = String::from_utf8(WorkbenchExportFixture::encode(&cancelled)?)?;
    assert!(text.contains("\"status\": \"cancelled\""));
    assert!(text.contains("\"reason\": \"user\""));
    assert!(text.contains("fixture compiler"));
    let mut timeout = fixture.job(WorkbenchEnumOperation::Compiler)?;
    fixture.start(&mut timeout)?;
    timeout.expire(timeout.id(), timeout.deadline())?;
    timeout.finish_reaped(
        timeout.id(),
        WorkbenchEnumCompletion::Terminated,
        timeout.deadline(),
    )?;
    let text = String::from_utf8(WorkbenchExportFixture::encode(&timeout)?)?;
    assert!(text.contains("\"reason\": \"deadline\""));
    assert!(text.contains("\"status\": \"terminated\""));
    Ok(())
}

#[test]
fn active_jobs_refuse_export_at_every_lifecycle_stage() -> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExportFixture::new()?;
    let mut job = fixture.job(WorkbenchEnumOperation::Compiler)?;
    let context = WorkbenchExportContext::unobserved(&job);
    assert!(matches!(
        WorkbenchExport::capture(&job, &context),
        Err(WorkbenchExportError::Active(_))
    ));
    job.admit(
        job.id(),
        fixture.budget.try_lease(1)?,
        ClockInstant::from_nanos(1),
    )?;
    assert!(matches!(
        WorkbenchExport::capture(&job, &context),
        Err(WorkbenchExportError::Active(_))
    ));
    job.start(job.id(), ClockInstant::from_nanos(2))?;
    assert!(matches!(
        WorkbenchExport::capture(&job, &context),
        Err(WorkbenchExportError::Active(_))
    ));
    job.cancel(job.id(), ClockInstant::from_nanos(3))?;
    assert!(matches!(
        WorkbenchExport::capture(&job, &context),
        Err(WorkbenchExportError::Active(_))
    ));
    Ok(())
}

struct WorkbenchExportMemoryPort {
    destination: Vec<u8>,
    staged: Vec<u8>,
    fail_write: bool,
    fail_commit: bool,
    cancel_after_write: bool,
    cancelled: bool,
    commits: usize,
}

impl WorkbenchExportMemoryPort {
    fn new() -> Self {
        Self {
            destination: b"original".to_vec(),
            staged: Vec::new(),
            fail_write: false,
            fail_commit: false,
            cancel_after_write: false,
            cancelled: false,
            commits: 0,
        }
    }
}

impl WorkbenchExportPort for WorkbenchExportMemoryPort {
    type Error = std::io::Error;

    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        if self.fail_write {
            self.staged.extend_from_slice(&bytes[..bytes.len() / 2]);
            return Err(std::io::Error::other("partial write"));
        }
        self.staged.extend_from_slice(bytes);
        self.cancelled = self.cancel_after_write;
        Ok(())
    }

    fn abort(&mut self) {
        self.staged.clear();
    }

    fn commit(&mut self) -> Result<(), Self::Error> {
        if self.fail_commit {
            return Err(std::io::Error::other("commit failure"));
        }
        self.commits += 1;
        self.destination = std::mem::take(&mut self.staged);
        Ok(())
    }
}

#[test]
fn writing_returns_receipt_only_after_commit_and_preserves_source_errors()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExportFixture::new()?;
    let mut job = fixture.job(WorkbenchEnumOperation::Compiler)?;
    fixture.start(&mut job)?;
    fixture.complete(&mut job)?;
    let context = WorkbenchExportContext::unobserved(&job);
    let export = WorkbenchExport::capture(&job, &context)?;
    let metadata = WorkbenchExportMetadata::default();
    let mut port = WorkbenchExportMemoryPort::new();
    let receipt = export.write(&metadata, &mut port)?;
    assert_eq!(port.commits, 1);
    assert_eq!(receipt.bytes(), u64::try_from(port.destination.len())?);
    assert_eq!(
        receipt.sha256(),
        chess::package::digest::PackageDigest::read(&port.destination[..])?
    );
    for mode in 0..4 {
        let mut port = WorkbenchExportMemoryPort::new();
        port.fail_write = mode == 0;
        port.fail_commit = mode == 1;
        port.cancel_after_write = mode == 2;
        port.cancelled = mode == 3;
        let error = export
            .write(&metadata, &mut port)
            .err()
            .ok_or("Expected write failure")?;
        if mode < 2 {
            assert!(error.source().is_some());
        }
        assert_eq!(port.commits, 0);
        assert_eq!(port.destination, b"original");
        assert!(port.staged.is_empty());
    }
    Ok(())
}

struct WorkbenchExportAtomicPort {
    stage: Option<StorageAtomicFileStage>,
    fail_after_prefix: bool,
    cancel_after_write: bool,
    cancelled: bool,
}

impl WorkbenchExportAtomicPort {
    fn new(path: std::path::PathBuf) -> Result<Self, StorageAtomicFileError> {
        Ok(Self {
            stage: Some(StorageAtomicFileStage::new(
                StorageAtomicFileTarget::capture(path)?,
            )?),
            fail_after_prefix: false,
            cancel_after_write: false,
            cancelled: false,
        })
    }
}

impl WorkbenchExportPort for WorkbenchExportAtomicPort {
    type Error = StorageAtomicFileError;

    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    fn abort(&mut self) {
        drop(self.stage.take());
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let stage = self
            .stage
            .as_mut()
            .ok_or(StorageAtomicFileError::InvalidLocation)?;
        if self.fail_after_prefix {
            stage.write(&bytes[..bytes.len() / 2])?;
            return Err(std::io::Error::other("injected partial write").into());
        }
        stage.write(bytes)?;
        self.cancelled = self.cancel_after_write;
        Ok(())
    }
    fn commit(&mut self) -> Result<(), Self::Error> {
        self.stage
            .take()
            .ok_or(StorageAtomicFileError::InvalidLocation)?
            .commit()?;
        Ok(())
    }
}

#[test]
fn existing_atomic_file_boundary_writes_actual_bytes_and_rejects_stale_selection()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExportFixture::new()?;
    let mut job = fixture.job(WorkbenchEnumOperation::Compiler)?;
    fixture.start(&mut job)?;
    fixture.complete(&mut job)?;
    let context = WorkbenchExportContext::unobserved(&job);
    let export = WorkbenchExport::capture(&job, &context)?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    std::fs::write(&path, b"original")?;
    let mut port = WorkbenchExportAtomicPort::new(path.clone())?;
    export.write(&WorkbenchExportMetadata::default(), &mut port)?;
    assert_eq!(std::fs::read(&path)?, WorkbenchExportFixture::encode(&job)?);
    let mut port = WorkbenchExportAtomicPort::new(path.clone())?;
    std::fs::write(&path, b"changed after selection")?;
    assert!(
        export
            .write(&WorkbenchExportMetadata::default(), &mut port)
            .is_err()
    );
    assert_eq!(std::fs::read(&path)?, b"changed after selection");
    for partial_failure in [false, true] {
        let mut port = WorkbenchExportAtomicPort::new(path.clone())?;
        port.fail_after_prefix = partial_failure;
        port.cancel_after_write = !partial_failure;
        assert!(
            export
                .write(&WorkbenchExportMetadata::default(), &mut port)
                .is_err()
        );
        assert!(port.stage.is_none());
        assert_eq!(std::fs::read(&path)?, b"changed after selection");
        assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn observed_configuration_is_an_owned_acknowledged_child_snapshot()
-> Result<(), Box<dyn Error>> {
    use crate::engine_lifecycle::EngineLifecycleProvider;
    use chess::engine::options::{EngineOptionsEnumValue, EngineOptionsSetting};
    use chess::engine::resource_config::EngineResourceConfig;

    let fixture = WorkbenchExportFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture.package).await?;
    for line in [
        "info string set Threads to 3",
        "info string set Hash to 64 MB",
        "info string set MoveOverhead to 25 ms",
    ] {
        provider.emit(line).await?;
    }
    session
        .configure_resources(EngineResourceConfig::new(
            3,
            64,
            ClockDuration::from_millis(25)?,
        )?)
        .await?;
    let mut job = fixture.job(WorkbenchEnumOperation::Compiler)?;
    let context = WorkbenchExportContext::observed(
        &job,
        session.identity(),
        session
            .applied_options()
            .ok_or("Missing acknowledged options")?,
    );
    let setting = EngineOptionsSetting::new(
        EngineOptionsEnumValue::Minimal(true),
        session.identity().capabilities(),
    )?;
    session.configure_option(setting).await?;
    assert!(
        session
            .applied_options()
            .ok_or("Missing updated options")?
            .is_minimal()
    );
    fixture.start(&mut job)?;
    fixture.complete(&mut job)?;
    let text = String::from_utf8(WorkbenchExport::capture(&job, &context)?.encode_content()?)?;
    assert!(text.contains("\"state\": \"observed\""));
    assert!(text.contains("\"threads\": 3"));
    assert!(text.contains("\"hash_mib\": 64"));
    assert!(text.contains("\"move_overhead_milliseconds\": \"25\""));
    assert!(text.contains("\"minimal\": false"));
    assert!(text.contains("\"multi_pv\": 1"));
    assert!(text.contains("\"tablebases\": {\n        \"state\": \"unconfigured\""));
    assert!(text.contains(session.identity().name()));
    session.shutdown().await?;
    Ok(())
}

#[test]
fn identities_and_supplied_timestamps_do_not_change_deterministic_content()
-> Result<(), Box<dyn Error>> {
    #[derive(serde::Deserialize)]
    struct WorkbenchExportEnvelope {
        content: Box<serde_json::value::RawValue>,
    }

    let fixture = WorkbenchExportFixture::new()?;
    let mut first = fixture.job(WorkbenchEnumOperation::Compiler)?;
    fixture.start(&mut first)?;
    fixture.complete(&mut first)?;
    let mut second = WorkbenchJob::new(
        WorkbenchJobId::try_from(Uuid::from_u128(10))?,
        WorkbenchEnumParameters::Compiler,
        fixture.package.select()?,
        WorkbenchJobDeadline::default(),
        ClockInstant::from_nanos(0),
    )?;
    fixture.start(&mut second)?;
    fixture.complete(&mut second)?;
    let first_context = WorkbenchExportContext::unobserved(&first);
    let second_context = WorkbenchExportContext::unobserved(&second);
    let first_export = WorkbenchExport::capture(&first, &first_context)?;
    let second_export = WorkbenchExport::capture(&second, &second_context)?;
    assert_eq!(
        first_export.encode_content()?,
        second_export.encode_content()?
    );
    assert!(matches!(
        WorkbenchExport::capture(&second, &first_context),
        Err(WorkbenchExportError::ContextMismatch)
    ));
    let first_bytes = first_export.encode(&WorkbenchExportMetadata::new(
        Uuid::from_u128(100),
        1,
        "fixture 1".to_owned(),
    )?)?;
    let second_bytes = second_export.encode(&WorkbenchExportMetadata::new(
        Uuid::from_u128(101),
        u64::MAX,
        "fixture 2".to_owned(),
    )?)?;
    assert_ne!(first_bytes, second_bytes);
    let first_document: WorkbenchExportEnvelope = serde_json::from_slice(&first_bytes)?;
    let second_document: WorkbenchExportEnvelope = serde_json::from_slice(&second_bytes)?;
    assert_eq!(first_document.content.get(), second_document.content.get());
    for (id, version) in [
        (Uuid::nil(), "valid"),
        (Uuid::from_u128(1), ""),
        (Uuid::from_u128(1), "bad\nversion"),
    ] {
        assert!(WorkbenchExportMetadata::new(id, 0, version.to_owned()).is_err());
    }
    Ok(())
}

#[test]
fn diagnostics_disclose_byte_budget_eviction_without_further_export_truncation()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExportFixture::new()?;
    let mut job = fixture.job(WorkbenchEnumOperation::Compiler)?;
    fixture.start(&mut job)?;
    let line = EngineProcessLine::new(
        EngineProcessEnumPipe::Stderr,
        "x".repeat(chess::engine::process_provider::EngineProcessLimits::LINE_BYTES),
    )?;
    for _ in 0..32 {
        job.record(job.id(), &line, ClockInstant::from_nanos(3))?;
    }
    fixture.complete(&mut job)?;
    let context = WorkbenchExportContext::unobserved(&job);
    let export = WorkbenchExport::capture(&job, &context)?;
    assert!(export.evicted_bytes() > 0);
    assert_eq!(export.evicted_bytes(), job.diagnostics().evicted_bytes());
    assert_eq!(export.diagnostic_lines(), job.diagnostics().entries().len());
    let bytes = export.prepare(&WorkbenchExportMetadata::default())?;
    assert!(bytes.length() > 900_000);
    Ok(())
}

#[test]
fn queued_cancellation_and_cancelled_failures_remain_distinct_from_success()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExportFixture::new()?;
    let mut queued = fixture.job(WorkbenchEnumOperation::Compiler)?;
    queued.cancel(queued.id(), ClockInstant::from_nanos(1))?;
    let text = String::from_utf8(WorkbenchExportFixture::encode(&queued)?)?;
    assert!(text.contains("\"completion\": null"));
    assert!(text.contains("\"status\": \"cancelled\""));
    let mut running = fixture.job(WorkbenchEnumOperation::Compiler)?;
    fixture.start(&mut running)?;
    running.cancel(running.id(), ClockInstant::from_nanos(3))?;
    running.finish_reaped(
        running.id(),
        WorkbenchEnumCompletion::Failed(WorkbenchJobFailure::Cleanup {
            operation: Box::new(WorkbenchJobFailure::InvalidResponse(
                WorkbenchEnumOperation::Compiler,
            )),
            cleanup: EngineProcessError::ShutdownTimeout,
        }),
        ClockInstant::from_nanos(4),
    )?;
    let text = String::from_utf8(WorkbenchExportFixture::encode(&running)?)?;
    assert!(text.contains("\"status\": \"cancelled\""));
    assert!(text.contains("\"kind\": \"cleanup\""));
    assert!(text.contains("\"kind\": \"invalid_response\""));
    assert!(text.contains("did not shut down"));
    Ok(())
}

#[test]
fn perft_export_preserves_every_root_beyond_presentation_row_limits() -> Result<(), Box<dyn Error>>
{
    let fixture = WorkbenchExportFixture::new()?;
    let position = BoardPosition::from_fen(
        "R6R/3Q4/1Q4Q1/4Q3/2Q4Q/Q4Q2/pp1Q4/kBNN1KB1 w - - 0 1",
        chess::board::enums::BoardEnumVariant::Standard,
    )?;
    let legal = position.legal_moves();
    assert!(legal.len() > 128);
    let mut job = WorkbenchJob::new(
        WorkbenchJobId::try_from(Uuid::from_u128(11))?,
        WorkbenchEnumParameters::Perft(WorkbenchPerftParameters::new(
            EngineSearchPosition::new(position, std::iter::empty::<&str>())?,
            WorkbenchPerftDepth::new(2)?,
        )),
        fixture.package.select()?,
        WorkbenchJobDeadline::default(),
        ClockInstant::from_nanos(0),
    )?;
    fixture.start(&mut job)?;
    fixture.complete(&mut job)?;
    let text = String::from_utf8(WorkbenchExportFixture::encode(&job)?)?;
    assert_eq!(text.matches("\"move_uci\"").count(), legal.len());
    for movement in legal {
        assert!(text.contains(&format!(
            "\"move_uci\": \"{}\"",
            movement.uci(chess::board::enums::BoardEnumVariant::Standard)
        )));
    }
    Ok(())
}

#[test]
fn explicit_benchmark_parameters_are_exported_without_reverting_to_defaults()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExportFixture::new()?;
    let mut job = WorkbenchJob::new(
        WorkbenchJobId::try_from(Uuid::from_u128(12))?,
        WorkbenchEnumParameters::Bench(WorkbenchBenchParameters::new(
            &WorkbenchExportFixture::capabilities()?,
            Some(32),
            Some(1),
            Some(7),
        )?),
        fixture.package.select()?,
        WorkbenchJobDeadline::default(),
        ClockInstant::from_nanos(0),
    )?;
    fixture.start(&mut job)?;
    fixture.complete(&mut job)?;
    let text = String::from_utf8(WorkbenchExportFixture::encode(&job)?)?;
    assert!(text.contains("\"hash_mib\": 32"));
    assert!(text.contains("\"depth_plies\": 7"));
    assert!(text.contains("\"state\": \"unobserved\""));
    Ok(())
}
