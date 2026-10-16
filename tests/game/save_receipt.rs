use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::BoardStart;
use crate::clock::duration::ClockInstant;
use crate::clock::state::ClockState;
use crate::clock::time_control::ClockTimeControl;
use crate::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use crate::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameEnumPhase, GameEnumSeat};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumEngineBarrier, GameResumeRequest};
use crate::game::save::{GameSaveEnumAcknowledgment, GameSaveEnumStatus, GameSaveFailureRequest};
use crate::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionEnumInterruption, GameSessionOptions,
};
use crate::game::storage_repository::{GameStorageReceipt, GameStorageRevision};

struct GameSaveReceiptFixture;

impl GameAdjudicationMateProofProvider for GameSaveReceiptFixture {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

impl GameSaveReceiptFixture {
    fn session() -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session = GameSession::new(GameSessionOptions {
            id: uuid::Uuid::from_u128(100).to_string().parse()?,
            root: uuid::Uuid::from_u128(1).to_string().parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?;
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::NotRequired,
            &Self,
        )?;
        Ok(session)
    }

    fn fail(session: &mut GameSession) -> Result<(), Box<dyn std::error::Error>> {
        session.interrupt_for_save_failure(
            GameSaveFailureRequest {
                expected_revision: session.revision(),
                received_at: ClockInstant::from_nanos(1),
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &Self,
        )?;
        Ok(())
    }
}

#[test]
fn only_an_exact_current_receipt_clears_save_gate_without_advancing_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSaveReceiptFixture::session()?;
    let older = GameStorageReceipt::new(
        session.id(),
        session.revision(),
        GameStorageRevision::new(1),
    );
    GameSaveReceiptFixture::fail(&mut session)?;
    let revision = session.revision();
    assert_eq!(
        session.acknowledge_save(older)?,
        GameSaveEnumAcknowledgment::Older
    );
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    let current = GameStorageReceipt::new(session.id(), revision, GameStorageRevision::new(2));
    assert_eq!(
        session.acknowledge_save(current)?,
        GameSaveEnumAcknowledgment::Current
    );
    assert_eq!(session.revision(), revision);
    assert_eq!(session.save_status(), GameSaveEnumStatus::Current);
    assert_eq!(
        session.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::AutosaveFailure
        ))
    );
    assert!(!session.clock().is_running());
    session.resume(
        GameResumeRequest {
            expected_revision: revision,
            received_at: ClockInstant::from_nanos(2),
            readiness: GameEnumEngineReadiness::NotRequired,
        },
        &GameSaveReceiptFixture,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Active);
    Ok(())
}

#[test]
fn foreign_and_future_receipts_cannot_acknowledge_the_pending_save()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSaveReceiptFixture::session()?;
    GameSaveReceiptFixture::fail(&mut session)?;
    let revision = session.revision();
    let foreign = GameStorageReceipt::new(
        uuid::Uuid::from_u128(200).to_string().parse()?,
        revision,
        GameStorageRevision::new(1),
    );
    assert!(session.acknowledge_save(foreign).is_err());
    let future =
        GameStorageReceipt::new(session.id(), revision.next()?, GameStorageRevision::new(1));
    assert!(session.acknowledge_save(future).is_err());
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    assert_eq!(session.revision(), revision);
    Ok(())
}

#[test]
fn a_successfully_saved_failure_pause_restores_without_the_ephemeral_gate()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSaveReceiptFixture::session()?;
    GameSaveReceiptFixture::fail(&mut session)?;
    let restored = GameSession::restore(session.restoration(ClockInstant::from_nanos(2))?)?;
    assert_eq!(restored.save_status(), GameSaveEnumStatus::Current);
    assert_eq!(
        restored.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::AutosaveFailure
        ))
    );
    assert!(!restored.clock().is_running());
    Ok(())
}
