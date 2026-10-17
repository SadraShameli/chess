use strum::IntoEnumIterator;
use std::collections::VecDeque;

use bevy_egui::egui;
use egui_kittest::Harness;

use chess::board::position::BoardPosition;
use chess::clock::duration::ClockInstant;
use chess::desktop::identity::DesktopRevision;
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::engine::package::selector::{EnginePackageSelected, EnginePackageSelector};
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::session::EngineSession;
use chess::package::digest::PackageDigest;
use chess::presentation::theme::font::PresentationThemeFont;
use chess::presentation::workbench::controller::{
    PresentationWorkbenchController, PresentationWorkbenchControllerContext,
    PresentationWorkbenchControllerEnumAdmission, PresentationWorkbenchControllerIntent,
};
use chess::presentation::workbench::snapshot::PresentationWorkbenchSnapshot;
use chess::presentation::workbench::view::PresentationWorkbenchView;
use chess::workbench::enums::WorkbenchEnumParameters;
use chess::workbench::job::{WorkbenchJob, WorkbenchJobDeadline, WorkbenchJobId};

pub(super) type PresentationWorkbenchTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) struct PresentationWorkbenchFixtureProcess {
    replies: VecDeque<EngineProcessLine>,
    closed: bool,
}

struct PresentationWorkbenchFixtureProvider;

impl EngineProcessProvider for PresentationWorkbenchFixtureProvider {
    type Process = PresentationWorkbenchFixtureProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        Ok(PresentationWorkbenchFixtureProcess {
            replies: VecDeque::new(),
            closed: false,
        })
    }
}

impl EngineProcess for PresentationWorkbenchFixtureProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        let lines = match command {
            "uci" => {
                let mut lines = vec![
                    "id name Reckless 0.9.0".to_owned(),
                    format!("id author {}", EngineIdentity::AUTHOR),
                ];
                for option in EngineEnumOption::iter() {
                    let descriptor = if option == EngineEnumOption::Threads {
                        "spin default 1 min 1 max 2048".to_owned()
                    } else {
                        option.descriptor()
                    };
                    lines.push(format!("option name {} type {descriptor}", option.name()));
                }
                lines.push("uciok".to_owned());
                lines
            }
            "isready" => vec!["readyok".to_owned()],
            _ => return Err(EngineProcessError::InvalidCommand),
        };
        for text in lines {
            self.replies
                .push_back(EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text)?);
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        self.replies
            .pop_front()
            .ok_or(EngineProcessError::Closed(EngineProcessEnumPipe::Stdout))
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.closed = true;
        Ok(())
    }

    async fn shutdown_retained(
        &mut self,
        _deadline: tokio::time::Instant,
    ) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }

    async fn terminate(
        &mut self,
        _deadline: tokio::time::Instant,
    ) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }

    fn is_shutdown_complete(&self) -> bool {
        self.closed
    }
}

pub(super) struct PresentationWorkbenchFixture {
    pub(super) engine: EngineSession<PresentationWorkbenchFixtureProcess>,
    _directory: tempfile::TempDir,
}

impl PresentationWorkbenchFixture {
    pub(super) async fn new() -> PresentationWorkbenchTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let bytes = b"workbench typed presentation fixture";
        let target = EnginePackageEnumTarget::MacosArm;
        std::fs::create_dir(directory.path().join("bin"))?;
        let executable = directory.path().join(target.relative_path());
        std::fs::write(&executable, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        }
        let manifest = EnginePackageManifest::new(EnginePackageEntry::portable(
            target,
            u64::try_from(bytes.len())?,
            PackageDigest::read(&bytes[..])?,
            "fixture compiler".to_owned(),
        )?)?;
        let package = EnginePackageSelector::select(
            directory.path(),
            &manifest,
            EnginePackageHost::from_observation(target, "14.0", "", true)?,
        )?;
        Ok(Self {
            engine: EngineSession::start(&PresentationWorkbenchFixtureProvider, package).await?,
            _directory: directory,
        })
    }

    pub(super) fn snapshot(
        &self,
    ) -> PresentationWorkbenchTestResult<PresentationWorkbenchSnapshot> {
        Ok(PresentationWorkbenchSnapshot::capture(
            self.engine.package(),
            self.engine.identity().capabilities(),
            self.engine.applied_options(),
            None,
        )?)
    }

    pub(super) fn controller(
        &self,
    ) -> PresentationWorkbenchTestResult<PresentationWorkbenchController> {
        let context = PresentationWorkbenchControllerContext::new(
            self.engine.generation(),
            DesktopRevision::ZERO,
            EngineSearchContext::new(
                EngineSearchGameId::try_from(uuid::Uuid::from_u128(1))?,
                EngineSearchRevision::new(0),
            ),
        );
        let mut controller = PresentationWorkbenchController::new(
            context,
            Some(EngineSearchPosition::new(
                BoardPosition::standard(),
                std::iter::empty::<&str>(),
            )?),
        );
        controller.receive_engine(context, self.snapshot()?)?;
        controller.set_admission(
            context,
            PresentationWorkbenchControllerEnumAdmission::Idle {
                available_workers: 4,
            },
        )?;
        Ok(controller)
    }

    pub(super) fn job(
        &self,
        parameters: WorkbenchEnumParameters,
        id: u128,
    ) -> PresentationWorkbenchTestResult<WorkbenchJob> {
        Ok(WorkbenchJob::new(
            WorkbenchJobId::try_from(uuid::Uuid::from_u128(id))?,
            parameters,
            self.engine.package().clone(),
            WorkbenchJobDeadline::default(),
            ClockInstant::from_nanos(0),
        )?)
    }
}

pub(super) struct PresentationWorkbenchMounted {
    pub(super) controller: PresentationWorkbenchController,
    pub(super) view: PresentationWorkbenchView,
    pub(super) intents: Vec<PresentationWorkbenchControllerIntent>,
    needs_fonts: bool,
}

impl PresentationWorkbenchMounted {
    pub(super) fn harness(
        controller: PresentationWorkbenchController,
    ) -> PresentationWorkbenchTestResult<Harness<'static, Self>> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 2400.0))
            .build_ui_state(
                Self::show,
                Self {
                    controller,
                    view: PresentationWorkbenchView::new(),
                    intents: Vec::new(),
                    needs_fonts: true,
                },
            );
        harness.run_ok().ok_or("Workbench did not settle")?;
        Ok(harness)
    }

    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_fonts {
            ui.ctx().set_fonts(PresentationThemeFont::definitions());
            ui.ctx().request_repaint();
            state.needs_fonts = false;
            return;
        }
        if let Some(intent) = state.view.show(ui, &mut state.controller) {
            state.intents.push(intent);
            ui.ctx()
                .request_discard("Exercise repeated workbench passes");
        }
    }
}
