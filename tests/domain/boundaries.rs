use std::io::Cursor;

use serde::Deserialize;

use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::clock::duration::ClockDuration;
use chess::game::enums::{GameIdentityError, GameMoveId};
use chess::notation::pgn_import::NotationPgnImport;
use chess::puzzle::csv_import::PuzzleCsvImport;
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::selection::PuzzleSelectionDataset;

type BoundaryResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct BoundaryPosition {
    fen: String,
    valid: bool,
}

struct DomainBoundaries;

impl DomainBoundaries {
    const POSITIONS: &str = include_str!("../fixtures/positions.json");
    const PGN: &str = "[Event \"Boundary\"]\n[White \"Ada\"]\n[Black \"Grace\"]\n[Result \"1-0\"]\n\n{opening} 1. e4 $1 {[%clk 0:09:59]} e5 (1... c5 2. Nf3) 2. Nf3 Nc6 1-0\n";
    const CSV: &str = "PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags\nabc12,rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1,e7e5 g1f3,1500,80,90,100,fork,https://example.test/game,Opening\n";
    const MAXIMUM_ITERATIONS: usize = 16;

    fn positions() -> BoundaryResult<Vec<BoundaryPosition>> {
        Ok(serde_json::from_str(Self::POSITIONS)?)
    }

    fn ids() -> impl FnMut() -> Result<GameMoveId, GameIdentityError> {
        let mut next = 0_u64;
        move || {
            next += 1;
            format!("00000000-0000-4000-8000-{next:012x}").parse()
        }
    }

    fn prefixes(text: &str) -> impl Iterator<Item = &str> {
        text.char_indices()
            .map(|(index, _character)| &text[..index])
            .chain(std::iter::once(text))
    }

    fn sample(seed: u64, count: usize) -> Vec<u64> {
        let mut state = seed;
        (0..count)
            .map(|_index| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state
            })
            .collect()
    }
}

#[test]
fn every_fixture_position_accepts_or_refuses_as_declared_and_round_trips() -> BoundaryResult {
    for fixture in DomainBoundaries::positions()? {
        let parsed = BoardPosition::from_fen(&fixture.fen, BoardEnumVariant::Standard);
        assert_eq!(parsed.is_ok(), fixture.valid, "{}", fixture.fen);
        if let Ok(position) = parsed {
            let again = BoardPosition::from_fen(&position.fen(), BoardEnumVariant::Standard)?;
            assert_eq!(position.identity(), again.identity());
            assert_eq!(position.fen(), again.fen());
        }
    }
    Ok(())
}

#[test]
fn truncated_and_corrupted_fen_never_panic_and_accepted_text_is_canonical() -> BoundaryResult {
    for fixture in DomainBoundaries::positions()? {
        for prefix in DomainBoundaries::prefixes(&fixture.fen) {
            if let Ok(position) = BoardPosition::from_fen(prefix, BoardEnumVariant::Standard) {
                assert!(position.fen().chars().all(|character| !character.is_control()));
                let again = BoardPosition::from_fen(&position.fen(), BoardEnumVariant::Standard)?;
                assert_eq!(position.identity(), again.identity());
            }
        }
        for index in 0..fixture.fen.len() {
            for injected in ['\0', '\n', '\u{7f}'] {
                let mut corrupted = fixture.fen.clone();
                corrupted.insert(index, injected);
                if let Ok(position) =
                    BoardPosition::from_fen(&corrupted, BoardEnumVariant::Standard)
                {
                    assert!(position.fen().chars().all(|character| !character.is_control()));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn oversized_and_non_ascii_fen_are_refused_before_parsing() {
    let oversized = "8/".repeat(BoardPosition::MAX_FEN_BYTES);
    assert!(oversized.len() > BoardPosition::MAX_FEN_BYTES);
    assert!(BoardPosition::from_fen(&oversized, BoardEnumVariant::Standard).is_err());
    assert!(
        BoardPosition::from_fen(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\u{e9}",
            BoardEnumVariant::Standard
        )
        .is_err()
    );
    assert!(BoardPosition::from_fen("", BoardEnumVariant::Standard).is_err());
}

#[test]
fn truncated_pgn_never_panics_and_every_accepted_game_has_a_root() {
    for prefix in DomainBoundaries::prefixes(DomainBoundaries::PGN) {
        let mut importer =
            NotationPgnImport::new(Cursor::new(prefix.as_bytes()), DomainBoundaries::ids());
        for _iteration in 0..DomainBoundaries::MAXIMUM_ITERATIONS {
            match importer.next_game(&|| false) {
                Ok(Some(game)) => assert!(!game.tree().is_empty()),
                Ok(None) | Err(_) => break,
            }
        }
    }
}

#[test]
fn cancelled_pgn_import_never_yields_a_game() {
    let mut importer = NotationPgnImport::new(
        Cursor::new(DomainBoundaries::PGN.as_bytes()),
        DomainBoundaries::ids(),
    );
    assert!(!matches!(importer.next_game(&|| true), Ok(Some(_))));
}

#[test]
fn clock_duration_arithmetic_is_checked_and_invertible_on_sampled_values() -> BoundaryResult {
    let maximum = ClockDuration::from_nanos(u64::MAX);
    assert!(maximum.checked_add(ClockDuration::from_nanos(1)).is_err());
    assert!(
        ClockDuration::from_nanos(0)
            .checked_sub(ClockDuration::from_nanos(1))
            .is_err()
    );
    for value in DomainBoundaries::sample(0x9e37_79b9_7f4a_7c15, 256) {
        let left = ClockDuration::from_nanos(value >> 2);
        let right = ClockDuration::from_nanos(value >> 3);
        let sum = left.checked_add(right)?;
        assert_eq!(sum.checked_sub(right)?.as_nanos(), left.as_nanos());
        assert!(sum.as_nanos() >= left.as_nanos());
    }
    Ok(())
}

#[test]
fn truncated_puzzle_csv_never_panics_and_reports_typed_rows() -> BoundaryResult {
    for prefix in DomainBoundaries::prefixes(DomainBoundaries::CSV) {
        let dataset = PuzzleSelectionDataset::from_identity(&PuzzleDefinitionIdentity::new(
            "personal/lichess".into(),
            "snapshot-1".into(),
            "seed".into(),
        )?);
        let mut parser = PuzzleCsvImport::new(Cursor::new(prefix.as_bytes().to_vec()), dataset);
        for _iteration in 0..DomainBoundaries::MAXIMUM_ITERATIONS {
            match parser.next_row(|| false) {
                Ok(Some(_row)) => {}
                Ok(None) | Err(_) => break,
            }
        }
    }
    Ok(())
}
