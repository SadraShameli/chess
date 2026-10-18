use std::num::{NonZeroU32, NonZeroU64};
use std::sync::Arc;

use bevy_egui::egui::{self, Context, RawInput, accesskit};
use thiserror::Error;
use tokio::sync::watch;

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::board::position::BoardPosition;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::time_provider::{ClockTimeProvider, ClockTimeProviderMonotonic};
use crate::desktop::application::{DesktopApplication, DesktopApplicationWork};
use crate::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot, DesktopEnumValue};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::messages::{DesktopEnumCommand, DesktopMessagesEntityId};
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
use crate::desktop::puzzle_bridge::{
    DesktopPuzzleBridgeEnumOperation, DesktopPuzzleBridgeEnumResult,
};
use crate::desktop::puzzle_commands::DesktopPuzzleCommands;
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceEnumOperation,
    DesktopPuzzleServiceEnumResult, DesktopPuzzleServiceState,
};
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::presentation::app::{PresentationApp, PresentationAppEnumWorkspace};
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::puzzle::controller::{
    PresentationPuzzleController, PresentationPuzzleControllerEnumAction,
};
use crate::presentation::puzzle::panel::{
    PresentationPuzzlePanel, PresentationPuzzlePanelConnection,
};
use crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus;
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::PuzzleEnumCountdownMode;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::PuzzleDefinitionMetadata;
use crate::puzzle::persistence::{
    PuzzlePersistenceEnumCommand, PuzzlePersistenceReceipt, PuzzlePersistenceRun,
    PuzzlePersistenceSaveRequest, PuzzlePersistenceStorageRevision,
};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::run::{PuzzleRunEnumConfirmation, PuzzleRunEnumStatus};
use crate::puzzle::selection::PuzzleSelectionFilter;
use crate::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use crate::puzzle::storage_repository::{PuzzleStorageRepository, PuzzleStorageWrite};
use crate::puzzle::timer::PuzzleTimerEnumReadiness;
use crate::scene::camera::SceneCamera;
use crate::scene::enums::SceneEnumLifecycle;
use crate::scene::picking::ScenePickingViewport;
use crate::scene::renderer::SceneRendererGeneration;

#[test]
fn empty_puzzle_workspace_never_falls_back_to_the_bootstrap_position()
-> Result<(), Box<dyn std::error::Error>> {
    let application = DesktopApplication::new(
        Arc::new(ClockTimeProviderMonotonic::new()),
        Arc::new(DesktopNativeEventLoopWake::new()),
    );
    let port = application.port();
    let mut app = PresentationApp::new(Arc::new(ClockTimeProviderMonotonic::new()));
    app.accept_snapshot(DesktopBootstrapSnapshot::new(
        &BoardPosition::standard(),
        NonZeroU32::MIN,
        DesktopEnumEngineSnapshot::Unavailable {
            message: "no engine needed for puzzle workspace routing".into(),
        },
    ));
    let (_state, receiver) = watch::channel(DesktopPuzzleServiceState::new(
        None,
        None,
        DesktopPuzzleServiceEnumActivity::Idle,
        None,
        None,
    ));
    app.mount_puzzles(PresentationPuzzlePanelConnection {
        owner: DesktopMessagesEntityId::new(),
        state: receiver,
    });
    let initial = app
        .board_snapshot()
        .ok_or("missing bootstrap board")?
        .fen()
        .to_owned();
    app.select_workspace(PresentationAppEnumWorkspace::Puzzles, &port)?;
    assert_eq!(app.workspace(), PresentationAppEnumWorkspace::Puzzles);
    assert!(app.board_snapshot().is_none());
    let work = application
        .try_next()?
        .ok_or("missing saved-run inventory request")?;
    application.complete_failure(
        &work,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "no fixture service".into(),
            true,
        ),
    )?;
    app.synchronize_puzzles(&port)?;
    assert!(app.board_snapshot().is_none());
    app.select_workspace(PresentationAppEnumWorkspace::Games, &port)?;
    assert_eq!(
        app.board_snapshot()
            .ok_or("missing restored game workspace")?
            .fen(),
        initial
    );
    app.mount_library(DesktopMessagesEntityId::new());
    app.select_workspace(PresentationAppEnumWorkspace::Library, &port)?;
    assert_eq!(app.workspace(), PresentationAppEnumWorkspace::Library);
    assert!(app.board_snapshot().is_none());
    assert!(app.game_snapshot().is_none());
    assert!(!app.is_puzzle_input_ready());
    app.select_workspace(PresentationAppEnumWorkspace::Games, &port)?;
    assert_eq!(
        app.board_snapshot()
            .ok_or("missing restored Play board")?
            .fen(),
        initial
    );
    Ok(())
}

type PresentationAppPuzzleTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationAppPuzzleTestClock;

impl ClockTimeProvider for PresentationAppPuzzleTestClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(0))
    }
}

#[derive(Debug, Error)]
enum PresentationAppPuzzleTestStorageError {
    #[error("The puzzle save receipt was already consumed")]
    Consumed,
    #[error("The puzzle storage revision overflowed")]
    Revision,
}

struct PresentationAppPuzzleTestWrite(Option<PuzzlePersistenceReceipt>);

impl PuzzleStorageWrite for PresentationAppPuzzleTestWrite {
    type Error = PresentationAppPuzzleTestStorageError;

    async fn wait(&mut self) -> Result<PuzzlePersistenceReceipt, Self::Error> {
        self.0
            .take()
            .ok_or(PresentationAppPuzzleTestStorageError::Consumed)
    }
}

struct PresentationAppPuzzleTestRepository;

impl PuzzleStorageRepository for PresentationAppPuzzleTestRepository {
    type Error = PresentationAppPuzzleTestStorageError;
    type Write = PresentationAppPuzzleTestWrite;

    fn begin_save(
        &self,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<Self::Write, Self::Error> {
        let revision = request
            .expected_storage_revision()
            .value()
            .checked_add(1)
            .ok_or(PresentationAppPuzzleTestStorageError::Revision)?;
        Ok(PresentationAppPuzzleTestWrite(Some(
            PuzzlePersistenceReceipt::committed(
                request,
                PuzzlePersistenceStorageRevision::new(revision),
            ),
        )))
    }
}

struct PresentationAppPuzzleFixture {
    application: DesktopApplication<PresentationAppPuzzleTestClock, DesktopNativeEventLoopWake>,
    state: watch::Sender<DesktopPuzzleServiceState>,
    owner: DesktopPuzzleCommands<PresentationAppPuzzleTestRepository>,
    context: Context,
}

impl PresentationAppPuzzleFixture {
    async fn new(initial: BoardPosition, moves: &[&str]) -> PresentationAppPuzzleTestResult<Self> {
        let mut position = initial.clone();
        let mut line = Vec::new();
        for uci in moves {
            let movement = position.parse_uci(uci)?;
            position = position.play(movement)?;
            line.push(movement);
        }
        let run = PuzzlePersistenceRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![PuzzleDefinition::new(
                PuzzleDefinitionIdentity::new("app-readiness".into(), "v1".into(), "first".into())?,
                initial,
                vec![line],
                PuzzleDefinitionMetadata::default(),
            )?],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(31),
        )?;
        let mut owner = DesktopPuzzleCommands::new(
            run,
            PresentationAppPuzzleTestRepository,
            ClockInstant::from_nanos(0),
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?;
        owner.begin_write(ClockInstant::from_nanos(0))?;
        for _ in 0..16 {
            if !owner.has_retained_write() {
                break;
            }
            owner.wait().await?;
            owner.settle(ClockInstant::from_nanos(0))?;
        }
        if owner.has_retained_write() {
            return Err("initial puzzle save did not drain".into());
        }
        let (state, _receiver) = watch::channel(DesktopPuzzleServiceState::new(
            None,
            None,
            DesktopPuzzleServiceEnumActivity::Idle,
            None,
            None,
        ));
        let context = Context::default();
        context.enable_accesskit();
        Ok(Self {
            application: DesktopApplication::new(
                Arc::new(PresentationAppPuzzleTestClock),
                Arc::new(DesktopNativeEventLoopWake::new()),
            ),
            state,
            owner,
            context,
        })
    }

    fn next(&self) -> PresentationAppPuzzleTestResult<DesktopApplicationWork> {
        self.application
            .try_next()?
            .ok_or_else(|| "missing puzzle application work".into())
    }

    fn command(
        work: &DesktopApplicationWork,
    ) -> PresentationAppPuzzleTestResult<PuzzlePersistenceEnumCommand> {
        if let DesktopEnumCommand::Puzzle {
            operation:
                DesktopPuzzleServiceEnumOperation::Session {
                    operation: DesktopPuzzleBridgeEnumOperation::Command(command),
                    ..
                },
            ..
        } = work.request().command().operation()
        {
            Ok(command.clone())
        } else {
            Err("unexpected puzzle application work".into())
        }
    }

    fn publish(&self, snapshot: Arc<DesktopPuzzleSnapshot>) {
        self.state.send_replace(DesktopPuzzleServiceState::new(
            Some(snapshot.context().session_id()),
            Some(snapshot),
            DesktopPuzzleServiceEnumActivity::Selected,
            None,
            None,
        ));
    }

    fn open(&mut self) -> PresentationAppPuzzleTestResult<PresentationPuzzlePanel> {
        let mut panel = PresentationPuzzlePanel::new(PresentationPuzzlePanelConnection {
            owner: DesktopMessagesEntityId::new(),
            state: self.state.subscribe(),
        });
        panel.open_saved(
            self.owner.context().session_id(),
            PuzzlePersistenceStorageRevision::new(1),
            &self.application.port(),
        )?;
        let work = self.next()?;
        assert!(matches!(
            work.request().command().operation(),
            DesktopEnumCommand::Puzzle {
                operation: DesktopPuzzleServiceEnumOperation::OpenSaved { .. },
                ..
            }
        ));
        self.application.begin_commit(&work)?;
        let snapshot = Arc::new(self.owner.snapshot(ClockInstant::from_nanos(0))?);
        self.publish(Arc::clone(&snapshot));
        self.application.complete_value(
            &work,
            DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Opened(snapshot))),
            None,
        )?;
        panel.synchronize(&self.application.port())?;
        assert!(panel.board().is_some());
        assert!(!panel.has_pending_work());
        Ok(panel)
    }

    fn finish(&mut self, panel: &mut PresentationPuzzlePanel) -> PresentationAppPuzzleTestResult {
        let work = self.next()?;
        let command = Self::command(&work)?;
        let DesktopEnumCommand::Puzzle {
            operation: DesktopPuzzleServiceEnumOperation::Session { expected, .. },
            ..
        } = work.request().command().operation()
        else {
            return Err("missing puzzle command context".into());
        };
        self.application.begin_commit(&work)?;
        self.owner
            .execute(expected, command, ClockInstant::from_nanos(0))?;
        let snapshot = Arc::new(self.owner.snapshot(ClockInstant::from_nanos(0))?);
        self.publish(Arc::clone(&snapshot));
        self.application.complete_value(
            &work,
            DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Session(
                DesktopPuzzleBridgeEnumResult::Snapshot(snapshot),
            ))),
            None,
        )?;
        panel.synchronize(&self.application.port())?;
        Ok(())
    }

    fn start(&mut self, panel: &mut PresentationPuzzlePanel) -> PresentationAppPuzzleTestResult {
        panel.report_readiness(PuzzleTimerEnumReadiness::Ready, &self.application.port())?;
        self.finish(panel)?;
        panel.report_readiness(PuzzleTimerEnumReadiness::Ready, &self.application.port())?;
        assert!(self.application.try_next()?.is_none());
        Ok(())
    }

    fn install(
        &self,
        panel: PresentationPuzzlePanel,
    ) -> PresentationAppPuzzleTestResult<PresentationApp> {
        let mut app = PresentationApp::new(Arc::new(ClockTimeProviderMonotonic::new()));
        app.puzzle_panel = Some(panel);
        app.select_workspace(
            PresentationAppEnumWorkspace::Puzzles,
            &self.application.port(),
        )?;
        Ok(app)
    }

    fn controller(
        app: &PresentationApp,
    ) -> PresentationAppPuzzleTestResult<&PresentationPuzzleController> {
        app.puzzle_panel()
            .and_then(PresentationPuzzlePanel::controller)
            .ok_or_else(|| "missing mounted puzzle controller".into())
    }

    fn frame(
        &self,
        app: &mut PresentationApp,
        scene: Option<&PresentationBoardScene>,
        events: Vec<egui::Event>,
    ) -> PresentationAppPuzzleTestResult<(egui::FullOutput, Option<ScenePickingViewport>)> {
        let mut result = None;
        let mut output = self.context.run_ui(
            RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let mut picker = |_point| Ok(None);
                result = Some(app.show(ui, &self.application.port(), scene, &mut picker));
            },
        );
        output.textures_delta.clear();
        Ok((
            output,
            result
                .ok_or("missing puzzle presentation frame")??
                .viewport(),
        ))
    }

    fn scene(
        &self,
        app: &mut PresentationApp,
    ) -> PresentationAppPuzzleTestResult<PresentationBoardScene> {
        self.frame(app, None, Vec::new())?;
        let (_, viewport) = self.frame(app, None, Vec::new())?;
        let viewport = viewport.ok_or("missing puzzle board viewport")?;
        let scene = PresentationBoardScene::new(
            egui::TextureId::User(31),
            SceneRendererGeneration::initial(),
            SceneEnumLifecycle::Ready,
            viewport,
            SceneCamera::initial(BoardEnumColor::White).fit(viewport.physical_size())?,
        );
        self.frame(app, Some(&scene), Vec::new())?;
        Ok(scene)
    }

    fn assert_end_confirmation_visible(
        &self,
        app: &mut PresentationApp,
    ) -> PresentationAppPuzzleTestResult {
        self.frame(app, None, Vec::new())?;
        let (output, _) = self.frame(app, None, Vec::new())?;
        assert!(
            output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::epaint::Shape::Text(text) if text.galley.job.text == "End this run?"
            )),
            "the mounted End Run modal is missing"
        );
        Ok(())
    }

    fn select(
        &self,
        app: &mut PresentationApp,
        scene: &PresentationBoardScene,
        square: &str,
    ) -> PresentationAppPuzzleTestResult {
        self.frame(
            app,
            Some(scene),
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: PresentationBoardSemanticGrid::square_id(square.parse()?)
                        .accesskit_id(),
                    data: None,
                },
            )],
        )?;
        Ok(())
    }
}

#[tokio::test]
async fn applied_puzzle_board_cannot_start_its_countdown_through_end_confirmation()
-> PresentationAppPuzzleTestResult {
    let mut fixture =
        PresentationAppPuzzleFixture::new(BoardPosition::standard(), &["e2e4", "e7e5", "g1f3"])
            .await?;
    let mut panel = fixture.open()?;
    panel.request_action(
        PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Unconfirmed),
        &fixture.application.port(),
    )?;
    let mut app = fixture.install(panel)?;
    let rendered = app
        .board_snapshot()
        .ok_or("missing applied puzzle board")?
        .clone();
    let controller = PresentationAppPuzzleFixture::controller(&app)?;
    let context = controller.snapshot().context().clone();
    assert_eq!(
        controller.snapshot().status(),
        PuzzleRunEnumStatus::AwaitingReadiness
    );
    assert_eq!(controller.snapshot().timer().elapsed(), ClockDuration::ZERO);
    let current = controller.board().ok_or("missing current puzzle board")?;
    assert_eq!(current.fen(), rendered.fen());
    assert_eq!(current.variant(), rendered.variant());
    assert!(controller.is_confirming_end());
    assert!(fixture.application.try_next()?.is_none());
    fixture.assert_end_confirmation_visible(&mut app)?;

    app.report_puzzle_readiness(PuzzleTimerEnumReadiness::Ready, &fixture.application.port())?;

    let controller = PresentationAppPuzzleFixture::controller(&app)?;
    assert!(
        controller.is_confirming_end(),
        "a native readiness report dismissed End Run confirmation"
    );
    assert!(!app.is_puzzle_input_ready());
    assert!(!controller.is_pending());
    assert!(
        fixture.application.try_next()?.is_none(),
        "Ready was admitted while End Run owned input"
    );
    assert_eq!(controller.snapshot().context(), &context);
    assert_eq!(
        controller.snapshot().status(),
        PuzzleRunEnumStatus::AwaitingReadiness
    );
    assert_eq!(controller.snapshot().timer().elapsed(), ClockDuration::ZERO);
    assert!(controller.snapshot().timer().deadline().is_none());
    fixture.assert_end_confirmation_visible(&mut app)?;
    Ok(())
}

#[tokio::test]
async fn legal_puzzle_promotion_keeps_canonical_choices_enabled() -> PresentationAppPuzzleTestResult
{
    let mut fixture = PresentationAppPuzzleFixture::new(
        BoardPosition::from_fen("7k/P7/8/8/8/8/8/7K w - - 0 1", BoardEnumVariant::Standard)?,
        &["a7a8q"],
    )
    .await?;
    let mut panel = fixture.open()?;
    fixture.start(&mut panel)?;
    let mut app = fixture.install(panel)?;
    let scene = fixture.scene(&mut app)?;
    fixture.select(&mut app, &scene, "a7")?;
    fixture.select(&mut app, &scene, "a8")?;
    let controller = PresentationAppPuzzleFixture::controller(&app)?;
    assert_eq!(controller.promotion_choices().len(), 4);
    assert!(controller.promotion_choices().iter().all(|movement| {
        movement.promotion().is_some()
            && controller.board().is_some_and(|board| {
                board
                    .legal_moves()
                    .iter()
                    .any(|legal| legal.movement() == *movement)
            })
    }));
    assert!(fixture.application.try_next()?.is_none());

    app.report_puzzle_readiness(PuzzleTimerEnumReadiness::Ready, &fixture.application.port())?;

    let controller = PresentationAppPuzzleFixture::controller(&app)?;
    assert_eq!(controller.promotion_choices().len(), 4);
    assert!(app.is_puzzle_input_ready());
    assert!(controller.can_input());
    assert!(controller.promotion_choices().iter().all(|movement| {
        controller.can_request(PresentationPuzzleControllerEnumAction::Move(*movement))
    }));
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[tokio::test]
async fn ordinary_puzzle_save_keeps_root_readiness_and_real_board_input_available()
-> PresentationAppPuzzleTestResult {
    let mut fixture =
        PresentationAppPuzzleFixture::new(BoardPosition::standard(), &["e2e4", "e7e5", "g1f3"])
            .await?;
    let mut panel = fixture.open()?;
    fixture.start(&mut panel)?;
    panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    fixture.finish(&mut panel)?;
    let mut app = fixture.install(panel)?;
    app.report_puzzle_readiness(PuzzleTimerEnumReadiness::Ready, &fixture.application.port())?;
    let controller = PresentationAppPuzzleFixture::controller(&app)?;
    assert!(controller.snapshot().has_retained_write());
    assert!(controller.snapshot().has_unsaved_state());
    assert_eq!(
        controller.snapshot().checkpoint_status(),
        PuzzleCheckpointEnumStatus::Open
    );
    assert!(app.is_puzzle_input_ready());
    assert!(controller.can_input());

    let scene = fixture.scene(&mut app)?;
    fixture.select(&mut app, &scene, "e2")?;
    fixture.select(&mut app, &scene, "e4")?;

    let work = fixture.next()?;
    assert_eq!(
        PresentationAppPuzzleFixture::command(&work)?,
        PuzzlePersistenceEnumCommand::Move(BoardPosition::standard().parse_uci("e2e4")?),
    );
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}
