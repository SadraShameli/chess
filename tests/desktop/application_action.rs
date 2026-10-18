use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::application::{
    DesktopApplication, DesktopApplicationAction, DesktopApplicationError,
    DesktopApplicationWakeError, DesktopApplicationWakeProvider,
};
use chess::desktop::dto::DesktopEnumValue;
use chess::desktop::file_handle::DesktopFileEnumPurpose;
use chess::desktop::identity::{DesktopRequestId, DesktopRevision};
use chess::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
};

#[derive(Default)]
struct DesktopApplicationActionFixtureClock {
    nanos: AtomicU64,
    reads: AtomicUsize,
}

impl DesktopApplicationActionFixtureClock {
    fn set(&self, nanos: u64) {
        self.nanos.store(nanos, Ordering::SeqCst);
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl ClockTimeProvider for DesktopApplicationActionFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(ClockInstant::from_nanos(self.nanos.load(Ordering::SeqCst)))
    }
}

#[derive(Default)]
struct DesktopApplicationActionFixtureWake(AtomicUsize);

impl DesktopApplicationActionFixtureWake {
    fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl DesktopApplicationWakeProvider for DesktopApplicationActionFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct DesktopApplicationActionFixture {
    clock: Arc<DesktopApplicationActionFixtureClock>,
    wake: Arc<DesktopApplicationActionFixtureWake>,
    application: DesktopApplication<
        DesktopApplicationActionFixtureClock,
        DesktopApplicationActionFixtureWake,
    >,
}

impl DesktopApplicationActionFixture {
    const ORDINARY_CAPACITY: usize = DesktopApplication::<
        DesktopApplicationActionFixtureClock,
        DesktopApplicationActionFixtureWake,
    >::ORDINARY_CAPACITY;

    fn new() -> Self {
        let clock = Arc::new(DesktopApplicationActionFixtureClock::default());
        let wake = Arc::new(DesktopApplicationActionFixtureWake::default());
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&wake));
        Self {
            clock,
            wake,
            application,
        }
    }

    fn bootstrap() -> DesktopCommand {
        DesktopCommand::new(DesktopEnumCommand::Bootstrap, None)
    }

    fn selection(
        owner: DesktopMessagesEntityId,
        revision: Option<DesktopRevision>,
    ) -> DesktopCommand {
        DesktopCommand::new(
            DesktopEnumCommand::FileSelect {
                owner,
                purpose: DesktopFileEnumPurpose::ReadFile,
            },
            revision,
        )
    }

    fn drain_bootstraps(&self) -> Result<Vec<DesktopRequestId>, DesktopApplicationError> {
        let mut identities = Vec::new();
        while let Some(work) = self.application.try_next()? {
            identities.push(work.request().request_id());
            self.application
                .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
        }
        Ok(identities)
    }
}

#[test]
fn oversized_editor_fen_is_refused_before_application_admission_or_clock_observation()
-> Result<(), Box<dyn std::error::Error>> {
    let oversized = "x".repeat(chess::board::position::BoardPosition::MAX_FEN_BYTES + 1);
    let mut excess_capacity =
        String::with_capacity(chess::board::position::BoardPosition::MAX_FEN_BYTES + 1);
    excess_capacity.push_str(&chess::board::position::BoardPosition::standard().fen());
    for fen in [excess_capacity, oversized] {
        let length = fen.len();
        let capacity = fen.capacity();
        let fixture = DesktopApplicationActionFixture::new();
        let port = fixture.application.port();
        let draft = chess::board::editor::BoardEditorDraftId::try_from(uuid::Uuid::from_u128(1))?;
        let opening = chess::desktop::editor_commands::DesktopEditorCommandsOpening::new(
            draft,
            DesktopRequestId::new(),
        );
        let request = port.request(DesktopCommand::editor(
            DesktopMessagesEntityId::for_editor(draft),
            chess::desktop::editor_commands::DesktopEditorCommandsEnumOperation::ImportFen {
                opening,
                fen,
                variant: chess::board::enums::BoardEnumVariant::Standard,
                expected: None,
            },
        ))?;
        let admission = port.try_submit(&request);
        let refused = admission.is_err();
        let reads = fixture.clock.reads();
        let quit =
            port.try_submit(&port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?)?;
        let work = fixture
            .application
            .try_next()?
            .ok_or("missing cleanup Quit")?;
        fixture.application.complete_control(&work)?;
        let settled = matches!(quit.response(), Some(DesktopMessagesEnumResult::QuitReady))
            && fixture.application.try_next()?.is_none();
        drop((admission, request, quit, work, port, fixture));
        assert!(settled);
        assert!(
            refused,
            "oversized editor FEN entered the application queue (length {length}, capacity {capacity})"
        );
        assert_eq!(reads, 0);
    }
    Ok(())
}

#[test]
fn prepared_identity_survives_binding_admission_dispatch_and_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationActionFixture::new();
    let port = fixture.application.port();
    fixture.clock.set(10);
    let action: DesktopApplicationAction = port.prepare_action()?;
    let identity = action.request_id();
    for _ in 0..32 {
        assert_eq!(action.request_id(), identity);
    }
    let revision: DesktopRevision = "9007199254740993".parse()?;
    let command =
        DesktopApplicationActionFixture::selection(DesktopMessagesEntityId::new(), Some(revision));
    fixture.clock.set(20);
    let request = action.bind(command.clone())?;
    assert_eq!(fixture.clock.reads(), 0);
    assert_eq!(fixture.wake.count(), 0);
    fixture.clock.set(30);
    let receipt = port.try_submit(&request)?;
    let repeated = port.try_submit(&request)?;
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing bound mutation")?;
    fixture.application.begin_commit(&work)?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::FileSelect(None),
        Some(revision.next()?),
    )?;
    let completed = port.try_submit(&request)?;
    assert_eq!(request.request_id(), identity);
    assert_eq!(receipt.request_id(), identity);
    assert_eq!(work.request().request_id(), identity);
    assert_eq!(work.request().command(), &command);
    assert_eq!(work.request().command().expected_revision(), Some(revision));
    assert_eq!(receipt.received_at(), ClockInstant::from_nanos(30));
    assert!(receipt.shares_outcome(work.receipt()));
    assert!(receipt.shares_outcome(&repeated));
    assert!(receipt.shares_outcome(&completed));
    assert!(matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::Value { .. })
    ));
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn dropped_unbound_actions_consume_no_work_capacity_clock_or_wake()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationActionFixture::new();
    let port = fixture.application.port();
    for _ in 0..=DesktopApplicationActionFixture::ORDINARY_CAPACITY {
        let _unbound = port.prepare_action()?;
    }
    assert_eq!(fixture.clock.reads(), 0);
    assert_eq!(fixture.wake.count(), 0);
    assert!(fixture.application.try_next()?.is_none());
    let mut receipts = Vec::new();
    for _ in 0..DesktopApplicationActionFixture::ORDINARY_CAPACITY {
        let request = port.request(DesktopApplicationActionFixture::bootstrap())?;
        receipts.push(port.try_submit(&request)?);
    }
    let overflow = port
        .prepare_action()?
        .bind(DesktopApplicationActionFixture::bootstrap())?;
    let refused = port.try_submit(&overflow);
    let identities = fixture.drain_bootstraps()?;
    assert!(matches!(refused, Err(DesktopApplicationError::Busy)));
    assert_eq!(
        identities.len(),
        DesktopApplicationActionFixture::ORDINARY_CAPACITY
    );
    assert!(receipts.iter().all(|receipt| receipt.response().is_some()));
    Ok(())
}

#[test]
fn control_deadline_starts_on_submission_after_preparation_and_binding()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationActionFixture::new();
    let port = fixture.application.port();
    let action = port.prepare_action()?;
    let identity = action.request_id();
    fixture.clock.set(5_000_000_000);
    let request = action.bind(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    assert_eq!(fixture.clock.reads(), 0);
    assert_eq!(fixture.wake.count(), 0);
    fixture.clock.set(10_000_000_000);
    let receipt = port.try_submit(&request)?;
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing bound quit")?;
    fixture.application.complete_control(&work)?;
    assert_eq!(receipt.request_id(), identity);
    assert_eq!(
        receipt.received_at(),
        ClockInstant::from_nanos(10_000_000_000)
    );
    assert_eq!(
        receipt.deadline(),
        Some(ClockInstant::from_nanos(12_000_000_000))
    );
    assert!(matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn bound_actions_cannot_be_submitted_through_another_application_session()
-> Result<(), Box<dyn std::error::Error>> {
    let first = DesktopApplicationActionFixture::new();
    let second = DesktopApplicationActionFixture::new();
    let action = first.application.port().prepare_action()?;
    let identity = action.request_id();
    let request = action.bind(DesktopApplicationActionFixture::bootstrap())?;
    let refused = second.application.port().try_submit(&request);
    assert_eq!(first.clock.reads(), 0);
    assert_eq!(second.clock.reads(), 0);
    assert_eq!(second.wake.count(), 0);
    assert!(second.application.try_next()?.is_none());
    let receipt = first.application.port().try_submit(&request)?;
    let identities = first.drain_bootstraps()?;
    assert!(matches!(
        refused,
        Err(DesktopApplicationError::WrongSession)
    ));
    assert_eq!(receipt.request_id(), identity);
    assert_eq!(identities, vec![identity]);
    assert!(receipt.response().is_some());
    Ok(())
}

#[test]
fn binding_and_direct_requests_refuse_mutations_without_expected_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationActionFixture::new();
    let port = fixture.application.port();
    let action = port.prepare_action()?;
    let invalid_identity = action.request_id();
    let command = DesktopApplicationActionFixture::selection(DesktopMessagesEntityId::new(), None);
    let bound = action.bind(command.clone());
    let direct = port.request(command);
    assert_eq!(fixture.clock.reads(), 0);
    assert_eq!(fixture.wake.count(), 0);
    assert!(fixture.application.try_next()?.is_none());
    let fresh = port
        .prepare_action()?
        .bind(DesktopApplicationActionFixture::bootstrap())?;
    let receipt = port.try_submit(&fresh)?;
    let identities = fixture.drain_bootstraps()?;
    assert!(matches!(
        bound,
        Err(DesktopApplicationError::MissingRevision)
    ));
    assert!(matches!(
        direct,
        Err(DesktopApplicationError::MissingRevision)
    ));
    assert_ne!(fresh.request_id(), invalid_identity);
    assert_eq!(identities, vec![fresh.request_id()]);
    assert_eq!(receipt.request_id(), fresh.request_id());
    Ok(())
}

#[test]
fn busy_retry_keeps_the_same_bound_request_and_first_submission_time()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationActionFixture::new();
    let port = fixture.application.port();
    for _ in 0..DesktopApplicationActionFixture::ORDINARY_CAPACITY {
        port.try_submit(&port.request(DesktopApplicationActionFixture::bootstrap())?)?;
    }
    let action = port.prepare_action()?;
    let identity = action.request_id();
    let request = action.bind(DesktopApplicationActionFixture::bootstrap())?;
    fixture.clock.set(100);
    let first_refusal = port.try_submit(&request);
    let running = fixture
        .application
        .try_next()?
        .ok_or("missing full-queue work")?;
    let second_refusal = port.try_submit(&request);
    fixture
        .application
        .complete_value(&running, DesktopEnumValue::StreamAcknowledge, None)?;
    fixture.clock.set(200);
    let receipt = port.try_submit(&request)?;
    let repeated = port.try_submit(&request)?;
    let identities = fixture.drain_bootstraps()?;
    assert!(matches!(first_refusal, Err(DesktopApplicationError::Busy)));
    assert!(matches!(second_refusal, Err(DesktopApplicationError::Busy)));
    assert_eq!(request.request_id(), identity);
    assert_eq!(receipt.request_id(), identity);
    assert_eq!(receipt.received_at(), ClockInstant::from_nanos(100));
    assert!(receipt.shares_outcome(&repeated));
    assert_eq!(
        identities
            .iter()
            .filter(|candidate| **candidate == identity)
            .count(),
        1
    );
    assert_eq!(
        identities.len(),
        DesktopApplicationActionFixture::ORDINARY_CAPACITY
    );
    assert!(receipt.response().is_some());
    Ok(())
}

#[test]
fn fresh_actions_and_direct_requests_dispatch_distinct_identities_in_submission_order()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationActionFixture::new();
    let port = fixture.application.port();
    let mut identities = HashSet::new();
    let mut expected = Vec::new();
    let mut receipts = Vec::new();
    for _ in 0..DesktopApplicationActionFixture::ORDINARY_CAPACITY / 3 {
        let first = port.prepare_action()?;
        let second = port.clone().prepare_action()?;
        assert!(identities.insert(first.request_id()));
        assert!(identities.insert(second.request_id()));
        let direct = port.request(DesktopApplicationActionFixture::bootstrap())?;
        assert!(identities.insert(direct.request_id()));
        let second = second.bind(DesktopApplicationActionFixture::bootstrap())?;
        let first = first.bind(DesktopApplicationActionFixture::bootstrap())?;
        for request in [second, direct, first] {
            let receipt = port.try_submit(&request)?;
            expected.push(request.request_id());
            assert_eq!(receipt.request_id(), request.request_id());
            assert!(receipt.shares_outcome(&port.try_submit(&request)?));
            receipts.push(receipt);
        }
    }
    let dispatched = fixture.drain_bootstraps()?;
    assert_eq!(dispatched, expected);
    assert_eq!(identities.len(), expected.len());
    for (index, receipt) in receipts.iter().enumerate() {
        assert!(receipt.response().is_some());
        assert!(
            receipts[..index]
                .iter()
                .all(|other| !receipt.shares_outcome(other))
        );
    }
    Ok(())
}

#[test]
fn expired_bound_actions_refuse_replay_while_fresh_actions_still_dispatch()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopApplicationActionFixture::new();
    let port = fixture.application.port();
    let request = port
        .prepare_action()?
        .bind(DesktopApplicationActionFixture::bootstrap())?;
    let receipt = port.try_submit(&request)?;
    let dispatched = fixture.drain_bootstraps()?;
    fixture.clock.set(300_000_000_000);
    let refused = port.try_submit(&request);
    let fresh = port
        .prepare_action()?
        .bind(DesktopApplicationActionFixture::bootstrap())?;
    let fresh_receipt = port.try_submit(&fresh)?;
    let fresh_dispatched = fixture.drain_bootstraps()?;
    assert!(matches!(
        refused,
        Err(DesktopApplicationError::ResyncRequired)
    ));
    assert_eq!(dispatched, vec![request.request_id()]);
    assert_eq!(fresh_dispatched, vec![fresh.request_id()]);
    assert_ne!(request.request_id(), fresh.request_id());
    assert!(!receipt.shares_outcome(&fresh_receipt));
    Ok(())
}
