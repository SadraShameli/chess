use std::cell::RefCell;
use std::collections::HashSet;
use std::error::Error;

use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use chess::board::piece::BoardPiece;
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;
use chess::board::start::{BoardStart, BoardStartEnumSelection, BoardStartIndex};
use chess::game::adjudication::{
    GameAdjudicationEnumDecision, GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationHistory, GameAdjudicationLocalAgreement,
    GameAdjudicationMateProofProvider,
};
use chess::game::draw_claim::{GameDrawClaimEnumIntent, GameDrawClaimEnumKind, GameDrawClaimError};
use chess::game::enums::{GameEnumOutcomeReason, GameEnumResult};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::Deserialize;

#[derive(Debug)]
struct GameRulesProof {
    answer: GameAdjudicationEnumMateProof,
    questions: RefCell<Vec<GameAdjudicationEnumMateQuestion>>,
}

impl GameRulesProof {
    fn new(answer: GameAdjudicationEnumMateProof) -> Self {
        Self {
            answer,
            questions: RefCell::new(Vec::new()),
        }
    }
}

impl GameAdjudicationMateProofProvider for GameRulesProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        self.questions.borrow_mut().push(question);
        self.answer
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GameRulesEnumFixtureProof {
    Possible,
    Impossible,
}

impl GameRulesEnumFixtureProof {
    fn answer(self) -> GameAdjudicationEnumMateProof {
        match self {
            Self::Possible => GameAdjudicationEnumMateProof::Possible,
            Self::Impossible => GameAdjudicationEnumMateProof::Impossible,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GameRulesFixture {
    name: String,
    fen: String,
    proof: GameRulesEnumFixtureProof,
    reason: GameEnumOutcomeReason,
}

struct GameRulesHistory;

struct GameRulesStart;

impl GameRulesStart {
    fn pieces(position: &BoardPosition, color: BoardEnumColor) -> Vec<(BoardSquare, BoardPiece)> {
        position
            .pieces()
            .filter(|(_, piece)| piece.color() == color)
            .collect()
    }
}

impl GameRulesHistory {
    const REPETITION_CYCLE: [&str; 4] = ["g1f3", "g8f6", "f3g1", "f6g8"];
    const QUIET_FEN: &str = "7k/8/5K2/8/8/8/8/R7 w - - 99 51";

    fn from_fen(fen: &str) -> Result<GameAdjudicationHistory, Box<dyn Error>> {
        Ok(GameAdjudicationHistory::new(BoardPosition::from_fen(
            fen,
            BoardEnumVariant::Standard,
        )?))
    }

    fn play(history: &mut GameAdjudicationHistory, movement: &str) -> Result<(), Box<dyn Error>> {
        let movement = history.position().parse_uci(movement)?;
        let candidate = history.prepare(movement)?;
        history.commit(candidate)?;
        Ok(())
    }

    fn cycle(history: &mut GameAdjudicationHistory) -> Result<(), Box<dyn Error>> {
        for movement in Self::REPETITION_CYCLE {
            Self::play(history, movement)?;
        }
        Ok(())
    }
}

#[test]
fn all_numbered_chess960_starts_are_unique_legal_and_keep_bishops_on_opposite_colors()
-> Result<(), Box<dyn Error>> {
    let mut placements = HashSet::new();
    for index in BoardStartIndex::MIN..=BoardStartIndex::MAX {
        let index = BoardStartIndex::try_from(index)?;
        let start = BoardStart::chess960(index)?;
        assert!(placements.insert(start.position().placement()));
        assert_eq!(
            start.selection(),
            BoardStartEnumSelection::Chess960 { index }
        );
        let bishops: Vec<_> = start
            .position()
            .pieces()
            .filter(|(_, piece)| {
                piece.color() == BoardEnumColor::White && piece.kind() == BoardEnumPieceKind::Bishop
            })
            .map(|(square, _)| u8::from(square) % 2)
            .collect();
        assert_eq!(bishops.len(), 2);
        assert_ne!(bishops.first(), bishops.last());
        BoardPosition::from_fen(start.initial_fen(), BoardEnumVariant::Chess960)?;
    }
    assert_eq!(placements.len(), usize::from(BoardStartIndex::MAX) + 1);
    Ok(())
}

#[test]
fn standard_numbering_boundaries_and_dfrc_back_ranks_are_preserved() -> Result<(), Box<dyn Error>> {
    assert!(BoardStartIndex::try_from(960).is_err());
    assert!(serde_json::from_str::<BoardStartIndex>("960").is_err());
    let standard = BoardStart::chess960(BoardStartIndex::STANDARD)?;
    assert_eq!(
        standard.position().placement(),
        BoardPosition::standard().placement()
    );
    assert!(standard.initial_fen().contains(" HAha "));
    let white_index = BoardStartIndex::try_from(0)?;
    let black_index = BoardStartIndex::try_from(959)?;
    let start = BoardStart::dfrc(white_index, black_index)?;
    let white = BoardStart::chess960(white_index)?;
    let black = BoardStart::chess960(black_index)?;
    assert_eq!(
        GameRulesStart::pieces(start.position(), BoardEnumColor::White),
        GameRulesStart::pieces(white.position(), BoardEnumColor::White)
    );
    assert_eq!(
        GameRulesStart::pieces(start.position(), BoardEnumColor::Black),
        GameRulesStart::pieces(black.position(), BoardEnumColor::Black)
    );
    assert_eq!(
        start.selection(),
        BoardStartEnumSelection::Dfrc {
            white_index,
            black_index
        }
    );
    Ok(())
}

#[test]
fn seeded_random_starts_are_reproducible_and_dfrc_draws_both_indexes() -> Result<(), Box<dyn Error>>
{
    let mut first = StdRng::seed_from_u64(4242);
    let mut second = StdRng::seed_from_u64(4242);
    let mut unequal = false;
    for _ in 0..128 {
        assert_eq!(
            BoardStart::random_chess960(&mut first)?,
            BoardStart::random_chess960(&mut second)?
        );
        let start = BoardStart::random_dfrc(&mut first)?;
        assert_eq!(start, BoardStart::random_dfrc(&mut second)?);
        if let BoardStartEnumSelection::Dfrc {
            white_index,
            black_index,
        } = start.selection()
        {
            unequal |= white_index != black_index;
        }
    }
    assert!(unequal);
    Ok(())
}

#[test]
fn start_round_trip_keeps_indexes_and_fen_and_rejects_conflicting_record()
-> Result<(), Box<dyn Error>> {
    let start = BoardStart::dfrc(
        BoardStartIndex::try_from(37)?,
        BoardStartIndex::try_from(812)?,
    )?;
    let encoded = serde_json::to_string(&start)?;
    assert!(encoded.contains(start.initial_fen()));
    assert_eq!(serde_json::from_str::<BoardStart>(&encoded)?, start);
    let invalid = encoded.replace(start.initial_fen(), BoardStart::standard().initial_fen());
    assert!(serde_json::from_str::<BoardStart>(&invalid).is_err());
    let custom = BoardStart::custom(BoardPosition::from_fen(
        GameRulesHistory::QUIET_FEN,
        BoardEnumVariant::Standard,
    )?);
    assert_eq!(
        serde_json::from_str::<BoardStart>(&serde_json::to_string(&custom)?)?,
        custom
    );
    Ok(())
}

#[test]
fn mandatory_results_follow_the_adversarial_fixture() -> Result<(), Box<dyn Error>> {
    let fixtures: Vec<GameRulesFixture> =
        serde_json::from_str(include_str!("../fixtures/adjudication.json"))?;
    assert!(!fixtures.is_empty());
    for fixture in fixtures {
        let history = GameRulesHistory::from_fen(&fixture.fen)?;
        let proof = GameRulesProof::new(fixture.proof.answer());
        let decision = history.adjudicate(&proof);
        let expected_result = if fixture.reason == GameEnumOutcomeReason::Checkmate {
            GameEnumResult::Victory(BoardEnumColor::White)
        } else {
            GameEnumResult::Draw
        };
        assert_eq!(
            decision,
            GameAdjudicationEnumDecision::Complete {
                result: expected_result,
                reason: fixture.reason
            },
            "{}",
            fixture.name
        );
    }
    Ok(())
}

#[test]
fn threefold_requires_a_claim_while_fivefold_is_automatic() -> Result<(), Box<dyn Error>> {
    let mut history = GameAdjudicationHistory::new(BoardPosition::standard());
    let proof = GameRulesProof::new(GameAdjudicationEnumMateProof::Possible);
    GameRulesHistory::cycle(&mut history)?;
    GameRulesHistory::cycle(&mut history)?;
    assert_eq!(history.occurrences(), 3);
    assert_eq!(
        history.adjudicate(&proof),
        GameAdjudicationEnumDecision::Ongoing
    );
    assert!(
        history
            .claimable()
            .contains(GameDrawClaimEnumKind::ThreefoldRepetition)
    );
    assert_eq!(
        history.claim(
            BoardEnumColor::White,
            GameDrawClaimEnumKind::ThreefoldRepetition,
            GameDrawClaimEnumIntent::Current
        )?,
        GameAdjudicationEnumDecision::Complete {
            result: GameEnumResult::Draw,
            reason: GameEnumOutcomeReason::ThreefoldRepetition
        }
    );
    GameRulesHistory::cycle(&mut history)?;
    GameRulesHistory::cycle(&mut history)?;
    assert_eq!(
        history.adjudicate(&proof),
        GameAdjudicationEnumDecision::Complete {
            result: GameEnumResult::Draw,
            reason: GameEnumOutcomeReason::FivefoldRepetition
        }
    );
    Ok(())
}

#[test]
fn intended_repetition_claim_does_not_commit_the_intended_move() -> Result<(), Box<dyn Error>> {
    let mut history = GameAdjudicationHistory::new(BoardPosition::standard());
    GameRulesHistory::cycle(&mut history)?;
    for movement in ["g1f3", "g8f6", "f3g1"] {
        GameRulesHistory::play(&mut history, movement)?;
    }
    let before = history.position().clone();
    let movement = before.parse_uci("f6g8")?;
    assert_eq!(
        history.claim(
            BoardEnumColor::Black,
            GameDrawClaimEnumKind::ThreefoldRepetition,
            GameDrawClaimEnumIntent::Intended(movement)
        )?,
        GameAdjudicationEnumDecision::Complete {
            result: GameEnumResult::Draw,
            reason: GameEnumOutcomeReason::ThreefoldRepetition
        }
    );
    assert_eq!(history.position(), &before);
    assert_eq!(history.occurrences(), 2);
    Ok(())
}

#[test]
fn intended_fifty_move_claim_validates_turn_and_legality_without_mutation()
-> Result<(), Box<dyn Error>> {
    let history = GameRulesHistory::from_fen(GameRulesHistory::QUIET_FEN)?;
    let before = history.position().clone();
    let movement = before.parse_uci("a1a2")?;
    assert!(matches!(
        history.claim(
            BoardEnumColor::Black,
            GameDrawClaimEnumKind::FiftyMove,
            GameDrawClaimEnumIntent::Intended(movement)
        ),
        Err(GameDrawClaimError::WrongTurn)
    ));
    assert!(matches!(
        history.claim(
            BoardEnumColor::White,
            GameDrawClaimEnumKind::FiftyMove,
            GameDrawClaimEnumIntent::Current
        ),
        Err(GameDrawClaimError::Unavailable(
            GameDrawClaimEnumKind::FiftyMove
        ))
    ));
    assert_eq!(
        history.claim(
            BoardEnumColor::White,
            GameDrawClaimEnumKind::FiftyMove,
            GameDrawClaimEnumIntent::Intended(movement)
        )?,
        GameAdjudicationEnumDecision::Complete {
            result: GameEnumResult::Draw,
            reason: GameEnumOutcomeReason::FiftyMove
        }
    );
    let illegal = BoardPosition::standard().parse_uci("e2e4")?;
    assert!(matches!(
        history.claim(
            BoardEnumColor::White,
            GameDrawClaimEnumKind::FiftyMove,
            GameDrawClaimEnumIntent::Intended(illegal)
        ),
        Err(GameDrawClaimError::Adjudication(_))
    ));
    assert_eq!(history.position(), &before);
    Ok(())
}

#[test]
fn intended_checkmate_cannot_be_replaced_by_a_fifty_move_claim() -> Result<(), Box<dyn Error>> {
    let history = GameRulesHistory::from_fen("7k/8/5KQ1/8/8/8/8/8 w - - 99 51")?;
    let movement = history.position().parse_uci("g6g7")?;
    assert!(matches!(
        history.claim(
            BoardEnumColor::White,
            GameDrawClaimEnumKind::FiftyMove,
            GameDrawClaimEnumIntent::Intended(movement)
        ),
        Err(GameDrawClaimError::TerminalPosition)
    ));
    Ok(())
}

#[test]
fn reckless_claims_current_or_intended_draws_without_protocol_messages()
-> Result<(), Box<dyn Error>> {
    let mut history = GameRulesHistory::from_fen(GameRulesHistory::QUIET_FEN)?;
    assert_eq!(history.reckless_claim(BoardEnumColor::White, None)?, None);
    let movement = history.position().parse_uci("a1a2")?;
    let expected = Some(GameAdjudicationEnumDecision::Complete {
        result: GameEnumResult::Draw,
        reason: GameEnumOutcomeReason::FiftyMove,
    });
    assert_eq!(
        history.reckless_claim(BoardEnumColor::White, Some(movement))?,
        expected
    );
    GameRulesHistory::play(&mut history, "a1a2")?;
    assert_eq!(
        history.reckless_claim(BoardEnumColor::Black, None)?,
        expected
    );
    Ok(())
}

#[test]
fn custom_starts_do_not_invent_repetition_history() -> Result<(), Box<dyn Error>> {
    let mut repeated = GameAdjudicationHistory::new(BoardPosition::standard());
    GameRulesHistory::cycle(&mut repeated)?;
    GameRulesHistory::cycle(&mut repeated)?;
    let custom = GameAdjudicationHistory::new(repeated.position().clone());
    assert_eq!(custom.occurrences(), 1);
    assert!(
        !custom
            .claimable()
            .contains(GameDrawClaimEnumKind::ThreefoldRepetition)
    );
    Ok(())
}

#[test]
fn lost_castling_rights_keep_same_placement_from_repeating() -> Result<(), Box<dyn Error>> {
    let mut history = GameRulesHistory::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1")?;
    for movement in ["h1h2", "h8h7", "h2h1", "h7h8"] {
        GameRulesHistory::play(&mut history, movement)?;
    }
    assert_eq!(history.occurrences(), 1);
    Ok(())
}

#[test]
fn stale_candidate_cannot_partially_mutate_history() -> Result<(), Box<dyn Error>> {
    let mut history = GameAdjudicationHistory::new(BoardPosition::standard());
    let stale = history.prepare(history.position().parse_uci("d2d4")?)?;
    let movement = history.position().parse_uci("e2e4")?;
    let candidate = history.prepare(movement)?;
    assert_eq!(candidate.movement(), movement);
    assert_eq!(candidate.previous_position(), history.position());
    assert_eq!(candidate.position(), &history.position().play(movement)?);
    history.commit(candidate)?;
    let before = history.position().clone();
    assert!(history.commit(stale).is_err());
    assert_eq!(history.position(), &before);
    assert_eq!(history.occurrences(), 1);
    Ok(())
}

#[test]
fn unknown_dead_position_proof_always_interrupts_and_preserves_state() {
    let history = GameAdjudicationHistory::new(BoardPosition::standard());
    let proof = GameRulesProof::new(GameAdjudicationEnumMateProof::Unknown(
        GameAdjudicationEnumInterruption::StateLimit,
    ));
    assert_eq!(
        history.adjudicate(&proof),
        GameAdjudicationEnumDecision::Interrupted {
            question: GameAdjudicationEnumMateQuestion::AnySide,
            cause: GameAdjudicationEnumInterruption::StateLimit
        }
    );
    assert_eq!(history.position(), &BoardPosition::standard());
    assert_eq!(
        proof.questions.take(),
        [GameAdjudicationEnumMateQuestion::AnySide]
    );
}

#[test]
fn flagfall_asks_about_the_opponents_mating_possibility() {
    let history = GameAdjudicationHistory::new(BoardPosition::standard());
    for (answer, expected) in [
        (
            GameAdjudicationEnumMateProof::Possible,
            GameAdjudicationEnumDecision::Complete {
                result: GameEnumResult::Victory(BoardEnumColor::Black),
                reason: GameEnumOutcomeReason::Flagfall,
            },
        ),
        (
            GameAdjudicationEnumMateProof::Impossible,
            GameAdjudicationEnumDecision::Complete {
                result: GameEnumResult::Draw,
                reason: GameEnumOutcomeReason::Flagfall,
            },
        ),
        (
            GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::TimeLimit),
            GameAdjudicationEnumDecision::Interrupted {
                question: GameAdjudicationEnumMateQuestion::Side(BoardEnumColor::Black),
                cause: GameAdjudicationEnumInterruption::TimeLimit,
            },
        ),
    ] {
        let proof = GameRulesProof::new(answer);
        assert_eq!(history.flag_fall(BoardEnumColor::White, &proof), expected);
        assert_eq!(
            proof.questions.take(),
            [GameAdjudicationEnumMateQuestion::Side(
                BoardEnumColor::Black
            )]
        );
    }
}

#[test]
fn accepted_checkmate_precedes_a_later_flagfall_or_resignation() -> Result<(), Box<dyn Error>> {
    let history = GameRulesHistory::from_fen("7k/6Q1/5K2/8/8/8/8/8 b - - 150 76")?;
    let proof = GameRulesProof::new(GameAdjudicationEnumMateProof::Unknown(
        GameAdjudicationEnumInterruption::MemoryLimit,
    ));
    let expected = GameAdjudicationEnumDecision::Complete {
        result: GameEnumResult::Victory(BoardEnumColor::White),
        reason: GameEnumOutcomeReason::Checkmate,
    };
    assert_eq!(history.flag_fall(BoardEnumColor::White, &proof), expected);
    assert_eq!(history.resign(BoardEnumColor::White), expected);
    assert!(proof.questions.take().is_empty());
    Ok(())
}

#[test]
fn draw_agreement_requires_both_local_seats_and_resignation_awards_opponent()
-> Result<(), Box<dyn Error>> {
    let history = GameAdjudicationHistory::new(BoardPosition::standard());
    let mut agreement = GameAdjudicationLocalAgreement::new();
    assert!(history.agree(&agreement).is_err());
    agreement.confirm(BoardEnumColor::White);
    assert!(!agreement.is_confirmed());
    assert!(history.agree(&agreement).is_err());
    agreement.confirm(BoardEnumColor::Black);
    assert!(agreement.is_confirmed());
    assert_eq!(
        history.agree(&agreement)?,
        GameAdjudicationEnumDecision::Complete {
            result: GameEnumResult::Draw,
            reason: GameEnumOutcomeReason::Agreement
        }
    );
    assert_eq!(
        history.resign(BoardEnumColor::Black),
        GameAdjudicationEnumDecision::Complete {
            result: GameEnumResult::Victory(BoardEnumColor::White),
            reason: GameEnumOutcomeReason::Resignation
        }
    );
    Ok(())
}
