use std::num::NonZeroU64;
use std::time::Duration;

use bevy_egui::egui;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use crate::board::position::BoardPosition;
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::preferences::DesktopPreferencesEnumTheme;
use crate::desktop::puzzle_commands::DesktopPuzzleCommands;
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::{
    PresentationBoardSemanticGrid, PresentationBoardSemanticGridFeedback,
};
use crate::presentation::puzzle::controller::{
    PresentationPuzzleController, PresentationPuzzleControllerEnumAction,
    PresentationPuzzleControllerError, PresentationPuzzleControllerIntent,
};
use crate::presentation::puzzle::setup::{
    PresentationPuzzleSetup, PresentationPuzzleSetupCollection, PresentationPuzzleSetupIntent,
};
use crate::presentation::puzzle::view::PresentationPuzzleView;
use crate::presentation::theme::tokens::PresentationThemeTokens;
use crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus;
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumPhase, PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::{PuzzleDefinitionDifficulty, PuzzleDefinitionMetadata};
use crate::puzzle::persistence::{
    PuzzlePersistenceEnumCommand, PuzzlePersistenceReceipt, PuzzlePersistenceRun,
    PuzzlePersistenceSaveRequest, PuzzlePersistenceStorageRevision,
};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::run::{PuzzleRunEnumConfirmation, PuzzleRunEnumStatus};
use crate::puzzle::selection::{PuzzleSelectionDataset, PuzzleSelectionFilter};
use crate::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use crate::puzzle::storage_repository::{PuzzleStorageRepository, PuzzleStorageWrite};
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};
use crate::scene::camera::SceneCamera;
use crate::scene::enums::SceneEnumLifecycle;
use crate::scene::picking::ScenePickingViewport;
use crate::scene::renderer::SceneRendererGeneration;

type PresentationPuzzleTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Error)]
enum PresentationPuzzleFixtureError {
    #[error("The controlled storage queue is full or closed")]
    Queue,
    #[error("The controlled storage write failed")]
    Write,
    #[error("The controlled storage receiver was abandoned")]
    Abandoned,
    #[error("The controlled storage revision overflowed")]
    Revision,
    #[error("The controlled storage response closed: {0}")]
    Receive(#[from] oneshot::error::RecvError),
}

struct PresentationPuzzleFixtureSave {
    request: PuzzlePersistenceSaveRequest,
    response: oneshot::Sender<Result<PuzzlePersistenceReceipt, PresentationPuzzleFixtureError>>,
}

impl PresentationPuzzleFixtureSave {
    fn finish(self, fail: bool) -> Result<(), PresentationPuzzleFixtureError> {
        let result = if fail {
            Err(PresentationPuzzleFixtureError::Write)
        } else {
            let revision = self
                .request
                .expected_storage_revision()
                .value()
                .checked_add(1)
                .ok_or(PresentationPuzzleFixtureError::Revision)?;
            Ok(PuzzlePersistenceReceipt::committed(
                self.request,
                PuzzlePersistenceStorageRevision::new(revision),
            ))
        };
        self.response
            .send(result)
            .map_err(|_result| PresentationPuzzleFixtureError::Abandoned)
    }
}

struct PresentationPuzzleFixtureWrite {
    response: oneshot::Receiver<Result<PuzzlePersistenceReceipt, PresentationPuzzleFixtureError>>,
}

impl PuzzleStorageWrite for PresentationPuzzleFixtureWrite {
    type Error = PresentationPuzzleFixtureError;

    async fn wait(&mut self) -> Result<PuzzlePersistenceReceipt, Self::Error> {
        (&mut self.response).await?
    }
}

struct PresentationPuzzleFixtureRepository {
    saves: mpsc::Sender<PresentationPuzzleFixtureSave>,
}

impl PuzzleStorageRepository for PresentationPuzzleFixtureRepository {
    type Error = PresentationPuzzleFixtureError;
    type Write = PresentationPuzzleFixtureWrite;

    fn begin_save(
        &self,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<Self::Write, Self::Error> {
        let (response, receiver) = oneshot::channel();
        self.saves
            .try_send(PresentationPuzzleFixtureSave { request, response })
            .map_err(|_error| PresentationPuzzleFixtureError::Queue)?;
        Ok(PresentationPuzzleFixtureWrite { response: receiver })
    }
}

struct PresentationPuzzleFixture {
    owner: DesktopPuzzleCommands<PresentationPuzzleFixtureRepository>,
    saves: mpsc::Receiver<PresentationPuzzleFixtureSave>,
}

impl PresentationPuzzleFixture {
    fn time(millis: u64) -> Result<ClockInstant, crate::clock::duration::ClockDurationError> {
        ClockInstant::from_millis(millis)
    }

    fn definition(
        initial: BoardPosition,
        line: &[&str],
        index: usize,
    ) -> PresentationPuzzleTestResult<PuzzleDefinition> {
        let mut position = initial.clone();
        let mut moves = Vec::new();
        for uci in line {
            let movement = position.parse_uci(uci)?;
            position = position.play(movement)?;
            moves.push(movement);
        }
        Ok(PuzzleDefinition::new(
            PuzzleDefinitionIdentity::new(
                "presentation-fixture".into(),
                "v1".into(),
                index.to_string(),
            )?,
            initial,
            vec![moves],
            PuzzleDefinitionMetadata::new(
                Some(PuzzleDefinitionDifficulty::new(1400)),
                Some(vec!["fork".parse()?]),
            )?,
        )?)
    }

    fn new(
        initial: BoardPosition,
        line: &[&str],
        count: usize,
    ) -> PresentationPuzzleTestResult<Self> {
        let definitions = (0..count)
            .map(|index| Self::definition(initial.clone(), line, index))
            .collect::<PresentationPuzzleTestResult<Vec<_>>>()?;
        let run = PuzzlePersistenceRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            definitions,
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(81),
        )?;
        Self::from_run(run)
    }

    fn from_run(run: PuzzlePersistenceRun) -> PresentationPuzzleTestResult<Self> {
        let (saves, receiver) = mpsc::channel(1);
        let mut owner = DesktopPuzzleCommands::new(
            run,
            PresentationPuzzleFixtureRepository { saves },
            Self::time(0)?,
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?;
        owner.begin_write(Self::time(0)?)?;
        Ok(Self {
            owner,
            saves: receiver,
        })
    }

    fn foreign_snapshot(millis: u64) -> PresentationPuzzleTestResult<DesktopPuzzleSnapshot> {
        let run = PuzzlePersistenceRun::new(
            PuzzleSessionId::new(NonZeroU64::new(2).ok_or("Invalid fixture session")?),
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![Self::definition(BoardPosition::standard(), &["e2e4"], 0)?],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(81),
        )?;
        Self::from_run(run)?.snapshot(millis)
    }

    fn snapshot(&mut self, millis: u64) -> PresentationPuzzleTestResult<DesktopPuzzleSnapshot> {
        Ok(self.owner.snapshot(Self::time(millis)?)?)
    }

    async fn commit_all(&mut self, millis: u64) -> PresentationPuzzleTestResult {
        for _ in 0..16 {
            if !self.owner.has_retained_write() {
                return Ok(());
            }
            self.saves.try_recv()?.finish(false)?;
            self.owner.wait().await?;
            self.owner.settle(Self::time(millis)?)?;
        }
        Err("Controlled writes did not drain".into())
    }

    async fn apply(
        &mut self,
        controller: &mut PresentationPuzzleController,
        intent: &PresentationPuzzleControllerIntent,
        millis: u64,
    ) -> PresentationPuzzleTestResult {
        let now = Self::time(millis)?;
        if let Some(command) = intent.action().command() {
            self.owner.execute(intent.context(), command, now)?;
        } else {
            self.owner.retry_save(intent.context(), now)?;
        }
        controller.observe_success(intent, self.owner.snapshot(now)?, None)?;
        Ok(())
    }

    async fn ready(
        &mut self,
        controller: &mut PresentationPuzzleController,
        millis: u64,
    ) -> PresentationPuzzleTestResult {
        if let Some(intent) =
            controller.report_readiness(PuzzleTimerEnumReadiness::Ready, DesktopRequestId::new())?
        {
            self.apply(controller, &intent, millis).await?;
        }
        assert!(
            controller
                .report_readiness(PuzzleTimerEnumReadiness::Ready, DesktopRequestId::new())?
                .is_none()
        );
        Ok(())
    }

    async fn start(&mut self) -> PresentationPuzzleTestResult<PresentationPuzzleController> {
        self.commit_all(0).await?;
        let mut controller = PresentationPuzzleController::new(self.snapshot(0)?);
        self.ready(&mut controller, 0).await?;
        Ok(controller)
    }

    fn intent(
        controller: &mut PresentationPuzzleController,
        action: PresentationPuzzleControllerEnumAction,
    ) -> PresentationPuzzleTestResult<PresentationPuzzleControllerIntent> {
        controller
            .request(action, DesktopRequestId::new())?
            .ok_or_else(|| "Missing puzzle intent".into())
    }
}

struct PresentationPuzzleMounted {
    needs_theme: bool,
    controller: PresentationPuzzleController,
    view: PresentationPuzzleView,
    scene: PresentationBoardScene,
    intents: Vec<PresentationPuzzleControllerIntent>,
}

impl PresentationPuzzleMounted {
    fn mount(
        controller: PresentationPuzzleController,
    ) -> PresentationPuzzleTestResult<Harness<'static, Self>> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1600.0));
        let image = egui::Rect::from_min_size(egui::pos2(600.0, 32.0), egui::vec2(680.0, 680.0));
        let viewport = ScenePickingViewport::new(image, screen, 1.0, 1.0)?;
        let scene = PresentationBoardScene::new(
            egui::TextureId::User(1),
            SceneRendererGeneration::initial(),
            SceneEnumLifecycle::Ready,
            viewport,
            SceneCamera::initial(BoardEnumColor::White).fit(viewport.physical_size())?,
        );
        let state = Self {
            needs_theme: true,
            controller,
            view: PresentationPuzzleView::new(),
            scene,
            intents: Vec::new(),
        };
        Ok(Harness::builder()
            .with_size(screen.size())
            .build_ui_state(Self::show, state))
    }

    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_theme {
            PresentationThemeTokens::apply(ui.ctx(), DesktopPreferencesEnumTheme::Dark);
            state.needs_theme = false;
            ui.ctx().request_repaint();
            return;
        }
        ui.set_max_width(550.0);
        if let Some(intent) = state.view.show(ui, &mut state.controller) {
            state.intents.push(intent);
            ui.ctx()
                .request_discard("Exercise repeated puzzle control passes");
        }
        if let Some(board) = state.controller.board() {
            let actions = PresentationBoardSemanticGrid::show(
                ui,
                &state.scene,
                board,
                PresentationBoardSemanticGridFeedback::new(state.controller.selected(), None),
                state.controller.can_input(),
            );
            assert!(actions.is_empty());
        }
    }
}

#[tokio::test(start_paused = true)]
async fn initial_save_and_actual_readiness_gate_countdown_without_animation_pause()
-> PresentationPuzzleTestResult {
    let mut fixture =
        PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4", "e7e5", "g1f3"], 1)?;
    let mut controller = PresentationPuzzleController::new(fixture.snapshot(0)?);
    assert!(controller.board().is_none());
    assert!(
        controller
            .report_readiness(PuzzleTimerEnumReadiness::Ready, DesktopRequestId::new())?
            .is_none()
    );
    fixture.commit_all(10).await?;
    assert!(controller.observe_snapshot(fixture.snapshot(10)?));
    assert!(controller.board().is_some());
    for readiness in [
        PuzzleTimerEnumReadiness::BoardNotReady,
        PuzzleTimerEnumReadiness::InputBlocked,
    ] {
        assert!(
            controller
                .report_readiness(readiness, DesktopRequestId::new())?
                .is_none()
        );
        assert_eq!(controller.snapshot().timer().elapsed(), ClockDuration::ZERO);
    }
    fixture.ready(&mut controller, 10).await?;
    let movement = BoardPosition::standard().parse_uci("e2e4")?;
    let intent = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Move(movement),
    )?;
    fixture.apply(&mut controller, &intent, 20).await?;
    assert!(!controller.can_request(PresentationPuzzleControllerEnumAction::OpponentReply));
    assert!(
        controller
            .report_readiness(
                PuzzleTimerEnumReadiness::InputBlocked,
                DesktopRequestId::new()
            )?
            .is_none()
    );
    controller.observe_snapshot(fixture.snapshot(220)?);
    assert_eq!(controller.snapshot().status(), PuzzleRunEnumStatus::Active);
    assert_eq!(
        controller.snapshot().timer().elapsed(),
        ClockDuration::from_millis(210)?
    );
    fixture.ready(&mut controller, 220).await?;
    let reply = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::OpponentReply,
    )?;
    assert!(
        controller
            .request(reply.action(), reply.request_id())?
            .is_none()
    );
    fixture.apply(&mut controller, &reply, 230).await?;
    assert_eq!(
        controller
            .snapshot()
            .attempt()
            .ok_or("Missing attempt")?
            .phase(),
        PuzzleEnumPhase::SolverMove
    );
    assert!(!controller.can_request(PresentationPuzzleControllerEnumAction::OpponentReply));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn mounted_hint_is_once_only_and_ordinary_retained_save_does_not_block_play()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let controller = fixture.start().await?;
    assert!(controller.snapshot().has_retained_write());
    assert!(controller.can_input());
    let mut harness = PresentationPuzzleMounted::mount(controller)?;
    harness.run_ok().ok_or("Panel did not settle")?;
    harness.get_by_label("Hint").click_accesskit();
    harness.run_ok().ok_or("Hint did not settle")?;
    harness.run_steps(5);
    assert_eq!(harness.state().intents.len(), 1);
    let hint = harness.state().intents[0].clone();
    assert_eq!(hint.action(), PresentationPuzzleControllerEnumAction::Hint);
    fixture
        .apply(&mut harness.state_mut().controller, &hint, 10)
        .await?;
    fixture
        .ready(&mut harness.state_mut().controller, 10)
        .await?;
    assert!(harness.state().controller.can_input());
    assert!(
        harness
            .state_mut()
            .controller
            .request(hint.action(), hint.request_id())?
            .is_none()
    );
    harness.run_ok().ok_or("Hint feedback did not settle")?;
    harness.get_by_label("Hint source: e2");
    assert!(harness.state().controller.snapshot().has_retained_write());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn urgent_pause_survives_pending_action_and_stale_receipt_cannot_restore_board()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let mut controller = fixture.start().await?;
    let hint = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Hint,
    )?;
    let pause = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Pause(PuzzleTimerEnumPauseReason::Manual),
    )?;
    assert!(controller.board().is_none());
    fixture.owner.execute(
        hint.context(),
        PuzzlePersistenceEnumCommand::Hint,
        PresentationPuzzleFixture::time(10)?,
    )?;
    let old = fixture.snapshot(10)?;
    let context = fixture.owner.context();
    fixture.owner.execute(
        &context,
        PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::Manual),
        PresentationPuzzleFixture::time(20)?,
    )?;
    let newest = fixture.snapshot(20)?;
    assert!(controller.observe_snapshot(newest.clone()));
    assert!(!controller.observe_success(&pause, newest, None)?);
    assert!(!controller.observe_success(&hint, old, None)?);
    assert!(controller.board().is_none());
    assert!(matches!(
        controller.snapshot().status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    assert!(
        controller
            .observe_failure(
                &hint,
                DesktopFailure::new(
                    DesktopEnumFailureCode::StaleRevision,
                    "Old action".into(),
                    true
                )
            )
            .is_err()
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn foreign_success_preserves_ordinary_pending_until_real_stale_receipt_completes()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let mut controller = fixture.start().await?;
    let before = controller.snapshot().context().clone();
    let hint = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Hint,
    )?;
    assert!(matches!(
        controller.observe_success(
            &hint,
            PresentationPuzzleFixture::foreign_snapshot(10)?,
            None,
        ),
        Err(PresentationPuzzleControllerError::StaleResult)
    ));
    assert!(controller.is_pending());
    assert!(controller.board().is_some());
    assert_eq!(controller.snapshot().context(), &before);
    assert!(!controller.can_input());
    assert!(matches!(
        controller.request(
            PresentationPuzzleControllerEnumAction::Hint,
            DesktopRequestId::new(),
        ),
        Err(PresentationPuzzleControllerError::Busy)
    ));

    let actual_time = PresentationPuzzleFixture::time(10)?;
    fixture.owner.execute(
        hint.context(),
        PuzzlePersistenceEnumCommand::Hint,
        actual_time,
    )?;
    let actual = fixture.owner.snapshot(actual_time)?;
    let actual_summary = fixture.owner.summary(actual_time)?;
    let newest_time = PresentationPuzzleFixture::time(20)?;
    let newest = fixture.owner.snapshot(newest_time)?;
    let newest_summary = fixture.owner.summary(newest_time)?;
    assert!(controller.observe_state(newest, Some(newest_summary)));
    assert!(controller.is_pending());
    assert!(!controller.observe_success(&hint, actual, Some(actual_summary))?);
    assert!(!controller.is_pending());
    assert_eq!(controller.snapshot().captured_at(), newest_time);
    assert_eq!(controller.snapshot().context(), &fixture.owner.context());
    assert_eq!(controller.summary(), Some(&newest_summary));
    fixture.ready(&mut controller, 20).await?;
    assert!(controller.can_input());
    assert!(
        controller
            .request(hint.action(), hint.request_id())?
            .is_none()
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn foreign_success_preserves_urgent_pending_and_concealment_until_real_receipt()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let mut controller = fixture.start().await?;
    let before = controller.snapshot().context().clone();
    let pause = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Pause(PuzzleTimerEnumPauseReason::Manual),
    )?;
    assert!(controller.board().is_none());
    assert!(matches!(
        controller.observe_success(
            &pause,
            PresentationPuzzleFixture::foreign_snapshot(10)?,
            None,
        ),
        Err(PresentationPuzzleControllerError::StaleResult)
    ));
    assert!(controller.is_pending());
    assert!(controller.board().is_none());
    assert_eq!(controller.snapshot().context(), &before);
    assert!(!controller.can_request(pause.action()));

    let now = PresentationPuzzleFixture::time(10)?;
    fixture.owner.execute(
        pause.context(),
        PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::Manual),
        now,
    )?;
    assert!(controller.observe_success(&pause, fixture.owner.snapshot(now)?, None)?);
    assert!(!controller.is_pending());
    assert!(controller.board().is_none());
    assert!(matches!(
        controller.snapshot().status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    assert!(
        controller
            .request(pause.action(), pause.request_id())?
            .is_none()
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn freeze_removes_content_and_accessibility_until_latest_pause_durable_and_explicit_resume()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let controller = fixture.start().await?;
    let mut harness = PresentationPuzzleMounted::mount(controller)?;
    harness.run_ok().ok_or("Initial board did not mount")?;
    harness.get_by_label("e2: White pawn");
    let cached = harness.state().controller.snapshot().clone();
    assert!(cached.attempt().is_some());
    harness
        .state_mut()
        .controller
        .observe_snapshot(fixture.snapshot(1500)?);
    assert!(harness.state().controller.board().is_none());
    assert!(matches!(
        harness.state().controller.snapshot().checkpoint_status(),
        PuzzleCheckpointEnumStatus::Frozen(_)
    ));
    harness.run_ok().ok_or("Freeze did not settle")?;
    assert!(harness.query_by_label("e2: White pawn").is_none());
    assert!(!harness.state_mut().controller.observe_snapshot(cached));
    assert!(
        !harness
            .state()
            .controller
            .can_request(PresentationPuzzleControllerEnumAction::Resume)
    );
    fixture.commit_all(1500).await?;
    harness
        .state_mut()
        .controller
        .observe_snapshot(fixture.snapshot(1501)?);
    fixture
        .ready(&mut harness.state_mut().controller, 1501)
        .await?;
    assert!(harness.state().controller.board().is_none());
    assert!(
        harness
            .state()
            .controller
            .can_request(PresentationPuzzleControllerEnumAction::Resume)
    );
    harness.run_ok().ok_or("Resume control did not settle")?;
    harness.get_by_label("Resume").click_accesskit();
    harness.run_ok().ok_or("Resume request did not settle")?;
    assert_eq!(harness.state().intents.len(), 1);
    let resume = harness.state().intents[0].clone();
    fixture
        .apply(&mut harness.state_mut().controller, &resume, 1502)
        .await?;
    assert!(harness.state().controller.board().is_some());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn save_failure_exposes_explicit_retry_and_never_resumes_on_recovery()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let mut controller = fixture.start().await?;
    fixture.saves.try_recv()?.finish(true)?;
    fixture.owner.wait().await?;
    assert!(
        fixture
            .owner
            .settle(PresentationPuzzleFixture::time(10)?)
            .is_err()
    );
    controller.observe_snapshot(fixture.snapshot(11)?);
    assert!(controller.board().is_none());
    let mut harness = PresentationPuzzleMounted::mount(controller)?;
    harness.run_ok().ok_or("Save failure did not mount")?;
    harness.get_by_label("Retry Save").click_accesskit();
    harness.run_ok().ok_or("Retry Save did not settle")?;
    assert_eq!(harness.state().intents.len(), 1);
    let retry = harness.state().intents[0].clone();
    fixture
        .apply(&mut harness.state_mut().controller, &retry, 12)
        .await?;
    fixture.commit_all(12).await?;
    harness
        .state_mut()
        .controller
        .observe_snapshot(fixture.snapshot(13)?);
    fixture
        .ready(&mut harness.state_mut().controller, 13)
        .await?;
    assert!(
        harness
            .state()
            .controller
            .can_request(PresentationPuzzleControllerEnumAction::Resume)
    );
    assert!(harness.state().controller.board().is_none());
    assert!(matches!(
        harness.state().controller.snapshot().status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn all_four_promotions_come_from_legal_snapshot_and_repeated_selection_is_inert()
-> PresentationPuzzleTestResult {
    for (kind, name) in [
        (BoardEnumPieceKind::Knight, "knight"),
        (BoardEnumPieceKind::Bishop, "bishop"),
        (BoardEnumPieceKind::Rook, "rook"),
        (BoardEnumPieceKind::Queen, "queen"),
    ] {
        let position =
            BoardPosition::from_fen("7k/P7/8/8/8/8/8/7K w - - 0 1", BoardEnumVariant::Standard)?;
        let mut fixture = PresentationPuzzleFixture::new(position, &["a7a8q"], 1)?;
        let mut controller = fixture.start().await?;
        let origin = DesktopRequestId::new();
        assert!(controller.select_square("a7".parse()?, origin)?.is_none());
        assert!(controller.select_square("a7".parse()?, origin)?.is_none());
        assert!(
            controller
                .select_square("a8".parse()?, DesktopRequestId::new())?
                .is_none()
        );
        assert_eq!(controller.promotion_choices().len(), 4);
        let mut harness = PresentationPuzzleMounted::mount(controller)?;
        harness.run_ok().ok_or("Promotion did not mount")?;
        harness
            .get_by_label(format!("Promote to {name}").as_str())
            .click_accesskit();
        harness.run_ok().ok_or("Promotion did not settle")?;
        harness.run_steps(4);
        assert_eq!(harness.state().intents.len(), 1);
        let intent = &harness.state().intents[0];
        assert!(
            matches!(intent.action(), PresentationPuzzleControllerEnumAction::Move(movement) if movement.promotion() == Some(kind) && harness.state().controller.snapshot().attempt().is_some_and(|attempt| attempt.board().legal_moves().iter().any(|legal| legal.movement() == movement)))
        );
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn unlisted_practice_completion_keeps_failed_primary_and_exact_owner_summary()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let mut controller = fixture.start().await?;
    let unlisted = BoardPosition::standard().parse_uci("d2d4")?;
    let intent = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Move(unlisted),
    )?;
    fixture.apply(&mut controller, &intent, 10).await?;
    let mut harness = PresentationPuzzleMounted::mount(controller)?;
    harness.run_ok().ok_or("Unlisted feedback did not mount")?;
    harness.get_by_label(
        "This legal move is not in the listed solution. No alternative solution was verified",
    );
    harness.get_by_label("Retry Practice").click_accesskit();
    harness.run_ok().ok_or("Practice retry did not settle")?;
    let retry = harness.state().intents[0].clone();
    fixture
        .apply(&mut harness.state_mut().controller, &retry, 20)
        .await?;
    fixture
        .ready(&mut harness.state_mut().controller, 20)
        .await?;
    let movement = BoardPosition::standard().parse_uci("e2e4")?;
    let solve = PresentationPuzzleFixture::intent(
        &mut harness.state_mut().controller,
        PresentationPuzzleControllerEnumAction::Move(movement),
    )?;
    fixture
        .apply(&mut harness.state_mut().controller, &solve, 30)
        .await?;
    let now = PresentationPuzzleFixture::time(30)?;
    let summary = fixture.owner.summary(now)?;
    assert!(
        harness
            .state_mut()
            .controller
            .observe_state(fixture.owner.snapshot(now)?, Some(summary))
    );
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Failed), 1);
    assert_eq!(
        summary.primary_count(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Unassisted
        )),
        0
    );
    harness.run_ok().ok_or("Summary did not settle")?;
    harness.get_by_label("Practice completed. The recorded primary failure remains failed");
    harness.get_by_label("Failures: 1");
    harness.get_by_label("Practice completions: 1");
    harness.get_by_label("Unassisted solve rate: 0/1");
    harness.get_by_label("Assisted solve rate: 0/1");
    harness.get_by_label("Active time: 30ms");
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn confirmation_cancel_is_inert_and_stale_confirmation_cannot_end_next_attempt()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 2)?;
    let controller = fixture.start().await?;
    let mut harness = PresentationPuzzleMounted::mount(controller)?;
    harness.run_ok().ok_or("End controls did not mount")?;
    harness.get_by_label("End Run").click_accesskit();
    harness.run_ok().ok_or("End confirmation did not open")?;
    harness.get_by_label("Keep Playing").click_accesskit();
    harness.run_ok().ok_or("Cancel did not settle")?;
    assert!(harness.state().intents.is_empty());
    assert!(!harness.state().controller.is_confirming_end());
    harness.state_mut().controller.begin_end_confirmation()?;
    let context = fixture.owner.context();
    fixture.owner.execute(
        &context,
        PuzzlePersistenceEnumCommand::Skip,
        PresentationPuzzleFixture::time(10)?,
    )?;
    harness
        .state_mut()
        .controller
        .observe_snapshot(fixture.snapshot(10)?);
    assert!(!harness.state().controller.is_confirming_end());
    assert!(
        harness
            .state_mut()
            .controller
            .request(
                PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Confirmed),
                DesktopRequestId::new()
            )
            .is_err()
    );
    assert_eq!(
        harness.state().controller.snapshot().status(),
        PuzzleRunEnumStatus::AwaitingReadiness
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn mounted_next_waits_for_explicit_reshuffle_and_new_board_readiness()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let mut controller = fixture.start().await?;
    let solve = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Move(BoardPosition::standard().parse_uci("e2e4")?),
    )?;
    fixture.apply(&mut controller, &solve, 10).await?;
    let mut harness = PresentationPuzzleMounted::mount(controller)?;
    harness.run_ok().ok_or("Review did not mount")?;
    harness.get_by_label("Next").click_accesskit();
    harness.run_ok().ok_or("Next did not settle")?;
    let next = harness.state().intents[0].clone();
    fixture
        .apply(&mut harness.state_mut().controller, &next, 20)
        .await?;
    let remaining = harness.state().controller.snapshot().timer().remaining();
    fixture.commit_all(20).await?;
    harness
        .state_mut()
        .controller
        .observe_snapshot(fixture.snapshot(2000)?);
    harness.run_ok().ok_or("Exhaustion did not mount")?;
    harness.run_steps(5);
    assert_eq!(harness.state().intents.len(), 1);
    assert_eq!(
        harness.state().controller.snapshot().timer().remaining(),
        remaining
    );
    assert!(harness.query_by_label("e2: White pawn").is_none());
    harness
        .get_by_label("Reshuffle and Continue")
        .click_accesskit();
    harness.run_ok().ok_or("Reshuffle did not settle")?;
    let reshuffle = harness.state().intents[1].clone();
    fixture
        .apply(&mut harness.state_mut().controller, &reshuffle, 2001)
        .await?;
    assert_eq!(
        harness.state().controller.snapshot().status(),
        PuzzleRunEnumStatus::AwaitingReadiness
    );
    assert_eq!(
        harness.state().controller.snapshot().timer().remaining(),
        remaining
    );
    assert!(!harness.state().controller.can_input());
    fixture
        .ready(&mut harness.state_mut().controller, 2002)
        .await?;
    assert!(harness.state().controller.can_input());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn confirmed_end_emits_once_and_authoritative_summary_records_skip()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let mut controller = fixture.start().await?;
    assert!(
        controller
            .request(
                PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Unconfirmed),
                DesktopRequestId::new()
            )?
            .is_none()
    );
    assert!(controller.is_confirming_end());
    let mut harness = PresentationPuzzleMounted::mount(controller)?;
    harness.run_ok().ok_or("Confirmation did not mount")?;
    harness.get_by_label("Confirm End Run").click_accesskit();
    harness.run_ok().ok_or("Confirm did not settle")?;
    harness.run_steps(4);
    assert_eq!(harness.state().intents.len(), 1);
    let end = harness.state().intents[0].clone();
    assert_eq!(
        end.action(),
        PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Confirmed)
    );
    fixture
        .apply(&mut harness.state_mut().controller, &end, 10)
        .await?;
    let now = PresentationPuzzleFixture::time(10)?;
    let summary = fixture.owner.summary(now)?;
    harness
        .state_mut()
        .controller
        .observe_state(fixture.owner.snapshot(now)?, Some(summary));
    harness.run_ok().ok_or("Ended summary did not settle")?;
    harness.get_by_label("Run ended");
    harness.get_by_label("Skips: 1");
    assert!(harness.query_by_label("e2: White pawn").is_none());
    assert!(
        harness
            .state_mut()
            .controller
            .request(end.action(), end.request_id())?
            .is_none()
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn hidden_intent_removes_cached_board_and_hint_accessibility_while_an_action_waits()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 1)?;
    let mut controller = fixture.start().await?;
    let hint = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Hint,
    )?;
    fixture.apply(&mut controller, &hint, 10).await?;
    fixture.ready(&mut controller, 10).await?;
    let mut harness = PresentationPuzzleMounted::mount(controller)?;
    harness.run_ok().ok_or("Hint board did not mount")?;
    harness.get_by_label("Hint source: e2");
    let pending = PresentationPuzzleFixture::intent(
        &mut harness.state_mut().controller,
        PresentationPuzzleControllerEnumAction::Hint,
    )?;
    let id = DesktopRequestId::new();
    let hidden = harness
        .state_mut()
        .controller
        .report_readiness(PuzzleTimerEnumReadiness::Hidden, id)?
        .ok_or("Missing urgent Hidden intent")?;
    assert_eq!(hidden.context(), pending.context());
    assert!(
        harness
            .state_mut()
            .controller
            .report_readiness(PuzzleTimerEnumReadiness::Hidden, id)?
            .is_none()
    );
    assert!(harness.state().controller.snapshot().attempt().is_some());
    harness.run_ok().ok_or("Hidden state did not settle")?;
    assert!(harness.query_by_label("Hint source: e2").is_none());
    assert!(harness.query_by_label("e2: White pawn").is_none());
    assert!(!harness.state().controller.can_input());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn stale_attempt_and_foreign_session_snapshots_cannot_replace_current_outcome()
-> PresentationPuzzleTestResult {
    let mut fixture = PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4"], 2)?;
    let mut controller = fixture.start().await?;
    let previous = controller.snapshot().clone();
    let skip = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Skip,
    )?;
    fixture.apply(&mut controller, &skip, 10).await?;
    let newest = controller.snapshot().context().clone();
    assert!(!controller.observe_snapshot(previous));
    assert_eq!(controller.snapshot().context(), &newest);
    assert!(!controller.observe_snapshot(PresentationPuzzleFixture::foreign_snapshot(20)?));
    assert_eq!(controller.snapshot().context(), &newest);
    let summary = fixture
        .owner
        .summary(PresentationPuzzleFixture::time(10)?)?;
    assert_eq!(summary.finalized(), 1);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Skipped), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn frozen_reply_requires_explicit_resume_and_is_dispatched_once_after_readiness()
-> PresentationPuzzleTestResult {
    let mut fixture =
        PresentationPuzzleFixture::new(BoardPosition::standard(), &["e2e4", "e7e5", "g1f3"], 1)?;
    let mut controller = fixture.start().await?;
    let movement = BoardPosition::standard().parse_uci("e2e4")?;
    let move_intent = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Move(movement),
    )?;
    fixture.apply(&mut controller, &move_intent, 10).await?;
    controller.observe_snapshot(fixture.snapshot(1500)?);
    assert!(!controller.can_request(PresentationPuzzleControllerEnumAction::OpponentReply));
    fixture.commit_all(1500).await?;
    controller.observe_snapshot(fixture.snapshot(1501)?);
    fixture.ready(&mut controller, 1501).await?;
    assert!(!controller.can_request(PresentationPuzzleControllerEnumAction::OpponentReply));
    let resume = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::Resume,
    )?;
    fixture.apply(&mut controller, &resume, 1502).await?;
    assert!(!controller.can_request(PresentationPuzzleControllerEnumAction::OpponentReply));
    fixture.ready(&mut controller, 1503).await?;
    let reply = PresentationPuzzleFixture::intent(
        &mut controller,
        PresentationPuzzleControllerEnumAction::OpponentReply,
    )?;
    fixture.apply(&mut controller, &reply, 1504).await?;
    assert!(
        controller
            .request(reply.action(), reply.request_id())?
            .is_none()
    );
    assert!(!controller.can_request(PresentationPuzzleControllerEnumAction::OpponentReply));
    assert_eq!(
        controller.snapshot().timer().elapsed(),
        ClockDuration::from_millis(1002)?
    );
    Ok(())
}

#[test]
fn setup_validates_real_metadata_filter_and_countdown_without_seed_or_factory_state()
-> PresentationPuzzleTestResult {
    let definition =
        PresentationPuzzleFixture::definition(BoardPosition::standard(), &["e2e4"], 0)?;
    let dataset = PuzzleSelectionDataset::from_identity(definition.identity());
    let collections = [PresentationPuzzleSetupCollection::new(
        dataset.clone(),
        "Available fixture".into(),
        1,
        vec!["fork".parse()?],
    )?];
    let mut setup = PresentationPuzzleSetup::new();
    assert!(setup.validate(&collections).is_err());
    setup.select_dataset(dataset.clone());
    assert_eq!(
        setup.validate(&collections)?.2,
        PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session)
    );
    for (minimum, maximum) in [
        ("1500", "1000"),
        ("1", ""),
        ("-1", "10"),
        ("4294967296", "4294967296"),
    ] {
        setup.set_difficulty(minimum.into(), maximum.into());
        assert!(setup.validate(&collections).is_err());
    }
    setup.set_difficulty("0".into(), u32::MAX.to_string());
    setup.set_themes(vec!["unknown".parse()?]);
    assert!(setup.validate(&collections).is_err());
    setup.set_themes(vec!["fork".parse()?]);
    for seconds in ["0", "+1", "go infinite", "18446744073709551615"] {
        setup.set_countdown(PuzzleEnumCountdownMode::PerPuzzle, seconds.into());
        assert!(setup.validate(&collections).is_err());
    }
    setup.set_countdown(PuzzleEnumCountdownMode::PerPuzzle, "60".into());
    let (selected, filter, config) = setup.validate(&collections)?;
    assert_eq!(selected, dataset);
    assert_eq!(filter.themes(), &["fork".parse()?]);
    assert_eq!(
        config,
        PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::PerPuzzle)
    );
    Ok(())
}

struct PresentationPuzzleSetupMounted {
    setup: PresentationPuzzleSetup,
    collections: Vec<PresentationPuzzleSetupCollection>,
    intents: Vec<PresentationPuzzleSetupIntent>,
}

impl PresentationPuzzleSetupMounted {
    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if let Some(intent) = state.setup.show(ui, &state.collections, true) {
            state.intents.push(intent);
            ui.ctx()
                .request_discard("Exercise repeated puzzle setup passes");
        }
    }
}

#[test]
fn mounted_setup_start_is_once_only_and_zero_summary_rates_are_unavailable()
-> PresentationPuzzleTestResult {
    let definition =
        PresentationPuzzleFixture::definition(BoardPosition::standard(), &["e2e4"], 0)?;
    let dataset = PuzzleSelectionDataset::from_identity(definition.identity());
    let state = PresentationPuzzleSetupMounted {
        setup: PresentationPuzzleSetup::new(),
        collections: vec![PresentationPuzzleSetupCollection::new(
            dataset,
            "Fixture collection".into(),
            1,
            vec!["fork".parse()?],
        )?],
        intents: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 1200.0))
        .build_ui_state(PresentationPuzzleSetupMounted::show, state);
    harness.run_ok().ok_or("Setup did not mount")?;
    assert!(
        harness
            .get_by_label("Start Run")
            .accesskit_node()
            .is_disabled()
    );
    harness.get_by_label("Fixture collection").click_accesskit();
    harness.run_ok().ok_or("Collection did not select")?;
    harness.get_by_label("Start Run").click_accesskit();
    harness.run_ok().ok_or("Start did not settle")?;
    harness.run_steps(5);
    assert_eq!(harness.state().intents.len(), 1);
    assert_eq!(
        harness.state().intents[0].config(),
        PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session)
    );
    let mut run = PuzzlePersistenceRun::new(
        PuzzleSessionId::new(NonZeroU64::MIN),
        PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
        vec![definition],
        PuzzleSelectionFilter::default(),
        PuzzleRandomProviderSeeded::new(81),
    )?;
    let now = PresentationPuzzleFixture::time(0)?;
    let summary = run.run().summary(now)?;
    let snapshot = run.capture(now, 0)?;
    assert_eq!(
        snapshot.context().session_id(),
        PuzzleSessionId::new(NonZeroU64::MIN)
    );
    let mut summary_harness = Harness::builder()
        .with_size(egui::vec2(900.0, 1200.0))
        .build_ui(move |ui| {
            crate::presentation::puzzle::summary::PresentationPuzzleSummary::show(ui, &summary)
        });
    summary_harness
        .run_ok()
        .ok_or("Empty summary did not mount")?;
    summary_harness.get_by_label("Unassisted solve rate: unavailable");
    summary_harness.get_by_label("Assisted solve rate: unavailable");
    summary_harness.get_by_label("Finalized puzzles: 0");
    assert_eq!(summary.elapsed_active(), Duration::ZERO);
    Ok(())
}
