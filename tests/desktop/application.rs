use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::application::{
    DesktopApplication, DesktopApplicationError, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use chess::desktop::dto::DesktopEnumValue;
use chess::desktop::file_handle::DesktopFileEnumPurpose;
use chess::desktop::identity::DesktopRevision;
use chess::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
    DesktopRequest,
};

#[derive(Default)]
struct DesktopApplicationFixtureClock(AtomicU64);

impl DesktopApplicationFixtureClock {
    fn set(&self, nanos: u64) {
        self.0.store(nanos, Ordering::SeqCst);
    }
}

impl ClockTimeProvider for DesktopApplicationFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}

#[derive(Default)]
struct DesktopApplicationFixtureWake(AtomicUsize);

impl DesktopApplicationWakeProvider for DesktopApplicationFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct DesktopApplicationFixture {
    clock: Arc<DesktopApplicationFixtureClock>,
    wake: Arc<DesktopApplicationFixtureWake>,
    application: DesktopApplication<DesktopApplicationFixtureClock, DesktopApplicationFixtureWake>,
}

struct DesktopApplicationFixtureFallibleWake(AtomicBool);

impl DesktopApplicationWakeProvider for DesktopApplicationFixtureFallibleWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        if self.0.load(Ordering::SeqCst) {
            Err(DesktopApplicationWakeError::Unavailable)
        } else {
            Ok(())
        }
    }
}

impl DesktopApplicationFixture {
    fn new() -> Self {
        let clock = Arc::new(DesktopApplicationFixtureClock::default());
        let wake = Arc::new(DesktopApplicationFixtureWake::default());
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&wake));
        Self {
            clock,
            wake,
            application,
        }
    }

    fn bootstrap(&self) -> Result<DesktopRequest, DesktopApplicationError> {
        self.application
            .port()
            .request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))
    }

    fn selection(
        &self,
        owner: DesktopMessagesEntityId,
    ) -> Result<DesktopRequest, DesktopApplicationError> {
        self.application.port().request(DesktopCommand::new(
            DesktopEnumCommand::FileSelect {
                owner,
                purpose: DesktopFileEnumPurpose::ReadFile,
            },
            Some(DesktopRevision::ZERO),
        ))
    }

    fn game_stop(&self, identity: u128) -> Result<DesktopRequest, Box<dyn std::error::Error>> {
        let game = uuid::Uuid::from_u128(identity).to_string().parse()?;
        let operation = chess::desktop::game_bridge::DesktopGameBridgeEnumOperation::Session {
            game,
            revision: chess::game::enums::GameRevision::ZERO,
            action: chess::desktop::game_commands::DesktopGameCommandsEnumAction::Stop,
        };
        let revision = operation.revision();
        Ok(self.application.port().request(DesktopCommand::new(
            DesktopEnumCommand::Game(operation),
            Some(revision),
        ))?)
    }
}

#[test]
fn distinct_game_stop_controls_have_finite_admission_and_reserved_quit()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let mut receipts = Vec::new();
    let mut refused = false;
    for identity in 1..=10_000 {
        match port.try_submit(&fixture.game_stop(identity)?) {
            Ok(receipt) => receipts.push(receipt),
            Err(DesktopApplicationError::Busy) => {
                refused = true;
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let first = receipts.first().ok_or("no game stop admitted")?;
    fixture.clock.set(1_000_000_000);
    let repeated = port.try_submit(&fixture.game_stop(1)?)?;
    let retained = first.shares_outcome(&repeated)
        && first.received_at() == ClockInstant::from_nanos(0)
        && first.deadline() == Some(ClockInstant::from_nanos(2_000_000_000));
    let quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    let quit_receipt = port.try_submit(&quit)?;
    let mut completed = 0;
    while let Some(work) = fixture.application.try_next()? {
        fixture.application.complete_control(&work)?;
        completed += 1;
    }
    assert_eq!(completed, receipts.len() + 1);
    assert!(
        receipts
            .iter()
            .all(|receipt| matches!(receipt.response(), Some(DesktopMessagesEnumResult::Stopped)))
    );
    assert!(matches!(
        quit_receipt.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    assert!(retained);
    assert!(fixture.application.try_next()?.is_none());
    assert!(
        refused,
        "all {} distinct unresolved Game Stop controls were admitted without Busy",
        receipts.len()
    );
    Ok(())
}

#[test]
fn completing_a_control_releases_capacity_for_a_refused_control()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    for identity in 1..=u128::try_from(DesktopApplication::<DesktopApplicationFixtureClock, DesktopApplicationFixtureWake>::CONTROL_CAPACITY)? {
        port.try_submit(&fixture.game_stop(identity)?)?;
    }
    let refused = fixture.game_stop(u128::MAX)?;
    assert!(matches!(
        port.try_submit(&refused),
        Err(DesktopApplicationError::Busy)
    ));
    let work = fixture.application.try_next()?.ok_or("missing game stop")?;
    fixture.application.complete_control(&work)?;
    port.try_submit(&refused)?;
    let mut completed = 1;
    while let Some(work) = fixture.application.try_next()? {
        fixture.application.complete_control(&work)?;
        completed += 1;
    }
    assert_eq!(completed, DesktopApplication::<DesktopApplicationFixtureClock, DesktopApplicationFixtureWake>::CONTROL_CAPACITY + 1);
    Ok(())
}

#[test]
fn repeated_native_passes_share_one_pending_receipt_and_one_dispatch()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let request = fixture.bootstrap()?;
    let receipt = port.try_submit(&request)?;
    for _ in 0..1000 {
        assert!(receipt.shares_outcome(&port.try_submit(&request)?));
        assert!(receipt.response().is_none());
    }
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing admitted action")?;
    assert_eq!(work.request().request_id(), request.request_id());
    assert!(fixture.application.try_next()?.is_none());
    fixture
        .application
        .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    assert!(matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::Value { .. })
    ));
    assert!(receipt.shares_outcome(&port.try_submit(&request)?));
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn ordinary_admission_is_bounded_before_waiting_work_is_allocated()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    for _ in 0..DesktopApplication::<DesktopApplicationFixtureClock, DesktopApplicationFixtureWake>::ORDINARY_CAPACITY {
        port.try_submit(&fixture.bootstrap()?)?;
    }
    let retry = fixture.bootstrap()?;
    assert!(matches!(
        port.try_submit(&retry),
        Err(DesktopApplicationError::Busy)
    ));
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing queued action")?;
    assert!(matches!(
        port.try_submit(&retry),
        Err(DesktopApplicationError::Busy)
    ));
    fixture
        .application
        .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    assert!(port.try_submit(&retry).is_ok());
    Ok(())
}

#[test]
fn unresolved_entity_controls_remain_bounded_after_their_workers_stop()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let mut controls = Vec::new();
    for _ in 0..DesktopApplication::<DesktopApplicationFixtureClock, DesktopApplicationFixtureWake>::ORDINARY_CAPACITY {
        let owner = DesktopMessagesEntityId::new();
        port.try_submit(&fixture.selection(owner)?)?;
        let running = fixture.application.try_next()?.ok_or("missing selection")?;
        let stop = port.request(DesktopCommand::new(DesktopEnumCommand::Stop { owner }, None))?;
        port.try_submit(&stop)?;
        controls.push(fixture.application.try_next()?.ok_or("missing reserved control")?);
        fixture.application.acknowledge_stopped(&running)?;
    }
    assert!(matches!(
        port.try_submit(&fixture.selection(DesktopMessagesEntityId::new())?),
        Err(DesktopApplicationError::Busy)
    ));
    for control in controls {
        fixture.application.complete_control(&control)?;
    }
    assert!(
        port.try_submit(&fixture.selection(DesktopMessagesEntityId::new())?)
            .is_ok()
    );
    Ok(())
}

#[test]
fn completed_coalesced_controls_retain_the_same_receipt_for_every_original_action()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let owner = DesktopMessagesEntityId::new();
    port.try_submit(&fixture.selection(owner)?)?;
    let running = fixture.application.try_next()?.ok_or("missing selection")?;
    let first = port.request(DesktopCommand::new(
        DesktopEnumCommand::Stop { owner },
        None,
    ))?;
    let second = port.request(DesktopCommand::new(
        DesktopEnumCommand::Stop { owner },
        None,
    ))?;
    let receipt = port.try_submit(&first)?;
    assert!(receipt.shares_outcome(&port.try_submit(&second)?));
    let control = fixture.application.try_next()?.ok_or("missing stop")?;
    fixture.application.acknowledge_stopped(&running)?;
    fixture.application.complete_control(&control)?;
    assert!(receipt.shares_outcome(&port.try_submit(&first)?));
    assert!(receipt.shares_outcome(&port.try_submit(&second)?));
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn entity_mutations_keep_native_revision_precision_and_single_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let owner = DesktopMessagesEntityId::new();
    let revision: DesktopRevision = "9007199254740993".parse()?;
    let request = port.request(DesktopCommand::new(
        DesktopEnumCommand::FileSelect {
            owner,
            purpose: DesktopFileEnumPurpose::Directory,
        },
        Some(revision),
    ))?;
    port.try_submit(&request)?;
    assert!(matches!(
        port.try_submit(&fixture.selection(owner)?),
        Err(DesktopApplicationError::EntityBusy)
    ));
    let work = fixture.application.try_next()?.ok_or("missing selection")?;
    assert_eq!(work.request().command().expected_revision(), Some(revision));
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::FileSelect(None),
        Some(revision.next()?),
    )?;
    assert!(port.try_submit(&fixture.selection(owner)?).is_ok());
    Ok(())
}

#[test]
fn full_ordinary_queue_preserves_coalesced_stop_pause_and_global_quit()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let owner = DesktopMessagesEntityId::new();
    port.try_submit(&fixture.selection(owner)?)?;
    let running = fixture.application.try_next()?.ok_or("missing selection")?;
    for _ in 1..DesktopApplication::<DesktopApplicationFixtureClock, DesktopApplicationFixtureWake>::ORDINARY_CAPACITY {
        port.try_submit(&fixture.bootstrap()?)?;
    }
    let stop = port.request(DesktopCommand::new(
        DesktopEnumCommand::Stop { owner },
        None,
    ))?;
    let first = port.try_submit(&stop)?;
    fixture.clock.set(1_000_000_000);
    let repeated = port.request(DesktopCommand::new(
        DesktopEnumCommand::Stop { owner },
        None,
    ))?;
    assert!(first.shares_outcome(&port.try_submit(&repeated)?));
    let pause = port.request(DesktopCommand::new(
        DesktopEnumCommand::Pause { owner },
        None,
    ))?;
    port.try_submit(&pause)?;
    let quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    let quit_receipt = port.try_submit(&quit)?;
    let repeated_quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    assert!(quit_receipt.shares_outcome(&port.try_submit(&repeated_quit)?));
    assert_eq!(
        first.deadline(),
        Some(ClockInstant::from_nanos(2_000_000_000))
    );
    assert!(running.is_cancellation_requested());
    assert!(matches!(
        port.try_submit(&fixture.bootstrap()?),
        Err(DesktopApplicationError::Closing)
    ));
    let urgent = fixture
        .application
        .try_next()?
        .ok_or("missing urgent control")?;
    assert!(urgent.request().command().operation().is_control());
    Ok(())
}

#[test]
fn stop_deadline_begins_on_first_port_receipt_even_before_actor_dispatch()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let owner = DesktopMessagesEntityId::new();
    port.try_submit(&fixture.selection(owner)?)?;
    let running = fixture.application.try_next()?.ok_or("missing selection")?;
    fixture.clock.set(7_000_000_000);
    let stop = port.request(DesktopCommand::new(
        DesktopEnumCommand::Stop { owner },
        None,
    ))?;
    let receipt = port.try_submit(&stop)?;
    fixture.clock.set(10_000_000_000);
    assert_eq!(
        receipt.received_at(),
        ClockInstant::from_nanos(7_000_000_000)
    );
    assert_eq!(
        receipt.deadline(),
        Some(ClockInstant::from_nanos(9_000_000_000))
    );
    assert!(receipt.is_reconciliation_required(fixture.clock.now()?));
    assert!(receipt.response().is_none());
    let control = fixture.application.try_next()?.ok_or("missing stop")?;
    assert!(matches!(
        fixture.application.complete_control(&control),
        Err(DesktopApplicationError::CancellationUnacknowledged)
    ));
    fixture.application.acknowledge_stopped(&running)?;
    fixture.application.complete_control(&control)?;
    assert!(matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::Stopped)
    ));
    Ok(())
}

#[test]
fn cancellation_blocks_late_commit_and_waits_for_observed_worker_stop()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let owner = DesktopMessagesEntityId::new();
    let original = port.try_submit(&fixture.selection(owner)?)?;
    let running = fixture.application.try_next()?.ok_or("missing selection")?;
    let stop = port.request(DesktopCommand::new(
        DesktopEnumCommand::Stop { owner },
        None,
    ))?;
    let stopped = port.try_submit(&stop)?;
    let control = fixture.application.try_next()?.ok_or("missing control")?;
    assert!(matches!(
        fixture.application.begin_commit(&running),
        Err(DesktopApplicationError::CancellationRequested)
    ));
    assert!(matches!(
        fixture
            .application
            .complete_value(&running, DesktopEnumValue::FileSelect(None), None),
        Err(DesktopApplicationError::CancellationRequested)
    ));
    assert!(matches!(
        fixture.application.complete_control(&control),
        Err(DesktopApplicationError::CancellationUnacknowledged)
    ));
    assert!(original.response().is_none());
    assert!(stopped.response().is_none());
    fixture.application.acknowledge_stopped(&running)?;
    fixture.application.complete_control(&control)?;
    assert!(matches!(
        original.response(),
        Some(DesktopMessagesEnumResult::Cancelled)
    ));
    assert!(matches!(
        stopped.response(),
        Some(DesktopMessagesEnumResult::Stopped)
    ));
    assert!(
        fixture
            .application
            .complete_value(&running, DesktopEnumValue::FileSelect(None), None)
            .is_err()
    );
    Ok(())
}

#[test]
fn cancellation_during_irreversible_commit_requires_actual_outcome_reconciliation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let owner = DesktopMessagesEntityId::new();
    let original = port.try_submit(&fixture.selection(owner)?)?;
    let running = fixture.application.try_next()?.ok_or("missing selection")?;
    fixture.application.begin_commit(&running)?;
    let stop = port.request(DesktopCommand::new(
        DesktopEnumCommand::Stop { owner },
        None,
    ))?;
    let stopped = port.try_submit(&stop)?;
    let control = fixture.application.try_next()?.ok_or("missing stop")?;
    assert!(matches!(
        fixture.application.acknowledge_stopped(&running),
        Err(DesktopApplicationError::CommitUnresolved)
    ));
    fixture.application.complete_value(
        &running,
        DesktopEnumValue::FileSelect(None),
        Some(DesktopRevision::ZERO),
    )?;
    fixture.application.complete_control(&control)?;
    assert!(matches!(
        original.response(),
        Some(DesktopMessagesEnumResult::Value { .. })
    ));
    assert!(matches!(
        stopped.response(),
        Some(DesktopMessagesEnumResult::ReconciliationRequired)
    ));
    Ok(())
}

#[test]
fn cancelling_queued_work_never_dispatches_the_cancelled_effect()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let owner = DesktopMessagesEntityId::new();
    let original = port.try_submit(&fixture.selection(owner)?)?;
    let pause = port.request(DesktopCommand::new(
        DesktopEnumCommand::Pause { owner },
        None,
    ))?;
    let paused = port.try_submit(&pause)?;
    assert!(matches!(
        original.response(),
        Some(DesktopMessagesEnumResult::Cancelled)
    ));
    let control = fixture.application.try_next()?.ok_or("missing pause")?;
    fixture.application.complete_control(&control)?;
    assert!(matches!(
        paused.response(),
        Some(DesktopMessagesEnumResult::Paused)
    ));
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn completed_cache_expiry_requires_resync_instead_of_replaying_old_action_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let request = fixture.bootstrap()?;
    port.try_submit(&request)?;
    let work = fixture.application.try_next()?.ok_or("missing action")?;
    fixture
        .application
        .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    fixture.clock.set(300_000_000_000);
    assert!(matches!(
        port.try_submit(&request),
        Err(DesktopApplicationError::ResyncRequired)
    ));
    assert!(fixture.application.try_next()?.is_none());
    assert!(port.try_submit(&fixture.bootstrap()?).is_ok());
    Ok(())
}

#[test]
fn completed_cache_capacity_eviction_does_not_turn_an_old_action_into_new_work()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    let original = fixture.bootstrap()?;
    port.try_submit(&original)?;
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing first action")?;
    fixture
        .application
        .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    for _ in 0..DesktopApplication::<DesktopApplicationFixtureClock, DesktopApplicationFixtureWake>::RESULT_CAPACITY {
        port.try_submit(&fixture.bootstrap()?)?;
        let work = fixture.application.try_next()?.ok_or("missing subsequent action")?;
        fixture.application.complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    }
    assert!(matches!(
        port.try_submit(&original),
        Err(DesktopApplicationError::ResyncRequired)
    ));
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn completion_wakes_native_loop_once_until_adapter_rearms() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = DesktopApplicationFixture::new();
    let port = fixture.application.port();
    for _ in 0..2 {
        port.try_submit(&fixture.bootstrap()?)?;
        let work = fixture.application.try_next()?.ok_or("missing action")?;
        fixture
            .application
            .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    }
    assert_eq!(fixture.wake.0.load(Ordering::SeqCst), 1);
    port.acknowledge_wake();
    port.try_submit(&fixture.bootstrap()?)?;
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing rearmed action")?;
    fixture
        .application
        .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    assert_eq!(fixture.wake.0.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn failed_native_wake_preserves_the_actual_result_and_supports_explicit_rearm()
-> Result<(), Box<dyn std::error::Error>> {
    let clock = Arc::new(DesktopApplicationFixtureClock::default());
    let wake = Arc::new(DesktopApplicationFixtureFallibleWake(AtomicBool::new(true)));
    let application = DesktopApplication::new(clock, Arc::clone(&wake));
    let port = application.port();
    let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
    let receipt = port.try_submit(&request)?;
    let work = application.try_next()?.ok_or("missing action")?;
    assert!(matches!(
        application.complete_value(&work, DesktopEnumValue::StreamAcknowledge, None),
        Err(DesktopApplicationError::Wake(
            DesktopApplicationWakeError::Unavailable
        ))
    ));
    assert!(matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::Value { .. })
    ));
    assert!(receipt.shares_outcome(&port.try_submit(&request)?));
    assert!(application.try_next()?.is_none());
    wake.0.store(false, Ordering::SeqCst);
    port.rearm_wake()?;
    Ok(())
}

#[test]
fn action_tokens_from_another_application_session_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let first = DesktopApplicationFixture::new();
    let second = DesktopApplicationFixture::new();
    assert!(matches!(
        second.application.port().try_submit(&first.bootstrap()?),
        Err(DesktopApplicationError::WrongSession)
    ));
    Ok(())
}

#[test]
fn monotonic_time_regression_is_a_typed_failure_without_dispatch()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationFixture::new();
    fixture.clock.set(100);
    fixture
        .application
        .port()
        .try_submit(&fixture.bootstrap()?)?;
    fixture.clock.set(99);
    assert!(matches!(
        fixture.application.port().try_submit(&fixture.bootstrap()?),
        Err(DesktopApplicationError::TimeOrder)
    ));
    Ok(())
}
