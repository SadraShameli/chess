use std::cell::Cell;
use std::error::Error;
use std::io::Cursor;

use chess::board::enums::BoardEnumVariant;
use chess::clock::duration::ClockDuration;
use chess::game::enums::{GameIdentityError, GameMoveId};
use chess::notation::game::NotationGameEnumWarning;
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::notation::pgn_import::{
    NotationPgnImport, NotationPgnImportEnumRecovery, NotationPgnImportReason,
};

struct NotationPgnImportFixture;

impl NotationPgnImportFixture {
    fn reader(
        input: &[u8],
    ) -> NotationPgnImport<Cursor<&[u8]>, impl FnMut() -> Result<GameMoveId, GameIdentityError>>
    {
        let mut next = 0_u64;
        NotationPgnImport::new(Cursor::new(input), move || {
            next += 1;
            format!("00000000-0000-4000-8000-{next:012x}").parse()
        })
    }
}

#[test]
fn annotated_nested_variations_keep_their_own_positions() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(
        b"{root} 1. e4 $1 {main[%clk 0:09:59.123456789][%csl Ge4][%cal Re2e4]} (1. d4 {side} d5 (1... Nf6 {nested})) e5 2. Nf3 *",
    );
    let game = reader.next_game(&|| false)?.ok_or("missing game")?;
    assert_eq!(game.tree().len(), 7);
    assert_eq!(
        game.tree().node(game.tree().root())?.annotations().notes(),
        "root"
    );
    let first = game.tree().node(game.tree().root())?.children()[0];
    let node = game.tree().node(first)?;
    assert_eq!(node.annotations().notes(), "main");
    assert_eq!(node.annotations().nags()[0].value(), 1);
    assert_eq!(node.annotations().markings().len(), 2);
    assert_eq!(
        game.clock(first),
        Some(ClockDuration::from_nanos(599_123_456_789))
    );
    assert_eq!(game.tree().node(game.mainline_tip())?.san(), Some("Nf3"));
    assert!(game.tree().nodes().all(|node| node.clock().is_none()));
    assert!(reader.next_game(&|| false)?.is_none());
    Ok(())
}

#[test]
fn unknown_tags_preserve_order_and_decoded_source_values() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(
        br#"[White "  ALICE  "] [Custom_Z "quote \" and slash \\"] [Custom_A "second"] [TimeControl "40/7200:3600"] *"#,
    );
    let game = reader.next_game(&|| false)?.ok_or("missing game")?;
    assert_eq!(
        game.metadata()
            .known()
            .white
            .as_ref()
            .ok_or("white")?
            .as_str(),
        "  ALICE  "
    );
    let tags = game.metadata().extensions();
    assert_eq!(tags[0].name(), "Custom_Z");
    assert_eq!(tags[0].value().as_str(), "quote \" and slash \\");
    assert_eq!(tags[1].name(), "Custom_A");
    assert_eq!(
        game.time_control().ok_or("control")?.as_str(),
        "40/7200:3600"
    );
    Ok(())
}

#[test]
fn header_absence_is_distinct_from_the_required_marker() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(b"1. e4 * [Result \"*\"] *");
    let first = reader.next_game(&|| false)?.ok_or("first")?;
    let second = reader.next_game(&|| false)?.ok_or("second")?;
    assert_eq!(first.result(), NotationGameIdentityEnumResult::Unfinished);
    assert_eq!(first.result_header(), None);
    assert_eq!(second.result_header(), Some(second.result()));
    Ok(())
}

#[test]
fn black_to_move_custom_start_keeps_fullmove_number() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(
        b"[SetUp \"1\"] [FEN \"4k3/8/8/8/8/8/8/4K3 b - - 0 17\"] 17... Kf7 *",
    );
    let game = reader.next_game(&|| false)?.ok_or("game")?;
    assert_eq!(game.start().position().fullmoves(), 17);
    assert_eq!(game.tree().node(game.mainline_tip())?.san(), Some("Kf7"));
    Ok(())
}

#[test]
fn castling_is_resolved_by_the_declared_variant() -> Result<(), Box<dyn Error>> {
    for input in [
        "[SetUp \"1\"] [FEN \"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1\"] 1. O-O O-O-O *",
        "[Variant \"Chess960\"] [SetUp \"1\"] [FEN \"r3k2r/8/8/8/8/8/8/R3K2R w HAha - 0 1\"] 1. O-O O-O-O *",
    ] {
        let mut reader = NotationPgnImportFixture::reader(input.as_bytes());
        let game = reader.next_game(&|| false)?.ok_or("game")?;
        assert_eq!(game.tree().len(), 3);
        let movement = game
            .tree()
            .node(game.mainline_tip())?
            .movement()
            .ok_or("move")?;
        assert!(movement.castling_rook().is_some());
    }
    Ok(())
}

#[test]
fn promotion_and_en_passant_use_domain_legality() -> Result<(), Box<dyn Error>> {
    for input in [
        "[SetUp \"1\"] [FEN \"4k3/P7/8/8/8/8/8/4K3 w - - 0 1\"] 1. a8=Q+ *",
        "[SetUp \"1\"] [FEN \"4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2\"] 2. exd6 *",
    ] {
        let mut reader = NotationPgnImportFixture::reader(input.as_bytes());
        let game = reader.next_game(&|| false)?.ok_or("game")?;
        assert_eq!(game.tree().len(), 2);
    }
    Ok(())
}

#[test]
fn semicolon_payloads_preserve_braces_slashes_and_empty_lines() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(b"1. e4 ;\n;{literal} \\path\\file\r\n;\n*");
    let game = reader.next_game(&|| false)?.ok_or("game")?;
    assert_eq!(
        game.tree().node(game.mainline_tip())?.annotations().notes(),
        "\n{literal} \\path\\file\n"
    );
    Ok(())
}

#[test]
fn brace_backslashes_are_plaintext() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(br"1. e4 {a\\b\q\} *");
    let game = reader.next_game(&|| false)?.ok_or("game")?;
    assert_eq!(
        game.tree().node(game.mainline_tip())?.annotations().notes(),
        "a\\\\b\\q\\"
    );
    Ok(())
}

#[test]
fn unknown_directives_stay_plaintext_with_typed_warnings() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(
        b"1. e4 { A[%future exact] B[%clk 0:00:00.000000001] C} *",
    );
    let game = reader.next_game(&|| false)?.ok_or("game")?;
    assert_eq!(
        game.tree().node(game.mainline_tip())?.annotations().notes(),
        " A[%future exact] B C"
    );
    assert_eq!(
        game.clock(game.mainline_tip()),
        Some(ClockDuration::from_nanos(1))
    );
    assert!(
        matches!(&game.warnings()[0], NotationGameEnumWarning::UnknownDirective { directive, .. } if directive == "[%future exact]")
    );
    Ok(())
}

#[test]
fn invalid_branch_excludes_whole_game_then_recovers() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(b"1. e4 (1. d4 d5 2. Ke3) e5 * 1. d4 d5 *");
    let error = reader
        .next_game(&|| false)
        .err()
        .ok_or("expected exclusion")?;
    assert_eq!(error.game_ordinal(), 1);
    assert_eq!(error.recovery(), NotationPgnImportEnumRecovery::NextGame);
    assert!(matches!(error.reason(), NotationPgnImportReason::Move(_)));
    let game = reader.next_game(&|| false)?.ok_or("second game")?;
    assert_eq!(game.tree().len(), 3);
    Ok(())
}

#[test]
fn result_mismatch_excludes_without_losing_next_game() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(b"[Result \"1-0\"] 1. e4 0-1 *");
    let error = reader.next_game(&|| false).err().ok_or("mismatch")?;
    assert_eq!(error.recovery(), NotationPgnImportEnumRecovery::NextGame);
    assert!(reader.next_game(&|| false)?.is_some());
    Ok(())
}

#[test]
fn malformed_tokens_are_never_silently_dropped() -> Result<(), Box<dyn Error>> {
    for input in [
        "1. e4 junk *",
        "1. e4 $256 *",
        "1. e4 $bad *",
        "[White bad] *",
        "1. e4 } *",
        "1. e4 ) *",
        "1. e4 (1. d4 *",
        "1. e4",
        "[Result \"win\"] *",
        "1. e4 {unterminated",
        "1. e4 ( ) *",
        "1. e4 {[%clk 0:61:00]} *",
        "1. e4 {[%csl Pe4]} *",
        "1. e4 {[%cal Ge4e4]} *",
        "[White \"a\"] [White \"b\"] *",
    ] {
        let mut reader = NotationPgnImportFixture::reader(input.as_bytes());
        assert!(reader.next_game(&|| false).is_err(), "{input}");
    }
    Ok(())
}

#[test]
fn repeated_variation_occurrences_cannot_overwrite_annotations() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(b"1. e4 (1. d4 {first}) (1. d4 {second}) *");
    let error = reader.next_game(&|| false).err().ok_or("repeat")?;
    assert!(matches!(
        error.reason(),
        NotationPgnImportReason::UnsupportedOccurrence
    ));
    assert_eq!(error.recovery(), NotationPgnImportEnumRecovery::NextGame);
    Ok(())
}

#[test]
fn lexical_failure_without_a_proven_boundary_terminates() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(b"1. e4 {broken * [Event \"fake\"] 1. d4 *");
    let error = reader.next_game(&|| false).err().ok_or("error")?;
    assert_eq!(error.recovery(), NotationPgnImportEnumRecovery::Terminated);
    assert!(reader.next_game(&|| false).is_err());
    Ok(())
}

#[test]
fn locations_are_byte_exact_across_unicode_and_crlf() -> Result<(), Box<dyn Error>> {
    let input = "[Event \"é\"]\r\n1. e4 nonsense *";
    let mut reader = NotationPgnImportFixture::reader(input.as_bytes());
    let error = reader.next_game(&|| false).err().ok_or("error")?;
    let offset = input.find("nonsense").ok_or("offset")?;
    assert_eq!(error.location().byte_offset(), u64::try_from(offset)?);
    assert_eq!(error.location().line(), 2);
    assert_eq!(error.location().column(), 7);
    Ok(())
}

#[test]
fn cancellation_is_checked_during_long_lexical_work() -> Result<(), Box<dyn Error>> {
    let text = format!("1. e4 {{{}}} *", "x".repeat(20_000));
    let checks = Cell::new(0_u32);
    let cancellation = || {
        checks.set(checks.get() + 1);
        checks.get() > 80
    };
    let mut reader = NotationPgnImportFixture::reader(text.as_bytes());
    let error = reader.next_game(&cancellation).err().ok_or("cancel")?;
    assert!(matches!(error.reason(), NotationPgnImportReason::Cancelled));
    assert_eq!(error.recovery(), NotationPgnImportEnumRecovery::Terminated);
    Ok(())
}

#[test]
fn invalid_utf8_and_nul_are_explicit_errors() {
    for input in [b"{\xff} *".as_slice(), b"{\0} *", b"[Event \"\xff\"] *"] {
        let mut reader = NotationPgnImportFixture::reader(input);
        assert!(reader.next_game(&|| false).is_err());
    }
}

#[test]
fn chess960_without_fen_is_not_silently_standard() {
    let mut reader = NotationPgnImportFixture::reader(b"[Variant \"Chess960\"] *");
    assert!(reader.next_game(&|| false).is_err());
}

#[test]
fn declared_standard_is_preserved() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(b"[Variant \"Standard\"] *");
    let game = reader.next_game(&|| false)?.ok_or("game")?;
    assert_eq!(
        game.start().position().variant(),
        BoardEnumVariant::Standard
    );
    Ok(())
}

#[test]
fn unavailable_ratings_preserve_the_original_optional_source_text() -> Result<(), Box<dyn Error>> {
    let mut reader =
        NotationPgnImportFixture::reader(b"[WhiteElo \"?\"] [BlackElo \"4294967296\"] *");
    let game = reader.next_game(&|| false)?.ok_or("game")?;
    assert!(game.metadata().known().white_elo.is_none());
    assert!(game.metadata().known().black_elo.is_none());
    assert!(
        matches!(&game.warnings()[0], NotationGameEnumWarning::UnavailableRating { value, .. } if value.as_str() == "?")
    );
    assert!(
        matches!(&game.warnings()[1], NotationGameEnumWarning::UnavailableRating { value, .. } if value.as_str() == "4294967296")
    );
    Ok(())
}

#[test]
fn malformed_directive_prefix_cannot_swallow_a_later_clock() -> Result<(), Box<dyn Error>> {
    let mut reader =
        NotationPgnImportFixture::reader(b"{literal [% broken[%clk 0:00:00.000000001]} *");
    let game = reader.next_game(&|| false)?.ok_or("game")?;
    assert_eq!(
        game.tree().node(game.tree().root())?.annotations().notes(),
        "literal [% broken"
    );
    assert_eq!(
        game.clock(game.tree().root()),
        Some(ClockDuration::from_nanos(1))
    );
    Ok(())
}

#[test]
fn root_annotations_and_maximum_nanosecond_clock_remain_typed() -> Result<(), Box<dyn Error>> {
    let mut reader = NotationPgnImportFixture::reader(
        b"$7 {root[%csl Ye4][%cal Ba1h8][%clk 5124095:34:33.709551615]} 1. e4 *",
    );
    let game = reader.next_game(&|| false)?.ok_or("game")?;
    let root = game.tree().root();
    assert_eq!(game.clock(root), Some(ClockDuration::from_nanos(u64::MAX)));
    assert_eq!(game.tree().node(root)?.annotations().notes(), "root");
    assert_eq!(game.tree().node(root)?.annotations().nags()[0].value(), 7);
    assert_eq!(game.tree().node(root)?.annotations().markings().len(), 2);
    Ok(())
}

#[test]
fn tag_escape_pairs_preserve_backslash_and_quote_runs() -> Result<(), Box<dyn Error>> {
    for (input, expected) in [
        (br#"[Custom "\\\\"] *"#.as_slice(), r"\\"),
        (br#"[Custom "\\\""] *"#.as_slice(), r#"\""#),
        (br#"[Custom "\\\\\""] *"#.as_slice(), r#"\\""#),
    ] {
        let mut reader = NotationPgnImportFixture::reader(input);
        let game = reader.next_game(&|| false)?.ok_or("game")?;
        assert_eq!(game.metadata().extensions()[0].value().as_str(), expected);
    }
    Ok(())
}
