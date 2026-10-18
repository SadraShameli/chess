use std::cell::Cell;
use std::mem::size_of;

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::BoardStart;
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::clock::state::{ClockState, ClockStateEnumStatus};
use crate::clock::time_control::ClockTimeControl;
use crate::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationMateProofProvider,
};
use crate::game::enums::{
    GameEnumEngineReadiness, GameEnumMode, GameEnumOutcomeReason, GameEnumPhase, GameEnumResult,
    GameEnumSeat, GameEnumTransitionKind, GameRevision, GameRevisionError,
};
use crate::game::pause::{
    GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseError, GamePauseRequest,
    GameResumeRequest,
};
use crate::game::premove::GamePremoveIntent;
use crate::game::save::{GameSaveEnumAcknowledgment, GameSaveEnumStatus, GameSaveFailureRequest};
use crate::game::session::lifecycle::{
    GameSessionLifecycleEnumCommit, GameSessionLifecycleEnumContentChange,
    GameSessionLifecycleEnumRequest, GameSessionLifecyclePendingResult,
    GameSessionLifecyclePrepared,
};
use crate::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionEnumInterruption, GameSessionEnumState,
    GameSessionError, GameSessionMoveRequest, GameSessionOptions,
};
use crate::game::storage_repository::{
    GameStorageReceipt, GameStorageRevision, GameStorageSaveRequest,
};
use crate::game::takeback::{GameTakebackEnumConsent, GameTakebackRequest};
use crate::storage::game_document::{StorageGameDocument, StorageGameDocumentError};

type GameSessionLifecycleTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct GameSessionLifecycleFixture;

impl GameAdjudicationMateProofProvider for GameSessionLifecycleFixture {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

impl GameSessionLifecycleFixture {
    fn session(control: ClockTimeControl) -> GameSessionLifecycleTestResult<GameSession> {
        Self::with_seats(
            control,
            GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
        )
    }

    fn with_seats(
        control: ClockTimeControl,
        seats: GameSeats,
    ) -> GameSessionLifecycleTestResult<GameSession> {
        let mut session = GameSession::new(GameSessionOptions {
            id: uuid::Uuid::from_u128(100).to_string().parse()?,
            root: uuid::Uuid::from_u128(1).to_string().parse()?,
            start: BoardStart::standard(),
            seats,
            clock: ClockState::new(control, BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?;
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::Ready,
            &Self,
        )?;
        Ok(session)
    }

    fn timed() -> GameSessionLifecycleTestResult<GameSession> {
        Self::session(ClockTimeControl::sudden_death(ClockDuration::from_millis(
            1_000,
        )?)?)
    }

    fn pause(
        session: &GameSession,
        millis: u64,
    ) -> GameSessionLifecycleTestResult<GamePauseRequest> {
        Ok(GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(millis)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        })
    }

    fn deadline(
        session: &GameSession,
        millis: u64,
    ) -> GameSessionLifecycleTestResult<GameSessionLifecycleEnumRequest> {
        Ok(GameSessionLifecycleEnumRequest::PollDeadline {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(millis)?,
        })
    }

    fn defer(
        session: &mut GameSession,
    ) -> GameSessionLifecycleTestResult<GameSessionLifecyclePendingResult> {
        let prepared = session.prepare_lifecycle(Self::deadline(session, 1_000)?, &Self)?;
        let (_, _, pending) = session
            .commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult)?
            .into_parts();
        Ok(pending.ok_or("A deferred terminal result must return its retry token")?)
    }

    fn play(session: &mut GameSession, uci: &str) -> GameSessionLifecycleTestResult {
        let position = session.tree().node(session.live())?.position();
        session.accept_move(
            GameSessionMoveRequest {
                expected_revision: session.revision(),
                seat: session.seats().seat(position.turn()),
                color: position.turn(),
                node: uuid::Uuid::from_u128(2).to_string().parse()?,
                movement: position.parse_uci(uci)?,
                received_at: ClockInstant::from_millis(100)?,
            },
            &Self,
        )?;
        Ok(())
    }
}

struct GameSessionLifecycleProof {
    calls: Cell<usize>,
    answer: GameAdjudicationEnumMateProof,
}

impl GameSessionLifecycleProof {
    fn new(answer: GameAdjudicationEnumMateProof) -> Self {
        Self {
            calls: Cell::new(0),
            answer,
        }
    }
}

impl GameAdjudicationMateProofProvider for GameSessionLifecycleProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        assert_eq!(
            question,
            GameAdjudicationEnumMateQuestion::Side(BoardEnumColor::Black)
        );
        self.calls.set(self.calls.get() + 1);
        self.answer
    }
}

#[test]
fn checkpoint_pause_prepares_without_mutation_and_accepts_without_result()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::session(ClockTimeControl::initial_default())?;
    let clock = session.clock().clone();
    let revision = session.revision();
    let prepared = session.prepare_lifecycle(
        GameSessionLifecycleEnumRequest::Pause(GameSessionLifecycleFixture::pause(
            &session, 1_000,
        )?),
        &GameSessionLifecycleFixture,
    )?;
    assert_eq!(
        prepared.content_change(),
        GameSessionLifecycleEnumContentChange::Checkpoint
    );
    assert_eq!(session.clock(), &clock);
    assert_eq!(session.revision(), revision);
    assert_eq!(session.phase(), GameEnumPhase::Active);
    let committed = session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
    let transition = committed
        .transition()
        .ok_or("A pause must return a transition")?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::Interrupted);
    assert_eq!(transition.previous_revision(), revision);
    assert_eq!(transition.revision(), revision.next()?);
    assert_eq!(
        transition.clock().elapsed(),
        ClockDuration::from_millis(1_000)?
    );
    assert_eq!(session.phase(), GameEnumPhase::Interrupted);
    assert_eq!(session.outcome(), None);
    assert!(committed.pending_result().is_none());
    assert_eq!(session.save_status(), GameSaveEnumStatus::Current);
    Ok(())
}

#[test]
fn terminal_preparation_proves_once_without_accepting_or_stopping_the_original_clock()
-> GameSessionLifecycleTestResult {
    for pause in [false, true] {
        let session = GameSessionLifecycleFixture::timed()?;
        let clock = session.clock().clone();
        let proof = GameSessionLifecycleProof::new(GameAdjudicationEnumMateProof::Possible);
        let request = if pause {
            GameSessionLifecycleEnumRequest::Pause(GameSessionLifecycleFixture::pause(
                &session, 1_000,
            )?)
        } else {
            GameSessionLifecycleFixture::deadline(&session, 1_000)?
        };
        let prepared = session.prepare_lifecycle(request, &proof)?;
        assert_eq!(
            prepared.content_change(),
            GameSessionLifecycleEnumContentChange::Result
        );
        assert_eq!(proof.calls.get(), 1);
        assert_eq!(session.clock(), &clock);
        assert_eq!(session.phase(), GameEnumPhase::Active);
        assert_eq!(session.outcome(), None);
        assert_eq!(session.revision(), GameRevision::ZERO.next()?);
    }
    Ok(())
}

#[test]
fn deferred_result_stops_the_original_deadline_and_exposes_only_a_required_pending_state()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let mut expected = session.clock().clone();
    let original = ClockInstant::from_millis(1_000)?;
    expected.poll(original)?;
    let revision = session.revision();
    let prepared = session.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&session, 1_000)?,
        &GameSessionLifecycleFixture,
    )?;
    let committed =
        session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult)?;
    let transition = committed
        .transition()
        .ok_or("Deferral must stop the live game")?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::Interrupted);
    assert_eq!(transition.outcome(), None);
    assert_eq!(
        transition.interruption(),
        Some(GameSessionEnumInterruption::PendingResult)
    );
    assert_eq!(session.clock(), &expected);
    assert_eq!(
        session.clock().status(),
        ClockStateEnumStatus::Flagged(BoardEnumColor::White)
    );
    assert_eq!(session.revision(), revision.next()?);
    assert_eq!(session.phase(), GameEnumPhase::Interrupted);
    assert_eq!(session.outcome(), None);
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    let pending = committed
        .pending_result()
        .ok_or("Deferral must retain a retry token")?;
    assert_eq!(pending.game(), session.id());
    assert_eq!(pending.revision(), session.revision());
    assert_eq!(pending.live(), session.live());
    assert_eq!(pending.observed_at(), original);
    Ok(())
}

#[test]
fn pending_result_refuses_capture_resume_takeback_and_content_edits()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let pending = GameSessionLifecycleFixture::defer(&mut session)?;
    let now = ClockInstant::from_millis(50_000)?;
    let clock = session.clock().clone();
    let revision = session.revision();
    assert!(matches!(
        session.restoration(now),
        Err(GameSessionError::PendingResult)
    ));
    assert!(matches!(
        GameStorageSaveRequest::capture(&session, now, GameStorageRevision::INITIAL),
        Err(GameSessionError::PendingResult)
    ));
    assert!(matches!(
        StorageGameDocument::capture(&session, now),
        Err(StorageGameDocumentError::Session(
            GameSessionError::PendingResult
        ))
    ));
    assert!(matches!(
        session.resume(
            GameResumeRequest {
                expected_revision: revision,
                received_at: now,
                readiness: GameEnumEngineReadiness::NotRequired
            },
            &GameSessionLifecycleFixture,
        ),
        Err(GameSessionError::PendingResult)
    ));
    assert!(matches!(
        session.takeback_target(),
        Err(GameSessionError::PendingResult)
    ));
    assert!(matches!(
        session.save_notes_and_nags(revision, session.live(), "changed".to_owned(), Vec::new()),
        Err(GameSessionError::PendingResult)
    ));
    assert!(matches!(
        session.configure_takeback_consent(revision, GameTakebackEnumConsent::NotRequired),
        Err(GameSessionError::PendingResult)
    ));
    assert_eq!(session.clock(), &clock);
    assert_eq!(session.revision(), revision);
    assert_eq!(session.outcome(), None);
    session.prepare_pending_result(&pending, now)?;
    Ok(())
}

#[test]
fn retry_accepts_the_retained_result_without_proof_replay_or_downtime()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let proof = GameSessionLifecycleProof::new(GameAdjudicationEnumMateProof::Possible);
    let prepared = session.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&session, 1_000)?,
        &proof,
    )?;
    let (_, _, pending) = session
        .commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult)?
        .into_parts();
    let pending = pending.ok_or("Deferral must retain its result")?;
    let stopped = session.clock().clone();
    let revision = session.revision();
    let prepared = session.prepare_pending_result(&pending, ClockInstant::from_millis(90_000)?)?;
    assert_eq!(
        prepared.content_change(),
        GameSessionLifecycleEnumContentChange::Result
    );
    assert_eq!(session.outcome(), None);
    assert_eq!(session.revision(), revision);
    let committed = session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
    let transition = committed
        .transition()
        .ok_or("Retry must accept the prepared result")?;
    let outcome = session
        .outcome()
        .ok_or("Retry must expose the accepted outcome")?;
    assert_eq!(
        outcome.result(),
        GameEnumResult::Victory(BoardEnumColor::Black)
    );
    assert_eq!(outcome.reason(), GameEnumOutcomeReason::Flagfall);
    assert_eq!(outcome.origin(), pending.live());
    assert_eq!(transition.kind(), GameEnumTransitionKind::ResultAccepted);
    assert_eq!(transition.revision(), revision.next()?);
    assert_eq!(session.clock(), &stopped);
    assert_eq!(proof.calls.get(), 1);
    assert!(committed.pending_result().is_none());
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    assert!(matches!(
        session.prepare_pending_result(&pending, ClockInstant::from_millis(100_000)?),
        Err(GameSessionError::Revision(GameRevisionError::Stale { .. }))
    ));
    let current = GameStorageReceipt::new(
        session.id(),
        session.revision(),
        GameStorageRevision::new(1),
    );
    assert_eq!(
        session.acknowledge_save(current)?,
        GameSaveEnumAcknowledgment::Current
    );
    assert_eq!(session.save_status(), GameSaveEnumStatus::Current);
    let json =
        StorageGameDocument::capture(&session, ClockInstant::from_millis(100_000)?)?.json()?;
    let restored = StorageGameDocument::restore(&json)?;
    assert_eq!(restored.outcome(), Some(outcome));
    assert_eq!(
        restored
            .clock()
            .snapshot(ClockInstant::from_millis(200_000)?)?,
        stopped.snapshot(ClockInstant::from_millis(100_000)?)?
    );
    Ok(())
}

#[test]
fn foreign_and_stale_preparations_are_rejected_before_mutation() -> GameSessionLifecycleTestResult {
    let source = GameSessionLifecycleFixture::timed()?;
    let mut destination = GameSessionLifecycleFixture::timed()?;
    destination.id = uuid::Uuid::from_u128(200).to_string().parse()?;
    let original = destination.clock().clone();
    let revision = destination.revision();
    let prepared = source.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&source, 1_000)?,
        &GameSessionLifecycleFixture,
    )?;
    assert!(matches!(
        destination.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult),
        Err(GameSessionError::ForeignLifecyclePreparation { expected, received }) if expected == destination.id() && received == source.id()
    ));
    assert_eq!(destination.clock(), &original);
    assert_eq!(destination.revision(), revision);
    assert_eq!(destination.save_status(), GameSaveEnumStatus::Current);
    let prepared = destination.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&destination, 1_000)?,
        &GameSessionLifecycleFixture,
    )?;
    destination.set_premoves_enabled(revision, true)?;
    let changed_revision = destination.revision();
    assert!(matches!(
        destination.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept),
        Err(GameSessionError::Revision(GameRevisionError::Stale { .. }))
    ));
    assert_eq!(destination.clock(), &original);
    assert_eq!(destination.revision(), changed_revision);
    assert_eq!(destination.phase(), GameEnumPhase::Active);
    Ok(())
}

#[test]
fn an_intervening_clock_observation_invalidates_preparation_even_without_a_revision_change()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let revision = session.revision();
    let prepared = session.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&session, 1_000)?,
        &GameSessionLifecycleFixture,
    )?;
    session.poll_deadline(
        revision,
        ClockInstant::from_millis(100)?,
        &GameSessionLifecycleFixture,
    )?;
    let observed = session.clock().clone();
    assert_eq!(session.revision(), revision);
    assert!(matches!(
        session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept),
        Err(GameSessionError::Revision(GameRevisionError::Stale { .. }))
    ));
    assert_eq!(session.clock(), &observed);
    assert_eq!(session.outcome(), None);
    Ok(())
}

#[test]
fn checkpoint_and_unresolved_proof_cannot_be_deferred_as_results() -> GameSessionLifecycleTestResult
{
    for terminal in [false, true] {
        let mut session = GameSessionLifecycleFixture::timed()?;
        let clock = session.clock().clone();
        let revision = session.revision();
        let proof = GameSessionLifecycleProof::new(GameAdjudicationEnumMateProof::Unknown(
            GameAdjudicationEnumInterruption::Unresolved,
        ));
        let prepared = session.prepare_lifecycle(
            GameSessionLifecycleFixture::deadline(&session, if terminal { 1_000 } else { 100 })?,
            &proof,
        )?;
        assert_eq!(
            prepared.content_change(),
            GameSessionLifecycleEnumContentChange::Checkpoint
        );
        assert!(matches!(
            session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult),
            Err(GameSessionError::LifecycleResultRequired)
        ));
        assert_eq!(session.clock(), &clock);
        assert_eq!(session.revision(), revision);
        assert_eq!(session.save_status(), GameSaveEnumStatus::Current);
    }
    Ok(())
}

#[test]
fn pending_safety_observations_and_navigation_leave_the_retry_token_valid()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    GameSessionLifecycleFixture::play(&mut session, "e2e4")?;
    let prepared = session.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&session, 1_100)?,
        &GameSessionLifecycleFixture,
    )?;
    let (_, _, pending) = session
        .commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult)?
        .into_parts();
    let pending = pending.ok_or("Deferral must retain its result")?;
    let revision = session.revision();
    let clock = session.clock().clone();
    let root = session.tree().root();
    session.select_view(root)?;
    assert_eq!(session.view(), root);
    for millis in [10_000, 20_000] {
        let prepared = session.prepare_lifecycle(
            GameSessionLifecycleFixture::deadline(&session, millis)?,
            &GameSessionLifecycleFixture,
        )?;
        assert_eq!(
            prepared.content_change(),
            GameSessionLifecycleEnumContentChange::Checkpoint
        );
        let committed =
            session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
        assert!(committed.transition().is_none());
        assert!(committed.pending_result().is_none());
        let transition = session.pause(
            GameSessionLifecycleFixture::pause(&session, millis)?,
            &GameSessionLifecycleFixture,
        )?;
        assert_eq!(transition.revision(), revision);
        assert_eq!(transition.outcome(), None);
        session.interrupt_for_save_failure(
            GameSaveFailureRequest {
                expected_revision: revision,
                received_at: ClockInstant::from_millis(millis)?,
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &GameSessionLifecycleFixture,
        )?;
        assert_eq!(session.revision(), revision);
        assert_eq!(session.clock(), &clock);
        assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    }
    session.return_to_live();
    let prepared = session.prepare_pending_result(&pending, ClockInstant::from_millis(30_000)?)?;
    session.select_view(root)?;
    session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert_eq!(session.view(), root);
    assert_eq!(session.clock(), &clock);
    Ok(())
}

#[test]
fn queued_receipts_cannot_clear_pending_result_or_the_later_acceptance_save_gate()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let older = GameStorageReceipt::new(
        session.id(),
        session.revision(),
        GameStorageRevision::new(1),
    );
    let pending = GameSessionLifecycleFixture::defer(&mut session)?;
    let pending_receipt = GameStorageReceipt::new(
        session.id(),
        session.revision(),
        GameStorageRevision::new(2),
    );
    assert_eq!(
        session.acknowledge_save(older)?,
        GameSaveEnumAcknowledgment::Older
    );
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    assert!(matches!(
        session.acknowledge_save(pending_receipt),
        Err(GameSessionError::PendingResult)
    ));
    let prepared = session.prepare_pending_result(&pending, ClockInstant::from_millis(50_000)?)?;
    session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
    assert_eq!(
        session.acknowledge_save(pending_receipt)?,
        GameSaveEnumAcknowledgment::Older
    );
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    Ok(())
}

#[test]
fn pending_tokens_refuse_foreign_and_stale_contexts_atomically() -> GameSessionLifecycleTestResult {
    let mut source = GameSessionLifecycleFixture::timed()?;
    let pending = GameSessionLifecycleFixture::defer(&mut source)?;
    let mut destination = GameSessionLifecycleFixture::timed()?;
    destination.id = uuid::Uuid::from_u128(200).to_string().parse()?;
    let original = destination.clock().clone();
    assert!(matches!(
        destination.prepare_pending_result(&pending, ClockInstant::from_millis(50_000)?),
        Err(GameSessionError::ForeignLifecyclePreparation { .. })
    ));
    assert_eq!(destination.clock(), &original);
    let prepared = source.prepare_pending_result(&pending, ClockInstant::from_millis(50_000)?)?;
    source.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
    let accepted = source.clock().clone();
    let revision = source.revision();
    assert!(matches!(
        source.prepare_pending_result(&pending, ClockInstant::from_millis(60_000)?),
        Err(GameSessionError::Revision(GameRevisionError::Stale { .. }))
    ));
    assert_eq!(source.clock(), &accepted);
    assert_eq!(source.revision(), revision);
    Ok(())
}

#[test]
fn pending_marker_without_its_runtime_token_cannot_be_restored_or_decoded()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    session.pause(
        GameSessionLifecycleFixture::pause(&session, 100)?,
        &GameSessionLifecycleFixture,
    )?;
    let mut snapshot = session.restoration(ClockInstant::from_millis(100)?)?;
    snapshot.state = GameSessionEnumState::Interrupted(GameSessionEnumInterruption::PendingResult);
    assert!(matches!(
        GameSession::restore(snapshot),
        Err(GameSessionError::PendingResult)
    ));
    let json = StorageGameDocument::capture(&session, ClockInstant::from_millis(100)?)?.json()?;
    let changed = json.replace("{\"paused\":\"manual\"}", "\"pending_result\"");
    assert_ne!(changed, json);
    assert!(StorageGameDocument::restore(&changed).is_err());
    Ok(())
}

#[test]
fn ordinary_pause_and_deadline_poll_share_the_prepared_acceptance_path()
-> GameSessionLifecycleTestResult {
    for pause in [false, true] {
        for millis in [100, 1_000] {
            let mut direct = GameSessionLifecycleFixture::timed()?;
            let mut prepared_session = GameSessionLifecycleFixture::timed()?;
            let request = if pause {
                GameSessionLifecycleEnumRequest::Pause(GameSessionLifecycleFixture::pause(
                    &prepared_session,
                    millis,
                )?)
            } else {
                GameSessionLifecycleFixture::deadline(&prepared_session, millis)?
            };
            let prepared =
                prepared_session.prepare_lifecycle(request, &GameSessionLifecycleFixture)?;
            let committed = prepared_session
                .commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
            let direct_transition = if pause {
                Some(direct.pause(
                    GameSessionLifecycleFixture::pause(&direct, millis)?,
                    &GameSessionLifecycleFixture,
                )?)
            } else {
                let polled = direct.poll_deadline(
                    direct.revision(),
                    ClockInstant::from_millis(millis)?,
                    &GameSessionLifecycleFixture,
                )?;
                assert_eq!(polled.update(), committed.update());
                polled.into_transition()
            };
            assert_eq!(direct.clock(), prepared_session.clock());
            assert_eq!(direct.state(), prepared_session.state());
            assert_eq!(direct.revision(), prepared_session.revision());
            assert_eq!(
                direct_transition
                    .as_ref()
                    .map(|transition| (transition.kind(), transition.outcome())),
                committed
                    .transition()
                    .map(|transition| (transition.kind(), transition.outcome()))
            );
        }
    }
    Ok(())
}

#[test]
fn unresolved_deadline_acceptance_stays_a_checkpoint_with_the_original_interruption()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let proof = GameSessionLifecycleProof::new(GameAdjudicationEnumMateProof::Unknown(
        GameAdjudicationEnumInterruption::StateLimit,
    ));
    let prepared = session.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&session, 1_000)?,
        &proof,
    )?;
    assert_eq!(
        prepared.content_change(),
        GameSessionLifecycleEnumContentChange::Checkpoint
    );
    let committed = session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
    assert_eq!(proof.calls.get(), 1);
    assert_eq!(session.outcome(), None);
    assert_eq!(
        session.interruption(),
        Some(GameSessionEnumInterruption::Adjudication {
            question: GameAdjudicationEnumMateQuestion::Side(BoardEnumColor::Black),
            cause: GameAdjudicationEnumInterruption::StateLimit,
        })
    );
    assert_eq!(
        committed.transition().map(|transition| transition.kind()),
        Some(GameEnumTransitionKind::FlagFall)
    );
    assert_eq!(session.save_status(), GameSaveEnumStatus::Current);
    Ok(())
}

#[test]
fn failed_preparation_does_not_consume_clock_warnings_or_run_a_proof()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let proof = GameSessionLifecycleProof::new(GameAdjudicationEnumMateProof::Possible);
    session.revision = "18446744073709551615".parse()?;
    let clock = session.clock().clone();
    assert!(matches!(
        session.prepare_lifecycle(
            GameSessionLifecycleFixture::deadline(&session, 1_000)?,
            &proof
        ),
        Err(GameSessionError::Revision(GameRevisionError::Exhausted))
    ));
    assert_eq!(proof.calls.get(), 0);
    assert_eq!(session.clock(), &clock);
    let checkpoint = session.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&session, 100)?,
        &proof,
    )?;
    session.commit_lifecycle(checkpoint, GameSessionLifecycleEnumCommit::Accept)?;
    assert_eq!(session.revision().value(), u64::MAX);
    Ok(())
}

#[test]
fn invalid_pause_barrier_and_reversed_observation_fail_before_mutation()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let clock = session.clock().clone();
    let mut request = GameSessionLifecycleFixture::pause(&session, 100)?;
    request.engine = GamePauseEnumEngineBarrier::FailedEngine;
    assert!(matches!(
        session.prepare_lifecycle(
            GameSessionLifecycleEnumRequest::Pause(request),
            &GameSessionLifecycleFixture
        ),
        Err(GameSessionError::Pause(
            GamePauseError::UnexpectedEngineBarrier
        ))
    ));
    assert_eq!(session.clock(), &clock);
    session.poll_deadline(
        session.revision(),
        ClockInstant::from_millis(100)?,
        &GameSessionLifecycleFixture,
    )?;
    let clock = session.clock().clone();
    assert!(matches!(
        session.prepare_lifecycle(
            GameSessionLifecycleFixture::deadline(&session, 99)?,
            &GameSessionLifecycleFixture
        ),
        Err(GameSessionError::Clock(_))
    ));
    assert_eq!(session.clock(), &clock);
    Ok(())
}

#[test]
fn deferral_clears_takeback_consent_and_queued_premoves_at_the_same_commit_boundary()
-> GameSessionLifecycleTestResult {
    let mut local = GameSessionLifecycleFixture::timed()?;
    GameSessionLifecycleFixture::play(&mut local, "e2e4")?;
    local.request_takeback(
        GameTakebackRequest {
            expected_revision: local.revision(),
            requester: BoardEnumColor::White,
            received_at: ClockInstant::from_millis(200)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameSessionLifecycleFixture,
    )?;
    assert!(local.takeback().pending().is_some());
    let prepared = local.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&local, 1_100)?,
        &GameSessionLifecycleFixture,
    )?;
    local.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult)?;
    assert!(local.takeback().pending().is_none());
    let mut engine = GameSessionLifecycleFixture::with_seats(
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
        GameSeats::new(
            GameEnumMode::HumanVersusEngine,
            GameEnumSeat::Human,
            GameEnumSeat::Reckless,
        )?,
    )?;
    GameSessionLifecycleFixture::play(&mut engine, "e2e4")?;
    engine.set_premoves_enabled(engine.revision(), true)?;
    engine.queue_premove(
        engine.revision(),
        GamePremoveIntent::new("g1".parse()?, "f3".parse()?, None)?,
    )?;
    assert!(engine.premove().pending().is_some());
    let prepared = engine.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&engine, 1_100)?,
        &GameSessionLifecycleFixture,
    )?;
    engine.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult)?;
    assert!(engine.premove().pending().is_none());
    Ok(())
}

#[test]
fn preparation_has_bounded_value_state_and_preserves_the_live_tree()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    GameSessionLifecycleFixture::play(&mut session, "e2e4")?;
    let tree = std::ptr::from_ref(session.tree());
    let live = session.live();
    let root = session.tree().root();
    let position = session.tree().node(live)?.position().clone();
    let nodes = session.tree().len();
    let prepared = session.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&session, 1_100)?,
        &GameSessionLifecycleFixture,
    )?;
    assert!(size_of::<GameSessionLifecyclePrepared>() <= 1_024);
    assert_eq!(std::ptr::from_ref(session.tree()), tree);
    assert_eq!(session.tree().len(), nodes);
    assert_eq!(session.tree().root(), root);
    assert_eq!(session.tree().node(live)?.position(), &position);
    assert_eq!(session.live(), live);
    session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
    assert_eq!(std::ptr::from_ref(session.tree()), tree);
    assert_eq!(session.tree().len(), nodes);
    Ok(())
}

#[test]
fn prepared_clock_warnings_are_delivered_once_only_when_the_candidate_is_accepted()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::session(ClockTimeControl::sudden_death(
        ClockDuration::from_millis(15_000)?,
    )?)?;
    let prepared = session.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&session, 6_000)?,
        &GameSessionLifecycleFixture,
    )?;
    assert!(!session.clock().warning_used(BoardEnumColor::White));
    let committed = session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
    assert_eq!(
        committed.update().new_warning(),
        Some(BoardEnumColor::White)
    );
    assert!(session.clock().warning_used(BoardEnumColor::White));
    assert!(
        session
            .poll_deadline(
                session.revision(),
                ClockInstant::from_millis(7_000)?,
                &GameSessionLifecycleFixture
            )?
            .update()
            .new_warning()
            .is_none()
    );
    Ok(())
}

#[test]
fn retry_preparation_can_be_abandoned_and_reprepared_but_cannot_be_deferred_again()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let pending = GameSessionLifecycleFixture::defer(&mut session)?;
    let revision = session.revision();
    let clock = session.clock().clone();
    let prepared = session.prepare_pending_result(&pending, ClockInstant::from_millis(10_000)?)?;
    drop(prepared);
    assert_eq!(session.outcome(), None);
    let prepared = session.prepare_pending_result(&pending, ClockInstant::from_millis(20_000)?)?;
    assert!(matches!(
        session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult),
        Err(GameSessionError::LifecycleResultRequired)
    ));
    assert_eq!(session.revision(), revision);
    assert_eq!(session.clock(), &clock);
    let first = session.prepare_pending_result(&pending, ClockInstant::from_millis(30_000)?)?;
    let stale = session.prepare_pending_result(&pending, ClockInstant::from_millis(40_000)?)?;
    session.commit_lifecycle(first, GameSessionLifecycleEnumCommit::Accept)?;
    let accepted_revision = session.revision();
    assert!(matches!(
        session.commit_lifecycle(stale, GameSessionLifecycleEnumCommit::Accept),
        Err(GameSessionError::Revision(GameRevisionError::Stale { .. }))
    ));
    assert_eq!(session.revision(), accepted_revision);
    assert_eq!(session.clock(), &clock);
    Ok(())
}

#[test]
fn pending_token_checks_live_and_result_state_in_addition_to_revision()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let pending = GameSessionLifecycleFixture::defer(&mut session)?;
    let revision = session.revision();
    let original_live = session.live;
    session.live = uuid::Uuid::from_u128(2).to_string().parse()?;
    assert!(matches!(
        session.prepare_pending_result(&pending, ClockInstant::from_millis(10_000)?),
        Err(GameSessionError::Revision(GameRevisionError::Stale { .. }))
    ));
    session.live = original_live;
    session.state = GameSessionEnumState::Active;
    assert!(matches!(
        session.prepare_pending_result(&pending, ClockInstant::from_millis(10_000)?),
        Err(GameSessionError::Revision(GameRevisionError::Stale { .. }))
    ));
    assert_eq!(session.revision(), revision);
    session.state = GameSessionEnumState::Interrupted(GameSessionEnumInterruption::PendingResult);
    session.prepare_pending_result(&pending, ClockInstant::from_millis(20_000)?)?;
    Ok(())
}

#[test]
fn a_proven_flagfall_draw_is_retained_without_turning_into_a_victory_at_retry()
-> GameSessionLifecycleTestResult {
    let mut session = GameSessionLifecycleFixture::timed()?;
    let proof = GameSessionLifecycleProof::new(GameAdjudicationEnumMateProof::Impossible);
    let prepared = session.prepare_lifecycle(
        GameSessionLifecycleFixture::deadline(&session, 1_000)?,
        &proof,
    )?;
    let (_, _, pending) = session
        .commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult)?
        .into_parts();
    let pending = pending.ok_or("Deferral must retain its proven draw")?;
    let prepared = session.prepare_pending_result(&pending, ClockInstant::from_millis(20_000)?)?;
    session.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
    let outcome = session
        .outcome()
        .ok_or("Retry must accept the retained draw")?;
    assert_eq!(outcome.result(), GameEnumResult::Draw);
    assert_eq!(outcome.reason(), GameEnumOutcomeReason::Flagfall);
    assert_eq!(proof.calls.get(), 1);
    Ok(())
}
