use std::collections::VecDeque;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use bevy::ecs::entity::Entity;
use bevy::ecs::message::Messages;
use bevy::ecs::world::World;
use bevy::window::{AppLifecycle, Window, WindowFocused, WindowOccluded};
use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};
use thiserror::Error;
use tokio::sync::watch;

use crate::board::position::BoardPosition;
use crate::capacity::budget::CapacityBudget;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationError, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use crate::desktop::dto::{DesktopBoardSnapshot, DesktopEnumValue};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};
use crate::desktop::native::lifecycle::DesktopNativeLifecyclePuzzleWindowId;
use crate::desktop::native::puzzle_window::{
    DesktopNativePuzzleWindow, DesktopNativePuzzleWindowError, DesktopNativePuzzleWindowFrame,
    DesktopNativePuzzleWindowReadiness,
};
use crate::desktop::puzzle_bridge::{DesktopPuzzleBridgeEnumHealth, DesktopPuzzleBridgeEnumResult};
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceEnumOperation,
    DesktopPuzzleServiceEnumResult, DesktopPuzzleServiceError, DesktopPuzzleServiceNativeProgress,
    DesktopPuzzleServiceOutcome, DesktopPuzzleServiceOutcomeSender, DesktopPuzzleServiceProvider,
    DesktopPuzzleServiceReceipt, DesktopPuzzleServiceRequest, DesktopPuzzleServiceState,
};
use crate::desktop::puzzle_snapshot::{DesktopPuzzleSnapshot, DesktopPuzzleSnapshotSave};
use crate::presentation::app::{PresentationApp, PresentationAppEnumWorkspace};
use crate::presentation::puzzle::controller::{
    PresentationPuzzleController, PresentationPuzzleControllerEnumAction,
};
use crate::presentation::puzzle::panel::PresentationPuzzlePanelConnection;
use crate::puzzle::checkpoint::{
    PuzzleCheckpointEnumStatus, PuzzleCheckpointEnumTrigger, PuzzleCheckpointObservation,
    PuzzleCheckpointSnapshot,
};
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::PuzzleEnumCountdownMode;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::PuzzleDefinitionMetadata;
use crate::puzzle::persistence::{
    PuzzlePersistenceEnumCommand, PuzzlePersistenceRun, PuzzlePersistenceStorageRevision,
};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::run::PuzzleRunEnumStatus;
use crate::puzzle::selection::PuzzleSelectionFilter;
use crate::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use crate::puzzle::storage_repository::PuzzleStorageDriver;
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::puzzle_repository::StoragePuzzleRepository;

type DesktopNativePuzzleWindowTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Error)]
enum DesktopNativePuzzleWindowTestError {
    #[error("The controlled native interruption state is poisoned")]
    Poisoned,
    #[error("There is no controlled native interruption to complete")]
    Missing,
}

struct DesktopNativePuzzleWindowTestClock;

impl ClockTimeProvider for DesktopNativePuzzleWindowTestClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(0))
    }
}

#[derive(Default)]
struct DesktopNativePuzzleWindowTestWake(AtomicUsize);

impl DesktopApplicationWakeProvider for DesktopNativePuzzleWindowTestWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[derive(Default)]
struct DesktopNativePuzzleWindowTestControl {
    attempted: Vec<(PuzzleSessionId, PuzzleTimerEnumPauseReason)>,
    failure: Option<Arc<DesktopPuzzleServiceError>>,
    pending: VecDeque<DesktopPuzzleServiceOutcomeSender<DesktopPuzzleServiceEnumResult>>,
}

#[derive(Clone)]
struct DesktopNativePuzzleWindowTestProvider {
    state: watch::Sender<DesktopPuzzleServiceState>,
    control: Arc<Mutex<DesktopNativePuzzleWindowTestControl>>,
}

impl DesktopNativePuzzleWindowTestProvider {
    fn lock(
        &self,
    ) -> Result<
        MutexGuard<'_, DesktopNativePuzzleWindowTestControl>,
        DesktopNativePuzzleWindowTestError,
    > {
        self.control
            .lock()
            .map_err(|_poisoned| DesktopNativePuzzleWindowTestError::Poisoned)
    }

    fn attempted(
        &self,
    ) -> DesktopNativePuzzleWindowTestResult<Vec<(PuzzleSessionId, PuzzleTimerEnumPauseReason)>>
    {
        Ok(self.lock()?.attempted.clone())
    }

    fn publish_snapshot(
        &self,
        snapshot: Arc<DesktopPuzzleSnapshot>,
        failure: Option<Arc<DesktopPuzzleServiceError>>,
    ) {
        let previous = self.state.borrow().clone();
        self.state.send_replace(DesktopPuzzleServiceState::new(
            previous.selected(),
            Some(snapshot),
            previous.activity(),
            previous.health().cloned(),
            failure,
        ));
    }

    fn finish(
        &self,
        result: DesktopPuzzleServiceOutcome<DesktopPuzzleServiceEnumResult>,
    ) -> DesktopNativePuzzleWindowTestResult {
        let sender = self
            .lock()?
            .pending
            .pop_front()
            .ok_or(DesktopNativePuzzleWindowTestError::Missing)?;
        sender.send_replace(Some(result));
        Ok(())
    }
}

impl DesktopPuzzleServiceProvider for DesktopNativePuzzleWindowTestProvider {
    fn submit(
        &self,
        _request: DesktopPuzzleServiceRequest,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt> {
        Err(Arc::new(DesktopPuzzleServiceError::Incomplete))
    }

    fn interrupt(
        &self,
        session: PuzzleSessionId,
        reason: PuzzleTimerEnumPauseReason,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt> {
        let mut control = self
            .lock()
            .map_err(|error| Arc::new(DesktopPuzzleServiceError::Preparation(Box::new(error))))?;
        control.attempted.push((session, reason));
        if let Some(error) = &control.failure {
            return Err(Arc::clone(error));
        }
        let (sender, receipt) = DesktopPuzzleServiceReceipt::channel();
        control.pending.push_back(sender);
        Ok(receipt)
    }

    fn state(&self) -> watch::Receiver<DesktopPuzzleServiceState> {
        self.state.subscribe()
    }
}

struct DesktopNativePuzzleWindowFixture {
    world: World,
    window: Entity,
    other: Entity,
    provider: DesktopNativePuzzleWindowTestProvider,
    adapter: DesktopNativePuzzleWindow<DesktopNativePuzzleWindowTestProvider>,
    application:
        DesktopApplication<DesktopNativePuzzleWindowTestClock, DesktopNativePuzzleWindowTestWake>,
    wake: Arc<DesktopNativePuzzleWindowTestWake>,
    presentation: PresentationApp,
    run: PuzzlePersistenceRun,
}

impl DesktopNativePuzzleWindowFixture {
    fn id(value: u64) -> DesktopNativePuzzleWindowTestResult<PuzzleSessionId> {
        Ok(PuzzleSessionId::new(
            NonZeroU64::new(value).ok_or("zero puzzle fixture identity")?,
        ))
    }

    fn run(id: u64) -> DesktopNativePuzzleWindowTestResult<PuzzlePersistenceRun> {
        Self::run_from(
            BoardPosition::standard(),
            ["e2e4", "e7e5", "g1f3"],
            Self::id(id)?,
        )
    }

    fn run_from(
        initial: BoardPosition,
        moves: [&str; 3],
        session: PuzzleSessionId,
    ) -> DesktopNativePuzzleWindowTestResult<PuzzlePersistenceRun> {
        let mut position = initial.clone();
        let mut line = Vec::new();
        for uci in moves {
            let movement = position.parse_uci(uci)?;
            position = position.play(movement)?;
            line.push(movement);
        }
        Ok(PuzzlePersistenceRun::new(
            session,
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![PuzzleDefinition::new(
                PuzzleDefinitionIdentity::new("native-window".into(), "v1".into(), "first".into())?,
                initial,
                vec![line],
                PuzzleDefinitionMetadata::default(),
            )?],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(71),
        )?)
    }

    fn new() -> DesktopNativePuzzleWindowTestResult<Self> {
        let mut world = World::new();
        let _reserved = world.spawn_empty().id();
        let window = world.spawn(Window::default()).id();
        let other = world.spawn(Window::default()).id();
        world.init_resource::<Messages<WindowFocused>>();
        world.init_resource::<Messages<WindowOccluded>>();
        world.init_resource::<Messages<AppLifecycle>>();
        let (state, _receiver) = watch::channel(DesktopPuzzleServiceState::new(
            Some(Self::id(1)?),
            None,
            DesktopPuzzleServiceEnumActivity::Selected,
            None,
            None,
        ));
        let provider = DesktopNativePuzzleWindowTestProvider {
            state,
            control: Arc::new(Mutex::new(DesktopNativePuzzleWindowTestControl::default())),
        };
        let adapter = DesktopNativePuzzleWindow::new(window, provider.clone())?;
        let wake = Arc::new(DesktopNativePuzzleWindowTestWake::default());
        Ok(Self {
            world,
            window,
            other,
            provider,
            adapter,
            application: DesktopApplication::new(
                Arc::new(DesktopNativePuzzleWindowTestClock),
                Arc::clone(&wake),
            ),
            wake,
            presentation: PresentationApp::new(),
            run: Self::run(1)?,
        })
    }

    async fn mount_puzzles(&mut self) -> DesktopNativePuzzleWindowTestResult {
        let directory = tempfile::tempdir()?;
        let store = StorageDocumentStore::open(
            directory.path().join("native-puzzle-window.sqlite3"),
            CapacityBudget::new(NonZeroUsize::MIN)?,
        )
        .await?;
        let repository = StoragePuzzleRepository::new(store.client());
        let now = ClockInstant::from_nanos(0);
        let mut initial = Self::run(self.run.run().context().session_id().value().get())?;
        let mut driver = PuzzleStorageDriver::new(
            repository.clone(),
            PuzzleCheckpointSnapshot::new(
                PuzzleCheckpointObservation::from_run(initial.run(), now)?,
                initial.capture(now, 0)?,
            ),
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?;
        driver.begin_write(now)?;
        driver.wait().await?;
        driver.settle(now)?;
        driver.record(
            PuzzleCheckpointSnapshot::new(
                PuzzleCheckpointObservation::from_run(self.run.run(), now)?,
                self.run.capture(now, 0)?,
            ),
            PuzzleCheckpointEnumTrigger::AcceptedTransition,
        )?;
        driver.begin_write(now)?;
        driver.wait().await?;
        driver.settle(now)?;
        let page = Arc::new(repository.inventory(None).await?);
        self.presentation
            .mount_puzzles(PresentationPuzzlePanelConnection {
                owner: DesktopMessagesEntityId::new(),
                state: self.provider.state(),
            });
        self.presentation.select_workspace(
            PresentationAppEnumWorkspace::Puzzles,
            &self.application.port(),
        )?;
        let inventory = self
            .application
            .try_next()?
            .ok_or("missing native window inventory request")?;
        assert!(matches!(
            inventory.request().command().operation(),
            DesktopEnumCommand::Puzzle {
                operation: DesktopPuzzleServiceEnumOperation::Inventory { after: None },
                ..
            }
        ));
        self.application.begin_commit(&inventory)?;
        self.application.complete_value(
            &inventory,
            DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Inventory(page))),
            None,
        )?;
        self.presentation
            .synchronize_puzzles(&self.application.port())?;
        self.click("Open")?;
        let open = self
            .application
            .try_next()?
            .ok_or("missing native window open request")?;
        assert!(matches!(
            open.request().command().operation(),
            DesktopEnumCommand::Puzzle {
                operation: DesktopPuzzleServiceEnumOperation::OpenSaved { id, .. },
                ..
            } if *id == self.run.run().context().session_id()
        ));
        let snapshot = Arc::new(self.snapshot()?);
        self.provider.publish_snapshot(Arc::clone(&snapshot), None);
        self.application.begin_commit(&open)?;
        self.application.complete_value(
            &open,
            DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Opened(snapshot))),
            None,
        )?;
        self.presentation
            .synchronize_puzzles(&self.application.port())?;
        self.report_readiness()?;
        store.close().await?;
        Ok(())
    }

    fn click(&mut self, label: &str) -> DesktopNativePuzzleWindowTestResult {
        let mut errors = Vec::new();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 1600.0))
            .build_ui(|ui| {
                let mut picker = |_point| Ok(None);
                if let Err(error) =
                    self.presentation
                        .show(ui, &self.application.port(), None, &mut picker)
                {
                    errors.push(error);
                }
            });
        harness
            .run_ok()
            .ok_or("native window panel did not settle")?;
        harness.get_by_label(label).click();
        harness
            .run_ok()
            .ok_or("native window panel click did not settle")?;
        drop(harness);
        if let Some(error) = errors.into_iter().next() {
            return Err(error.into());
        }
        Ok(())
    }

    fn report_readiness(&mut self) -> DesktopNativePuzzleWindowTestResult {
        let rendered = self.presentation.board_snapshot().cloned();
        self.adapter.report_readiness(
            DesktopNativePuzzleWindowFrame {
                world: &self.world,
                graphics_ready: true,
                rendered_board: rendered.as_ref(),
            },
            &mut self.presentation,
            &self.application.port(),
        )?;
        Ok(())
    }

    fn update(&mut self, graphics_failed: bool) -> DesktopNativePuzzleWindowTestResult {
        self.adapter.update(
            &self.world,
            &mut self.presentation,
            &self.application.port(),
            graphics_failed,
        )?;
        Ok(())
    }

    fn focus(&mut self, window: Entity, focused: bool) -> DesktopNativePuzzleWindowTestResult {
        self.world
            .get_mut::<Window>(window)
            .ok_or("missing fixture window")?
            .focused = focused;
        self.world
            .resource_mut::<Messages<WindowFocused>>()
            .write(WindowFocused { window, focused });
        Ok(())
    }

    fn occlude(&mut self, window: Entity, occluded: bool) {
        self.world
            .resource_mut::<Messages<WindowOccluded>>()
            .write(WindowOccluded { window, occluded });
    }

    fn lifecycle(&mut self, lifecycle: AppLifecycle) {
        self.world
            .resource_mut::<Messages<AppLifecycle>>()
            .write(lifecycle);
    }

    fn select(&self, selected: Option<PuzzleSessionId>) {
        self.provider
            .state
            .send_replace(DesktopPuzzleServiceState::new(
                selected,
                None,
                DesktopPuzzleServiceEnumActivity::Selected,
                None,
                None,
            ));
    }

    fn snapshot(&self) -> DesktopNativePuzzleWindowTestResult<DesktopPuzzleSnapshot> {
        let now = ClockInstant::from_nanos(0);
        let durable = PuzzleCheckpointObservation::from_run(self.run.run(), now)?;
        Ok(DesktopPuzzleSnapshot::capture(
            &self.run,
            now,
            DesktopPuzzleSnapshotSave {
                status: PuzzleCheckpointEnumStatus::Open,
                durable: Some(&durable),
                retained_write: false,
                unsaved_state: false,
            },
            None,
        )?)
    }

    fn succeed(&self) -> DesktopNativePuzzleWindowTestResult {
        self.provider
            .finish(Ok(DesktopPuzzleServiceEnumResult::Session(
                DesktopPuzzleBridgeEnumResult::Snapshot(Arc::new(self.snapshot()?)),
            )))
    }

    fn execute(
        &mut self,
        command: PuzzlePersistenceEnumCommand,
    ) -> DesktopNativePuzzleWindowTestResult {
        self.run.execute(command, ClockInstant::from_nanos(0))?;
        Ok(())
    }

    fn frame<'a>(
        &'a self,
        rendered_board: Option<&'a DesktopBoardSnapshot>,
    ) -> DesktopNativePuzzleWindowFrame<'a> {
        DesktopNativePuzzleWindowFrame {
            world: &self.world,
            graphics_ready: true,
            rendered_board,
        }
    }
}

#[test]
fn entity_bits_are_checked_without_a_global_window_identity() -> DesktopNativePuzzleWindowTestResult
{
    let fixture = DesktopNativePuzzleWindowFixture::new()?;
    assert_eq!(
        fixture.adapter.window_id,
        DesktopNativeLifecyclePuzzleWindowId::new(
            NonZeroU64::new(fixture.window.to_bits()).ok_or("zero bound window")?
        )
    );
    let other = DesktopNativePuzzleWindow::new(fixture.other, fixture.provider)?;
    assert_ne!(fixture.adapter.window_id, other.window_id);
    Ok(())
}

#[test]
fn other_window_events_do_not_interrupt_the_bound_puzzle() -> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.update(false)?;
    fixture.focus(fixture.other, false)?;
    fixture.occlude(fixture.other, true);
    fixture.update(false)?;
    assert!(fixture.provider.attempted()?.is_empty());
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn focus_loss_uses_the_native_lane_once_and_focus_gain_never_resumes()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.update(false)?;
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    for _ in 0..4 {
        fixture.update(false)?;
    }
    fixture.focus(fixture.window, true)?;
    fixture.update(false)?;
    assert_eq!(
        fixture.provider.attempted()?,
        vec![(
            DesktopNativePuzzleWindowFixture::id(1)?,
            PuzzleTimerEnumPauseReason::FocusLost
        )]
    );
    assert!(fixture.application.try_next()?.is_none());
    assert!(!fixture.adapter.is_settled());
    fixture.succeed()?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn ordinary_application_saturation_cannot_block_a_native_interruption()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    let port = fixture.application.port();
    let mut receipts = Vec::new();
    for _ in 0..DesktopApplication::<
        DesktopNativePuzzleWindowTestClock,
        DesktopNativePuzzleWindowTestWake,
    >::ORDINARY_CAPACITY
    {
        let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        receipts.push(port.try_submit(&request)?);
    }
    let refused = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
    assert!(matches!(
        port.try_submit(&refused),
        Err(DesktopApplicationError::Busy)
    ));
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    assert_eq!(
        fixture.provider.attempted()?,
        vec![(
            DesktopNativePuzzleWindowFixture::id(1)?,
            PuzzleTimerEnumPauseReason::FocusLost
        )]
    );
    assert!(!fixture.adapter.is_settled());
    assert!(receipts.iter().all(|receipt| receipt.response().is_none()));
    Ok(())
}

#[test]
fn matching_focus_loss_is_observed_even_if_the_window_is_already_refocused()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.update(false)?;
    fixture.focus(fixture.window, false)?;
    fixture.focus(fixture.window, true)?;
    fixture.update(false)?;
    assert_eq!(fixture.provider.attempted()?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn refocused_interruption_conceals_until_authoritative_completion()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.execute(PuzzlePersistenceEnumCommand::Readiness(
        PuzzleTimerEnumReadiness::Ready,
    ))?;
    fixture.execute(PuzzlePersistenceEnumCommand::Hint)?;
    fixture.mount_puzzles().await?;
    fixture.update(false)?;
    assert!(fixture.presentation.board_snapshot().is_some());
    fixture.focus(fixture.window, false)?;
    fixture.focus(fixture.window, true)?;
    fixture.update(false)?;
    assert!(
        fixture
            .adapter
            .readiness
            .as_ref()
            .is_some_and(|readiness| readiness.value == PuzzleTimerEnumReadiness::Hidden),
        "the observed focus-loss edge must conceal content before its receipt completes"
    );
    assert!(!fixture.adapter.is_settled());
    let hidden = fixture
        .application
        .try_next()?
        .ok_or("missing Hidden delivery")?;
    assert!(matches!(
        hidden.request().command().operation(),
        DesktopEnumCommand::Puzzle {
            operation: DesktopPuzzleServiceEnumOperation::Interrupt { session, reason: PuzzleTimerEnumPauseReason::FocusLost },
            ..
        } if *session == DesktopNativePuzzleWindowFixture::id(1)?
    ));
    fixture.succeed()?;
    fixture.update(false)?;
    for _ in 0..3 {
        fixture.report_readiness()?;
        assert!(fixture.presentation.board_snapshot().is_none());
        let controller = fixture
            .presentation
            .puzzle_panel()
            .and_then(|panel| panel.controller())
            .ok_or("missing concealed controller")?;
        assert!(
            controller
                .snapshot()
                .attempt()
                .and_then(|attempt| attempt.hint())
                .is_some()
        );
        assert!(controller.board().is_none());
        assert!(!controller.can_input());
        assert!(!controller.can_request(PresentationPuzzleControllerEnumAction::Hint));
    }
    assert!(fixture.application.try_next()?.is_none());
    fixture.execute(PuzzlePersistenceEnumCommand::Pause(
        PuzzleTimerEnumPauseReason::FocusLost,
    ))?;
    let paused = Arc::new(fixture.snapshot()?);
    fixture.provider.publish_snapshot(Arc::clone(&paused), None);
    fixture.application.complete_puzzle_control(
        &hidden,
        Ok(DesktopPuzzleServiceEnumResult::Session(
            DesktopPuzzleBridgeEnumResult::Snapshot(paused),
        )),
    )?;
    fixture.update(false)?;
    fixture.report_readiness()?;
    let controller = fixture
        .presentation
        .puzzle_panel()
        .and_then(|panel| panel.controller())
        .ok_or("missing paused controller")?;
    assert!(matches!(
        controller.snapshot().status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    assert!(controller.can_request(PresentationPuzzleControllerEnumAction::Resume));
    assert!(fixture.presentation.board_snapshot().is_none());
    assert!(fixture.application.try_next()?.is_none());
    fixture.click("Resume")?;
    let resume = fixture
        .application
        .try_next()?
        .ok_or("missing explicit Resume")?;
    assert!(matches!(
        resume.request().command().operation(),
        DesktopEnumCommand::Puzzle {
            operation: DesktopPuzzleServiceEnumOperation::Session {
                operation: crate::desktop::puzzle_bridge::DesktopPuzzleBridgeEnumOperation::Command(
                    PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready)
                ),
                ..
            },
            ..
        }
    ));
    Ok(())
}

#[tokio::test]
async fn original_session_concealment_cannot_hide_a_mounted_replacement()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.focus(fixture.window, false)?;
    fixture.focus(fixture.window, true)?;
    fixture.update(false)?;
    fixture.run = DesktopNativePuzzleWindowFixture::run(2)?;
    fixture.execute(PuzzlePersistenceEnumCommand::Readiness(
        PuzzleTimerEnumReadiness::Ready,
    ))?;
    fixture.select(Some(DesktopNativePuzzleWindowFixture::id(2)?));
    fixture.mount_puzzles().await?;
    fixture.update(false)?;
    fixture.report_readiness()?;
    assert!(fixture.presentation.board_snapshot().is_some());
    let controller = fixture
        .presentation
        .puzzle_panel()
        .and_then(|panel| panel.controller())
        .ok_or("missing replacement controller")?;
    assert_eq!(
        controller.snapshot().context().session_id(),
        DesktopNativePuzzleWindowFixture::id(2)?
    );
    assert!(controller.can_input());
    assert_eq!(
        fixture.provider.attempted()?,
        vec![(
            DesktopNativePuzzleWindowFixture::id(1)?,
            PuzzleTimerEnumPauseReason::FocusLost
        )]
    );
    fixture
        .provider
        .finish(Err(Arc::new(DesktopPuzzleServiceError::SessionMismatch {
            expected: DesktopNativePuzzleWindowFixture::id(1)?,
            selected: Some(DesktopNativePuzzleWindowFixture::id(2)?),
        })))?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    assert!(fixture.presentation.board_snapshot().is_some());
    Ok(())
}

#[test]
fn occlusion_and_visibility_changes_map_to_window_hidden() -> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.update(false)?;
    fixture.occlude(fixture.window, true);
    fixture.update(false)?;
    fixture.succeed()?;
    fixture.update(false)?;
    fixture.occlude(fixture.window, false);
    fixture.update(false)?;
    fixture
        .world
        .get_mut::<Window>(fixture.window)
        .ok_or("missing window")?
        .visible = false;
    fixture.update(false)?;
    fixture.succeed()?;
    fixture.update(false)?;
    fixture
        .world
        .get_mut::<Window>(fixture.window)
        .ok_or("missing window")?
        .visible = true;
    fixture.update(false)?;
    assert_eq!(
        fixture.provider.attempted()?,
        vec![
            (
                DesktopNativePuzzleWindowFixture::id(1)?,
                PuzzleTimerEnumPauseReason::WindowHidden
            );
            2
        ]
    );
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn suspend_pair_interrupts_once_and_resume_only_clears_native_suspension()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.update(false)?;
    fixture.lifecycle(AppLifecycle::WillSuspend);
    fixture.update(false)?;
    fixture.lifecycle(AppLifecycle::Suspended);
    fixture.update(false)?;
    assert!(fixture.adapter.suspended);
    fixture.succeed()?;
    fixture.lifecycle(AppLifecycle::WillResume);
    fixture.lifecycle(AppLifecycle::Running);
    fixture.update(false)?;
    assert!(!fixture.adapter.suspended);
    assert_eq!(
        fixture.provider.attempted()?,
        vec![(
            DesktopNativePuzzleWindowFixture::id(1)?,
            PuzzleTimerEnumPauseReason::Sleep
        )]
    );
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn render_failure_is_one_recovery_interruption_per_edge() -> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.update(true)?;
    fixture.succeed()?;
    for _ in 0..4 {
        fixture.update(true)?;
    }
    assert!(fixture.adapter.is_hidden(&fixture.world)?);
    fixture.update(false)?;
    fixture.update(true)?;
    assert_eq!(
        fixture.provider.attempted()?,
        vec![
            (
                DesktopNativePuzzleWindowFixture::id(1)?,
                PuzzleTimerEnumPauseReason::Recovery
            );
            2
        ]
    );
    Ok(())
}

#[test]
fn no_selected_owner_never_fabricates_an_interruption() -> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.select(None);
    fixture.focus(fixture.window, false)?;
    fixture.occlude(fixture.window, true);
    fixture.lifecycle(AppLifecycle::WillSuspend);
    fixture.update(true)?;
    fixture.select(Some(DesktopNativePuzzleWindowFixture::id(2)?));
    fixture.update(true)?;
    assert!(fixture.provider.attempted()?.is_empty());
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn busy_admission_keeps_its_original_session_until_service_progress()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.provider.lock()?.failure = Some(Arc::new(DesktopPuzzleServiceError::Busy));
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    let wakes = fixture.wake.0.load(Ordering::Relaxed);
    for _ in 0..4 {
        fixture.update(false)?;
    }
    assert_eq!(fixture.provider.attempted()?.len(), 1);
    assert_eq!(fixture.wake.0.load(Ordering::Relaxed), wakes);
    assert!(!fixture.adapter.is_settled());
    fixture.provider.lock()?.failure = None;
    fixture.select(Some(DesktopNativePuzzleWindowFixture::id(2)?));
    fixture.update(false)?;
    assert_eq!(
        fixture.provider.attempted()?,
        vec![
            (
                DesktopNativePuzzleWindowFixture::id(1)?,
                PuzzleTimerEnumPauseReason::FocusLost
            );
            2
        ]
    );
    fixture.succeed()?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn coalescing_preserves_the_latest_event_binding_when_an_older_receipt_fails()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    fixture.select(Some(DesktopNativePuzzleWindowFixture::id(2)?));
    fixture.occlude(fixture.window, true);
    fixture.update(false)?;
    fixture.select(Some(DesktopNativePuzzleWindowFixture::id(3)?));
    fixture.lifecycle(AppLifecycle::WillSuspend);
    fixture.update(false)?;
    fixture.select(Some(DesktopNativePuzzleWindowFixture::id(4)?));
    fixture
        .provider
        .finish(Err(Arc::new(DesktopPuzzleServiceError::SessionMismatch {
            expected: DesktopNativePuzzleWindowFixture::id(1)?,
            selected: Some(DesktopNativePuzzleWindowFixture::id(4)?),
        })))?;
    fixture.update(false)?;
    assert_eq!(
        fixture.provider.attempted()?,
        vec![
            (
                DesktopNativePuzzleWindowFixture::id(1)?,
                PuzzleTimerEnumPauseReason::FocusLost
            ),
            (
                DesktopNativePuzzleWindowFixture::id(3)?,
                PuzzleTimerEnumPauseReason::Sleep
            ),
        ]
    );
    assert!(
        fixture
            .presentation
            .failure()
            .is_some_and(|failure| failure.contains("requested puzzle session"))
    );
    assert!(!fixture.adapter.is_settled());
    fixture.succeed()?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn busy_receipt_does_not_rearm_itself_or_discard_a_newer_request()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    fixture.select(Some(DesktopNativePuzzleWindowFixture::id(2)?));
    fixture.occlude(fixture.window, true);
    fixture.update(false)?;
    fixture
        .provider
        .finish(Err(Arc::new(DesktopPuzzleServiceError::Busy)))?;
    fixture.update(false)?;
    for _ in 0..4 {
        fixture.update(false)?;
    }
    assert_eq!(fixture.provider.attempted()?.len(), 1);
    fixture.select(Some(DesktopNativePuzzleWindowFixture::id(3)?));
    fixture.update(false)?;
    assert_eq!(
        fixture.provider.attempted()?,
        vec![
            (
                DesktopNativePuzzleWindowFixture::id(1)?,
                PuzzleTimerEnumPauseReason::FocusLost
            ),
            (
                DesktopNativePuzzleWindowFixture::id(2)?,
                PuzzleTimerEnumPauseReason::WindowHidden
            ),
        ]
    );
    Ok(())
}

#[test]
fn busy_receipt_without_a_newer_event_retains_its_original_session()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    fixture
        .provider
        .finish(Err(Arc::new(DesktopPuzzleServiceError::Busy)))?;
    fixture.update(false)?;
    assert!(!fixture.adapter.is_settled());
    fixture.select(Some(DesktopNativePuzzleWindowFixture::id(2)?));
    fixture.update(false)?;
    assert_eq!(
        fixture.provider.attempted()?,
        vec![
            (
                DesktopNativePuzzleWindowFixture::id(1)?,
                PuzzleTimerEnumPauseReason::FocusLost
            );
            2
        ]
    );
    fixture.succeed()?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn busy_receipt_and_coalesced_service_progress_retry_the_original_session_once()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    let initial = Arc::new(fixture.snapshot()?);
    fixture
        .provider
        .publish_snapshot(Arc::clone(&initial), None);
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    assert!(!fixture.adapter.is_settled());
    let busy = Arc::new(DesktopPuzzleServiceError::Busy);
    fixture.provider.lock()?.failure = Some(Arc::clone(&busy));
    fixture.provider.finish(Err(Arc::clone(&busy)))?;
    fixture
        .provider
        .publish_snapshot(Arc::clone(&initial), Some(Arc::clone(&busy)));
    fixture.execute(PuzzlePersistenceEnumCommand::Readiness(
        PuzzleTimerEnumReadiness::Ready,
    ))?;
    let progress = Arc::new(fixture.snapshot()?);
    assert_ne!(progress.context(), initial.context());
    fixture.provider.lock()?.failure = None;
    fixture.provider.publish_snapshot(progress, Some(busy));
    fixture.update(false)?;
    let expected = vec![
        (
            DesktopNativePuzzleWindowFixture::id(1)?,
            PuzzleTimerEnumPauseReason::FocusLost
        );
        2
    ];
    assert_eq!(fixture.provider.attempted()?, expected);
    assert!(!fixture.adapter.is_settled());
    let wakes = fixture.wake.0.load(Ordering::Relaxed);
    for _ in 0..4 {
        fixture.update(false)?;
    }
    assert_eq!(fixture.provider.attempted()?, expected);
    assert_eq!(fixture.wake.0.load(Ordering::Relaxed), wakes);
    fixture.succeed()?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    assert_eq!(fixture.provider.attempted()?, expected);
    Ok(())
}

#[test]
fn busy_only_service_publication_never_counts_as_retry_progress()
-> DesktopNativePuzzleWindowTestResult {
    for publish_before_observation in [false, true] {
        let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
        let snapshot = Arc::new(fixture.snapshot()?);
        fixture
            .provider
            .publish_snapshot(Arc::clone(&snapshot), None);
        fixture.focus(fixture.window, false)?;
        fixture.update(false)?;
        let busy = Arc::new(DesktopPuzzleServiceError::Busy);
        fixture.provider.finish(Err(Arc::clone(&busy)))?;
        if !publish_before_observation {
            fixture.update(false)?;
        }
        fixture.provider.publish_snapshot(snapshot, Some(busy));
        fixture.update(false)?;
        let wakes = fixture.wake.0.load(Ordering::Relaxed);
        for _ in 0..4 {
            fixture.update(false)?;
        }
        assert_eq!(
            fixture.provider.attempted()?,
            vec![(
                DesktopNativePuzzleWindowFixture::id(1)?,
                PuzzleTimerEnumPauseReason::FocusLost
            )]
        );
        assert_eq!(fixture.wake.0.load(Ordering::Relaxed), wakes);
        assert!(!fixture.adapter.is_settled());
    }
    Ok(())
}

#[test]
fn busy_receipt_preserves_fresh_snapshot_progress_without_a_context_revision()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    let initial = Arc::new(fixture.snapshot()?);
    fixture
        .provider
        .publish_snapshot(Arc::clone(&initial), None);
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    let busy = Arc::new(DesktopPuzzleServiceError::Busy);
    fixture.provider.finish(Err(Arc::clone(&busy)))?;
    let progress = Arc::new(fixture.snapshot()?);
    assert_eq!(progress.context(), initial.context());
    assert!(!Arc::ptr_eq(&initial, &progress));
    fixture.provider.publish_snapshot(progress, Some(busy));
    fixture.update(false)?;
    assert_eq!(fixture.provider.attempted()?.len(), 2);
    fixture.succeed()?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn native_lane_release_retries_without_a_new_owner_snapshot() -> DesktopNativePuzzleWindowTestResult
{
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    let snapshot = Arc::new(fixture.snapshot()?);
    let initial = fixture.provider.state.borrow().clone();
    fixture.provider.state.send_replace(
        DesktopPuzzleServiceState::new(
            initial.selected(),
            Some(snapshot),
            initial.activity(),
            None,
            None,
        )
        .with_native_admission_progress(DesktopPuzzleServiceNativeProgress::new()),
    );
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    fixture
        .provider
        .finish(Err(Arc::new(DesktopPuzzleServiceError::Busy)))?;
    fixture.update(false)?;
    let previous = fixture.provider.state.borrow().clone();
    fixture.provider.state.send_replace(previous.clone());
    fixture.update(false)?;
    assert_eq!(fixture.provider.attempted()?.len(), 1);
    fixture.provider.state.send_replace(
        previous
            .clone()
            .with_native_admission_progress(DesktopPuzzleServiceNativeProgress::new()),
    );
    fixture.update(false)?;
    assert_eq!(fixture.provider.attempted()?.len(), 2);
    let current = fixture.provider.state.borrow().clone();
    assert!(Arc::ptr_eq(
        previous.snapshot().ok_or("missing retained snapshot")?,
        current.snapshot().ok_or("missing unchanged snapshot")?,
    ));
    fixture.succeed()?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn busy_admission_retries_on_activity_and_health_progress_without_rebasing()
-> DesktopNativePuzzleWindowTestResult {
    for (activity, health) in [
        (
            DesktopPuzzleServiceEnumActivity::Loading {
                id: DesktopNativePuzzleWindowFixture::id(2)?,
            },
            None,
        ),
        (
            DesktopPuzzleServiceEnumActivity::Selected,
            Some(DesktopPuzzleBridgeEnumHealth::Ready),
        ),
    ] {
        let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
        let busy = Arc::new(DesktopPuzzleServiceError::Busy);
        fixture.provider.lock()?.failure = Some(Arc::clone(&busy));
        fixture.focus(fixture.window, false)?;
        fixture.update(false)?;
        fixture.provider.lock()?.failure = None;
        fixture
            .provider
            .state
            .send_replace(DesktopPuzzleServiceState::new(
                Some(DesktopNativePuzzleWindowFixture::id(1)?),
                None,
                activity,
                health,
                Some(busy),
            ));
        fixture.update(false)?;
        let expected = vec![
            (
                DesktopNativePuzzleWindowFixture::id(1)?,
                PuzzleTimerEnumPauseReason::FocusLost
            );
            2
        ];
        assert_eq!(fixture.provider.attempted()?, expected);
        fixture
            .provider
            .finish(Err(Arc::new(DesktopPuzzleServiceError::Busy)))?;
        fixture.update(false)?;
        for _ in 0..3 {
            let repeated = fixture.provider.state.borrow().clone();
            fixture.provider.state.send_replace(repeated);
            fixture.update(false)?;
        }
        assert_eq!(fixture.provider.attempted()?, expected);
        assert!(!fixture.adapter.is_settled());
    }
    Ok(())
}

#[test]
fn settled_shutdown_waits_for_both_active_and_coalesced_receipts()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    fixture.lifecycle(AppLifecycle::WillSuspend);
    fixture.update(false)?;
    assert!(!fixture.adapter.is_settled());
    fixture.succeed()?;
    fixture.update(false)?;
    assert!(!fixture.adapter.is_settled());
    assert_eq!(fixture.provider.attempted()?.len(), 2);
    fixture.succeed()?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn closed_receipt_is_a_terminal_observed_error_and_allows_settled_shutdown()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    assert!(!fixture.adapter.is_settled());
    let sender = fixture
        .provider
        .lock()?
        .pending
        .pop_front()
        .ok_or("missing receipt sender")?;
    drop(sender);
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    assert!(
        fixture
            .presentation
            .failure()
            .is_some_and(|failure| failure.contains("outcome channel closed"))
    );
    fixture.update(false)?;
    assert_eq!(fixture.provider.attempted()?.len(), 1);
    Ok(())
}

#[test]
fn terminal_admission_error_is_presented_without_a_graphics_failure()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.provider.lock()?.failure = Some(Arc::new(DesktopPuzzleServiceError::Closed));
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    assert!(fixture.adapter.is_settled());
    assert!(!fixture.adapter.graphics_failed);
    assert!(
        fixture
            .presentation
            .failure()
            .is_some_and(|failure| failure.contains("service is unavailable"))
    );
    Ok(())
}

#[test]
fn paused_run_gets_ready_without_a_board_and_stays_paused() -> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.execute(PuzzlePersistenceEnumCommand::Readiness(
        PuzzleTimerEnumReadiness::Ready,
    ))?;
    fixture.execute(PuzzlePersistenceEnumCommand::Pause(
        PuzzleTimerEnumPauseReason::FocusLost,
    ))?;
    let mut controller = PresentationPuzzleController::new(fixture.snapshot()?);
    assert!(controller.board().is_none());
    let readiness = fixture.frame(None).board_readiness(Some(&controller), true);
    assert_eq!(readiness, PuzzleTimerEnumReadiness::Ready);
    assert!(
        controller
            .report_readiness(readiness, crate::desktop::identity::DesktopRequestId::new())?
            .is_none()
    );
    assert!(controller.can_request(PresentationPuzzleControllerEnumAction::Resume));
    assert!(matches!(
        controller.snapshot().status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    Ok(())
}

#[test]
fn visible_board_requires_the_actual_current_rendered_fen_and_variant()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.execute(PuzzlePersistenceEnumCommand::Readiness(
        PuzzleTimerEnumReadiness::Ready,
    ))?;
    let mut controller = PresentationPuzzleController::new(fixture.snapshot()?);
    let old_board = controller.board().ok_or("missing active board")?.clone();
    assert_eq!(
        fixture
            .frame(Some(&old_board))
            .board_readiness(Some(&controller), true),
        PuzzleTimerEnumReadiness::Ready
    );
    fixture.execute(PuzzlePersistenceEnumCommand::Move(
        BoardPosition::standard().parse_uci("e2e4")?,
    ))?;
    assert!(controller.observe_snapshot(fixture.snapshot()?));
    assert_eq!(
        fixture
            .frame(Some(&old_board))
            .board_readiness(Some(&controller), true),
        PuzzleTimerEnumReadiness::BoardNotReady
    );
    assert_eq!(
        fixture.frame(None).board_readiness(Some(&controller), true),
        PuzzleTimerEnumReadiness::BoardNotReady
    );
    let current_board = controller.board().ok_or("missing newer board")?;
    assert_eq!(
        fixture
            .frame(Some(current_board))
            .board_readiness(Some(&controller), true),
        PuzzleTimerEnumReadiness::Ready
    );
    Ok(())
}

#[test]
fn equal_fen_with_a_different_variant_is_not_ready() -> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    let fen = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
    fixture.run = DesktopNativePuzzleWindowFixture::run_from(
        BoardPosition::from_fen(fen, crate::board::enums::BoardEnumVariant::Standard)?,
        ["e2e4", "e8e7", "e1f2"],
        DesktopNativePuzzleWindowFixture::id(1)?,
    )?;
    fixture.execute(PuzzlePersistenceEnumCommand::Readiness(
        PuzzleTimerEnumReadiness::Ready,
    ))?;
    let controller = PresentationPuzzleController::new(fixture.snapshot()?);
    let variant_board = DesktopBoardSnapshot::from_position(&BoardPosition::from_fen(
        fen,
        crate::board::enums::BoardEnumVariant::Chess960,
    )?);
    assert_eq!(
        controller
            .board()
            .ok_or("missing variant fixture board")?
            .fen(),
        variant_board.fen()
    );
    assert_eq!(
        fixture
            .frame(Some(&variant_board))
            .board_readiness(Some(&controller), true),
        PuzzleTimerEnumReadiness::BoardNotReady
    );
    Ok(())
}

#[test]
fn physical_window_state_blocks_readiness_until_each_native_condition_clears()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.update(false)?;
    assert!(!fixture.adapter.is_hidden(&fixture.world)?);
    fixture
        .world
        .get_mut::<Window>(fixture.window)
        .ok_or("missing window")?
        .focused = false;
    fixture.update(false)?;
    assert!(fixture.adapter.is_hidden(&fixture.world)?);
    fixture.focus(fixture.window, true)?;
    fixture.occlude(fixture.window, true);
    fixture.update(false)?;
    assert!(fixture.adapter.is_hidden(&fixture.world)?);
    fixture.occlude(fixture.window, false);
    fixture.lifecycle(AppLifecycle::WillSuspend);
    fixture.update(false)?;
    assert!(fixture.adapter.is_hidden(&fixture.world)?);
    fixture.lifecycle(AppLifecycle::Running);
    fixture.update(false)?;
    assert!(!fixture.adapter.is_hidden(&fixture.world)?);
    fixture
        .world
        .get_mut::<Window>(fixture.window)
        .ok_or("missing window")?
        .visible = false;
    assert!(fixture.adapter.is_hidden(&fixture.world)?);
    Ok(())
}

#[test]
fn graphics_and_input_readiness_gate_even_paused_runs() -> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.execute(PuzzlePersistenceEnumCommand::Pause(
        PuzzleTimerEnumPauseReason::Manual,
    ))?;
    let controller = PresentationPuzzleController::new(fixture.snapshot()?);
    let mut frame = fixture.frame(None);
    assert_eq!(
        frame.board_readiness(Some(&controller), false),
        PuzzleTimerEnumReadiness::BoardNotReady
    );
    frame.graphics_ready = false;
    assert_eq!(
        frame.board_readiness(Some(&controller), true),
        PuzzleTimerEnumReadiness::BoardNotReady
    );
    assert_eq!(
        frame.board_readiness(None, true),
        PuzzleTimerEnumReadiness::BoardNotReady
    );
    Ok(())
}

#[test]
fn hidden_readiness_ignores_revision_and_pending_changes_but_ready_rearms()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    let first = fixture.snapshot()?;
    fixture.execute(PuzzlePersistenceEnumCommand::Readiness(
        PuzzleTimerEnumReadiness::Ready,
    ))?;
    let next = fixture.snapshot()?;
    let hidden = DesktopNativePuzzleWindowReadiness::new(
        PuzzleTimerEnumReadiness::Hidden,
        Some(first.context()),
        false,
    );
    assert!(hidden.matches(PuzzleTimerEnumReadiness::Hidden, Some(next.context()), true));
    let ready = DesktopNativePuzzleWindowReadiness::new(
        PuzzleTimerEnumReadiness::Ready,
        Some(first.context()),
        false,
    );
    assert!(!ready.matches(PuzzleTimerEnumReadiness::Ready, Some(next.context()), false));
    assert!(!ready.matches(PuzzleTimerEnumReadiness::Ready, Some(first.context()), true));
    assert!(ready.matches(
        PuzzleTimerEnumReadiness::Ready,
        Some(first.context()),
        false
    ));
    assert!(!hidden.matches(
        PuzzleTimerEnumReadiness::BoardNotReady,
        Some(first.context()),
        false
    ));
    Ok(())
}

#[test]
fn repeated_hidden_frames_do_not_create_an_interruption_or_repaint_loop()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    fixture.focus(fixture.window, false)?;
    fixture.update(false)?;
    fixture.succeed()?;
    fixture.update(false)?;
    let wakes = fixture.wake.0.load(Ordering::Relaxed);
    for _ in 0..8 {
        fixture.application.port().acknowledge_wake();
        fixture.adapter.report_readiness(
            DesktopNativePuzzleWindowFrame {
                world: &fixture.world,
                graphics_ready: true,
                rendered_board: None,
            },
            &mut fixture.presentation,
            &fixture.application.port(),
        )?;
        fixture.update(false)?;
    }
    assert_eq!(fixture.wake.0.load(Ordering::Relaxed), wakes);
    assert_eq!(fixture.provider.attempted()?.len(), 1);
    assert!(fixture.application.try_next()?.is_none());
    assert!(fixture.adapter.is_settled());
    Ok(())
}

#[test]
fn games_workspace_and_missing_bound_window_cannot_acknowledge_ready()
-> DesktopNativePuzzleWindowTestResult {
    let mut fixture = DesktopNativePuzzleWindowFixture::new()?;
    assert_eq!(
        fixture.presentation.workspace(),
        PresentationAppEnumWorkspace::Games
    );
    fixture.adapter.report_readiness(
        DesktopNativePuzzleWindowFrame {
            world: &fixture.world,
            graphics_ready: true,
            rendered_board: None,
        },
        &mut fixture.presentation,
        &fixture.application.port(),
    )?;
    assert_eq!(
        fixture
            .adapter
            .readiness
            .as_ref()
            .ok_or("missing readiness")?
            .value,
        PuzzleTimerEnumReadiness::Hidden
    );
    assert!(fixture.world.despawn(fixture.window));
    assert!(matches!(
        fixture.adapter.update(
            &fixture.world,
            &mut fixture.presentation,
            &fixture.application.port(),
            false
        ),
        Err(DesktopNativePuzzleWindowError::MissingWindow(_))
    ));
    Ok(())
}
