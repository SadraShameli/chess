use strum::IntoEnumIterator;
use std::collections::VecDeque;
use std::error::Error;
use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::task::Poll;

use tokio::sync::oneshot;
use tokio::time::Instant;
use uuid::Uuid;

use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationReceipt, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use crate::desktop::engine_commands::DesktopEngineCommandsAllocation;
use crate::desktop::error::DesktopEnumFailureCode;
use crate::desktop::file_handle::{DesktopFileEnumSelection, DesktopFileHandleError};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::desktop::workbench_commands::{
    DesktopWorkbenchCommands, DesktopWorkbenchCommandsRequest, DesktopWorkbenchCommandsResult,
};
use crate::desktop::workbench_export::{
    DesktopWorkbenchExport, DesktopWorkbenchExportError, DesktopWorkbenchExportPort,
    DesktopWorkbenchExportPreview, DesktopWorkbenchExportRequest, DesktopWorkbenchExportWrite,
};
use crate::engine::capabilities::EngineIdentity;
use crate::engine::enums::EngineEnumOption;
use crate::engine::package::host::EnginePackageHost;
use crate::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumTarget, EnginePackageManifest,
};
use crate::engine::package::selector::{EnginePackageSelected, EnginePackageSelector};
use crate::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use crate::package::digest::PackageDigest;
use crate::storage::atomic_file::{
    StorageAtomicFileError, StorageAtomicFileStage, StorageAtomicFileTarget,
};
use crate::workbench::enums::WorkbenchEnumParameters;
use crate::workbench::export::{
    WorkbenchExport, WorkbenchExportMetadata, WorkbenchExportPort, WorkbenchExportWriteError,
};
use crate::workbench::job::{WorkbenchJobDeadline, WorkbenchJobId};

type DesktopWorkbenchExportTestResult<T = ()> = Result<T, Box<dyn Error>>;

struct DesktopWorkbenchExportTestClock;

impl ClockTimeProvider for DesktopWorkbenchExportTestClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(7))
    }
}

struct DesktopWorkbenchExportTestWake;

impl DesktopApplicationWakeProvider for DesktopWorkbenchExportTestWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

struct DesktopWorkbenchExportTestProvider;

impl EngineProcessProvider for DesktopWorkbenchExportTestProvider {
    type Process = DesktopWorkbenchExportTestProcess;

    fn spawn(&self, package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        Ok(DesktopWorkbenchExportTestProcess {
            target: package.target(),
            lines: VecDeque::new(),
            reaped: false,
        })
    }
}

struct DesktopWorkbenchExportTestProcess {
    target: EnginePackageEnumTarget,
    lines: VecDeque<String>,
    reaped: bool,
}

impl EngineProcess for DesktopWorkbenchExportTestProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        match command {
            "uci" => {
                self.lines.push_back("id name Reckless 0.9.0".to_owned());
                self.lines
                    .push_back(format!("id author {}", EngineIdentity::AUTHOR));
                self.lines.extend(EngineEnumOption::iter().map(|option| {
                    format!("option name {} type {}", option.name(), option.descriptor())
                }));
                self.lines.push_back("uciok".to_owned());
            }
            "isready" => self.lines.push_back("readyok".to_owned()),
            "setoption name Threads value 1" => self
                .lines
                .push_back("info string set Threads to 1".to_owned()),
            "setoption name Hash value 16" => self
                .lines
                .push_back("info string set Hash to 16 MB".to_owned()),
            "setoption name MoveOverhead value 100" => self
                .lines
                .push_back("info string set MoveOverhead to 100 ms".to_owned()),
            "setoption name UCI_Chess960 value false" => self
                .lines
                .push_back("info string set UCI_Chess960 to false".to_owned()),
            "compiler" => self.lines.extend([
                "Compiler Version: rustc 1.99.0".to_owned(),
                format!("Compiler Target: {}", self.target.triple()),
                "Compiler Features: export fixture".to_owned(),
            ]),
            _ => {}
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        match self.lines.pop_front() {
            Some(text) => EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text),
            None => std::future::pending().await,
        }
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.reaped = true;
        Ok(())
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }

    async fn terminate(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }

    fn is_shutdown_complete(&self) -> bool {
        self.reaped
    }
}

struct DesktopWorkbenchExportTestFixture {
    _directory: tempfile::TempDir,
    result: DesktopWorkbenchCommandsResult,
    budget: CapacityBudget,
    executor: CapacityExecutor,
}

impl DesktopWorkbenchExportTestFixture {
    async fn new() -> DesktopWorkbenchExportTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let bytes = b"verified export engine fixture";
        let target = EnginePackageEnumTarget::MacosArm;
        std::fs::create_dir(directory.path().join("bin"))?;
        let path = directory.path().join(target.relative_path());
        std::fs::write(&path, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        }
        let manifest = EnginePackageManifest::new(EnginePackageEntry::portable(
            target,
            u64::try_from(bytes.len())?,
            PackageDigest::read(&bytes[..])?,
            "export fixture".to_owned(),
        )?)?;
        let package = EnginePackageSelector::select(
            directory.path(),
            &manifest,
            EnginePackageHost::from_observation(target, "14.0", "", true)?,
        )?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let mut commands = DesktopWorkbenchCommands::new();
        commands
            .start(
                &DesktopWorkbenchExportTestProvider,
                DesktopWorkbenchCommandsRequest {
                    package,
                    parameters: WorkbenchEnumParameters::Compiler,
                    deadline: WorkbenchJobDeadline::default(),
                    id: WorkbenchJobId::try_from(Uuid::from_u128(91))?,
                    now: ClockInstant::from_nanos(0),
                },
                DesktopEngineCommandsAllocation::new(budget.try_lease(1)?, ()),
            )
            .await?;
        commands.run().await?;
        let result = commands
            .observe_completion()?
            .ok_or("The actual diagnostic did not complete")?;
        Ok(Self {
            _directory: directory,
            result,
            executor: CapacityExecutor::new(budget.clone())?,
            budget,
        })
    }

    fn preview(
        &self,
        owner: &mut DesktopWorkbenchExport,
    ) -> DesktopWorkbenchExportTestResult<Arc<DesktopWorkbenchExportPreview>> {
        Ok(owner.preview(
            "a1027132-9777-4a85-b0aa-c6034b5e70d9".parse()?,
            &self.result,
        )?)
    }

    fn receipt() -> DesktopWorkbenchExportTestResult<DesktopApplicationReceipt> {
        let application = DesktopApplication::new(
            Arc::new(DesktopWorkbenchExportTestClock),
            Arc::new(DesktopWorkbenchExportTestWake),
        );
        let port = application.port();
        let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        Ok(port.try_submit(&request)?)
    }

    fn request(
        preview: &DesktopWorkbenchExportPreview,
        receipt: DesktopApplicationReceipt,
        path: PathBuf,
    ) -> DesktopWorkbenchExportTestResult<DesktopWorkbenchExportRequest> {
        Ok(DesktopWorkbenchExportRequest {
            preview: preview.id(),
            digest: preview.digest(),
            destination: DesktopFileEnumSelection::Destination(StorageAtomicFileTarget::capture(
                path,
            )?),
            receipt,
        })
    }

    fn controlled(
        &self,
        owner: &mut DesktopWorkbenchExport,
        preview: &Arc<DesktopWorkbenchExportPreview>,
        request: DesktopWorkbenchExportRequest,
    ) -> DesktopWorkbenchExportTestResult<(oneshot::Receiver<()>, mpsc::SyncSender<()>)> {
        let preview = Arc::clone(preview);
        let cancellation = request.receipt.cancellation();
        let (ready, staged) = oneshot::channel();
        let ready = std::sync::Mutex::new(ready);
        let (release, released) = mpsc::sync_channel(1);
        let job = self.executor.submit(move |worker| {
            let mut port = DesktopWorkbenchExportTestGate {
                native: DesktopWorkbenchExportPort {
                    stage: Some(StorageAtomicFileStage::new(
                        request.destination.into_destination()?,
                    )?),
                    worker,
                    cancellation: &cancellation,
                },
                ready: Some(ready.into_inner().map_err(|_poisoned| {
                    StorageAtomicFileError::Io(std::io::Error::other(
                        "The publication signal is poisoned",
                    ))
                })?),
                released,
            };
            Ok::<_, DesktopWorkbenchExportError>(preview.encoded.write(&mut port)?)
        })?;
        owner.writing = Some(DesktopWorkbenchExportWrite {
            preview: request.preview,
            digest: request.digest,
            receipt: request.receipt,
            future: Box::pin(async move {
                job.join()
                    .await
                    .map_err(DesktopWorkbenchExportError::UnresolvedWrite)
                    .and_then(|result| result)
                    .map_err(Arc::new)
            }),
        });
        Ok((staged, release))
    }
}

struct DesktopWorkbenchExportTestGate<'a> {
    native: DesktopWorkbenchExportPort<'a>,
    ready: Option<oneshot::Sender<()>>,
    released: mpsc::Receiver<()>,
}

impl WorkbenchExportPort for DesktopWorkbenchExportTestGate<'_> {
    type Error = StorageAtomicFileError;

    fn is_cancelled(&self) -> bool {
        self.native.is_cancelled()
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.native.write(bytes)
    }

    fn commit(&mut self) -> Result<(), Self::Error> {
        self.ready
            .take()
            .ok_or(StorageAtomicFileError::InvalidLocation)?
            .send(())
            .map_err(|_closed| std::io::Error::other("The publication observer disappeared"))?;
        self.released.recv().map_err(std::io::Error::other)?;
        self.native.commit()
    }

    fn abort(&mut self) {
        self.native.abort();
    }
}

#[tokio::test]
async fn actual_terminal_preview_freezes_exact_bytes_provenance_and_optional_timestamp()
-> DesktopWorkbenchExportTestResult {
    let fixture = DesktopWorkbenchExportTestFixture::new().await?;
    let mut owner = DesktopWorkbenchExport::new();
    let preview = fixture.preview(&mut owner)?;
    let repeated = fixture.preview(&mut owner)?;
    assert!(Arc::ptr_eq(&preview, &repeated));
    assert_eq!(preview.job(), fixture.result.job().id());
    assert_eq!(preview.length(), u64::try_from(preview.bytes().len())?);
    assert_eq!(preview.digest(), PackageDigest::read(preview.bytes())?);
    let context = fixture.result.export_context();
    let export = WorkbenchExport::capture(fixture.result.job(), &context)?;
    assert_eq!(preview.diagnostic_lines(), export.diagnostic_lines());
    assert_eq!(preview.evicted_lines(), export.evicted_lines());
    assert_eq!(preview.evicted_bytes(), export.evicted_bytes());
    let metadata = WorkbenchExportMetadata::with_optional_timestamp(
        preview.id().as_uuid(),
        None,
        env!("CARGO_PKG_VERSION").to_owned(),
    )?;
    assert_eq!(preview.bytes(), export.prepare(&metadata)?.bytes());
    let text = std::str::from_utf8(preview.bytes())?;
    assert!(text.contains("\"created_unix_milliseconds\": null"));
    assert!(text.contains(&format!(
        "\"application_version\": \"{}\"",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(text.contains(&preview.id().as_uuid().to_string()));
    assert!(text.contains("\"state\": \"observed\""));
    assert!(text.contains("export fixture"));
    let replacement = owner.preview(DesktopRequestId::new(), &fixture.result)?;
    assert_ne!(preview.id(), replacement.id());
    assert_eq!(preview.bytes(), repeated.bytes());
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn native_apply_publishes_only_frozen_bytes_and_retries_return_the_exact_receipt()
-> DesktopWorkbenchExportTestResult {
    let fixture = DesktopWorkbenchExportTestFixture::new().await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    let unrelated = directory.path().join("unrelated.json");
    std::fs::write(&unrelated, b"unrelated")?;
    let mut owner = DesktopWorkbenchExport::new();
    let preview = fixture.preview(&mut owner)?;
    let receipt = DesktopWorkbenchExportTestFixture::receipt()?;
    let request =
        DesktopWorkbenchExportTestFixture::request(&preview, receipt.clone(), path.clone())?;
    let written = owner.apply(request, &fixture.executor).await?;
    assert!(!owner.is_pending());
    assert_eq!(written.bytes(), preview.length());
    assert_eq!(written.sha256(), preview.digest());
    assert_eq!(std::fs::read(&path)?, preview.bytes());
    receipt.cancellation().request();
    assert_eq!(owner.observe().await?, written);
    let retry = DesktopWorkbenchExportTestFixture::request(&preview, receipt, unrelated.clone())?;
    assert_eq!(owner.apply(retry, &fixture.executor).await?, written);
    assert_eq!(std::fs::read(&unrelated)?, b"unrelated");
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 2);
    owner.shutdown_retained(Instant::now()).await?;
    assert_eq!(owner.observe().await?, written);
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn stale_preview_wrong_digest_and_non_destination_selection_cannot_publish()
-> DesktopWorkbenchExportTestResult {
    let fixture = DesktopWorkbenchExportTestFixture::new().await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    std::fs::write(&path, b"original")?;
    let mut owner = DesktopWorkbenchExport::new();
    let preview = fixture.preview(&mut owner)?;
    let mut request = DesktopWorkbenchExportTestFixture::request(
        &preview,
        DesktopWorkbenchExportTestFixture::receipt()?,
        path.clone(),
    )?;
    request.preview = DesktopRequestId::new();
    assert!(matches!(
        owner
            .begin_apply(request, &fixture.executor)
            .err()
            .ok_or("Stale preview accepted")?
            .code(),
        DesktopEnumFailureCode::ResourceRefusal
    ));
    let mut request = DesktopWorkbenchExportTestFixture::request(
        &preview,
        DesktopWorkbenchExportTestFixture::receipt()?,
        path.clone(),
    )?;
    request.digest = PackageDigest::read(&b"different frozen bytes"[..])?;
    assert!(matches!(
        owner
            .begin_apply(request, &fixture.executor)
            .err()
            .ok_or("Wrong digest accepted")?
            .code(),
        DesktopEnumFailureCode::InvalidInput
    ));
    let replacement = owner.preview(DesktopRequestId::new(), &fixture.result)?;
    let stale = DesktopWorkbenchExportTestFixture::request(
        &preview,
        DesktopWorkbenchExportTestFixture::receipt()?,
        path.clone(),
    )?;
    assert!(owner.begin_apply(stale, &fixture.executor).is_err());
    let mut request = DesktopWorkbenchExportTestFixture::request(
        &replacement,
        DesktopWorkbenchExportTestFixture::receipt()?,
        path.clone(),
    )?;
    request.destination = DesktopFileEnumSelection::Directory {
        path: directory.path().to_path_buf(),
        identity: same_file::Handle::from_path(directory.path())?,
    };
    let failure = owner
        .apply(request, &fixture.executor)
        .await
        .err()
        .ok_or("Wrong selection accepted")?;
    assert!(matches!(
        failure.code(),
        DesktopEnumFailureCode::FileUnavailable
    ));
    assert!(matches!(
        owner.error(),
        Some(DesktopWorkbenchExportError::Selection(
            DesktopFileHandleError::WrongType
        ))
    ));
    assert_eq!(std::fs::read(&path)?, b"original");
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn shared_capacity_refusal_is_retryable_without_starting_a_file_job()
-> DesktopWorkbenchExportTestResult {
    let fixture = DesktopWorkbenchExportTestFixture::new().await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    std::fs::write(&path, b"original")?;
    let mut owner = DesktopWorkbenchExport::new();
    let preview = fixture.preview(&mut owner)?;
    let receipt = DesktopWorkbenchExportTestFixture::receipt()?;
    let lease = fixture.budget.try_lease(1)?;
    let request =
        DesktopWorkbenchExportTestFixture::request(&preview, receipt.clone(), path.clone())?;
    let failure = owner
        .begin_apply(request, &fixture.executor)
        .err()
        .ok_or("Capacity overcommitted")?;
    assert!(matches!(
        failure.code(),
        DesktopEnumFailureCode::ResourceRefusal
    ));
    assert!(failure.is_retryable());
    assert!(!owner.is_pending());
    assert_eq!(std::fs::read(&path)?, b"original");
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    drop(lease);
    let request = DesktopWorkbenchExportTestFixture::request(&preview, receipt, path.clone())?;
    assert_eq!(
        owner.apply(request, &fixture.executor).await?.sha256(),
        preview.digest()
    );
    assert_eq!(std::fs::read(&path)?, preview.bytes());
    fixture.executor.shutdown().await?;
    Ok(())
}

#[derive(Clone, Copy)]
enum DesktopWorkbenchExportTestEnumFailure {
    PartialWrite,
    Commit,
}

struct DesktopWorkbenchExportTestFailurePort<'a> {
    native: DesktopWorkbenchExportPort<'a>,
    failure: DesktopWorkbenchExportTestEnumFailure,
}

impl WorkbenchExportPort for DesktopWorkbenchExportTestFailurePort<'_> {
    type Error = StorageAtomicFileError;

    fn is_cancelled(&self) -> bool {
        self.native.is_cancelled()
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        match self.failure {
            DesktopWorkbenchExportTestEnumFailure::PartialWrite => {
                self.native.write(&bytes[..bytes.len() / 2])?;
                Err(std::io::Error::other("Controlled partial native write").into())
            }
            DesktopWorkbenchExportTestEnumFailure::Commit => self.native.write(bytes),
        }
    }

    fn commit(&mut self) -> Result<(), Self::Error> {
        Err(std::io::Error::other("Controlled native commit failure").into())
    }

    fn abort(&mut self) {
        self.native.abort();
    }
}

#[tokio::test]
async fn partial_write_and_commit_failures_abort_real_staging_and_preserve_sources()
-> DesktopWorkbenchExportTestResult {
    let fixture = DesktopWorkbenchExportTestFixture::new().await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    std::fs::write(&path, b"original")?;
    let mut owner = DesktopWorkbenchExport::new();
    let preview = fixture.preview(&mut owner)?;
    for failure in [
        DesktopWorkbenchExportTestEnumFailure::PartialWrite,
        DesktopWorkbenchExportTestEnumFailure::Commit,
    ] {
        let destination = StorageAtomicFileTarget::capture(path.clone())?;
        let frozen = Arc::clone(&preview);
        let cancellation = DesktopWorkbenchExportTestFixture::receipt()?.cancellation();
        let result = fixture
            .executor
            .submit(move |worker| {
                let mut port = DesktopWorkbenchExportTestFailurePort {
                    native: DesktopWorkbenchExportPort {
                        stage: Some(StorageAtomicFileStage::new(destination)?),
                        worker,
                        cancellation: &cancellation,
                    },
                    failure,
                };
                let result = frozen.encoded.write(&mut port);
                assert!(port.native.stage.is_none());
                Ok::<_, StorageAtomicFileError>(result)
            })?
            .join()
            .await??;
        let error = result.err().ok_or("A partial export produced a receipt")?;
        assert!(matches!(
            &error,
            WorkbenchExportWriteError::Destination(StorageAtomicFileError::Io(_))
        ));
        assert!(error.source().is_some());
        let message = error.to_string();
        match failure {
            DesktopWorkbenchExportTestEnumFailure::PartialWrite => {
                assert!(message.contains("Controlled partial native write"))
            }
            DesktopWorkbenchExportTestEnumFailure::Commit => {
                assert!(message.contains("Controlled native commit failure"))
            }
        }
        assert_eq!(std::fs::read(&path)?, b"original");
        assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    }
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn indeterminate_publication_is_retained_and_refuses_blind_replay_or_shutdown_success()
-> DesktopWorkbenchExportTestResult {
    let fixture = DesktopWorkbenchExportTestFixture::new().await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    std::fs::write(&path, b"unrelated")?;
    let mut owner = DesktopWorkbenchExport::new();
    let preview = fixture.preview(&mut owner)?;
    let receipt = DesktopWorkbenchExportTestFixture::receipt()?;
    owner.writing = Some(DesktopWorkbenchExportWrite {
        preview: preview.id(),
        digest: preview.digest(),
        receipt: receipt.clone(),
        future: Box::pin(async {
            Err(Arc::new(DesktopWorkbenchExportError::Write(
                WorkbenchExportWriteError::Destination(
                    StorageAtomicFileError::PublicationIndeterminate(std::io::Error::other(
                        "Controlled postpublication synchronization failure",
                    )),
                ),
            )))
        }),
    });
    let failure = owner
        .observe()
        .await
        .err()
        .ok_or("Indeterminate publication returned success")?;
    assert!(!failure.is_retryable());
    assert!(owner.error().is_some_and(|error| error.source().is_some()));
    let request = DesktopWorkbenchExportTestFixture::request(&preview, receipt, path.clone())?;
    let replay = owner
        .apply(request, &fixture.executor)
        .await
        .err()
        .ok_or("Indeterminate replay returned success")?;
    assert_eq!(replay.message(), failure.message());
    assert!(!owner.is_pending());
    assert!(
        owner
            .preview(DesktopRequestId::new(), &fixture.result)
            .is_err()
    );
    assert!(owner.shutdown_retained(Instant::now()).await.is_err());
    let request = DesktopWorkbenchExportTestFixture::request(
        &preview,
        DesktopWorkbenchExportTestFixture::receipt()?,
        path.clone(),
    )?;
    assert!(owner.begin_apply(request, &fixture.executor).is_err());
    assert_eq!(std::fs::read(&path)?, b"unrelated");
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn stale_destination_and_prepublication_cancellation_preserve_the_selected_file()
-> DesktopWorkbenchExportTestResult {
    let fixture = DesktopWorkbenchExportTestFixture::new().await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    std::fs::write(&path, b"original")?;
    let mut owner = DesktopWorkbenchExport::new();
    let preview = fixture.preview(&mut owner)?;
    let request = DesktopWorkbenchExportTestFixture::request(
        &preview,
        DesktopWorkbenchExportTestFixture::receipt()?,
        path.clone(),
    )?;
    std::fs::write(&path, b"external replacement after selection")?;
    assert!(owner.apply(request, &fixture.executor).await.is_err());
    assert!(matches!(
        owner.error(),
        Some(DesktopWorkbenchExportError::Staging(
            StorageAtomicFileError::StaleSelection
        ))
    ));
    let receipt = DesktopWorkbenchExportTestFixture::receipt()?;
    receipt.cancellation().request();
    let request = DesktopWorkbenchExportTestFixture::request(&preview, receipt, path.clone())?;
    let failure = owner
        .begin_apply(request, &fixture.executor)
        .err()
        .ok_or("Cancellation accepted")?;
    assert!(matches!(failure.code(), DesktopEnumFailureCode::Cancelled));
    assert_eq!(
        std::fs::read(&path)?,
        b"external replacement after selection"
    );
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    let retry = DesktopWorkbenchExportTestFixture::request(
        &preview,
        DesktopWorkbenchExportTestFixture::receipt()?,
        path.clone(),
    )?;
    assert_eq!(
        owner.apply(retry, &fixture.executor).await?.sha256(),
        preview.digest()
    );
    assert_eq!(std::fs::read(&path)?, preview.bytes());
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn dropped_wait_retains_native_stage_job_and_exact_receipt()
-> DesktopWorkbenchExportTestResult {
    let fixture = DesktopWorkbenchExportTestFixture::new().await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    std::fs::write(&path, b"original")?;
    let mut owner = DesktopWorkbenchExport::new();
    let preview = fixture.preview(&mut owner)?;
    let request = DesktopWorkbenchExportTestFixture::request(
        &preview,
        DesktopWorkbenchExportTestFixture::receipt()?,
        path.clone(),
    )?;
    let (staged, release) = fixture.controlled(&mut owner, &preview, request)?;
    staged.await?;
    let mut wait = Box::pin(owner.observe());
    assert!(
        poll_fn(|context| Poll::Ready(wait.as_mut().poll(context)))
            .await
            .is_pending()
    );
    drop(wait);
    assert!(owner.is_pending());
    assert_eq!(fixture.budget.available(), 0);
    assert_eq!(std::fs::read(&path)?, b"original");
    let another = DesktopWorkbenchExportTestFixture::request(
        &preview,
        DesktopWorkbenchExportTestFixture::receipt()?,
        directory.path().join("unrelated.json"),
    )?;
    assert!(owner.begin_apply(another, &fixture.executor).is_err());
    assert!(!directory.path().join("unrelated.json").exists());
    assert!(
        owner
            .preview(DesktopRequestId::new(), &fixture.result)
            .is_err()
    );
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 2);
    release.send(())?;
    let receipt = owner.observe().await?;
    assert!(!owner.is_pending());
    assert_eq!(fixture.budget.available(), fixture.budget.workers());
    assert_eq!(receipt.sha256(), preview.digest());
    assert_eq!(receipt.bytes(), preview.length());
    assert_eq!(owner.observe().await?, receipt);
    assert_eq!(std::fs::read(&path)?, preview.bytes());
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn expired_shutdown_keeps_the_pending_receipt_and_publication_guard_cancels_the_stage()
-> DesktopWorkbenchExportTestResult {
    let fixture = DesktopWorkbenchExportTestFixture::new().await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    std::fs::write(&path, b"original")?;
    let mut owner = DesktopWorkbenchExport::new();
    let preview = fixture.preview(&mut owner)?;
    let receipt = DesktopWorkbenchExportTestFixture::receipt()?;
    let request =
        DesktopWorkbenchExportTestFixture::request(&preview, receipt.clone(), path.clone())?;
    let (staged, release) = fixture.controlled(&mut owner, &preview, request)?;
    staged.await?;
    assert!(owner.shutdown_retained(Instant::now()).await.is_err());
    assert!(owner.is_pending());
    assert!(receipt.is_cancellation_requested());
    assert!(
        owner
            .writing
            .as_ref()
            .is_some_and(|writing| writing.receipt.shares_outcome(&receipt))
    );
    assert_eq!(std::fs::read(&path)?, b"original");
    release.send(())?;
    let failure = owner
        .observe()
        .await
        .err()
        .ok_or("Cancelled stage published")?;
    assert!(matches!(failure.code(), DesktopEnumFailureCode::Cancelled));
    assert!(matches!(
        owner.error(),
        Some(DesktopWorkbenchExportError::Write(
            WorkbenchExportWriteError::Destination(StorageAtomicFileError::Cancelled)
        ))
    ));
    assert!(owner.observe().await.is_err());
    owner.shutdown_retained(Instant::now()).await?;
    assert_eq!(std::fs::read(&path)?, b"original");
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    fixture.executor.shutdown().await?;
    Ok(())
}
