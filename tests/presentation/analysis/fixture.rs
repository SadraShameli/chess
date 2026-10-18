use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::Mutex;
use strum::IntoEnumIterator;

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockDuration;
use chess::desktop::identity::DesktopRequestId;
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::options::{EngineOptionsEnumValue, EngineOptionsSetting};
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::engine::package::selector::{EnginePackageSelected, EnginePackageSelector};
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision, EngineSearchTag,
};
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumEvent, EngineSearchSessionMetadata,
    EngineSearchSessionRequest, EngineSearchSessionTerminal,
};
use chess::engine::session::EngineSession;
use chess::package::digest::PackageDigest;
use chess::presentation::analysis::controller::{
    PresentationAnalysisController, PresentationAnalysisControllerEnumAction,
    PresentationAnalysisControllerEnumWorkspace, PresentationAnalysisControllerSelection,
};
use tokio::sync::mpsc;
use tokio::time::Instant;

pub(super) type PresentationAnalysisTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationAnalysisFixtureProvider(Mutex<Option<mpsc::Receiver<EngineProcessLine>>>);

impl EngineProcessProvider for PresentationAnalysisFixtureProvider {
    type Process = PresentationAnalysisFixtureProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        Ok(PresentationAnalysisFixtureProcess {
            input: self
                .0
                .lock()
                .map_err(|_error| std::io::Error::other("fixture lock poisoned"))?
                .take()
                .ok_or_else(|| std::io::Error::other("fixture already started"))?,
            replies: VecDeque::new(),
            is_closed: false,
        })
    }
}

pub(super) struct PresentationAnalysisFixtureProcess {
    input: mpsc::Receiver<EngineProcessLine>,
    replies: VecDeque<EngineProcessLine>,
    is_closed: bool,
}

impl PresentationAnalysisFixtureProcess {
    fn reply(&mut self, text: String) -> Result<(), EngineProcessError> {
        self.replies
            .push_back(EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text)?);
        Ok(())
    }
}

impl EngineProcess for PresentationAnalysisFixtureProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        match command {
            "uci" => {
                self.reply("id name Reckless 0.9.0".into())?;
                self.reply(format!("id author {}", EngineIdentity::AUTHOR))?;
                for option in EngineEnumOption::iter() {
                    self.reply(format!(
                        "option name {} type {}",
                        option.name(),
                        option.descriptor()
                    ))?;
                }
                self.reply("uciok".into())?;
            }
            "isready" => self.reply("readyok".into())?,
            value => {
                if let Some(value) = value.strip_prefix("setoption name ") {
                    let (name, number) = value
                        .split_once(" value ")
                        .ok_or(EngineProcessError::InvalidCommand)?;
                    let suffix = if name == EngineEnumOption::Hash.name() {
                        " MB"
                    } else if name == EngineEnumOption::MoveOverhead.name() {
                        " ms"
                    } else {
                        ""
                    };
                    self.reply(format!("info string set {name} to {number}{suffix}"))?;
                }
            }
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        match self.replies.pop_front() {
            Some(line) => Ok(line),
            None => self
                .input
                .recv()
                .await
                .ok_or(EngineProcessError::Closed(EngineProcessEnumPipe::Stdout)),
        }
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.is_closed = true;
        Ok(())
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }

    fn is_shutdown_complete(&self) -> bool {
        self.is_closed
    }

    async fn terminate(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }
}

pub(super) struct PresentationAnalysisFixture {
    session: EngineSearchSession<PresentationAnalysisFixtureProcess>,
    input: mpsc::Sender<EngineProcessLine>,
    budget: CapacityBudget,
    _directory: tempfile::TempDir,
}

impl PresentationAnalysisFixture {
    pub(super) async fn new() -> PresentationAnalysisTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let bytes = b"analysis UI transcript fixture";
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
            "fixture compiler".into(),
        )?)?;
        let host = EnginePackageHost::from_observation(target, "14.0", "", true)?;
        let package = EnginePackageSelector::select(directory.path(), &manifest, host)?;
        let (input, output) = mpsc::channel(64);
        let provider = PresentationAnalysisFixtureProvider(Mutex::new(Some(output)));
        let mut engine = EngineSession::start(&provider, package).await?;
        engine
            .configure_resources(EngineResourceConfig::new(1, 32, ClockDuration::ZERO)?)
            .await?;
        engine
            .configure_option(EngineOptionsSetting::new(
                EngineOptionsEnumValue::MultiPv(2),
                engine.identity().capabilities(),
            )?)
            .await?;
        Ok(Self {
            session: EngineSearchSession::new(engine),
            input,
            budget: CapacityBudget::new(NonZeroUsize::MIN)?,
            _directory: directory,
        })
    }

    pub(super) fn position() -> PresentationAnalysisTestResult<EngineSearchPosition> {
        Ok(EngineSearchPosition::new(
            BoardPosition::standard(),
            std::iter::empty::<&str>(),
        )?)
    }

    pub(super) fn selection(
        position: EngineSearchPosition,
        revision: u64,
    ) -> PresentationAnalysisTestResult<PresentationAnalysisControllerSelection> {
        Ok(PresentationAnalysisControllerSelection::new(
            EngineSearchContext::new(
                EngineSearchGameId::try_from(uuid::Uuid::from_u128(1))?,
                EngineSearchRevision::new(revision),
            ),
            position,
            PresentationAnalysisControllerEnumWorkspace::Analysis,
        ))
    }

    pub(super) fn controller(
        &self,
        position: EngineSearchPosition,
        revision: u64,
    ) -> PresentationAnalysisTestResult<PresentationAnalysisController> {
        Ok(PresentationAnalysisController::new(Self::selection(
            position, revision,
        )?))
    }

    pub(super) async fn start(
        &mut self,
        controller: &mut PresentationAnalysisController,
    ) -> PresentationAnalysisTestResult<EngineSearchTag> {
        let request = DesktopRequestId::new();
        let intent = controller
            .start(
                chess::engine::search::limit::EngineSearchLimit::infinite(),
                request,
            )?
            .ok_or("missing Start")?;
        let PresentationAnalysisControllerEnumAction::Start(limit) = intent.action() else {
            return Err("expected Start".into());
        };
        let tag = self
            .session
            .start(EngineSearchSessionRequest::new(
                intent.selection().context(),
                intent.selection().position().clone(),
                *limit,
                self.budget.try_lease(1)?,
            ))
            .await?;
        controller.observe_started(request, self.metadata()?)?;
        Ok(tag)
    }

    pub(super) fn metadata(&self) -> PresentationAnalysisTestResult<&EngineSearchSessionMetadata> {
        self.session
            .metadata()
            .ok_or_else(|| "missing metadata".into())
    }

    pub(super) async fn emit(&self, text: &str) -> PresentationAnalysisTestResult {
        self.input
            .send(EngineProcessLine::new(
                EngineProcessEnumPipe::Stdout,
                text.into(),
            )?)
            .await?;
        Ok(())
    }

    pub(super) async fn next(
        &mut self,
        tag: EngineSearchTag,
    ) -> PresentationAnalysisTestResult<EngineSearchSessionEnumEvent> {
        Ok(self.session.next_event(tag).await?)
    }

    pub(super) async fn snapshot(
        &mut self,
        tag: EngineSearchTag,
        lines: &[&str],
    ) -> PresentationAnalysisTestResult<EngineSearchSessionEnumEvent> {
        for text in lines {
            self.emit(text).await?;
        }
        self.next(tag).await
    }

    pub(super) async fn cancel(
        &mut self,
    ) -> PresentationAnalysisTestResult<EngineSearchSessionTerminal> {
        self.session
            .shutdown_retained(Instant::now() + std::time::Duration::from_secs(3))
            .await?;
        self.session
            .terminal()
            .cloned()
            .ok_or_else(|| "missing cancelled terminal".into())
    }

    pub(super) async fn stop(
        &mut self,
        tag: EngineSearchTag,
    ) -> PresentationAnalysisTestResult<EngineSearchSessionTerminal> {
        self.emit("bestmove e2e4").await?;
        Ok(self
            .session
            .stop(chess::engine::search::stop::EngineSearchStop::received_at(
                tag,
                Instant::now(),
            )?)
            .await?)
    }

    pub(super) async fn close(&mut self) -> PresentationAnalysisTestResult {
        self.session
            .shutdown_retained(Instant::now() + std::time::Duration::from_secs(3))
            .await?;
        assert_eq!(self.budget.available(), 1);
        Ok(())
    }
}
