use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::BoardStart;
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::clock::state::ClockState;
use crate::clock::time_control::ClockTimeControl;
use crate::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use crate::game::engine_provider::{
    GameEngineContext, GameEngineQuiescenceReceipt, GameEngineSearchConfig,
    GameEngineSearchIdentity,
};
use crate::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameEnumPhase, GameEnumSeat};
use crate::game::pause::{
    GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseRequest, GameResumeRequest,
};
use crate::game::premove::GamePremoveIntent;
use crate::game::result::GameResultResignRequest;
use crate::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionMoveRequest, GameSessionOptions,
};
use crate::game::takeback::GameTakebackRequest;

struct GameEngineBarrierFixture;

impl GameAdjudicationMateProofProvider for GameEngineBarrierFixture {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

impl GameEngineBarrierFixture {
    fn session() -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session = GameSession::new(GameSessionOptions {
            id: uuid::Uuid::from_u128(100).to_string().parse()?,
            root: uuid::Uuid::from_u128(1).to_string().parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::HumanVersusEngine,
                GameEnumSeat::Human,
                GameEnumSeat::Reckless,
            )?,
            clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
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

    fn play(
        session: &mut GameSession,
        node: u128,
        uci: &str,
        millis: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let position = session.tree().node(session.live())?.position();
        let request = GameSessionMoveRequest {
            expected_revision: session.revision(),
            seat: session.seats().seat(position.turn()),
            color: position.turn(),
            node: uuid::Uuid::from_u128(node).to_string().parse()?,
            movement: position.parse_uci(uci)?,
            received_at: ClockInstant::from_millis(millis)?,
        };
        session.accept_move(request, &Self)?;
        Ok(())
    }

    fn barrier(session: &GameSession, stopped: bool) -> GamePauseEnumEngineBarrier {
        let context = GameEngineContext::new(session.id(), session.revision());
        let generation = uuid::Uuid::from_u128(3);
        let identity = stopped
            .then(|| GameEngineSearchIdentity::new(context, uuid::Uuid::from_u128(4), generation));
        GamePauseEnumEngineBarrier::Quiescent(GameEngineQuiescenceReceipt::new(
            context, generation, identity,
        ))
    }
}

#[test]
fn observed_engine_stop_allows_immediate_takeback_without_local_consent()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameEngineBarrierFixture::session()?;
    let root = session.live();
    GameEngineBarrierFixture::play(&mut session, 2, "e2e4", 1_000)?;
    let engine = GameEngineBarrierFixture::barrier(&session, true);
    session.request_takeback(
        GameTakebackRequest {
            expected_revision: session.revision(),
            requester: BoardEnumColor::White,
            received_at: ClockInstant::from_millis(2_000)?,
            engine,
        },
        &GameEngineBarrierFixture,
    )?;
    assert_eq!(session.live(), root);
    assert!(session.takeback().pending().is_none());
    assert_eq!(session.tree().len(), 2);
    Ok(())
}

#[test]
fn stale_engine_barrier_cannot_undo_a_newer_accepted_position()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameEngineBarrierFixture::session()?;
    GameEngineBarrierFixture::play(&mut session, 2, "e2e4", 1_000)?;
    let engine = GameEngineBarrierFixture::barrier(&session, true);
    GameEngineBarrierFixture::play(&mut session, 3, "e7e5", 2_000)?;
    let revision = session.revision();
    let live = session.live();
    let clock = session.clock().clone();
    assert!(
        session
            .request_takeback(
                GameTakebackRequest {
                    expected_revision: revision,
                    requester: BoardEnumColor::White,
                    received_at: ClockInstant::from_millis(3_000)?,
                    engine,
                },
                &GameEngineBarrierFixture
            )
            .is_err()
    );
    assert_eq!(session.revision(), revision);
    assert_eq!(session.live(), live);
    assert_eq!(session.clock(), &clock);
    assert_eq!(session.tree().len(), 3);
    Ok(())
}

#[test]
fn healthy_idle_receipt_pauses_human_turn_and_resume_builds_a_fresh_full_search()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameEngineBarrierFixture::session()?;
    let engine = GameEngineBarrierFixture::barrier(&session, false);
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(1_000)?,
            engine,
        },
        &GameEngineBarrierFixture,
    )?;
    session.resume(
        GameResumeRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(2_000)?,
            readiness: GameEnumEngineReadiness::Ready,
        },
        &GameEngineBarrierFixture,
    )?;
    GameEngineBarrierFixture::play(&mut session, 2, "e2e4", 3_000)?;
    let config =
        GameEngineSearchConfig::new(ClockDuration::from_millis(7_000)?, ClockDuration::ZERO)?;
    let first = session.engine_request(config)?;
    let engine = GameEngineBarrierFixture::barrier(&session, true);
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(5_000)?,
            engine,
        },
        &GameEngineBarrierFixture,
    )?;
    session.resume(
        GameResumeRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(20_000)?,
            readiness: GameEnumEngineReadiness::Ready,
        },
        &GameEngineBarrierFixture,
    )?;
    let resumed = session.engine_request(config)?;
    assert_eq!(session.phase(), GameEnumPhase::Active);
    assert_eq!(
        resumed.config().untimed_move_time(),
        ClockDuration::from_millis(7_000)?
    );
    assert_eq!(resumed.history(), first.history());
    assert_ne!(resumed.context(), first.context());
    assert_eq!(resumed.context().revision(), session.revision());
    Ok(())
}

#[test]
fn engine_resignation_consumes_current_quiescence_and_clears_queued_premove()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameEngineBarrierFixture::session()?;
    let stale = GameEngineBarrierFixture::barrier(&session, false);
    GameEngineBarrierFixture::play(&mut session, 2, "e2e4", 1_000)?;
    session.set_premoves_enabled(session.revision(), true)?;
    session.queue_premove(
        session.revision(),
        GamePremoveIntent::new("g1".parse()?, "f3".parse()?, None)?,
    )?;
    let revision = session.revision();
    assert!(
        session
            .resign(
                GameResultResignRequest {
                    expected_revision: revision,
                    resigning: BoardEnumColor::White,
                    received_at: ClockInstant::from_millis(2_000)?,
                    engine: stale,
                },
                &GameEngineBarrierFixture
            )
            .is_err()
    );
    assert_eq!(session.revision(), revision);
    assert!(session.premove().pending().is_some());
    let engine = GameEngineBarrierFixture::barrier(&session, true);
    session.resign(
        GameResultResignRequest {
            expected_revision: revision,
            resigning: BoardEnumColor::White,
            received_at: ClockInstant::from_millis(2_000)?,
            engine,
        },
        &GameEngineBarrierFixture,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert!(session.premove().pending().is_none());
    assert!(!session.clock().is_running());
    Ok(())
}
