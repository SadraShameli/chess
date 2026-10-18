use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPositionError;
use chess::notation::fen::NotationFen;

struct NotationFenFixture;

impl NotationFenFixture {
    const STANDARD: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    const RANDOMIZED: &str = "r3k2r/8/8/8/8/8/8/R3K2R w HAha - 0 1";

    fn padded(bytes: usize) -> String {
        format!(
            "{}{}",
            Self::STANDARD,
            " ".repeat(bytes - Self::STANDARD.len())
        )
    }
}

#[test]
fn standard_fen_round_trip_preserves_all_six_fields() -> Result<(), Box<dyn std::error::Error>> {
    let fen = "4k3/8/8/3pP3/8/8/8/4K3 w - d6 99 200";
    let position = NotationFen::import(fen, BoardEnumVariant::Standard)?;
    assert_eq!(NotationFen::export(&position), fen);
    assert_eq!(position.fullmoves(), 200);
    assert_eq!(position.halfmoves(), 99);
    Ok(())
}

#[test]
fn declared_variant_is_retained_even_for_standard_placement()
-> Result<(), Box<dyn std::error::Error>> {
    let standard = NotationFen::import(NotationFenFixture::STANDARD, BoardEnumVariant::Standard)?;
    let randomized = NotationFen::import(NotationFenFixture::STANDARD, BoardEnumVariant::Chess960)?;
    assert_eq!(standard.variant(), BoardEnumVariant::Standard);
    assert_eq!(randomized.variant(), BoardEnumVariant::Chess960);
    assert_ne!(standard.identity(), randomized.identity());
    assert_eq!(
        NotationFen::export(&randomized),
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w HAha - 0 1"
    );
    Ok(())
}

#[test]
fn chess960_export_uses_rook_file_rights() -> Result<(), Box<dyn std::error::Error>> {
    let position = NotationFen::import(NotationFenFixture::RANDOMIZED, BoardEnumVariant::Chess960)?;
    assert_eq!(
        NotationFen::export(&position),
        NotationFenFixture::RANDOMIZED
    );
    let restored =
        NotationFen::import(&NotationFen::export(&position), BoardEnumVariant::Chess960)?;
    assert_eq!(position, restored);
    Ok(())
}

#[test]
fn accepts_exactly_512_input_bytes_without_changing_the_position()
-> Result<(), Box<dyn std::error::Error>> {
    let fen = NotationFenFixture::padded(512);
    assert_eq!(fen.len(), 512);
    let position = NotationFen::import(&fen, BoardEnumVariant::Standard)?;
    assert_eq!(NotationFen::export(&position), NotationFenFixture::STANDARD);
    Ok(())
}

#[test]
fn rejects_513_input_bytes_instead_of_trimming_before_the_bound() {
    assert!(matches!(
        NotationFen::import(&NotationFenFixture::padded(513), BoardEnumVariant::Standard),
        Err(BoardPositionError::InvalidFenShape)
    ));
}

#[test]
fn invalid_fen_and_non_ascii_are_delegated_to_the_board_boundary() {
    for fen in [
        "8/8/8/8/8/8/8/8 w - - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w - - 0 0",
        "4k3/8/8/8/8/8/8/4K3 w - - 1000001 1",
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1 extra",
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1é",
    ] {
        assert!(
            NotationFen::import(fen, BoardEnumVariant::Standard).is_err(),
            "{fen}"
        );
    }
}

#[test]
fn black_turn_and_maximum_counters_survive_export() -> Result<(), Box<dyn std::error::Error>> {
    let fen = "4k3/8/8/8/8/8/8/4K3 b - - 1000000 1000000";
    let position = NotationFen::import(fen, BoardEnumVariant::Standard)?;
    assert_eq!(NotationFen::export(&position), fen);
    Ok(())
}
