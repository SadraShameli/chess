use std::cell::Cell;
use std::error::Error;
use std::time::Duration;

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationMateProofProvider,
};
use chess::game::mate_proof::{
    GameMateProof, GameMateProofBudget, GameMateProofControl, GameMateProofError,
};
use serde::Deserialize;

#[derive(Default)]
struct GameMateProofTestControl {
    now: Cell<Duration>,
    tick: Duration,
    cancelled: bool,
}

impl GameMateProofControl for GameMateProofTestControl {
    fn now(&self) -> Duration {
        let now = self.now.get();
        self.now.set(now.saturating_add(self.tick));
        now
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

struct GameMateProofTestScriptedClock {
    readings: Vec<Duration>,
    cursor: Cell<usize>,
}

impl GameMateProofControl for GameMateProofTestScriptedClock {
    fn now(&self) -> Duration {
        let index = self.cursor.get().min(self.readings.len().saturating_sub(1));
        self.cursor.set(self.cursor.get().saturating_add(1));
        self.readings.get(index).copied().unwrap_or_default()
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Deserialize)]
struct GameMateProofTestFixture {
    name: String,
    moves: Vec<String>,
    expect_possible: bool,
}

struct GameMateProofTestPosition;

impl GameMateProofTestPosition {
    fn parse(fen: &str) -> Result<BoardPosition, Box<dyn Error>> {
        Ok(BoardPosition::from_fen(fen, BoardEnumVariant::Standard)?)
    }

    fn budget(states: u32, bytes: usize) -> Result<GameMateProofBudget, Box<dyn Error>> {
        Ok(GameMateProofBudget::new(
            Duration::from_secs(2),
            states,
            bytes,
        )?)
    }
}

#[test]
fn default_budget_matches_selected_time_state_and_memory_limits() {
    let budget = GameMateProofBudget::default();
    assert_eq!(budget.duration(), Duration::from_secs(2));
    assert_eq!(budget.states(), 1_000_000);
    assert_eq!(budget.bytes(), 128 * 1024 * 1024);
    assert!(GameMateProofBudget::new(Duration::ZERO, 1, 1).is_err());
    assert!(GameMateProofBudget::new(Duration::from_secs(1), 0, 1).is_err());
    assert!(GameMateProofBudget::new(Duration::from_secs(1), 1, 0).is_err());
    assert!(GameMateProofBudget::new(Duration::from_secs(3), 1, 1).is_err());
    assert!(GameMateProofBudget::new(Duration::from_secs(1), 1_000_001, 1).is_err());
    assert!(GameMateProofBudget::new(Duration::from_secs(1), 1, 128 * 1024 * 1024 + 1).is_err());
}

#[test]
fn material_certificates_are_side_specific_and_conservative() -> Result<(), Box<dyn Error>> {
    let proof = GameMateProof::new(
        GameMateProofBudget::default(),
        GameMateProofTestControl::default(),
    );
    let kings = GameMateProofTestPosition::parse("7k/8/5K2/8/8/8/8/8 w - - 0 1")?;
    let knight = GameMateProofTestPosition::parse("7k/8/5K2/8/8/8/8/N7 w - - 0 1")?;
    let bishop = GameMateProofTestPosition::parse("7k/8/5K2/8/8/8/8/B7 w - - 0 1")?;
    for position in [kings, knight, bishop] {
        assert_eq!(
            proof.prove(&position, GameAdjudicationEnumMateQuestion::AnySide),
            GameAdjudicationEnumMateProof::Impossible
        );
    }
    let rook = GameMateProofTestPosition::parse("7k/8/5K2/8/8/8/8/R7 w - - 0 1")?;
    assert!(rook.has_insufficient_mating_material(BoardEnumColor::Black));
    assert!(!rook.has_insufficient_mating_material(BoardEnumColor::White));
    assert_eq!(
        proof.prove(
            &rook,
            GameAdjudicationEnumMateQuestion::Side(BoardEnumColor::Black)
        ),
        GameAdjudicationEnumMateProof::Impossible
    );
    Ok(())
}

#[test]
fn cooperative_knight_mate_is_not_rejected_by_a_piece_count_guess() -> Result<(), Box<dyn Error>> {
    let position = GameMateProofTestPosition::parse("6rk/5N2/7K/8/8/8/8/8 b - - 0 1")?;
    assert!(position.is_checkmate());
    let proof = GameMateProof::new(
        GameMateProofBudget::default(),
        GameMateProofTestControl::default(),
    );
    assert_eq!(
        proof.prove(
            &position,
            GameAdjudicationEnumMateQuestion::Side(BoardEnumColor::White)
        ),
        GameAdjudicationEnumMateProof::Possible
    );
    assert_eq!(
        proof.prove(
            &position,
            GameAdjudicationEnumMateQuestion::Side(BoardEnumColor::Black)
        ),
        GameAdjudicationEnumMateProof::Impossible
    );
    Ok(())
}

#[test]
fn stalemate_is_a_closed_graph_with_no_mating_continuation() -> Result<(), Box<dyn Error>> {
    let position = GameMateProofTestPosition::parse("7k/5K2/6Q1/8/8/8/8/8 b - - 0 1")?;
    let proof = GameMateProof::new(
        GameMateProofBudget::default(),
        GameMateProofTestControl::default(),
    );
    assert_eq!(
        proof.prove(&position, GameAdjudicationEnumMateQuestion::AnySide),
        GameAdjudicationEnumMateProof::Impossible
    );
    Ok(())
}

#[test]
fn standard_position_has_replayable_witnesses_for_either_side() -> Result<(), Box<dyn Error>> {
    let position = BoardPosition::standard();
    let proof = GameMateProof::new(
        GameMateProofBudget::default(),
        GameMateProofTestControl::default(),
    );
    for side in [BoardEnumColor::White, BoardEnumColor::Black] {
        let report = proof.report(&position, GameAdjudicationEnumMateQuestion::Side(side));
        assert_eq!(report.proof(), GameAdjudicationEnumMateProof::Possible);
        let witness = report
            .witness()
            .ok_or("Possible proof did not retain its legal witness")?;
        assert_eq!(witness.winner(), side);
        assert!(witness.verify(&position)?);
        assert!(!witness.movements().is_empty());
        assert!(report.requested_bytes() <= GameMateProofBudget::default().bytes());
    }
    Ok(())
}

#[test]
fn a_cached_witness_advances_only_for_the_exact_accepted_next_move() -> Result<(), Box<dyn Error>> {
    let position = BoardPosition::standard();
    let proof = GameMateProof::new(
        GameMateProofBudget::default(),
        GameMateProofTestControl::default(),
    );
    let report = proof.report(
        &position,
        GameAdjudicationEnumMateQuestion::Side(BoardEnumColor::Black),
    );
    let witness = report.witness().ok_or("Missing initial witness")?;
    let movement = *witness
        .movements()
        .first()
        .ok_or("Missing first witness move")?;
    let after = position.play(movement)?;
    let suffix = witness
        .advance(&position, movement, &after)?
        .ok_or("Exact move did not retain a witness suffix")?;
    assert!(suffix.verify(&after)?);
    assert_eq!(suffix.movements().len() + 1, witness.movements().len());
    let other = position.parse_uci("a2a4")?;
    assert!(
        witness
            .advance(&position, other, &position.play(other)?)?
            .is_none()
    );
    assert!(witness.advance(&position, movement, &position)?.is_none());
    Ok(())
}

#[test]
fn one_ply_reachability_finds_a_real_mate_and_keeps_the_input_unchanged()
-> Result<(), Box<dyn Error>> {
    let position = GameMateProofTestPosition::parse("7k/8/5KQ1/8/8/8/8/8 w - - 0 1")?;
    let before = position.clone();
    let proof = GameMateProof::new(
        GameMateProofBudget::default(),
        GameMateProofTestControl::default(),
    );
    let report = proof.report(&position, GameAdjudicationEnumMateQuestion::AnySide);
    assert_eq!(report.proof(), GameAdjudicationEnumMateProof::Possible);
    assert!(
        report
            .witness()
            .ok_or("Missing one-ply witness")?
            .verify(&position)?
    );
    assert_eq!(position, before);
    Ok(())
}

#[test]
fn cancellation_and_each_exhausted_budget_produce_unknown() -> Result<(), Box<dyn Error>> {
    let position = BoardPosition::standard();
    let cancelled = GameMateProof::new(
        GameMateProofBudget::default(),
        GameMateProofTestControl {
            cancelled: true,
            ..GameMateProofTestControl::default()
        },
    );
    assert_eq!(
        cancelled.prove(&position, GameAdjudicationEnumMateQuestion::AnySide),
        GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::Cancelled)
    );
    let timed = GameMateProof::new(
        GameMateProofBudget::default(),
        GameMateProofTestControl {
            tick: Duration::from_secs(2),
            ..GameMateProofTestControl::default()
        },
    );
    assert_eq!(
        timed.prove(&position, GameAdjudicationEnumMateQuestion::AnySide),
        GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::TimeLimit)
    );
    let states = GameMateProof::new(
        GameMateProofTestPosition::budget(1, 128 * 1024 * 1024)?,
        GameMateProofTestControl::default(),
    );
    let state_report = states.report(&position, GameAdjudicationEnumMateQuestion::AnySide);
    assert_eq!(
        state_report.proof(),
        GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::StateLimit)
    );
    assert!(state_report.states() <= 1);
    let memory = GameMateProof::new(
        GameMateProofTestPosition::budget(100, 1)?,
        GameMateProofTestControl::default(),
    );
    let memory_report = memory.report(&position, GameAdjudicationEnumMateQuestion::AnySide);
    assert_eq!(
        memory_report.proof(),
        GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::MemoryLimit)
    );
    assert_eq!(memory_report.requested_bytes(), 0);
    Ok(())
}

#[test]
fn proof_successors_normalize_counters_and_preserve_real_rights_and_en_passant()
-> Result<(), Box<dyn Error>> {
    let position =
        GameMateProofTestPosition::parse("r3k2r/8/8/3pP3/8/8/8/R3K2R w KQkq d6 1000000 1000000")?;
    for uci in ["e1g1", "e5d6", "a1a2"] {
        let movement = position.parse_uci(uci)?;
        let next = position.play_for_mate_proof(movement)?;
        assert_eq!(next.halfmoves(), 0);
        assert!(next.fen().ends_with(" 0 1"));
        let low = GameMateProofTestPosition::parse("r3k2r/8/8/3pP3/8/8/8/R3K2R w KQkq d6 0 1")?;
        assert_eq!(next.identity(), low.play(low.parse_uci(uci)?)?.identity());
    }
    Ok(())
}

#[test]
fn common_openings_and_middlegames_never_receive_unproved_draw_certificates()
-> Result<(), Box<dyn Error>> {
    let fixtures: Vec<GameMateProofTestFixture> =
        serde_json::from_str(include_str!("../fixtures/mate_proof.json"))?;
    assert!(!fixtures.is_empty());
    for fixture in fixtures {
        let mut position = BoardPosition::standard();
        for movement in fixture.moves {
            position = position.play_uci(&movement)?;
        }
        let proof = GameMateProof::new(
            GameMateProofTestPosition::budget(256, 1024 * 1024)?,
            GameMateProofTestControl::default(),
        );
        let report = proof.report(&position, GameAdjudicationEnumMateQuestion::AnySide);
        assert_ne!(
            report.proof(),
            GameAdjudicationEnumMateProof::Impossible,
            "{}",
            fixture.name
        );
        if fixture.expect_possible {
            assert_eq!(
                report.proof(),
                GameAdjudicationEnumMateProof::Possible,
                "{}",
                fixture.name
            );
        }
        if let Some(witness) = report.witness() {
            assert!(witness.verify(&position)?, "{}", fixture.name);
        }
        assert!(report.states() <= 256);
        assert!(report.requested_bytes() <= 1024 * 1024);
    }
    Ok(())
}

#[test]
fn a_clock_moving_backwards_is_unknown_with_a_typed_failure() -> Result<(), Box<dyn Error>> {
    let proof = GameMateProof::new(
        GameMateProofBudget::default(),
        GameMateProofTestScriptedClock {
            readings: vec![Duration::from_secs(5), Duration::from_secs(1)],
            cursor: Cell::new(0),
        },
    );
    let report = proof.report(
        &BoardPosition::standard(),
        GameAdjudicationEnumMateQuestion::AnySide,
    );
    assert_eq!(
        report.proof(),
        GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::Unresolved)
    );
    assert!(report.witness().is_none());
    assert!(matches!(
        report.failure(),
        Some(GameMateProofError::ClockWentBackwards { .. })
    ));
    Ok(())
}

#[test]
fn a_mid_search_deadline_is_unknown_never_impossible() -> Result<(), Box<dyn Error>> {
    let position = GameMateProofTestPosition::parse("4k3/8/8/8/8/8/PPPPPPPP/4K3 w - - 0 1")?;
    let proof = GameMateProof::new(
        GameMateProofBudget::new(
            Duration::from_secs(2),
            GameMateProofBudget::DEFAULT_STATES,
            GameMateProofBudget::DEFAULT_BYTES,
        )?,
        GameMateProofTestControl {
            tick: Duration::from_millis(1),
            ..GameMateProofTestControl::default()
        },
    );
    let report = proof.report(&position, GameAdjudicationEnumMateQuestion::AnySide);
    assert_ne!(report.proof(), GameAdjudicationEnumMateProof::Impossible);
    assert!(report.elapsed() <= Duration::from_secs(2) + Duration::from_millis(8));
    Ok(())
}

#[test]
fn unknown_is_reported_for_a_side_question_under_each_limit_without_a_witness()
-> Result<(), Box<dyn Error>> {
    let position = BoardPosition::standard();
    for states in [1_u32, 2, 5, 50] {
        let proof = GameMateProof::new(
            GameMateProofTestPosition::budget(states, 128 * 1024 * 1024)?,
            GameMateProofTestControl::default(),
        );
        let report = proof.report(
            &position,
            GameAdjudicationEnumMateQuestion::Side(BoardEnumColor::White),
        );
        assert!(report.states() <= states);
        match report.proof() {
            GameAdjudicationEnumMateProof::Possible => {
                assert!(
                    report
                        .witness()
                        .ok_or("Possible without witness")?
                        .verify(&position)?
                );
            }
            GameAdjudicationEnumMateProof::Unknown(_) => assert!(report.witness().is_none()),
            GameAdjudicationEnumMateProof::Impossible => {
                return Err("A bounded search certified an unproved draw".into());
            }
        }
    }
    Ok(())
}
