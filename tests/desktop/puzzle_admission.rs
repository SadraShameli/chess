use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::board::position::BoardPosition;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationError, DesktopApplicationReceipt,
    DesktopApplicationWakeError, DesktopApplicationWakeProvider, DesktopApplicationWork,
};
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::identity::DesktopRevision;
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
    DesktopRequest,
};
use crate::desktop::puzzle_bridge::{
    DesktopPuzzleBridgeEnumOperation, DesktopPuzzleBridgeEnumResult,
};
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumOperation, DesktopPuzzleServiceEnumResult,
};
use crate::desktop::puzzle_snapshot::{DesktopPuzzleSnapshot, DesktopPuzzleSnapshotSave};
use crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus;
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::PuzzleEnumCountdownMode;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::PuzzleDefinitionMetadata;
use crate::puzzle::persistence::{
    PuzzlePersistenceEnumCommand, PuzzlePersistenceRun, PuzzlePersistenceStorageRevision,
};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::run::PuzzleRunEnumConfirmation;
use crate::puzzle::selection::PuzzleSelectionFilter;
use crate::puzzle::session::{
    PuzzleSessionConfig, PuzzleSessionContext, PuzzleSessionId, PuzzleSessionRevision,
};
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};

type DesktopPuzzleAdmissionTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct DesktopPuzzleAdmissionFixtureClock(AtomicU64);

impl ClockTimeProvider for DesktopPuzzleAdmissionFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}

struct DesktopPuzzleAdmissionFixtureWake;

impl DesktopApplicationWakeProvider for DesktopPuzzleAdmissionFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

struct DesktopPuzzleAdmissionFixture {
    application:
        DesktopApplication<DesktopPuzzleAdmissionFixtureClock, DesktopPuzzleAdmissionFixtureWake>,
    clock: Arc<DesktopPuzzleAdmissionFixtureClock>,
    owner: DesktopMessagesEntityId,
    run: PuzzlePersistenceRun,
}

impl DesktopPuzzleAdmissionFixture {
    fn identity() -> DesktopPuzzleAdmissionTestResult<PuzzleDefinitionIdentity> {
        Ok(PuzzleDefinitionIdentity::new(
            "admission".into(),
            "v1".into(),
            "first".into(),
        )?)
    }

    fn session_id(value: u64) -> DesktopPuzzleAdmissionTestResult<PuzzleSessionId> {
        Ok(PuzzleSessionId::new(
            NonZeroU64::new(value).ok_or("zero fixture session")?,
        ))
    }

    fn run(session: u64) -> DesktopPuzzleAdmissionTestResult<PuzzlePersistenceRun> {
        let initial = BoardPosition::standard();
        let movement = initial.parse_uci("e2e4")?;
        let definition = PuzzleDefinition::new(
            Self::identity()?,
            initial,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default(),
        )?;
        Ok(PuzzlePersistenceRun::new(
            Self::session_id(session)?,
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![definition],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(81),
        )?)
    }

    fn new() -> DesktopPuzzleAdmissionTestResult<Self> {
        let run = Self::run(1)?;
        let clock = Arc::new(DesktopPuzzleAdmissionFixtureClock::default());
        Ok(Self {
            application: DesktopApplication::new(
                Arc::clone(&clock),
                Arc::new(DesktopPuzzleAdmissionFixtureWake),
            ),
            clock,
            owner: DesktopMessagesEntityId::new(),
            run,
        })
    }

    fn context(&self) -> PuzzleSessionContext {
        self.run.run().context()
    }

    fn operation(
        &self,
        operation: DesktopPuzzleBridgeEnumOperation,
    ) -> DesktopPuzzleServiceEnumOperation {
        DesktopPuzzleServiceEnumOperation::Session {
            expected: self.context(),
            operation,
        }
    }

    fn request(
        &self,
        operation: DesktopPuzzleServiceEnumOperation,
    ) -> DesktopPuzzleAdmissionTestResult<DesktopRequest> {
        Ok(self
            .application
            .port()
            .request(DesktopCommand::puzzle(self.owner, operation))?)
    }

    fn submit(
        &self,
        operation: DesktopPuzzleServiceEnumOperation,
    ) -> DesktopPuzzleAdmissionTestResult<DesktopApplicationReceipt> {
        Ok(self
            .application
            .port()
            .try_submit(&self.request(operation)?)?)
    }

    fn interrupt(
        &self,
        session: u64,
        reason: PuzzleTimerEnumPauseReason,
    ) -> DesktopPuzzleAdmissionTestResult<DesktopPuzzleServiceEnumOperation> {
        Ok(DesktopPuzzleServiceEnumOperation::Interrupt {
            session: Self::session_id(session)?,
            reason,
        })
    }

    fn next(&self) -> DesktopPuzzleAdmissionTestResult<DesktopApplicationWork> {
        Ok(self
            .application
            .try_next()?
            .ok_or("missing admitted puzzle work")?)
    }

    fn snapshot(&self) -> DesktopPuzzleAdmissionTestResult<Arc<DesktopPuzzleSnapshot>> {
        Ok(Arc::new(DesktopPuzzleSnapshot::capture(
            &self.run,
            self.clock.now()?,
            DesktopPuzzleSnapshotSave {
                status: PuzzleCheckpointEnumStatus::Open,
                durable: None,
                retained_write: false,
                unsaved_state: true,
            },
            None,
        )?))
    }

    fn result(&self) -> DesktopPuzzleAdmissionTestResult<DesktopPuzzleServiceEnumResult> {
        Ok(DesktopPuzzleServiceEnumResult::Session(
            DesktopPuzzleBridgeEnumResult::Snapshot(self.snapshot()?),
        ))
    }

    fn complete(&self, work: &DesktopApplicationWork) -> DesktopPuzzleAdmissionTestResult {
        if work.request().command().operation().is_control() {
            self.application
                .complete_puzzle_control(work, Ok(self.result()?))?;
        } else {
            if matches!(
                work.request().command().operation(),
                DesktopEnumCommand::Puzzle {
                    operation: DesktopPuzzleServiceEnumOperation::Inventory { .. },
                    ..
                }
            ) {
                self.application.complete_failure(work, Self::failure())?;
                return Ok(());
            }
            if work
                .request()
                .command()
                .operation()
                .mutation_entity()
                .is_some()
            {
                self.application.begin_commit(work)?;
            }
            let value = if matches!(
                work.request().command().operation(),
                DesktopEnumCommand::Puzzle {
                    operation: DesktopPuzzleServiceEnumOperation::OpenSaved { .. },
                    ..
                }
            ) {
                DesktopPuzzleServiceEnumResult::Opened(self.snapshot()?)
            } else {
                self.result()?
            };
            self.application.complete_value(
                work,
                DesktopEnumValue::Puzzle(Box::new(value)),
                Some(DesktopRevision::for_puzzle(self.context().revision())),
            )?;
        }
        Ok(())
    }

    fn failure() -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "controlled save refusal".into(),
            true,
        )
    }
}

#[test]
fn puzzle_protocol_preserves_typed_revisions_and_exact_command_payloads()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let storage_revision = PuzzlePersistenceStorageRevision::new(9_007_199_254_740_993);
    let operation = DesktopPuzzleServiceEnumOperation::OpenSaved {
        id: DesktopPuzzleAdmissionFixture::session_id(2)?,
        storage_revision,
    };
    let command = DesktopCommand::puzzle(fixture.owner, operation.clone());
    assert_eq!(
        command.expected_revision(),
        Some(DesktopRevision::for_puzzle_storage(storage_revision))
    );
    assert_eq!(
        command.operation(),
        &DesktopEnumCommand::Puzzle {
            owner: fixture.owner,
            operation
        }
    );
    let session = PuzzleSessionContext::new(
        fixture.context().session_id(),
        crate::puzzle::attempt::PuzzleAttemptId::new(NonZeroU64::MIN),
        DesktopPuzzleAdmissionFixture::identity()?,
        PuzzleSessionRevision::new(u64::MAX),
    );
    let command = DesktopCommand::puzzle(
        fixture.owner,
        DesktopPuzzleServiceEnumOperation::Session {
            expected: session.clone(),
            operation: DesktopPuzzleBridgeEnumOperation::Summary,
        },
    );
    assert_eq!(
        command.expected_revision(),
        Some(DesktopRevision::for_puzzle(session.revision()))
    );
    for operation in [
        DesktopPuzzleServiceEnumOperation::Inventory { after: None },
        fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?,
    ] {
        assert_eq!(
            DesktopCommand::puzzle(fixture.owner, operation).expected_revision(),
            Some(DesktopRevision::ZERO)
        );
    }
    Ok(())
}

#[test]
fn puzzle_mutations_require_commit_and_release_the_workspace_owner()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    for operation in [
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Command(
            PuzzlePersistenceEnumCommand::Hint,
        )),
        DesktopPuzzleServiceEnumOperation::OpenSaved {
            id: fixture.context().session_id(),
            storage_revision: PuzzlePersistenceStorageRevision::INITIAL,
        },
    ] {
        let receipt = fixture.submit(operation.clone())?;
        assert!(matches!(
            fixture
                .application
                .port()
                .try_submit(&fixture.request(operation.clone())?),
            Err(DesktopApplicationError::EntityBusy)
        ));
        let work = fixture.next()?;
        assert!(matches!(
            fixture.application.complete_value(
                &work,
                DesktopEnumValue::Puzzle(Box::new(fixture.result()?)),
                None
            ),
            Err(DesktopApplicationError::CommitNotStarted)
        ));
        fixture.complete(&work)?;
        assert!(receipt.response().is_some());
        fixture.submit(operation)?;
        fixture.complete(&fixture.next()?)?;
    }
    Ok(())
}

#[test]
fn puzzle_inventory_snapshot_and_summary_do_not_claim_mutation_ownership()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    for operation in [
        DesktopPuzzleServiceEnumOperation::Inventory { after: None },
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Snapshot),
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Summary),
    ] {
        let command = DesktopCommand::puzzle(fixture.owner, operation.clone());
        assert_eq!(command.operation().mutation_entity(), None);
        fixture.submit(operation)?;
    }
    fixture.submit(fixture.operation(DesktopPuzzleBridgeEnumOperation::RetrySave))?;
    while let Some(work) = fixture.application.try_next()? {
        fixture.complete(&work)?;
    }
    Ok(())
}

#[test]
fn full_ordinary_admission_preserves_independent_pause_hidden_and_end_controls()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let port = fixture.application.port();
    for _ in 0..DesktopApplication::<
        DesktopPuzzleAdmissionFixtureClock,
        DesktopPuzzleAdmissionFixtureWake,
    >::ORDINARY_CAPACITY
    {
        let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        port.try_submit(&request)?;
    }
    let pause = fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?;
    let first = fixture.submit(pause.clone())?;
    fixture.clock.0.store(1_000_000_000, Ordering::SeqCst);
    assert!(first.shares_outcome(&fixture.submit(pause)?));
    assert_eq!(
        first.deadline(),
        Some(ClockInstant::from_nanos(2_000_000_000))
    );
    fixture.submit(fixture.interrupt(1, PuzzleTimerEnumPauseReason::FocusLost)?)?;
    let end = fixture.operation(DesktopPuzzleBridgeEnumOperation::Command(
        PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed),
    ));
    fixture.submit(end.clone())?;
    for expected in [None, None, Some(end)] {
        let work = fixture.next()?;
        assert!(work.request().command().operation().is_control());
        if let Some(expected) = expected {
            assert_eq!(
                work.request().command().operation(),
                &DesktopEnumCommand::Puzzle {
                    owner: fixture.owner,
                    operation: expected
                }
            );
        }
        fixture.complete(&work)?;
    }
    while let Some(work) = fixture.application.try_next()? {
        fixture
            .application
            .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    }
    Ok(())
}

#[test]
fn matching_session_interrupt_cancels_only_its_mutation_and_observes_completion()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let original =
        fixture.submit(fixture.operation(DesktopPuzzleBridgeEnumOperation::RetrySave))?;
    let work = fixture.next()?;
    let control_receipt =
        fixture.submit(fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?)?;
    let control = fixture.next()?;
    assert!(work.is_cancellation_requested());
    assert!(matches!(
        fixture.application.begin_commit(&work),
        Err(DesktopApplicationError::CancellationRequested)
    ));
    assert!(matches!(
        fixture
            .application
            .complete_puzzle_control(&control, Ok(fixture.result()?)),
        Err(DesktopApplicationError::CancellationUnacknowledged)
    ));
    fixture.application.acknowledge_stopped(&work)?;
    fixture.complete(&control)?;
    assert!(matches!(
        original.response(),
        Some(DesktopMessagesEnumResult::Cancelled)
    ));
    assert!(
        matches!(control_receipt.response(), Some(DesktopMessagesEnumResult::Value { value: DesktopEnumValue::Puzzle(_), revision: Some(revision) }) if *revision == DesktopRevision::for_puzzle(fixture.context().revision()))
    );
    Ok(())
}

#[test]
fn old_session_controls_leave_replacement_and_new_session_mutations_uncancelled()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let replacement = DesktopPuzzleServiceEnumOperation::OpenSaved {
        id: DesktopPuzzleAdmissionFixture::session_id(2)?,
        storage_revision: PuzzlePersistenceStorageRevision::INITIAL,
    };
    fixture.submit(replacement)?;
    let replacement_work = fixture.next()?;
    fixture.submit(fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?)?;
    let old_control = fixture.next()?;
    assert!(!replacement_work.is_cancellation_requested());
    fixture
        .application
        .complete_failure(&replacement_work, DesktopPuzzleAdmissionFixture::failure())?;
    let next_context = DesktopPuzzleAdmissionFixture::run(2)?.run().context();
    fixture.submit(DesktopPuzzleServiceEnumOperation::Session {
        expected: next_context,
        operation: DesktopPuzzleBridgeEnumOperation::RetrySave,
    })?;
    let next_work = fixture.next()?;
    fixture.submit(fixture.interrupt(1, PuzzleTimerEnumPauseReason::FocusLost)?)?;
    let hidden_control = fixture.next()?;
    assert!(!next_work.is_cancellation_requested());
    fixture.complete(&old_control)?;
    fixture.complete(&hidden_control)?;
    fixture
        .application
        .complete_failure(&next_work, DesktopPuzzleAdmissionFixture::failure())?;
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn closing_admits_only_canonical_puzzle_recovery_and_keeps_original_quit_deadline()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let port = fixture.application.port();
    let quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    let quit_receipt = port.try_submit(&quit)?;
    let quit_work = fixture.next()?;
    fixture
        .application
        .report_pending_control(&quit_work, DesktopPuzzleAdmissionFixture::failure())?;
    fixture.clock.0.store(1_000_000_000, Ordering::SeqCst);
    let allowed = [
        fixture.operation(DesktopPuzzleBridgeEnumOperation::RetrySave),
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Snapshot),
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Summary),
        fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?,
        fixture.interrupt(1, PuzzleTimerEnumPauseReason::FocusLost)?,
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Command(
            PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::Manual),
        )),
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Command(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Hidden),
        )),
    ];
    for operation in allowed {
        assert!(operation.is_available_while_closing());
        fixture.submit(operation)?;
        fixture.complete(&fixture.next()?)?;
    }
    for operation in [
        DesktopPuzzleServiceEnumOperation::Inventory { after: None },
        DesktopPuzzleServiceEnumOperation::OpenSaved {
            id: DesktopPuzzleAdmissionFixture::session_id(2)?,
            storage_revision: PuzzlePersistenceStorageRevision::INITIAL,
        },
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Command(
            PuzzlePersistenceEnumCommand::Hint,
        )),
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Command(
            PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed),
        )),
    ] {
        assert!(!operation.is_available_while_closing());
        assert!(matches!(
            port.try_submit(&fixture.request(operation)?),
            Err(DesktopApplicationError::Closing)
        ));
    }
    port.retry_quit()?;
    let repeated = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    assert!(quit_receipt.shares_outcome(&port.try_submit(&repeated)?));
    assert_eq!(
        quit_receipt.deadline(),
        Some(ClockInstant::from_nanos(2_000_000_000))
    );
    fixture.application.complete_control(&quit_work)?;
    assert!(matches!(
        quit_receipt.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    Ok(())
}

#[test]
fn unknown_session_puzzle_controls_are_finite_and_duplicate_receipts_survive_saturation()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let capacity = DesktopApplication::<
        DesktopPuzzleAdmissionFixtureClock,
        DesktopPuzzleAdmissionFixtureWake,
    >::CONTROL_CAPACITY;
    let first_operation = fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?;
    let first = fixture.submit(first_operation.clone())?;
    for identity in 2..=u64::try_from(capacity)? {
        fixture.submit(fixture.interrupt(identity, PuzzleTimerEnumPauseReason::Manual)?)?;
    }
    assert!(first.shares_outcome(&fixture.submit(first_operation)?));
    let retry =
        fixture.request(fixture.interrupt(u64::MAX, PuzzleTimerEnumPauseReason::Manual)?)?;
    assert!(matches!(
        fixture.application.port().try_submit(&retry),
        Err(DesktopApplicationError::Busy)
    ));
    fixture.complete(&fixture.next()?)?;
    fixture.application.port().try_submit(&retry)?;
    while let Some(work) = fixture.application.try_next()? {
        fixture
            .application
            .complete_puzzle_control(&work, Err(DesktopPuzzleAdmissionFixture::failure()))?;
    }
    assert!(first.response().is_some());
    Ok(())
}

#[test]
fn queued_puzzle_mutation_is_cancelled_before_dispatch_and_keeps_its_receipt()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let port = fixture.application.port();
    let request =
        fixture.request(fixture.operation(DesktopPuzzleBridgeEnumOperation::RetrySave))?;
    let receipt = port.try_submit(&request)?;
    fixture.submit(fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?)?;
    let control = fixture.next()?;
    assert!(control.request().command().operation().is_control());
    fixture.complete(&control)?;
    assert!(matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::Cancelled)
    ));
    assert!(receipt.shares_outcome(&port.try_submit(&request)?));
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn puzzle_control_failure_waits_for_the_actual_committing_mutation_outcome()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let original =
        fixture.submit(fixture.operation(DesktopPuzzleBridgeEnumOperation::RetrySave))?;
    let work = fixture.next()?;
    fixture.application.begin_commit(&work)?;
    let control_receipt =
        fixture.submit(fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?)?;
    let control = fixture.next()?;
    assert!(matches!(
        fixture.application.acknowledge_stopped(&work),
        Err(DesktopApplicationError::CommitUnresolved)
    ));
    assert!(matches!(
        fixture
            .application
            .complete_puzzle_control(&control, Err(DesktopPuzzleAdmissionFixture::failure())),
        Err(DesktopApplicationError::CancellationUnacknowledged)
    ));
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Puzzle(Box::new(fixture.result()?)),
        Some(DesktopRevision::for_puzzle(fixture.context().revision())),
    )?;
    fixture
        .application
        .complete_puzzle_control(&control, Err(DesktopPuzzleAdmissionFixture::failure()))?;
    assert!(matches!(
        original.response(),
        Some(DesktopMessagesEnumResult::Value { .. })
    ));
    assert!(
        matches!(control_receipt.response(), Some(DesktopMessagesEnumResult::Failure(failure)) if failure.message() == DesktopPuzzleAdmissionFixture::failure().message())
    );
    Ok(())
}

#[test]
fn typed_puzzle_end_completion_retains_storage_revision_and_rejects_other_control_families()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let receipt = fixture.submit(fixture.operation(DesktopPuzzleBridgeEnumOperation::Command(
        PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed),
    )))?;
    let work = fixture.next()?;
    let storage_revision = PuzzlePersistenceStorageRevision::new(9_007_199_254_740_993);
    fixture.application.complete_puzzle_control(
        &work,
        Ok(DesktopPuzzleServiceEnumResult::Ended {
            id: fixture.context().session_id(),
            storage_revision,
            summary: fixture.run.run().summary(fixture.clock.now()?)?,
        }),
    )?;
    assert!(
        matches!(receipt.response(), Some(DesktopMessagesEnumResult::Value { value: DesktopEnumValue::Puzzle(value), revision: Some(revision) }) if matches!(value.as_ref(), DesktopPuzzleServiceEnumResult::Ended { storage_revision: actual, .. } if *actual == storage_revision) && *revision == DesktopRevision::for_puzzle_storage(storage_revision))
    );
    let port = fixture.application.port();
    let quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    port.try_submit(&quit)?;
    let quit_work = fixture.next()?;
    assert!(matches!(
        fixture
            .application
            .complete_puzzle_control(&quit_work, Err(DesktopPuzzleAdmissionFixture::failure())),
        Err(DesktopApplicationError::InvalidTransition)
    ));
    fixture.application.complete_control(&quit_work)?;
    Ok(())
}

#[test]
fn review_idle_control_saturation_preserves_tracked_mutation_safety_controls()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let port = fixture.application.port();
    let mutation_receipt =
        fixture.submit(fixture.operation(DesktopPuzzleBridgeEnumOperation::RetrySave))?;
    let mutation = fixture.next()?;
    let mut idle_receipts = Vec::new();
    for identity in 1
        ..=u128::try_from(
            DesktopApplication::<
                DesktopPuzzleAdmissionFixtureClock,
                DesktopPuzzleAdmissionFixtureWake,
            >::CONTROL_CAPACITY,
        )?
    {
        let operation = crate::desktop::game_bridge::DesktopGameBridgeEnumOperation::Session {
            game: uuid::Uuid::from_u128(identity).to_string().parse()?,
            revision: crate::game::enums::GameRevision::ZERO,
            action: crate::desktop::game_commands::DesktopGameCommandsEnumAction::Stop,
        };
        let revision = operation.revision();
        let request = port.request(DesktopCommand::new(
            DesktopEnumCommand::Game(operation),
            Some(revision),
        ))?;
        idle_receipts.push(port.try_submit(&request)?);
    }
    fixture.clock.0.store(1_000_000_000, Ordering::SeqCst);
    let stop = port.request(DesktopCommand::new(
        DesktopEnumCommand::Stop {
            owner: fixture.owner,
        },
        None,
    ))?;
    let stop_receipt = port.try_submit(&stop);
    let repeated_stop = port.request(DesktopCommand::new(
        DesktopEnumCommand::Stop {
            owner: fixture.owner,
        },
        None,
    ))?;
    let repeated_stop_receipt = port.try_submit(&repeated_stop);
    let pause_receipt = port
        .try_submit(&fixture.request(fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?)?);
    let hidden_receipt = port.try_submit(
        &fixture.request(fixture.interrupt(1, PuzzleTimerEnumPauseReason::FocusLost)?)?,
    );
    let cancellation_was_requested = mutation.is_cancellation_requested();
    if cancellation_was_requested {
        fixture.application.acknowledge_stopped(&mutation)?;
    } else {
        fixture.complete(&mutation)?;
    }
    let mut completed_controls = 0;
    while let Some(work) = fixture.application.try_next()? {
        if matches!(
            work.request().command().operation(),
            DesktopEnumCommand::Puzzle { .. }
        ) {
            fixture.complete(&work)?;
        } else {
            fixture.application.complete_control(&work)?;
        }
        completed_controls += 1;
    }
    assert!(
        idle_receipts
            .iter()
            .all(|receipt| matches!(receipt.response(), Some(DesktopMessagesEnumResult::Stopped)))
    );
    assert!(fixture.application.try_next()?.is_none());
    assert!(
        stop_receipt.is_ok() && pause_receipt.is_ok() && hidden_receipt.is_ok(),
        "idle Game Stop saturation refused the tracked mutation's Generic Stop or puzzle Pause/Hidden"
    );
    assert!(cancellation_was_requested);
    assert!(matches!(
        mutation_receipt.response(),
        Some(DesktopMessagesEnumResult::Cancelled)
    ));
    let stop_receipt = stop_receipt?;
    let repeated_stop_receipt = repeated_stop_receipt?;
    assert!(stop_receipt.shares_outcome(&repeated_stop_receipt));
    assert_eq!(
        stop_receipt.deadline(),
        Some(ClockInstant::from_nanos(3_000_000_000))
    );
    assert!(stop_receipt.response().is_some());
    assert!(pause_receipt?.response().is_some());
    assert!(hidden_receipt?.response().is_some());
    assert_eq!(completed_controls, idle_receipts.len() + 3);
    Ok(())
}

#[test]
fn review_quit_waits_for_recovery_admitted_after_its_dispatch() -> DesktopPuzzleAdmissionTestResult
{
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let port = fixture.application.port();
    let quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    let quit_receipt = port.try_submit(&quit)?;
    let quit_work = fixture.next()?;
    fixture.clock.0.store(1_000_000_000, Ordering::SeqCst);
    let recovery_request =
        fixture.request(fixture.operation(DesktopPuzzleBridgeEnumOperation::RetrySave))?;
    let recovery_receipt = port.try_submit(&recovery_request)?;
    let premature_completion = fixture.application.complete_control(&quit_work);
    let quit_remained_pending = quit_receipt.response().is_none();
    let recovery = fixture.next()?;
    fixture.complete(&recovery)?;
    let completion_after_recovery = fixture.application.complete_control(&quit_work);
    let repeated_quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    let replayed_quit = port.try_submit(&repeated_quit)?;
    assert!(recovery_receipt.response().is_some());
    assert!(fixture.application.try_next()?.is_none());
    assert!(
        matches!(
            premature_completion,
            Err(DesktopApplicationError::CancellationUnacknowledged)
        ),
        "Quit completed while a later admitted RetrySave remained unresolved"
    );
    assert!(quit_remained_pending);
    completion_after_recovery?;
    assert!(quit_receipt.shares_outcome(&replayed_quit));
    assert_eq!(
        quit_receipt.deadline(),
        Some(ClockInstant::from_nanos(2_000_000_000))
    );
    assert!(matches!(
        quit_receipt.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    assert!(recovery_receipt.shares_outcome(&port.try_submit(&recovery_request)?));
    Ok(())
}

#[test]
fn review_terminal_quit_refuses_fresh_recovery_but_retains_original_receipts()
-> DesktopPuzzleAdmissionTestResult {
    let fixture = DesktopPuzzleAdmissionFixture::new()?;
    let port = fixture.application.port();
    let original_recovery =
        fixture.request(fixture.operation(DesktopPuzzleBridgeEnumOperation::RetrySave))?;
    let original_recovery_receipt = port.try_submit(&original_recovery)?;
    fixture.complete(&fixture.next()?)?;
    let quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    let quit_receipt = port.try_submit(&quit)?;
    fixture.application.complete_control(&fixture.next()?)?;
    fixture.clock.0.store(1_000_000_000, Ordering::SeqCst);
    let mut refused = Vec::new();
    for operation in [
        fixture.operation(DesktopPuzzleBridgeEnumOperation::RetrySave),
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Snapshot),
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Summary),
        fixture.interrupt(1, PuzzleTimerEnumPauseReason::Manual)?,
        fixture.interrupt(1, PuzzleTimerEnumPauseReason::FocusLost)?,
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Command(
            PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::Manual),
        )),
        fixture.operation(DesktopPuzzleBridgeEnumOperation::Command(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Hidden),
        )),
    ] {
        let submitted = port.try_submit(&fixture.request(operation)?);
        refused.push(matches!(submitted, Err(DesktopApplicationError::Closing)));
        if let Ok(receipt) = submitted {
            fixture.complete(&fixture.next()?)?;
            if receipt.response().is_none() {
                return Err("fresh recovery cleanup left an unresolved receipt".into());
            }
        }
    }
    let repeated_quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    let replayed_quit = port.try_submit(&repeated_quit)?;
    let replayed_original_quit = port.try_submit(&quit)?;
    let replayed_recovery = port.try_submit(&original_recovery)?;
    assert!(fixture.application.try_next()?.is_none());
    assert!(matches!(
        quit_receipt.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    assert!(quit_receipt.shares_outcome(&replayed_quit));
    assert!(quit_receipt.shares_outcome(&replayed_original_quit));
    assert!(original_recovery_receipt.shares_outcome(&replayed_recovery));
    assert_eq!(
        quit_receipt.deadline(),
        Some(ClockInstant::from_nanos(2_000_000_000))
    );
    assert!(
        refused.into_iter().all(|refused| refused),
        "fresh canonical recovery remained admissible after terminal QuitReady"
    );
    Ok(())
}
