use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy_egui::egui;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use thiserror::Error;
use tokio::sync::watch;

use crate::board::position::BoardPosition;
use crate::capacity::budget::CapacityBudget;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationWakeError, DesktopApplicationWakeProvider,
    DesktopApplicationWork,
};
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};
use crate::desktop::puzzle_bridge::{
    DesktopPuzzleBridgeEnumOperation, DesktopPuzzleBridgeEnumResult,
};
use crate::desktop::puzzle_commands::DesktopPuzzleCommands;
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceEnumOperation,
    DesktopPuzzleServiceEnumResult, DesktopPuzzleServiceState,
};
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::presentation::puzzle::controller::PresentationPuzzleControllerEnumAction;
use crate::presentation::puzzle::panel::{
    PresentationPuzzlePanel, PresentationPuzzlePanelConnection,
};
use crate::puzzle::checkpoint::{
    PuzzleCheckpoint, PuzzleCheckpointObservation, PuzzleCheckpointSnapshot,
};
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::{PuzzleEnumCountdownMode, PuzzleEnumPhase};
use crate::puzzle::history::{PuzzleHistoryEnumConfirmation, PuzzleHistoryEnumScope};
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
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::puzzle_repository::StoragePuzzleRepository;

type PresentationPuzzlePanelTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationPuzzlePanelTestClock;

impl ClockTimeProvider for PresentationPuzzlePanelTestClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(0))
    }
}

struct PresentationPuzzlePanelTestWake;

impl DesktopApplicationWakeProvider for PresentationPuzzlePanelTestWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

#[derive(Debug, Error)]
enum PresentationPuzzlePanelTestStorageError {
    #[error("The controlled storage completion has already been consumed")]
    Consumed,
    #[error("The controlled storage write failed")]
    Write,
}

struct PresentationPuzzlePanelTestWrite(
    Option<Result<PuzzlePersistenceReceipt, PresentationPuzzlePanelTestStorageError>>,
);

impl PuzzleStorageWrite for PresentationPuzzlePanelTestWrite {
    type Error = PresentationPuzzlePanelTestStorageError;

    async fn wait(&mut self) -> Result<PuzzlePersistenceReceipt, Self::Error> {
        self.0
            .take()
            .ok_or(PresentationPuzzlePanelTestStorageError::Consumed)?
    }
}

struct PresentationPuzzlePanelTestRepository(Arc<AtomicBool>);

impl PuzzleStorageRepository for PresentationPuzzlePanelTestRepository {
    type Error = PresentationPuzzlePanelTestStorageError;
    type Write = PresentationPuzzlePanelTestWrite;

    fn begin_save(
        &self,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<Self::Write, Self::Error> {
        let revision = request
            .expected_storage_revision()
            .value()
            .checked_add(1)
            .ok_or(PresentationPuzzlePanelTestStorageError::Write)?;
        let outcome = if self.0.swap(false, Ordering::AcqRel) {
            Err(PresentationPuzzlePanelTestStorageError::Write)
        } else {
            Ok(PuzzlePersistenceReceipt::committed(
                request,
                PuzzlePersistenceStorageRevision::new(revision),
            ))
        };
        Ok(PresentationPuzzlePanelTestWrite(Some(outcome)))
    }
}

struct PresentationPuzzlePanelFixture {
    application:
        DesktopApplication<PresentationPuzzlePanelTestClock, PresentationPuzzlePanelTestWake>,
    state: watch::Sender<DesktopPuzzleServiceState>,
    panel: PresentationPuzzlePanel,
    owner: DesktopPuzzleCommands<PresentationPuzzlePanelTestRepository>,
    fail_next: Arc<AtomicBool>,
}

impl PresentationPuzzlePanelFixture {
    fn id(value: u64) -> PresentationPuzzlePanelTestResult<PuzzleSessionId> {
        Ok(PuzzleSessionId::new(
            NonZeroU64::new(value).ok_or("zero fixture identity")?,
        ))
    }

    fn owner(
        id: u64,
    ) -> PresentationPuzzlePanelTestResult<
        DesktopPuzzleCommands<PresentationPuzzlePanelTestRepository>,
    > {
        Self::owner_with_repository(id, Arc::new(AtomicBool::new(false)))
    }

    fn run(id: u64) -> PresentationPuzzlePanelTestResult<PuzzlePersistenceRun> {
        let initial = BoardPosition::standard();
        let mut position = initial.clone();
        let mut line = Vec::new();
        for text in ["e2e4", "e7e5", "g1f3"] {
            let movement = position.parse_uci(text)?;
            position = position.play(movement)?;
            line.push(movement);
        }
        let definition = PuzzleDefinition::new(
            PuzzleDefinitionIdentity::new("panel-fixture".into(), "v1".into(), "first".into())?,
            initial,
            vec![line],
            PuzzleDefinitionMetadata::default(),
        )?;
        Ok(PuzzlePersistenceRun::new(
            Self::id(id)?,
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![definition],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(91),
        )?)
    }

    fn owner_with_repository(
        id: u64,
        fail_next: Arc<AtomicBool>,
    ) -> PresentationPuzzlePanelTestResult<
        DesktopPuzzleCommands<PresentationPuzzlePanelTestRepository>,
    > {
        Ok(DesktopPuzzleCommands::new(
            Self::run(id)?,
            PresentationPuzzlePanelTestRepository(fail_next),
            ClockInstant::from_nanos(0),
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?)
    }

    fn new() -> PresentationPuzzlePanelTestResult<Self> {
        let application = DesktopApplication::new(
            Arc::new(PresentationPuzzlePanelTestClock),
            Arc::new(PresentationPuzzlePanelTestWake),
        );
        let (state, receiver) = watch::channel(DesktopPuzzleServiceState::new(
            None,
            None,
            DesktopPuzzleServiceEnumActivity::Idle,
            None,
            None,
        ));
        let panel = PresentationPuzzlePanel::new(PresentationPuzzlePanelConnection {
            owner: DesktopMessagesEntityId::new(),
            state: receiver,
        });
        let fail_next = Arc::new(AtomicBool::new(false));
        Ok(Self {
            application,
            state,
            panel,
            owner: Self::owner_with_repository(1, Arc::clone(&fail_next))?,
            fail_next,
        })
    }

    fn next(&self) -> PresentationPuzzlePanelTestResult<DesktopApplicationWork> {
        self.application
            .try_next()?
            .ok_or_else(|| "missing application work".into())
    }

    fn operation(
        work: &DesktopApplicationWork,
    ) -> PresentationPuzzlePanelTestResult<&DesktopPuzzleServiceEnumOperation> {
        match work.request().command().operation() {
            DesktopEnumCommand::Puzzle { operation, .. } => Ok(operation),
            DesktopEnumCommand::Game(_)
            | DesktopEnumCommand::Editor { .. }
            | DesktopEnumCommand::Library(_)
            | DesktopEnumCommand::Engine(_)
            | DesktopEnumCommand::Bootstrap
            | DesktopEnumCommand::FileSelect { .. }
            | DesktopEnumCommand::StreamSubscribe
            | DesktopEnumCommand::StreamAcknowledge { .. }
            | DesktopEnumCommand::StreamResync { .. }
            | DesktopEnumCommand::Backup(_)
            | DesktopEnumCommand::Stop { .. }
            | DesktopEnumCommand::Pause { .. }
            | DesktopEnumCommand::Quit => Err("unexpected application operation".into()),
        }
    }

    fn publish(
        &mut self,
        snapshot: DesktopPuzzleSnapshot,
        activity: DesktopPuzzleServiceEnumActivity,
    ) {
        self.state.send_replace(DesktopPuzzleServiceState::new(
            Some(snapshot.context().session_id()),
            Some(Arc::new(snapshot)),
            activity,
            None,
            None,
        ));
    }

    async fn durable(&mut self) -> PresentationPuzzlePanelTestResult {
        self.owner.begin_write(ClockInstant::from_nanos(0))?;
        for _ in 0..16 {
            if !self.owner.has_retained_write() {
                return Ok(());
            }
            self.owner.wait().await?;
            self.owner.settle(ClockInstant::from_nanos(0))?;
        }
        Err("controlled puzzle writes did not drain".into())
    }

    async fn open(&mut self) -> PresentationPuzzlePanelTestResult {
        self.durable().await?;
        self.panel.open_saved(
            Self::id(1)?,
            PuzzlePersistenceStorageRevision::new(1),
            &self.application.port(),
        )?;
        let work = self.next()?;
        self.application.begin_commit(&work)?;
        let snapshot = self.owner.snapshot(ClockInstant::from_nanos(0))?;
        self.publish(snapshot.clone(), DesktopPuzzleServiceEnumActivity::Selected);
        self.application.complete_value(
            &work,
            DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Opened(Arc::new(
                snapshot,
            )))),
            None,
        )?;
        self.panel.synchronize(&self.application.port())?;
        Ok(())
    }

    async fn finish(&mut self, work: DesktopApplicationWork) -> PresentationPuzzlePanelTestResult {
        let operation = Self::operation(&work)?.clone();
        let wants_summary = matches!(
            operation,
            DesktopPuzzleServiceEnumOperation::Session {
                operation: DesktopPuzzleBridgeEnumOperation::Summary,
                ..
            }
        );
        let control = work.request().command().operation().is_control();
        if !control {
            self.application.begin_commit(&work)?;
        }
        match operation {
            DesktopPuzzleServiceEnumOperation::Session {
                expected,
                operation,
            } => match operation {
                DesktopPuzzleBridgeEnumOperation::Command(command) => {
                    self.owner
                        .execute(&expected, command, ClockInstant::from_nanos(0))?
                }
                DesktopPuzzleBridgeEnumOperation::RetrySave => self
                    .owner
                    .retry_save(&expected, ClockInstant::from_nanos(0))?,
                DesktopPuzzleBridgeEnumOperation::Snapshot
                | DesktopPuzzleBridgeEnumOperation::Summary => {}
            },
            DesktopPuzzleServiceEnumOperation::Interrupt { session, reason } => {
                assert_eq!(session, self.owner.context().session_id());
                self.owner.execute(
                    &self.owner.context(),
                    PuzzlePersistenceEnumCommand::Pause(reason),
                    ClockInstant::from_nanos(0),
                )?;
            }
            DesktopPuzzleServiceEnumOperation::Inventory { .. }
            | DesktopPuzzleServiceEnumOperation::OpenSaved { .. }
            | DesktopPuzzleServiceEnumOperation::DeleteHistory { .. } => {
                return Err("unexpected fixture operation".into());
            }
        }
        self.durable().await?;
        let snapshot = Arc::new(self.owner.snapshot(ClockInstant::from_nanos(0))?);
        let result = DesktopPuzzleServiceEnumResult::Session(if wants_summary {
            DesktopPuzzleBridgeEnumResult::Summary {
                snapshot: Arc::clone(&snapshot),
                summary: self.owner.summary(ClockInstant::from_nanos(0))?,
            }
        } else {
            DesktopPuzzleBridgeEnumResult::Snapshot(Arc::clone(&snapshot))
        });
        let activity = self.state.borrow().activity();
        self.publish((*snapshot).clone(), activity);
        if control {
            self.application
                .complete_puzzle_control(&work, Ok(result))?;
        } else {
            self.application.complete_value(
                &work,
                DesktopEnumValue::Puzzle(Box::new(result)),
                None,
            )?;
        }
        self.panel.synchronize(&self.application.port())?;
        Ok(())
    }

    async fn ready(&mut self) -> PresentationPuzzlePanelTestResult {
        self.panel
            .report_readiness(PuzzleTimerEnumReadiness::Ready, &self.application.port())?;
        self.finish(self.next()?).await?;
        self.panel
            .report_readiness(PuzzleTimerEnumReadiness::Ready, &self.application.port())?;
        Ok(())
    }
}

#[tokio::test]
async fn initial_watch_does_not_install_a_run_and_idle_has_no_pending_work()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    let snapshot = fixture.owner.snapshot(ClockInstant::from_nanos(0))?;
    fixture.publish(snapshot, DesktopPuzzleServiceEnumActivity::Selected);
    fixture.panel.synchronize(&fixture.application.port())?;
    assert!(fixture.panel.controller().is_none());
    assert!(fixture.panel.board().is_none());
    assert!(!fixture.panel.has_pending_work());
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[tokio::test]
async fn admission_reuses_the_actual_request_and_receipt_and_submits_once()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let pending = fixture
        .panel
        .ordinary
        .as_ref()
        .ok_or("missing pending action")?;
    let request = pending
        .request
        .as_ref()
        .ok_or("missing application request")?
        .clone();
    let receipt = pending.receipt.as_ref().ok_or("missing receipt")?.clone();
    let intent = pending
        .intent
        .as_ref()
        .ok_or("missing original intent")?
        .clone();
    assert_ne!(intent.request_id(), request.request_id());
    assert!(receipt.shares_outcome(&fixture.application.port().try_submit(&request)?));
    for _ in 0..4 {
        fixture.panel.synchronize(&fixture.application.port())?;
    }
    let work = fixture.next()?;
    assert_eq!(work.request().request_id(), request.request_id());
    assert!(work.receipt().shares_outcome(&receipt));
    assert!(fixture.application.try_next()?.is_none());
    fixture.finish(work).await?;
    assert!(
        !fixture
            .panel
            .controller()
            .ok_or("missing controller")?
            .is_pending()
    );
    assert!(!fixture.panel.has_pending_work());
    Ok(())
}

#[tokio::test]
async fn entity_busy_retains_one_request_until_the_owner_is_available()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    let blocker = fixture.application.port().request(DesktopCommand::puzzle(
        fixture.panel.connection.owner,
        DesktopPuzzleServiceEnumOperation::Session {
            expected: fixture.owner.context(),
            operation: DesktopPuzzleBridgeEnumOperation::Command(
                PuzzlePersistenceEnumCommand::Hint,
            ),
        },
    ))?;
    fixture.application.port().try_submit(&blocker)?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let pending = fixture
        .panel
        .ordinary
        .as_ref()
        .ok_or("missing retained action")?;
    let id = pending
        .request
        .as_ref()
        .ok_or("missing request")?
        .request_id();
    assert!(pending.receipt.is_none());
    fixture.panel.synchronize(&fixture.application.port())?;
    assert_eq!(
        fixture
            .panel
            .ordinary
            .as_ref()
            .and_then(|pending| pending.request.as_ref())
            .map(|request| request.request_id()),
        Some(id)
    );
    let work = fixture.next()?;
    fixture.application.complete_failure(
        &work,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "Controlled owner release".into(),
            true,
        ),
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    assert_eq!(fixture.next()?.request().request_id(), id);
    Ok(())
}

#[tokio::test]
async fn hidden_interrupt_is_retained_behind_an_ordinary_action_for_its_original_session()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let ordinary = fixture.next()?;
    fixture.application.begin_commit(&ordinary)?;
    fixture.panel.report_readiness(
        PuzzleTimerEnumReadiness::Hidden,
        &fixture.application.port(),
    )?;
    assert!(fixture.panel.board().is_none());
    let urgent = fixture.next()?;
    assert!(
        matches!(PresentationPuzzlePanelFixture::operation(&urgent)?, DesktopPuzzleServiceEnumOperation::Interrupt { session, reason: PuzzleTimerEnumPauseReason::FocusLost } if *session == PresentationPuzzlePanelFixture::id(1)?)
    );
    let pending = fixture
        .panel
        .urgent
        .as_ref()
        .ok_or("missing urgent action")?;
    assert_eq!(
        pending
            .intent
            .as_ref()
            .ok_or("missing urgent intent")?
            .context()
            .session_id(),
        PresentationPuzzlePanelFixture::id(1)?
    );
    assert!(fixture.panel.ordinary.is_some());
    fixture.application.complete_failure(
        &ordinary,
        DesktopFailure::new(
            DesktopEnumFailureCode::Cancelled,
            "Controlled cancelled hint".into(),
            true,
        ),
    )?;
    fixture.finish(urgent).await?;
    assert!(fixture.panel.board().is_none());
    assert!(
        !fixture
            .panel
            .controller()
            .ok_or("missing controller")?
            .is_pending()
    );
    Ok(())
}

#[tokio::test]
async fn readiness_while_paused_enables_only_explicit_resume() -> PresentationPuzzlePanelTestResult
{
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    fixture.panel.report_readiness(
        PuzzleTimerEnumReadiness::Hidden,
        &fixture.application.port(),
    )?;
    fixture.finish(fixture.next()?).await?;
    assert!(fixture.panel.board().is_none());
    fixture
        .panel
        .report_readiness(PuzzleTimerEnumReadiness::Ready, &fixture.application.port())?;
    let controller = fixture.panel.controller().ok_or("missing controller")?;
    assert!(controller.can_request(PresentationPuzzleControllerEnumAction::Resume));
    assert!(matches!(
        controller.snapshot().status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    assert!(fixture.application.try_next()?.is_none());
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Resume,
        &fixture.application.port(),
    )?;
    fixture.finish(fixture.next()?).await?;
    assert!(fixture.panel.board().is_some());
    Ok(())
}

#[tokio::test]
async fn service_loading_replacing_and_closing_hide_the_previous_board()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    for activity in [
        DesktopPuzzleServiceEnumActivity::Loading {
            id: PresentationPuzzlePanelFixture::id(2)?,
        },
        DesktopPuzzleServiceEnumActivity::Replacing {
            id: PresentationPuzzlePanelFixture::id(2)?,
        },
        DesktopPuzzleServiceEnumActivity::Closing,
    ] {
        let snapshot = fixture.owner.snapshot(ClockInstant::from_nanos(0))?;
        fixture.publish(snapshot, activity);
        assert!(fixture.panel.board().is_none());
        fixture.panel.synchronize(&fixture.application.port())?;
        assert!(fixture.panel.board().is_none());
    }
    Ok(())
}

#[tokio::test]
async fn foreign_watch_and_foreign_completion_cannot_install_or_finish_the_current_intent()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    fixture.application.begin_commit(&work)?;
    let foreign =
        Arc::new(PresentationPuzzlePanelFixture::owner(2)?.snapshot(ClockInstant::from_nanos(0))?);
    fixture.publish(
        (*foreign).clone(),
        DesktopPuzzleServiceEnumActivity::Selected,
    );
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Session(
            DesktopPuzzleBridgeEnumResult::Snapshot(foreign),
        ))),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    let controller = fixture.panel.controller().ok_or("missing controller")?;
    assert_eq!(
        controller.snapshot().context().session_id(),
        PresentationPuzzlePanelFixture::id(1)?
    );
    assert!(controller.is_pending());
    assert!(fixture.panel.failure().is_some());
    assert!(
        fixture
            .panel
            .ordinary
            .as_ref()
            .and_then(|pending| pending.receipt.as_ref())
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn a_same_session_late_completion_finishes_only_its_slot_without_rollback()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    fixture.application.begin_commit(&work)?;
    let old = Arc::new(fixture.owner.snapshot(ClockInstant::from_nanos(0))?);
    let context = fixture.owner.context();
    fixture.owner.execute(
        &context,
        PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::Manual),
        ClockInstant::from_nanos(0),
    )?;
    fixture.durable().await?;
    let newer = fixture.owner.snapshot(ClockInstant::from_nanos(0))?;
    let context = newer.context().clone();
    fixture.publish(newer, DesktopPuzzleServiceEnumActivity::Selected);
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Session(
            DesktopPuzzleBridgeEnumResult::Snapshot(old),
        ))),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    let controller = fixture.panel.controller().ok_or("missing controller")?;
    assert_eq!(controller.snapshot().context(), &context);
    assert!(!controller.is_pending());
    assert!(fixture.panel.board().is_none());
    Ok(())
}

#[tokio::test]
async fn a_listed_opponent_reply_requires_actual_readiness_and_is_requested_once()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    let movement = BoardPosition::standard().parse_uci("e2e4")?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Move(movement),
        &fixture.application.port(),
    )?;
    fixture.finish(fixture.next()?).await?;
    assert_eq!(
        fixture
            .panel
            .controller()
            .ok_or("missing controller")?
            .snapshot()
            .attempt()
            .ok_or("missing attempt")?
            .phase(),
        PuzzleEnumPhase::OpponentReply
    );
    assert!(fixture.application.try_next()?.is_none());
    fixture
        .panel
        .report_readiness(PuzzleTimerEnumReadiness::Ready, &fixture.application.port())?;
    let reply = fixture.next()?;
    assert!(matches!(
        PresentationPuzzlePanelFixture::operation(&reply)?,
        DesktopPuzzleServiceEnumOperation::Session {
            operation: DesktopPuzzleBridgeEnumOperation::Command(
                PuzzlePersistenceEnumCommand::OpponentReply
            ),
            ..
        }
    ));
    for _ in 0..3 {
        fixture.panel.synchronize(&fixture.application.port())?;
        fixture
            .panel
            .report_readiness(PuzzleTimerEnumReadiness::Ready, &fixture.application.port())?;
    }
    assert!(fixture.application.try_next()?.is_none());
    fixture.finish(reply).await?;
    Ok(())
}

#[tokio::test]
async fn terminal_failure_keeps_the_actual_receipt_and_original_intent_until_explicit_retry()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    let receipt = work.receipt().clone();
    let intent = fixture
        .panel
        .ordinary
        .as_ref()
        .and_then(|pending| pending.intent.as_ref())
        .ok_or("missing intent")?
        .clone();
    fixture.application.complete_failure(
        &work,
        DesktopFailure::new(
            DesktopEnumFailureCode::FileUnavailable,
            "Controlled save error".into(),
            true,
        ),
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    let pending = fixture
        .panel
        .ordinary
        .as_ref()
        .ok_or("lost failed request")?;
    assert!(
        pending
            .receipt
            .as_ref()
            .ok_or("lost failed receipt")?
            .shares_outcome(&receipt)
    );
    assert_eq!(pending.intent.as_ref(), Some(&intent));
    assert!(fixture.panel.failure().is_some());
    assert!(!fixture.panel.has_pending_work());
    fixture.panel.retry(&fixture.application.port())?;
    let retry = fixture.next()?;
    assert_ne!(retry.receipt().request_id(), receipt.request_id());
    assert!(
        fixture
            .panel
            .last_ordinary
            .as_ref()
            .and_then(|pending| pending.receipt.as_ref())
            .ok_or("lost prior receipt")?
            .shares_outcome(&receipt)
    );
    fixture.finish(retry).await?;
    Ok(())
}

#[tokio::test]
async fn ended_open_uses_only_the_returned_canonical_summary() -> PresentationPuzzlePanelTestResult
{
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.durable().await?;
    fixture.owner.execute(
        &fixture.owner.context(),
        PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed),
        ClockInstant::from_nanos(0),
    )?;
    let summary = fixture.owner.summary(ClockInstant::from_nanos(0))?;
    fixture.panel.open_saved(
        PresentationPuzzlePanelFixture::id(1)?,
        PuzzlePersistenceStorageRevision::new(1),
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Ended {
            id: PresentationPuzzlePanelFixture::id(1)?,
            storage_revision: PuzzlePersistenceStorageRevision::new(1),
            summary,
        })),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    assert!(fixture.panel.controller().is_none());
    assert!(fixture.panel.board().is_none());
    assert_eq!(fixture.panel.summary(), Some(&summary));
    Ok(())
}

#[tokio::test]
async fn confirmed_end_keeps_exact_context_instead_of_becoming_an_interrupt()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    let context = fixture
        .panel
        .controller()
        .ok_or("missing controller")?
        .snapshot()
        .context()
        .clone();
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Unconfirmed),
        &fixture.application.port(),
    )?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Confirmed),
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    assert!(
        matches!(PresentationPuzzlePanelFixture::operation(&work)?, DesktopPuzzleServiceEnumOperation::Session { expected, operation: DesktopPuzzleBridgeEnumOperation::Command(PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed)) } if expected == &context)
    );
    Ok(())
}

#[tokio::test]
async fn real_inventory_uses_explicit_bounded_pages_and_selected_storage_revisions()
-> PresentationPuzzlePanelTestResult {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("panel.sqlite3"),
        CapacityBudget::new(NonZeroUsize::MIN)?,
    )
    .await?;
    let repository = StoragePuzzleRepository::new(store.client());
    for index in 1..=StoragePuzzleRepository::INVENTORY_PAGE_SIZE + 1 {
        let id = if index > StoragePuzzleRepository::INVENTORY_PAGE_SIZE {
            u64::MAX
        } else {
            u64::try_from(index)?
        };
        let mut run = PresentationPuzzlePanelFixture::run(id)?;
        let now = ClockInstant::from_nanos(0);
        let mut checkpoint = PuzzleCheckpoint::new(PuzzleCheckpointSnapshot::new(
            PuzzleCheckpointObservation::from_run(run.run(), now)?,
            run.capture(now, 0)?,
        ))?;
        let token = checkpoint.begin_write(now)?;
        let payload = checkpoint
            .pending()
            .ok_or("missing checkpoint")?
            .payload()
            .clone();
        repository
            .begin_save(PuzzlePersistenceSaveRequest::new(
                payload,
                PuzzlePersistenceStorageRevision::INITIAL,
                token,
            )?)?
            .wait()
            .await?;
    }
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.panel.first_page(&fixture.application.port())?;
    let work = fixture.next()?;
    assert!(matches!(
        PresentationPuzzlePanelFixture::operation(&work)?,
        DesktopPuzzleServiceEnumOperation::Inventory { after: None }
    ));
    let first = Arc::new(repository.inventory(None).await?);
    assert_eq!(
        first.entries().len(),
        StoragePuzzleRepository::INVENTORY_PAGE_SIZE
    );
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Inventory(
            Arc::clone(&first),
        ))),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    assert!(Arc::ptr_eq(
        fixture.panel.inventory().ok_or("missing first page")?,
        &first
    ));
    assert!(fixture.panel.controller().is_none());
    fixture.panel.next_page(&fixture.application.port())?;
    let work = fixture.next()?;
    let cursor = first.next_cursor().ok_or("missing cursor")?;
    assert!(
        matches!(PresentationPuzzlePanelFixture::operation(&work)?, DesktopPuzzleServiceEnumOperation::Inventory { after: Some(after) } if *after == cursor)
    );
    let second = Arc::new(repository.inventory(Some(cursor)).await?);
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Inventory(
            Arc::clone(&second),
        ))),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    assert!(Arc::ptr_eq(
        fixture.panel.inventory().ok_or("missing next page")?,
        &second
    ));
    assert_eq!(second.entries().len(), 1);
    assert_eq!(second.next_cursor(), None);
    assert!(
        fixture
            .panel
            .next_page(&fixture.application.port())
            .is_err()
    );
    let metadata = second.entries()[0];
    fixture.panel.open_saved(
        metadata.id(),
        metadata.storage_revision(),
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    assert!(
        matches!(PresentationPuzzlePanelFixture::operation(&work)?, DesktopPuzzleServiceEnumOperation::OpenSaved { id, storage_revision } if *id == PresentationPuzzlePanelFixture::id(u64::MAX)? && *storage_revision == metadata.storage_revision())
    );
    assert!(fixture.panel.board().is_none());
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn empty_inventory_and_missing_collection_disable_start_without_fabricating_a_run()
-> PresentationPuzzlePanelTestResult {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("empty.sqlite3"),
        CapacityBudget::new(NonZeroUsize::MIN)?,
    )
    .await?;
    let page = Arc::new(
        StoragePuzzleRepository::new(store.client())
            .inventory(None)
            .await?,
    );
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.panel.first_page(&fixture.application.port())?;
    let work = fixture.next()?;
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Inventory(page))),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    let mut errors = Vec::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 1600.0))
        .build_ui(|ui| {
            if let Err(error) = fixture.panel.show(ui, &fixture.application.port()) {
                errors.push(error);
            }
        });
    harness.run_ok().ok_or("saved-run setup did not settle")?;
    harness.get_by_label("No saved puzzle runs on this page");
    harness.get_by_label("No local puzzle collection is available");
    assert!(
        harness
            .get_by_label("Start Run")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        harness
            .get_by_label("Next Page")
            .accesskit_node()
            .is_disabled()
    );
    drop(harness);
    assert!(errors.is_empty());
    assert!(fixture.application.try_next()?.is_none());
    assert!(fixture.panel.controller().is_none());
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn missing_and_corrupt_open_errors_preserve_request_identity_until_explicit_retry()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.panel.open_saved(
        PresentationPuzzlePanelFixture::id(1)?,
        PuzzlePersistenceStorageRevision::new(9),
        &fixture.application.port(),
    )?;
    for message in [
        "The saved puzzle run is missing",
        "The saved puzzle repository rows are incomplete or inconsistent",
    ] {
        let work = fixture.next()?;
        let id = work.request().request_id();
        fixture.application.complete_failure(
            &work,
            DesktopFailure::new(
                DesktopEnumFailureCode::FileUnavailable,
                message.into(),
                true,
            ),
        )?;
        fixture.panel.synchronize(&fixture.application.port())?;
        let pending = fixture.panel.ordinary.as_ref().ok_or("lost failed open")?;
        assert_eq!(
            pending.receipt.as_ref().ok_or("lost receipt")?.request_id(),
            id
        );
        assert!(
            fixture
                .panel
                .failure()
                .ok_or("lost open failure")?
                .to_string()
                .contains(message)
        );
        assert!(fixture.panel.board().is_none());
        assert!(!fixture.panel.has_pending_work());
        fixture.panel.retry(&fixture.application.port())?;
    }
    Ok(())
}

#[tokio::test]
async fn session_summary_is_requested_explicitly_and_uses_only_the_service_result()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    assert!(fixture.panel.summary().is_none());
    fixture.panel.request_summary(&fixture.application.port())?;
    let work = fixture.next()?;
    let expected = fixture.owner.context();
    assert!(
        matches!(PresentationPuzzlePanelFixture::operation(&work)?, DesktopPuzzleServiceEnumOperation::Session { expected: context, operation: DesktopPuzzleBridgeEnumOperation::Summary } if *context == expected)
    );
    let summary = fixture.owner.summary(ClockInstant::from_nanos(0))?;
    let snapshot = Arc::new(fixture.owner.snapshot(ClockInstant::from_nanos(0))?);
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Session(
            DesktopPuzzleBridgeEnumResult::Summary { snapshot, summary },
        ))),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    assert_eq!(fixture.panel.summary(), Some(&summary));
    Ok(())
}

#[tokio::test]
async fn cancellation_preserves_the_actual_receipt_before_recovery()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let ordinary = fixture.next()?;
    fixture.panel.report_readiness(
        PuzzleTimerEnumReadiness::Hidden,
        &fixture.application.port(),
    )?;
    let urgent = fixture.next()?;
    assert!(ordinary.is_cancellation_requested());
    fixture.application.acknowledge_stopped(&ordinary)?;
    fixture.panel.synchronize(&fixture.application.port())?;
    let pending = fixture
        .panel
        .ordinary
        .as_ref()
        .ok_or("lost cancelled action")?;
    assert!(
        pending
            .receipt
            .as_ref()
            .ok_or("lost cancelled receipt")?
            .shares_outcome(ordinary.receipt())
    );
    assert!(pending.intent.is_some());
    assert!(
        fixture
            .panel
            .urgent
            .as_ref()
            .ok_or("lost urgent request")?
            .is_pending()
    );
    fixture.finish(urgent).await?;
    assert!(fixture.panel.board().is_none());
    Ok(())
}

#[tokio::test]
async fn closing_admits_snapshot_summary_retry_save_and_interrupt_recovery()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    fixture.fail_next.store(true, Ordering::Release);
    fixture.owner.execute(
        &fixture.owner.context(),
        PuzzlePersistenceEnumCommand::Hint,
        ClockInstant::from_nanos(0),
    )?;
    fixture.owner.wait().await?;
    assert!(fixture.owner.settle(ClockInstant::from_nanos(0)).is_err());
    let snapshot = fixture.owner.snapshot(ClockInstant::from_nanos(0))?;
    fixture.publish(snapshot, DesktopPuzzleServiceEnumActivity::Closing);
    fixture.panel.synchronize(&fixture.application.port())?;
    let quit = fixture
        .application
        .port()
        .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    fixture.application.port().try_submit(&quit)?;
    let _quit_work = fixture.next()?;
    fixture
        .panel
        .refresh_snapshot(&fixture.application.port())?;
    fixture.finish(fixture.next()?).await?;
    fixture.panel.request_summary(&fixture.application.port())?;
    fixture.finish(fixture.next()?).await?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::RetrySave,
        &fixture.application.port(),
    )?;
    let retry = fixture.next()?;
    assert!(matches!(
        PresentationPuzzlePanelFixture::operation(&retry)?,
        DesktopPuzzleServiceEnumOperation::Session {
            operation: DesktopPuzzleBridgeEnumOperation::RetrySave,
            ..
        }
    ));
    fixture.finish(retry).await?;
    fixture.panel.report_readiness(
        PuzzleTimerEnumReadiness::Hidden,
        &fixture.application.port(),
    )?;
    let interrupt = fixture.next()?;
    assert!(matches!(
        PresentationPuzzlePanelFixture::operation(&interrupt)?,
        DesktopPuzzleServiceEnumOperation::Interrupt { .. }
    ));
    fixture.finish(interrupt).await?;
    assert!(fixture.panel.board().is_none());
    Ok(())
}

#[tokio::test]
async fn ordinary_write_keeps_the_board_and_countdown_visible_until_the_checkpoint_gate_blocks()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    fixture.application.begin_commit(&work)?;
    fixture.owner.execute(
        &fixture.owner.context(),
        PuzzlePersistenceEnumCommand::Hint,
        ClockInstant::from_nanos(0),
    )?;
    let snapshot = fixture.owner.snapshot(ClockInstant::from_nanos(0))?;
    assert!(snapshot.has_retained_write());
    fixture.publish(snapshot, DesktopPuzzleServiceEnumActivity::Selected);
    fixture.panel.synchronize(&fixture.application.port())?;
    assert!(fixture.panel.board().is_some());
    assert_eq!(
        fixture
            .panel
            .controller()
            .ok_or("missing controller")?
            .snapshot()
            .status(),
        PuzzleRunEnumStatus::Active
    );
    Ok(())
}

#[tokio::test]
async fn global_busy_keeps_the_same_request_until_capacity_returns()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    for _ in 0..DesktopApplication::<
        PresentationPuzzlePanelTestClock,
        PresentationPuzzlePanelTestWake,
    >::ORDINARY_CAPACITY
    {
        let request = fixture
            .application
            .port()
            .request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        fixture.application.port().try_submit(&request)?;
    }
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let pending = fixture
        .panel
        .ordinary
        .as_ref()
        .ok_or("missing capacity waiter")?;
    let id = pending
        .request
        .as_ref()
        .ok_or("missing cached request")?
        .request_id();
    assert!(pending.receipt.is_none());
    fixture.panel.synchronize(&fixture.application.port())?;
    let released = fixture.next()?;
    fixture.application.complete_failure(
        &released,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "Controlled capacity release".into(),
            true,
        ),
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    let pending = fixture
        .panel
        .ordinary
        .as_ref()
        .ok_or("missing admitted waiter")?;
    assert_eq!(
        pending
            .request
            .as_ref()
            .ok_or("missing request")?
            .request_id(),
        id
    );
    assert_eq!(
        pending
            .receipt
            .as_ref()
            .ok_or("missing admitted receipt")?
            .request_id(),
        id
    );
    Ok(())
}

#[tokio::test]
async fn interrupt_uses_the_original_session_after_an_inflight_action_advances_its_context()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    let original = fixture.owner.context();
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Hint,
        &fixture.application.port(),
    )?;
    let ordinary = fixture.next()?;
    fixture.application.begin_commit(&ordinary)?;
    fixture.owner.execute(
        &original,
        PuzzlePersistenceEnumCommand::Hint,
        ClockInstant::from_nanos(0),
    )?;
    let advanced = Arc::new(fixture.owner.snapshot(ClockInstant::from_nanos(0))?);
    assert_ne!(advanced.context(), &original);
    fixture.panel.report_readiness(
        PuzzleTimerEnumReadiness::Hidden,
        &fixture.application.port(),
    )?;
    let urgent = fixture.next()?;
    assert!(
        matches!(PresentationPuzzlePanelFixture::operation(&urgent)?, DesktopPuzzleServiceEnumOperation::Interrupt { session, .. } if *session == original.session_id())
    );
    assert_eq!(
        fixture
            .panel
            .urgent
            .as_ref()
            .and_then(|pending| pending.intent.as_ref())
            .ok_or("lost original urgent intent")?
            .context(),
        &original
    );
    fixture.application.complete_value(
        &ordinary,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Session(
            DesktopPuzzleBridgeEnumResult::Snapshot(advanced),
        ))),
        None,
    )?;
    fixture.finish(urgent).await?;
    let controller = fixture.panel.controller().ok_or("missing controller")?;
    assert!(matches!(
        controller.snapshot().status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    assert!(!controller.is_pending());
    assert!(fixture.panel.board().is_none());
    Ok(())
}

#[tokio::test]
async fn state_disconnection_conceals_the_board_and_returns_a_typed_error()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    assert!(fixture.panel.board().is_some());
    drop(fixture.state);
    assert!(fixture.panel.board().is_none());
    assert!(matches!(
        fixture.panel.synchronize(&fixture.application.port()),
        Err(crate::presentation::puzzle::panel::PresentationPuzzlePanelError::Disconnected(_))
    ));
    Ok(())
}

#[tokio::test]
async fn matching_opened_receipt_preserves_a_newer_same_session_watch_snapshot()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.durable().await?;
    fixture.panel.open_saved(
        PresentationPuzzlePanelFixture::id(1)?,
        PuzzlePersistenceStorageRevision::new(1),
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    fixture.application.begin_commit(&work)?;
    let opened = Arc::new(fixture.owner.snapshot(ClockInstant::from_nanos(0))?);
    let opened_context = opened.context().clone();
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Opened(opened))),
        None,
    )?;
    fixture.owner.execute(
        &opened_context,
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    fixture.durable().await?;
    let latest = fixture.owner.snapshot(ClockInstant::from_nanos(0))?;
    let latest_context = latest.context().clone();
    assert!(latest_context.revision() > opened_context.revision());
    assert_eq!(latest.status(), PuzzleRunEnumStatus::Active);
    fixture.publish(latest, DesktopPuzzleServiceEnumActivity::Selected);
    assert!(fixture.panel.controller().is_none());
    fixture.panel.synchronize(&fixture.application.port())?;
    let controller = fixture
        .panel
        .controller()
        .ok_or("missing authorized controller")?;
    assert_eq!(controller.snapshot().context(), &latest_context);
    assert_eq!(controller.snapshot().status(), PuzzleRunEnumStatus::Active);
    assert!(!fixture.panel.has_pending_work());
    let retained = fixture
        .panel
        .last_ordinary
        .as_ref()
        .ok_or("lost completed open")?;
    assert!(
        retained
            .receipt
            .as_ref()
            .ok_or("lost actual open receipt")?
            .shares_outcome(work.receipt())
    );
    assert_eq!(
        retained
            .request
            .as_ref()
            .ok_or("lost actual open request")?
            .request_id(),
        work.request().request_id()
    );
    fixture.panel.synchronize(&fixture.application.port())?;
    assert_eq!(
        fixture
            .panel
            .controller()
            .ok_or("lost latest controller")?
            .snapshot()
            .context(),
        &latest_context
    );
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[tokio::test]
async fn terminal_failed_open_restores_the_existing_selected_active_run()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    let original = fixture
        .panel
        .controller()
        .ok_or("missing original controller")?
        .snapshot()
        .context()
        .clone();
    assert!(fixture.panel.board().is_some());
    let replacement = PresentationPuzzlePanelFixture::id(2)?;
    let revision = PuzzlePersistenceStorageRevision::new(7);
    fixture.panel.submit(
        DesktopPuzzleServiceEnumOperation::OpenSaved {
            id: replacement,
            storage_revision: revision,
        },
        None,
        &fixture.application.port(),
    )?;
    assert!(fixture.panel.is_opening());
    assert!(fixture.panel.board().is_none());
    let work = fixture.next()?;
    assert!(
        matches!(PresentationPuzzlePanelFixture::operation(&work)?, DesktopPuzzleServiceEnumOperation::OpenSaved { id, storage_revision } if *id == replacement && *storage_revision == revision)
    );
    let refusal = crate::desktop::puzzle_service::DesktopPuzzleServiceError::ReplacementBlocked;
    fixture.application.complete_failure(
        &work,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            refusal.to_string(),
            true,
        ),
    )?;
    assert_eq!(
        fixture.state.borrow().activity(),
        DesktopPuzzleServiceEnumActivity::Selected
    );
    assert_eq!(
        fixture.state.borrow().selected(),
        Some(original.session_id())
    );
    fixture.panel.synchronize(&fixture.application.port())?;
    assert!(!fixture.panel.has_pending_work());
    assert!(!fixture.panel.is_opening());
    assert!(fixture.panel.board().is_some());
    let controller = fixture
        .panel
        .controller()
        .ok_or("lost original controller")?;
    assert_eq!(controller.snapshot().context(), &original);
    assert_eq!(controller.snapshot().status(), PuzzleRunEnumStatus::Active);
    assert!(controller.can_request(PresentationPuzzleControllerEnumAction::Hint));
    assert!(fixture.panel.failure().is_some());
    let retained = fixture
        .panel
        .ordinary
        .as_ref()
        .ok_or("lost failed replacement")?;
    assert!(
        retained
            .receipt
            .as_ref()
            .ok_or("lost failed replacement receipt")?
            .shares_outcome(work.receipt())
    );
    assert_eq!(
        retained
            .request
            .as_ref()
            .ok_or("lost failed replacement request")?
            .request_id(),
        work.request().request_id()
    );
    assert!(fixture.panel.retry(&fixture.application.port()).is_err());
    assert!(fixture.panel.board().is_some());
    assert!(fixture.application.try_next()?.is_none());
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Pause(PuzzleTimerEnumPauseReason::Manual),
        &fixture.application.port(),
    )?;
    fixture.finish(fixture.next()?).await?;
    assert!(matches!(
        fixture
            .panel
            .controller()
            .ok_or("lost paused controller")?
            .snapshot()
            .status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    fixture.panel.retry(&fixture.application.port())?;
    let retry = fixture.next()?;
    assert_ne!(retry.request().request_id(), work.request().request_id());
    assert!(
        matches!(PresentationPuzzlePanelFixture::operation(&retry)?, DesktopPuzzleServiceEnumOperation::OpenSaved { id, storage_revision } if *id == replacement && *storage_revision == revision)
    );
    assert!(
        fixture
            .panel
            .last_ordinary
            .as_ref()
            .and_then(|pending| pending.receipt.as_ref())
            .ok_or("lost original failure during retry")?
            .shares_outcome(work.receipt())
    );
    assert!(fixture.panel.board().is_none());
    Ok(())
}

#[tokio::test]
async fn active_run_open_refuses_before_admission_or_concealing_the_running_countdown()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.open().await?;
    fixture.ready().await?;
    let original = fixture
        .panel
        .controller()
        .ok_or("missing active controller")?
        .snapshot()
        .context()
        .clone();
    assert_eq!(
        fixture
            .panel
            .controller()
            .ok_or("missing active controller")?
            .snapshot()
            .status(),
        PuzzleRunEnumStatus::Active
    );
    assert!(fixture.panel.board().is_some());
    let result = fixture.panel.open_saved(
        PresentationPuzzlePanelFixture::id(2)?,
        PuzzlePersistenceStorageRevision::new(1),
        &fixture.application.port(),
    );
    assert!(result.is_err());
    assert!(fixture.application.try_next()?.is_none());
    assert!(!fixture.panel.has_pending_work());
    assert!(!fixture.panel.is_opening());
    assert!(fixture.panel.board().is_some());
    let controller = fixture.panel.controller().ok_or("lost active controller")?;
    assert_eq!(controller.snapshot().context(), &original);
    assert_eq!(controller.snapshot().status(), PuzzleRunEnumStatus::Active);
    assert!(controller.can_request(PresentationPuzzleControllerEnumAction::Hint));
    fixture.panel.request_action(
        PresentationPuzzleControllerEnumAction::Pause(PuzzleTimerEnumPauseReason::Manual),
        &fixture.application.port(),
    )?;
    fixture.finish(fixture.next()?).await?;
    fixture.panel.open_saved(
        PresentationPuzzlePanelFixture::id(2)?,
        PuzzlePersistenceStorageRevision::new(1),
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    assert!(
        matches!(PresentationPuzzlePanelFixture::operation(&work)?, DesktopPuzzleServiceEnumOperation::OpenSaved { id, .. } if *id == PresentationPuzzlePanelFixture::id(2)?)
    );
    Ok(())
}

#[tokio::test]
async fn newer_watch_requires_a_matching_opened_receipt_before_installing_a_controller()
-> PresentationPuzzlePanelTestResult {
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.durable().await?;
    fixture.panel.open_saved(
        PresentationPuzzlePanelFixture::id(1)?,
        PuzzlePersistenceStorageRevision::new(1),
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    let context = fixture.owner.context();
    fixture.owner.execute(
        &context,
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    fixture.durable().await?;
    let newer = fixture.owner.snapshot(ClockInstant::from_nanos(0))?;
    fixture.publish(newer, DesktopPuzzleServiceEnumActivity::Selected);
    fixture.panel.synchronize(&fixture.application.port())?;
    assert!(fixture.panel.controller().is_none());
    assert!(fixture.panel.board().is_none());
    assert!(fixture.panel.has_pending_work());
    let foreign =
        Arc::new(PresentationPuzzlePanelFixture::owner(2)?.snapshot(ClockInstant::from_nanos(0))?);
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Opened(foreign))),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    assert!(fixture.panel.controller().is_none());
    assert!(fixture.panel.board().is_none());
    assert!(fixture.panel.failure().is_some());
    assert!(!fixture.panel.has_pending_work());
    let retained = fixture
        .panel
        .ordinary
        .as_ref()
        .ok_or("lost foreign-result request")?;
    assert!(
        retained
            .receipt
            .as_ref()
            .ok_or("lost foreign-result receipt")?
            .shares_outcome(work.receipt())
    );
    assert_eq!(
        retained
            .request
            .as_ref()
            .ok_or("lost original request")?
            .request_id(),
        work.request().request_id()
    );
    Ok(())
}

#[tokio::test]
async fn history_deletion_dispatches_only_after_explicit_confirmation()
-> PresentationPuzzlePanelTestResult {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("delete.sqlite3"),
        CapacityBudget::new(NonZeroUsize::MIN)?,
    )
    .await?;
    let page = Arc::new(
        StoragePuzzleRepository::new(store.client())
            .inventory(None)
            .await?,
    );
    let mut fixture = PresentationPuzzlePanelFixture::new()?;
    fixture.panel.first_page(&fixture.application.port())?;
    let work = fixture.next()?;
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Inventory(page))),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    {
        let mut errors = Vec::new();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1100.0, 1600.0))
            .build_ui(|ui| {
                if let Err(error) = fixture.panel.show(ui, &fixture.application.port()) {
                    errors.push(error);
                }
            });
        harness.run_ok().ok_or("panel did not settle")?;
        harness.get_by_label("Delete All History").click();
        harness.run_ok().ok_or("confirmation did not settle")?;
        harness.get_by_label("Delete puzzle history?");
        harness.get_by_label("Keep History");
        drop(harness);
        assert!(errors.is_empty());
    }
    assert_eq!(
        fixture.panel.pending_deletion(),
        Some(PuzzleHistoryEnumScope::All)
    );
    assert!(fixture.application.try_next()?.is_none());
    fixture.panel.request_history_deletion(
        PuzzleHistoryEnumScope::All,
        PuzzleHistoryEnumConfirmation::Confirmed,
        &fixture.application.port(),
    )?;
    let work = fixture.next()?;
    assert!(matches!(
        PresentationPuzzlePanelFixture::operation(&work)?,
        DesktopPuzzleServiceEnumOperation::DeleteHistory {
            scope: PuzzleHistoryEnumScope::All,
            confirmation: PuzzleHistoryEnumConfirmation::Confirmed
        }
    ));
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::HistoryDeleted {
            removed: 0,
        })),
        None,
    )?;
    fixture.panel.synchronize(&fixture.application.port())?;
    assert!(fixture.panel.inventory().is_none());
    assert_eq!(fixture.panel.pending_deletion(), None);
    store.close().await?;
    Ok(())
}
