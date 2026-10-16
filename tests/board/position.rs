use std::collections::HashSet;
use std::hash::{BuildHasherDefault, Hasher};

use chess::board::enums::BoardEnumPieceKind;
use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;
use serde::Deserialize;

#[derive(Deserialize)]
struct BoardPositionFixture {
    name: String,
    fen: String,
    valid: bool,
}

#[derive(Default)]
struct BoardPositionCollisionHasher;

impl Hasher for BoardPositionCollisionHasher {
    fn finish(&self) -> u64 {
        0
    }

    fn write(&mut self, _bytes: &[u8]) {}
}

#[test]
fn validates_every_position_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let fixtures: Vec<BoardPositionFixture> =
        serde_json::from_str(include_str!("../fixtures/positions.json"))?;
    assert!(!fixtures.is_empty());
    for fixture in fixtures {
        assert_eq!(
            BoardPosition::from_fen(&fixture.fen, BoardEnumVariant::Standard).is_ok(),
            fixture.valid,
            "{}",
            fixture.name
        );
    }
    Ok(())
}

#[test]
fn derives_legal_moves_and_rejects_browser_declarations() -> Result<(), Box<dyn std::error::Error>>
{
    let position = BoardPosition::standard();
    assert_eq!(position.legal_moves().len(), 20);
    assert!(position.play_uci("e2e5").is_err());
    let advanced = position.play_uci("e2e4")?;
    assert_eq!(advanced.legal_moves().len(), 20);
    assert!(advanced.play_uci("d2d4").is_err());
    assert_eq!(position, BoardPosition::standard());
    Ok(())
}

#[test]
fn repetition_ignores_counters_and_irrelevant_en_passant() -> Result<(), Box<dyn std::error::Error>>
{
    let first = BoardPosition::from_fen(
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        BoardEnumVariant::Standard,
    )?;
    let counters = BoardPosition::from_fen(
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 99 200",
        BoardEnumVariant::Standard,
    )?;
    assert_eq!(first.identity(), counters.identity());
    let without = BoardPosition::from_fen(
        "4k3/8/8/3pP3/8/8/8/4K3 w - - 0 1",
        BoardEnumVariant::Standard,
    )?;
    assert_ne!(first.identity(), without.identity());
    let pinned = BoardPosition::from_fen(
        "k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        BoardEnumVariant::Standard,
    )?;
    let pinned_without = BoardPosition::from_fen(
        "k3r3/8/8/3pP3/8/8/8/4K3 w - - 0 1",
        BoardEnumVariant::Standard,
    )?;
    assert_eq!(pinned.identity(), pinned_without.identity());
    Ok(())
}

#[test]
fn full_identity_survives_hash_collisions() -> Result<(), Box<dyn std::error::Error>> {
    let mut identities =
        HashSet::with_hasher(BuildHasherDefault::<BoardPositionCollisionHasher>::default());
    let initial = BoardPosition::standard();
    identities.insert(initial.identity());
    identities.insert(initial.play_uci("e2e4")?.identity());
    assert_eq!(identities.len(), 2);
    Ok(())
}

#[test]
fn retains_original_castling_rook_identity() -> Result<(), Box<dyn std::error::Error>> {
    let first = BoardPosition::from_fen(
        "4k3/8/8/8/8/8/8/RR2K3 w A - 0 1",
        BoardEnumVariant::Chess960,
    )?;
    let second = BoardPosition::from_fen(
        "4k3/8/8/8/8/8/8/RR2K3 w B - 0 1",
        BoardEnumVariant::Chess960,
    )?;
    assert_ne!(first.identity(), second.identity());
    Ok(())
}

#[test]
fn maps_castling_to_the_selected_uci_convention() -> Result<(), Box<dyn std::error::Error>> {
    let fen = "4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1";
    let standard = BoardPosition::from_fen(fen, BoardEnumVariant::Standard)?;
    let randomized = BoardPosition::from_fen(fen, BoardEnumVariant::Chess960)?;
    assert_eq!(
        standard.play_uci("e1g1")?.placement(),
        randomized.play_uci("e1h1")?.placement()
    );
    assert_eq!(standard.san("e1g1")?, "O-O");
    assert_eq!(randomized.san("e1h1")?, "O-O");
    Ok(())
}

#[test]
fn promotions_require_an_explicit_valid_piece() -> Result<(), Box<dyn std::error::Error>> {
    let position =
        BoardPosition::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", BoardEnumVariant::Standard)?;
    let from: BoardSquare = "a7".parse()?;
    let to: BoardSquare = "a8".parse()?;
    assert!(position.move_intent(from, to, None).is_err());
    assert!(
        position
            .move_intent(from, to, Some(BoardEnumPieceKind::King))
            .is_err()
    );
    for kind in [
        BoardEnumPieceKind::Knight,
        BoardEnumPieceKind::Bishop,
        BoardEnumPieceKind::Rook,
        BoardEnumPieceKind::Queen,
    ] {
        let movement = position.move_intent(from, to, Some(kind))?;
        assert_eq!(movement.promotion(), Some(kind));
        let notation = position.san(&movement.uci(position.variant()))?;
        assert_eq!(position.parse_san(&notation)?, movement);
        assert_ne!(position.play(movement)?.placement(), position.placement());
    }
    Ok(())
}

#[test]
fn en_passant_and_stationary_king_castling_use_authoritative_rules()
-> Result<(), Box<dyn std::error::Error>> {
    let position = BoardPosition::from_fen(
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        BoardEnumVariant::Standard,
    )?;
    assert_eq!(
        position.play_uci("e5d6")?.placement(),
        "4k3/8/3P4/8/8/8/8/4K3"
    );
    let randomized =
        BoardPosition::from_fen("4k3/8/8/8/8/8/8/6KR w H - 0 1", BoardEnumVariant::Chess960)?;
    assert_eq!(
        randomized.play_uci("g1h1")?.placement(),
        "4k3/8/8/8/8/8/8/5RK1"
    );
    assert_eq!(randomized.fen(), randomized.shredder_fen());
    let round_trip = BoardPosition::from_fen(&randomized.fen(), BoardEnumVariant::Chess960)?;
    assert_eq!(round_trip.identity(), randomized.identity());
    Ok(())
}

#[test]
fn refuses_invalid_square_variant_move_and_counter_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(serde_json::from_str::<BoardSquare>("64").is_err());
    assert!(serde_json::from_str::<BoardEnumVariant>("\"atomic\"").is_err());
    let position = BoardPosition::standard();
    assert!(position.parse_uci("0000").is_err());
    assert!(position.parse_uci("P@e4").is_err());
    assert!(position.parse_san("e4+").is_err());
    let maximum = BoardPosition::from_fen(
        "4k3/8/8/8/8/8/8/4K3 b - - 0 1000000",
        BoardEnumVariant::Standard,
    )?;
    assert!(maximum.play_uci("e8e7").is_err());
    let oversized = " ".repeat(BoardPosition::MAX_FEN_BYTES + 1);
    assert!(BoardPosition::from_fen(&oversized, BoardEnumVariant::Standard).is_err());
    Ok(())
}
