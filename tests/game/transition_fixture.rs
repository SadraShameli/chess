use std::sync::atomic::{AtomicUsize, Ordering};

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::clock::duration::ClockInstant;
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationMateProofProvider,
};
use chess::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameEnumSeat, GameMoveId};
use chess::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionMoveRequest, GameSessionOptions,
};

pub(crate) struct GameTransitionFixtureProof;

impl GameAdjudicationMateProofProvider for GameTransitionFixtureProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

pub(crate) struct GameTransitionFixtureUnresolvedProof {
    question: GameAdjudicationEnumMateQuestion,
    calls: AtomicUsize,
}

impl GameTransitionFixtureUnresolvedProof {
    pub(crate) fn new(question: GameAdjudicationEnumMateQuestion) -> Self {
        Self {
            question,
            calls: AtomicUsize::new(0),
        }
    }

    pub(crate) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl GameAdjudicationMateProofProvider for GameTransitionFixtureUnresolvedProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        assert_eq!(question, self.question);
        self.calls.fetch_add(1, Ordering::SeqCst);
        GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::Unresolved)
    }
}

pub(crate) struct GameTransitionFixture;

impl GameTransitionFixture {
    pub(crate) fn id(value: u128) -> Result<GameMoveId, Box<dyn std::error::Error>> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    pub(crate) fn session(
        control: ClockTimeControl,
        engine: Option<BoardEnumColor>,
    ) -> Result<GameSession, Box<dyn std::error::Error>> {
        let seat = |color| {
            if engine == Some(color) {
                GameEnumSeat::Reckless
            } else {
                GameEnumSeat::Human
            }
        };
        let mode = match engine {
            None => GameEnumMode::LocalTwoPlayer,
            Some(_) => GameEnumMode::HumanVersusEngine,
        };
        let mut session = GameSession::new(GameSessionOptions {
            id: "123e4567-e89b-42d3-a456-426614174000".parse()?,
            root: Self::id(1)?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                mode,
                seat(BoardEnumColor::White),
                seat(BoardEnumColor::Black),
            )?,
            clock: ClockState::new(control, BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?;
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::Ready,
            &GameTransitionFixtureProof,
        )?;
        Ok(session)
    }

    pub(crate) fn play(
        session: &mut GameSession,
        id: u128,
        uci: &str,
        millis: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Self::play_with_proof(session, id, uci, millis, &GameTransitionFixtureProof)
    }

    pub(crate) fn play_with_proof<P: GameAdjudicationMateProofProvider>(
        session: &mut GameSession,
        id: u128,
        uci: &str,
        millis: u64,
        proof: &P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let position = session.tree().node(session.live())?.position();
        let request = GameSessionMoveRequest {
            expected_revision: session.revision(),
            seat: session.seats().seat(position.turn()),
            color: position.turn(),
            node: Self::id(id)?,
            movement: position.parse_uci(uci)?,
            received_at: ClockInstant::from_millis(millis)?,
        };
        session.accept_move(request, proof)?;
        Ok(())
    }
}
