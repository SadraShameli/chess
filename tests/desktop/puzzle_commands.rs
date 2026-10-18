use std::future::{Future, poll_fn};
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::board::position::BoardPosition;
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::desktop::puzzle_commands::{DesktopPuzzleCommands, DesktopPuzzleCommandsError};
use crate::puzzle::attempt::PuzzleAttemptId;
use crate::puzzle::checkpoint::{
    PuzzleCheckpointEnumFreezeCause, PuzzleCheckpointEnumStatus, PuzzleCheckpointError,
};
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumPhase, PuzzleEnumPractice,
    PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::PuzzleDefinitionMetadata;
use crate::puzzle::persistence::{
    PuzzlePersistenceCodec, PuzzlePersistenceEnumCommand, PuzzlePersistenceReceipt,
    PuzzlePersistenceRun, PuzzlePersistenceSaveRequest, PuzzlePersistenceStorageRevision,
};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::run::{PuzzleRunEnumConfirmation, PuzzleRunEnumEndReason, PuzzleRunEnumStatus};
use crate::puzzle::selection::PuzzleSelectionFilter;
use crate::puzzle::session::{
    PuzzleSessionConfig, PuzzleSessionContext, PuzzleSessionId, PuzzleSessionRevision,
};
use crate::puzzle::storage_repository::{
    PuzzleStorageDriverError, PuzzleStorageRepository, PuzzleStorageWrite,
};
use crate::puzzle::timer::{
    PuzzleTimerEnumPauseReason, PuzzleTimerEnumPhase, PuzzleTimerEnumReadiness,
};

#[derive(Debug, Error)]
enum DesktopPuzzleCommandsFixtureError {
    #[error("The fixture storage queue refused a write")]
    Admission,
    #[error("The fixture commit failed")]
    Commit,
    #[error("The fixture storage response was abandoned")]
    Abandoned,
    #[error("The fixture storage response channel closed: {0}")]
    Receive(#[from] oneshot::error::RecvError),
    #[error("The fixture storage revision overflowed")]
    Revision,
}

struct DesktopPuzzleCommandsFixtureRequest {
    request: PuzzlePersistenceSaveRequest,
    response: oneshot::Sender<Result<PuzzlePersistenceReceipt, DesktopPuzzleCommandsFixtureError>>,
}

impl DesktopPuzzleCommandsFixtureRequest {
    fn commit(self) -> Result<(), DesktopPuzzleCommandsFixtureError> {
        let Self { request, response } = self;
        let revision = request
            .expected_storage_revision()
            .value()
            .checked_add(1)
            .ok_or(DesktopPuzzleCommandsFixtureError::Revision)?;
        let receipt = PuzzlePersistenceReceipt::committed(
            request,
            PuzzlePersistenceStorageRevision::new(revision),
        );
        Self::send(response, Ok(receipt))
    }

    fn respond(
        self,
        result: Result<PuzzlePersistenceReceipt, DesktopPuzzleCommandsFixtureError>,
    ) -> Result<(), DesktopPuzzleCommandsFixtureError> {
        Self::send(self.response, result)
    }

    fn send(
        response: oneshot::Sender<
            Result<PuzzlePersistenceReceipt, DesktopPuzzleCommandsFixtureError>,
        >,
        result: Result<PuzzlePersistenceReceipt, DesktopPuzzleCommandsFixtureError>,
    ) -> Result<(), DesktopPuzzleCommandsFixtureError> {
        response
            .send(result)
            .map_err(|_result| DesktopPuzzleCommandsFixtureError::Abandoned)
    }
}

struct DesktopPuzzleCommandsFixtureWrite {
    response:
        oneshot::Receiver<Result<PuzzlePersistenceReceipt, DesktopPuzzleCommandsFixtureError>>,
}

impl PuzzleStorageWrite for DesktopPuzzleCommandsFixtureWrite {
    type Error = DesktopPuzzleCommandsFixtureError;

    async fn wait(&mut self) -> Result<PuzzlePersistenceReceipt, Self::Error> {
        (&mut self.response).await?
    }
}

struct DesktopPuzzleCommandsFixtureRepository {
    requests: mpsc::Sender<DesktopPuzzleCommandsFixtureRequest>,
    begun: Arc<AtomicUsize>,
}

impl PuzzleStorageRepository for DesktopPuzzleCommandsFixtureRepository {
    type Error = DesktopPuzzleCommandsFixtureError;
    type Write = DesktopPuzzleCommandsFixtureWrite;

    fn begin_save(
        &self,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<Self::Write, Self::Error> {
        let (response, receiver) = oneshot::channel();
        if self
            .requests
            .try_send(DesktopPuzzleCommandsFixtureRequest { request, response })
            .is_err()
        {
            return Err(DesktopPuzzleCommandsFixtureError::Admission);
        }
        self.begun.fetch_add(1, Ordering::Relaxed);
        Ok(DesktopPuzzleCommandsFixtureWrite { response: receiver })
    }
}

struct DesktopPuzzleCommandsFixture {
    owner: DesktopPuzzleCommands<DesktopPuzzleCommandsFixtureRepository>,
    requests: mpsc::Receiver<DesktopPuzzleCommandsFixtureRequest>,
    begun: Arc<AtomicUsize>,
}

impl DesktopPuzzleCommandsFixture {
    const RUN_DURATION_MILLIS: u64 = 60_000;

    fn session(puzzles: usize, line: &[&str]) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new(
            PuzzleEnumCountdownMode::Session,
            Self::RUN_DURATION_MILLIS,
            puzzles,
            line,
        )
    }

    fn time(millis: u64) -> Result<ClockInstant, crate::clock::duration::ClockDurationError> {
        ClockInstant::from_millis(millis)
    }

    fn run(
        mode: PuzzleEnumCountdownMode,
        duration_millis: u64,
        puzzles: usize,
        line: &[&str],
    ) -> Result<PuzzlePersistenceRun, Box<dyn std::error::Error>> {
        let initial = BoardPosition::standard();
        let mut position = initial.clone();
        let mut moves = Vec::new();
        for uci in line {
            let movement = position.parse_uci(uci)?;
            position = position.play(movement)?;
            moves.push(movement);
        }
        let definitions = (0..puzzles)
            .map(|index| {
                Ok(PuzzleDefinition::new(
                    PuzzleDefinitionIdentity::new(
                        "command-owner".into(),
                        "v1".into(),
                        index.to_string(),
                    )?,
                    initial.clone(),
                    vec![moves.clone()],
                    PuzzleDefinitionMetadata::default(),
                )?)
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        Ok(PuzzlePersistenceRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::new(mode, Duration::from_millis(duration_millis))?,
            definitions,
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(81),
        )?)
    }

    fn from_run(run: PuzzlePersistenceRun) -> Result<Self, Box<dyn std::error::Error>> {
        let (sender, requests) = mpsc::channel(1);
        let begun = Arc::new(AtomicUsize::new(0));
        let owner = DesktopPuzzleCommands::new(
            run,
            DesktopPuzzleCommandsFixtureRepository {
                requests: sender,
                begun: Arc::clone(&begun),
            },
            Self::time(0)?,
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?;
        Ok(Self {
            owner,
            requests,
            begun,
        })
    }

    fn new(
        mode: PuzzleEnumCountdownMode,
        duration_millis: u64,
        puzzles: usize,
        line: &[&str],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::from_run(Self::run(mode, duration_millis, puzzles, line)?)
    }

    fn execute(
        &mut self,
        command: PuzzlePersistenceEnumCommand,
        millis: u64,
    ) -> Result<(), DesktopPuzzleCommandsError<DesktopPuzzleCommandsFixtureError>> {
        let now = Self::time(millis).map_err(PuzzleCheckpointError::from)?;
        self.owner.execute(&self.owner.context(), command, now)
    }

    async fn complete(&mut self, millis: u64) -> Result<(), Box<dyn std::error::Error>> {
        self.requests.try_recv()?.commit()?;
        self.owner.wait().await?;
        self.owner.settle(Self::time(millis)?)?;
        Ok(())
    }

    async fn finish_writes(&mut self, millis: u64) -> Result<(), Box<dyn std::error::Error>> {
        for _ in 0..8 {
            if !self.owner.has_retained_write() {
                return Ok(());
            }
            self.complete(millis).await?;
        }
        Err("The fixture did not drain its bounded retained writes".into())
    }

    async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.owner.begin_write(Self::time(0)?)?;
        self.finish_writes(0).await?;
        self.execute(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
            0,
        )?;
        Ok(())
    }
}

#[tokio::test]
async fn initial_commit_blocks_countdown_and_then_requires_fresh_explicit_readiness()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    assert!(matches!(
        fixture.execute(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
            0
        ),
        Err(DesktopPuzzleCommandsError::SaveRequired(_))
    ));
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(9000)?)?;
    assert_eq!(snapshot.status(), PuzzleRunEnumStatus::AwaitingReadiness);
    assert_eq!(snapshot.timer().elapsed(), ClockDuration::ZERO);
    assert!(snapshot.attempt().is_some());
    assert!(snapshot.durable_context().is_none());
    fixture.complete(9000).await?;
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(9000)?)?;
    assert_eq!(
        snapshot.checkpoint_status(),
        PuzzleCheckpointEnumStatus::Open
    );
    assert_eq!(snapshot.status(), PuzzleRunEnumStatus::AwaitingReadiness);
    assert!(!snapshot.has_unsaved_state());
    fixture.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        9000,
    )?;
    assert_eq!(
        fixture.owner.next_deadline()?,
        Some(DesktopPuzzleCommandsFixture::time(9250)?)
    );
    assert_eq!(
        fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(9000)?)?
            .status(),
        PuzzleRunEnumStatus::Active
    );
    Ok(())
}

#[tokio::test]
async fn recovered_initial_commit_keeps_resume_gated_and_invalid_resume_does_not_open_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = DesktopPuzzleCommandsFixture::run(
        PuzzleEnumCountdownMode::Session,
        DesktopPuzzleCommandsFixture::RUN_DURATION_MILLIS,
        1,
        &["e2e4"],
    )?;
    let bytes =
        PuzzlePersistenceCodec::encode(&run.capture(DesktopPuzzleCommandsFixture::time(0)?, 0)?)?;
    let mut fixture =
        DesktopPuzzleCommandsFixture::from_run(PuzzlePersistenceCodec::decode(&bytes)?)?;
    fixture
        .owner
        .begin_write(DesktopPuzzleCommandsFixture::time(0)?)?;
    fixture.complete(0).await?;
    assert!(matches!(
        fixture.execute(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
            0
        ),
        Err(DesktopPuzzleCommandsError::SaveRequired(_))
    ));
    assert!(
        fixture
            .execute(
                PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::InputBlocked),
                0
            )
            .is_err()
    );
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(0)?)?;
    assert!(matches!(
        snapshot.checkpoint_status(),
        PuzzleCheckpointEnumStatus::AwaitingResume(_)
    ));
    assert!(snapshot.attempt().is_none());
    fixture.execute(
        PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
        0,
    )?;
    assert_eq!(
        fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(0)?)?
            .status(),
        PuzzleRunEnumStatus::Active
    );
    Ok(())
}

#[tokio::test]
async fn delayed_snapshot_freezes_at_the_exact_bound_and_commits_latest_pause_before_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    fixture.start().await?;
    for millis in [249, 250, 500, 750] {
        fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(millis)?)?;
    }
    assert_eq!(fixture.begun.load(Ordering::Relaxed), 2);
    assert_eq!(
        fixture
            .owner
            .storage
            .checkpoint()
            .coalesced()
            .ok_or("Missing latest countdown")?
            .observation()
            .elapsed_active(),
        ClockDuration::from_millis(750)?
    );
    let late = DesktopPuzzleCommandsFixture::time(9000)?;
    let snapshot = fixture.owner.snapshot(late)?;
    let paused = snapshot.context().clone();
    assert_eq!(
        snapshot.timer().elapsed(),
        ClockDuration::from_millis(1000)?
    );
    assert_eq!(
        snapshot.timer().remaining(),
        ClockDuration::from_millis(59000)?
    );
    assert!(matches!(
        snapshot.status(),
        PuzzleRunEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::SaveBackpressure,
            ..
        }
    ));
    assert!(
        matches!(snapshot.checkpoint_status(), PuzzleCheckpointEnumStatus::Frozen(freeze) if freeze.cause() == PuzzleCheckpointEnumFreezeCause::DurableLag && freeze.pause_at() == DesktopPuzzleCommandsFixture::time(1000)?)
    );
    assert!(snapshot.attempt().is_none());
    assert_eq!(fixture.owner.next_deadline()?, None);
    fixture.complete(9000).await?;
    let snapshot = fixture.owner.snapshot(late)?;
    assert_eq!(snapshot.context(), &paused);
    assert_ne!(snapshot.durable_context(), Some(&paused));
    assert!(
        fixture
            .execute(
                PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
                9000
            )
            .is_err()
    );
    fixture.complete(9000).await?;
    let snapshot = fixture.owner.snapshot(late)?;
    assert_eq!(snapshot.durable_context(), Some(&paused));
    assert!(matches!(
        snapshot.checkpoint_status(),
        PuzzleCheckpointEnumStatus::AwaitingResume(_)
    ));
    assert!(
        fixture
            .execute(
                PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
                10000
            )
            .is_err()
    );
    fixture.execute(
        PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
        10000,
    )?;
    assert_eq!(
        fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(10000)?)?
            .timer()
            .elapsed(),
        ClockDuration::from_millis(1000)?
    );
    Ok(())
}

#[tokio::test]
async fn cancelled_wait_keeps_one_lower_write_handle_and_the_same_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    fixture
        .owner
        .begin_write(DesktopPuzzleCommandsFixture::time(0)?)?;
    let token = fixture
        .owner
        .storage
        .pending_request()
        .ok_or("Missing retained request")?
        .token()
        .clone();
    let mut waiting = Box::pin(fixture.owner.wait());
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert!(fixture.owner.has_retained_write());
    assert_eq!(
        fixture
            .owner
            .storage
            .pending_request()
            .ok_or("Request lost after cancellation")?
            .token(),
        &token
    );
    assert_eq!(fixture.begun.load(Ordering::Relaxed), 1);
    fixture.complete(0).await?;
    assert!(!fixture.owner.has_retained_write());
    Ok(())
}

#[tokio::test]
async fn failed_save_retries_exact_payload_then_saves_the_newest_paused_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    fixture.start().await?;
    fixture.execute(PuzzlePersistenceEnumCommand::Hint, 100)?;
    let pending = fixture.requests.try_recv()?;
    let original = pending.request.clone();
    let original_bytes = PuzzlePersistenceCodec::encode(original.snapshot())?;
    pending.respond(Err(DesktopPuzzleCommandsFixtureError::Commit))?;
    fixture.owner.wait().await?;
    let error = fixture
        .owner
        .settle(DesktopPuzzleCommandsFixture::time(100)?)
        .err()
        .ok_or("Expected typed storage failure")?;
    assert!(matches!(
        &error,
        DesktopPuzzleCommandsError::Storage(PuzzleStorageDriverError::Repository(
            DesktopPuzzleCommandsFixtureError::Commit
        ))
    ));
    assert!(std::error::Error::source(&error).is_some());
    let paused = fixture.owner.context();
    assert!(!fixture.owner.has_retained_write());
    fixture
        .owner
        .retry_save(&paused, DesktopPuzzleCommandsFixture::time(100)?)?;
    let retry = fixture.requests.try_recv()?;
    assert_ne!(retry.request.token().id(), original.token().id());
    assert_eq!(retry.request.token().context(), original.token().context());
    assert_eq!(
        retry.request.expected_storage_revision(),
        original.expected_storage_revision()
    );
    assert_eq!(
        PuzzlePersistenceCodec::encode(retry.request.snapshot())?,
        original_bytes
    );
    retry.commit()?;
    fixture.owner.wait().await?;
    fixture
        .owner
        .settle(DesktopPuzzleCommandsFixture::time(100)?)?;
    assert!(
        fixture
            .execute(
                PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
                100
            )
            .is_err()
    );
    fixture.complete(100).await?;
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(100)?)?;
    assert_eq!(snapshot.durable_context(), Some(&paused));
    assert!(snapshot.attempt().is_none());
    assert_eq!(
        fixture
            .owner
            .summary(DesktopPuzzleCommandsFixture::time(100)?)?
            .hints(),
        1
    );
    fixture.execute(
        PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
        100,
    )?;
    Ok(())
}

#[tokio::test]
async fn full_session_attempt_definition_and_revision_correlation_rejects_stale_actions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    fixture.start().await?;
    let context = fixture.owner.context();
    let identity = PuzzleDefinitionIdentity::new("command-owner".into(), "v1".into(), "0".into())?;
    let other = NonZeroU64::new(2).ok_or("Missing fixture identifier")?;
    let stale = [
        PuzzleSessionContext::new(
            PuzzleSessionId::new(other),
            PuzzleAttemptId::new(NonZeroU64::MIN),
            identity.clone(),
            context.revision(),
        ),
        PuzzleSessionContext::new(
            context.session_id(),
            PuzzleAttemptId::new(other),
            identity.clone(),
            context.revision(),
        ),
        PuzzleSessionContext::new(
            context.session_id(),
            PuzzleAttemptId::new(NonZeroU64::MIN),
            PuzzleDefinitionIdentity::new("command-owner".into(), "v2".into(), "0".into())?,
            context.revision(),
        ),
        PuzzleSessionContext::new(
            context.session_id(),
            PuzzleAttemptId::new(NonZeroU64::MIN),
            PuzzleDefinitionIdentity::new("command-owner".into(), "v1".into(), "foreign".into())?,
            context.revision(),
        ),
        PuzzleSessionContext::new(
            context.session_id(),
            PuzzleAttemptId::new(NonZeroU64::MIN),
            identity,
            PuzzleSessionRevision::ZERO,
        ),
    ];
    let events = fixture.owner.run.event_count();
    for expected in stale {
        assert!(matches!(
            fixture.owner.execute(
                &expected,
                PuzzlePersistenceEnumCommand::Hint,
                DesktopPuzzleCommandsFixture::time(0)?
            ),
            Err(DesktopPuzzleCommandsError::StaleContext { .. })
        ));
        assert_eq!(fixture.owner.context(), context);
        assert_eq!(fixture.owner.run.event_count(), events);
    }
    fixture.execute(PuzzlePersistenceEnumCommand::Hint, 0)?;
    assert!(matches!(
        fixture.owner.execute(
            &context,
            PuzzlePersistenceEnumCommand::Hint,
            DesktopPuzzleCommandsFixture::time(0)?
        ),
        Err(DesktopPuzzleCommandsError::StaleContext { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn old_receipt_never_replaces_the_newer_attempt_and_duplicate_receipt_stays_retained()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(2, &["e2e4"])?;
    fixture.start().await?;
    let old = fixture.requests.try_recv()?;
    let old_request = old.request.clone();
    fixture.execute(
        PuzzlePersistenceEnumCommand::Move(BoardPosition::standard().parse_uci("e2e4")?),
        0,
    )?;
    fixture.execute(PuzzlePersistenceEnumCommand::Next, 0)?;
    let newer = fixture.owner.context();
    old.commit()?;
    fixture.owner.wait().await?;
    fixture
        .owner
        .settle(DesktopPuzzleCommandsFixture::time(0)?)?;
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(0)?)?;
    assert_eq!(snapshot.context(), &newer);
    assert_ne!(snapshot.durable_context(), Some(&newer));
    assert_eq!(snapshot.archived_attempts(), 1);
    let pending = fixture.requests.try_recv()?;
    pending.respond(Ok(PuzzlePersistenceReceipt::committed(
        old_request,
        PuzzlePersistenceStorageRevision::new(2),
    )))?;
    fixture.owner.wait().await?;
    assert!(
        fixture
            .owner
            .settle(DesktopPuzzleCommandsFixture::time(0)?)
            .is_err()
    );
    assert!(fixture.owner.has_retained_write());
    assert_eq!(fixture.owner.context(), newer);
    Ok(())
}

#[tokio::test]
async fn focus_sleep_hide_and_manual_pause_capture_immediately_and_ready_never_resumes()
-> Result<(), Box<dyn std::error::Error>> {
    for reason in [
        PuzzleTimerEnumPauseReason::FocusLost,
        PuzzleTimerEnumPauseReason::Sleep,
        PuzzleTimerEnumPauseReason::WindowHidden,
        PuzzleTimerEnumPauseReason::Manual,
    ] {
        let mut fixture = DesktopPuzzleCommandsFixture::new(
            PuzzleEnumCountdownMode::PerPuzzle,
            DesktopPuzzleCommandsFixture::RUN_DURATION_MILLIS,
            1,
            &["e2e4"],
        )?;
        fixture.start().await?;
        fixture.finish_writes(0).await?;
        fixture.execute(PuzzlePersistenceEnumCommand::Pause(reason), 10)?;
        assert_eq!(fixture.requests.len(), 1);
        let snapshot = fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(10000)?)?;
        assert!(
            matches!(snapshot.status(), PuzzleRunEnumStatus::Paused { reason: actual, .. } if actual == reason)
        );
        assert!(snapshot.attempt().is_none());
        assert_eq!(snapshot.timer().elapsed(), ClockDuration::from_millis(10)?);
        fixture.finish_writes(10000).await?;
        fixture.execute(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
            10000,
        )?;
        assert!(
            fixture
                .owner
                .snapshot(DesktopPuzzleCommandsFixture::time(10000)?)?
                .attempt()
                .is_none()
        );
        fixture.execute(
            PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
            10000,
        )?;
        assert_eq!(
            fixture
                .owner
                .snapshot(DesktopPuzzleCommandsFixture::time(10000)?)?
                .status(),
            PuzzleRunEnumStatus::Active
        );
    }
    Ok(())
}

#[tokio::test]
async fn real_hints_practice_replies_review_reshuffle_and_end_preserve_primary_outcomes()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4", "e7e5", "g1f3"])?;
    fixture.start().await?;
    fixture.finish_writes(0).await?;
    fixture.execute(
        PuzzlePersistenceEnumCommand::Move(BoardPosition::standard().parse_uci("d2d4")?),
        1,
    )?;
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(1)?)?;
    let attempt = snapshot.attempt().ok_or("Missing failed attempt")?;
    assert_eq!(attempt.phase(), PuzzleEnumPhase::PracticeReady);
    assert_eq!(
        attempt.primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Failed)
    );
    for (millis, command) in [
        (2, PuzzlePersistenceEnumCommand::RetryPractice),
        (3, PuzzlePersistenceEnumCommand::Hint),
        (
            4,
            PuzzlePersistenceEnumCommand::Move(BoardPosition::standard().parse_uci("e2e4")?),
        ),
        (5, PuzzlePersistenceEnumCommand::OpponentReply),
        (6, PuzzlePersistenceEnumCommand::Hint),
        (
            7,
            PuzzlePersistenceEnumCommand::Move(
                BoardPosition::standard()
                    .play_uci("e2e4")?
                    .play_uci("e7e5")?
                    .parse_uci("g1f3")?,
            ),
        ),
    ] {
        fixture.finish_writes(millis).await?;
        fixture.execute(command, millis)?;
    }
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(7)?)?;
    let attempt = snapshot.attempt().ok_or("Missing review")?;
    assert_eq!(attempt.phase(), PuzzleEnumPhase::Review);
    assert_eq!(attempt.practice(), PuzzleEnumPractice::Completed);
    assert_eq!(attempt.assistance(), PuzzleEnumAssistance::Assisted);
    assert_eq!(
        attempt.primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Failed)
    );
    let summary = fixture
        .owner
        .summary(DesktopPuzzleCommandsFixture::time(7)?)?;
    assert_eq!(summary.finalized(), 1);
    assert_eq!(summary.practice_completions(), 1);
    assert_eq!(summary.hints(), 2);
    fixture.finish_writes(8).await?;
    fixture.execute(PuzzlePersistenceEnumCommand::Next, 8)?;
    assert_eq!(
        fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(8)?)?
            .status(),
        PuzzleRunEnumStatus::Exhausted
    );
    fixture.execute(PuzzlePersistenceEnumCommand::Reshuffle, 9)?;
    fixture.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        10,
    )?;
    assert!(
        fixture
            .execute(
                PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Unconfirmed),
                11
            )
            .is_err()
    );
    fixture.execute(
        PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed),
        11,
    )?;
    assert_eq!(
        fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(11)?)?
            .status(),
        PuzzleRunEnumStatus::Ended(PuzzleRunEnumEndReason::Confirmed)
    );
    let summary = fixture
        .owner
        .summary(DesktopPuzzleCommandsFixture::time(11)?)?;
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Failed), 1);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Skipped), 1);
    Ok(())
}

#[tokio::test]
async fn expiry_deadline_precedes_cadence_and_snapshot_applies_each_mode_once()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        let mut fixture = DesktopPuzzleCommandsFixture::new(mode, 100, 2, &["e2e4"])?;
        fixture.start().await?;
        assert_eq!(
            fixture.owner.next_deadline()?,
            Some(DesktopPuzzleCommandsFixture::time(100)?)
        );
        let snapshot = fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(100)?)?;
        let after = snapshot.context().clone();
        match mode {
            PuzzleEnumCountdownMode::Session => assert_eq!(
                snapshot.status(),
                PuzzleRunEnumStatus::Ended(PuzzleRunEnumEndReason::SessionExpired)
            ),
            PuzzleEnumCountdownMode::PerPuzzle => {
                assert_eq!(snapshot.status(), PuzzleRunEnumStatus::AwaitingReadiness);
                assert_eq!(snapshot.archived_attempts(), 1);
            }
        }
        let again = fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(1000)?)?;
        assert_eq!(again.context(), &after);
        assert_eq!(again.timer().elapsed(), ClockDuration::from_millis(100)?);
        assert_eq!(
            fixture
                .owner
                .summary(DesktopPuzzleCommandsFixture::time(1000)?)?
                .primary_count(PuzzleEnumPrimaryOutcome::TimedOut),
            1
        );
    }
    Ok(())
}

#[tokio::test]
async fn rejected_storage_admission_still_freezes_and_retains_the_original_request()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    fixture
        .owner
        .begin_write(DesktopPuzzleCommandsFixture::time(0)?)?;
    fixture.finish_writes(0).await?;
    fixture.requests.close();
    assert!(matches!(
        fixture.execute(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
            0
        ),
        Err(DesktopPuzzleCommandsError::Storage(
            PuzzleStorageDriverError::Repository(DesktopPuzzleCommandsFixtureError::Admission)
        ))
    ));
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(0)?)?;
    assert!(snapshot.attempt().is_none());
    assert!(matches!(
        snapshot.status(),
        PuzzleRunEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::SaveBackpressure,
            ..
        }
    ));
    assert!(matches!(
        snapshot.checkpoint_status(),
        PuzzleCheckpointEnumStatus::RetryRequired { .. }
    ));
    assert!(fixture.owner.storage.pending_request().is_some());
    assert!(fixture.owner.storage.checkpoint().coalesced().is_some());
    Ok(())
}

#[tokio::test]
async fn late_input_is_guarded_before_its_domain_operation_and_captures_the_boundary_pause()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    fixture.start().await?;
    let context = fixture.owner.context();
    assert!(matches!(
        fixture.owner.execute(
            &context,
            PuzzlePersistenceEnumCommand::Hint,
            DesktopPuzzleCommandsFixture::time(9000)?
        ),
        Err(DesktopPuzzleCommandsError::StaleContext { .. })
    ));
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(9000)?)?;
    assert_eq!(
        snapshot.timer().elapsed(),
        ClockDuration::from_millis(1000)?
    );
    assert!(snapshot.attempt().is_none());
    assert_eq!(
        fixture
            .owner
            .summary(DesktopPuzzleCommandsFixture::time(9000)?)?
            .hints(),
        0
    );
    assert_eq!(
        fixture
            .owner
            .storage
            .checkpoint()
            .coalesced()
            .ok_or("Missing guarded pause")?
            .observation()
            .context(),
        snapshot.context()
    );
    Ok(())
}

#[tokio::test]
async fn late_completion_uses_fresh_settlement_time_and_cannot_erase_a_missed_freeze()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    fixture.start().await?;
    fixture.requests.try_recv()?.commit()?;
    fixture.owner.wait().await?;
    fixture
        .owner
        .settle(DesktopPuzzleCommandsFixture::time(9000)?)?;
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(9000)?)?;
    assert_eq!(
        snapshot.timer().elapsed(),
        ClockDuration::from_millis(1000)?
    );
    assert!(snapshot.attempt().is_none());
    assert!(snapshot.has_retained_write());
    assert!(
        matches!(snapshot.checkpoint_status(), PuzzleCheckpointEnumStatus::Frozen(freeze) if freeze.pause_at() == DesktopPuzzleCommandsFixture::time(1000)?)
    );
    Ok(())
}

#[tokio::test]
async fn failed_initial_write_requires_resume_after_exact_retry_and_newest_pause_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    fixture
        .owner
        .begin_write(DesktopPuzzleCommandsFixture::time(0)?)?;
    fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(9000)?)?;
    fixture
        .requests
        .try_recv()?
        .respond(Err(DesktopPuzzleCommandsFixtureError::Commit))?;
    fixture.owner.wait().await?;
    assert!(
        fixture
            .owner
            .settle(DesktopPuzzleCommandsFixture::time(9000)?)
            .is_err()
    );
    fixture.owner.retry_save(
        &fixture.owner.context(),
        DesktopPuzzleCommandsFixture::time(9000)?,
    )?;
    fixture.finish_writes(9000).await?;
    assert!(matches!(
        fixture.execute(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
            9000
        ),
        Err(DesktopPuzzleCommandsError::SaveRequired(_))
    ));
    assert!(
        fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(9000)?)?
            .attempt()
            .is_none()
    );
    fixture.execute(
        PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
        9000,
    )?;
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(9000)?)?;
    assert_eq!(snapshot.status(), PuzzleRunEnumStatus::Active);
    assert_eq!(snapshot.timer().elapsed(), ClockDuration::ZERO);
    Ok(())
}

#[tokio::test]
async fn countdown_read_model_reuses_the_immutable_attempt_and_invalidates_on_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4"])?;
    fixture.start().await?;
    let first = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(0)?)?;
    let tick = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(250)?)?;
    assert!(std::ptr::eq(
        first.attempt().ok_or("Missing initial board")?,
        tick.attempt().ok_or("Missing countdown board")?
    ));
    assert_eq!(first.timer().elapsed(), ClockDuration::ZERO);
    assert_eq!(tick.timer().elapsed(), ClockDuration::from_millis(250)?);
    fixture.execute(PuzzlePersistenceEnumCommand::Hint, 250)?;
    let hint = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(250)?)?;
    assert!(!std::ptr::eq(
        first.attempt().ok_or("Missing retained board")?,
        hint.attempt().ok_or("Missing hint board")?
    ));
    assert_eq!(
        hint.attempt().ok_or("Missing current hint")?.assistance(),
        PuzzleEnumAssistance::Assisted
    );
    assert_eq!(
        first
            .attempt()
            .ok_or("Missing immutable prior board")?
            .assistance(),
        PuzzleEnumAssistance::Unassisted
    );
    Ok(())
}

#[tokio::test]
async fn illegal_move_keeps_real_position_and_primary_outcome_while_countdown_continues()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopPuzzleCommandsFixture::session(1, &["e2e4", "e7e5", "g1f3"])?;
    fixture.start().await?;
    let movement = BoardPosition::standard().parse_uci("e2e4")?;
    fixture.execute(PuzzlePersistenceEnumCommand::Move(movement), 0)?;
    fixture.execute(PuzzlePersistenceEnumCommand::OpponentReply, 0)?;
    let before = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(0)?)?;
    let events = fixture.owner.run.event_count();
    assert!(
        fixture
            .execute(PuzzlePersistenceEnumCommand::Move(movement), 100)
            .is_err()
    );
    let after = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(100)?)?;
    assert_eq!(before.context(), after.context());
    assert_eq!(
        before
            .attempt()
            .ok_or("Missing prior position")?
            .board()
            .fen(),
        after
            .attempt()
            .ok_or("Missing current position")?
            .board()
            .fen()
    );
    assert_eq!(
        after
            .attempt()
            .ok_or("Missing rejected-input outcome")?
            .primary_outcome(),
        None
    );
    assert_eq!(after.timer().elapsed(), ClockDuration::from_millis(100)?);
    assert_eq!(fixture.owner.run.event_count(), events);
    Ok(())
}

#[tokio::test]
async fn exact_deadline_focus_loss_pauses_and_hides_the_next_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture =
        DesktopPuzzleCommandsFixture::new(PuzzleEnumCountdownMode::PerPuzzle, 100, 2, &["e2e4"])?;
    fixture.start().await?;
    fixture.finish_writes(0).await?;
    fixture.execute(
        PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::FocusLost),
        100,
    )?;
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(100)?)?;
    assert_eq!(
        snapshot.status(),
        PuzzleRunEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::FocusLost,
            phase: PuzzleTimerEnumPhase::AwaitingReadiness,
        }
    );
    assert!(snapshot.attempt().is_none());
    assert_eq!(snapshot.archived_attempts(), 1);
    fixture.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        100,
    )?;
    assert!(
        fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(1000)?)?
            .attempt()
            .is_none()
    );
    assert_eq!(
        fixture
            .owner
            .summary(DesktopPuzzleCommandsFixture::time(1000)?)?
            .primary_count(PuzzleEnumPrimaryOutcome::TimedOut),
        1
    );
    Ok(())
}

#[tokio::test]
async fn exact_deadline_hidden_readiness_pauses_and_hides_the_next_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture =
        DesktopPuzzleCommandsFixture::new(PuzzleEnumCountdownMode::PerPuzzle, 100, 2, &["e2e4"])?;
    fixture.start().await?;
    fixture.finish_writes(0).await?;
    fixture.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Hidden),
        100,
    )?;
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(100)?)?;
    assert_eq!(
        snapshot.status(),
        PuzzleRunEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::WindowHidden,
            phase: PuzzleTimerEnumPhase::AwaitingReadiness,
        }
    );
    assert!(snapshot.attempt().is_none());
    assert_eq!(snapshot.archived_attempts(), 1);
    assert_eq!(snapshot.timer().elapsed(), ClockDuration::from_millis(100)?);
    fixture.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        1000,
    )?;
    assert!(
        fixture
            .owner
            .snapshot(DesktopPuzzleCommandsFixture::time(1000)?)?
            .attempt()
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn exact_deadline_confirmed_end_finalizes_without_advancing_to_an_open_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture =
        DesktopPuzzleCommandsFixture::new(PuzzleEnumCountdownMode::PerPuzzle, 100, 2, &["e2e4"])?;
    fixture.start().await?;
    fixture.finish_writes(0).await?;
    fixture.execute(
        PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed),
        100,
    )?;
    let snapshot = fixture
        .owner
        .snapshot(DesktopPuzzleCommandsFixture::time(1000)?)?;
    assert_eq!(
        snapshot.status(),
        PuzzleRunEnumStatus::Ended(PuzzleRunEnumEndReason::Confirmed)
    );
    assert!(snapshot.attempt().is_none());
    assert_eq!(snapshot.archived_attempts(), 0);
    assert_eq!(snapshot.timer().elapsed(), ClockDuration::from_millis(100)?);
    let summary = fixture
        .owner
        .summary(DesktopPuzzleCommandsFixture::time(1000)?)?;
    assert_eq!(summary.finalized(), 1);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::TimedOut), 1);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Skipped), 0);
    Ok(())
}
