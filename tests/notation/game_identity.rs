use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, DefaultHasher, Hash, Hasher};

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::r#move::{BoardMove, BoardMoveError};
use chess::board::position::BoardPosition;
use chess::board::start::{BoardStart, BoardStartIndex};
use chess::game::enums::GameEnumResult;
use chess::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataError, GameMetadataExtension,
    GameMetadataKnownTags, GameMetadataText,
};
use chess::notation::game_identity::{
    NotationGameIdentity, NotationGameIdentityEnumMatch, NotationGameIdentityEnumResult,
    NotationGameIdentityError,
};

struct NotationGameIdentityFixture<'a> {
    white: Option<&'a str>,
    black: Option<&'a str>,
    date: Option<GameMetadataDate>,
    result: Option<NotationGameIdentityEnumResult>,
}

impl<'a> NotationGameIdentityFixture<'a> {
    fn complete() -> Result<Self, GameMetadataError> {
        Ok(Self {
            white: Some("Alice"),
            black: Some("Bob"),
            date: Some(GameMetadataDate::new(Some(2026), Some(10), Some(17))?),
            result: Some(NotationGameIdentityEnumResult::Completed(
                GameEnumResult::Draw,
            )),
        })
    }

    fn metadata(&self) -> Result<GameMetadata, GameMetadataError> {
        GameMetadata::new(
            GameMetadataKnownTags {
                white: Self::text(self.white)?,
                black: Self::text(self.black)?,
                date: self.date,
                ..GameMetadataKnownTags::default()
            },
            Vec::new(),
        )
    }

    fn text(value: Option<&str>) -> Result<Option<GameMetadataText>, GameMetadataError> {
        value
            .map(|value| GameMetadataText::new(value.to_owned()))
            .transpose()
    }

    fn build(
        &self,
        start: &BoardPosition,
        mainline: &[BoardMove],
    ) -> Result<NotationGameIdentity, Box<dyn std::error::Error>> {
        Ok(NotationGameIdentity::new(
            start,
            mainline,
            &self.metadata()?,
            self.result,
        )?)
    }

    fn standard(&self) -> Result<NotationGameIdentity, Box<dyn std::error::Error>> {
        self.build(&BoardPosition::standard(), &[])
    }

    fn line(start: &BoardPosition, moves: &[&str]) -> Result<Vec<BoardMove>, BoardMoveError> {
        let mut position = start.clone();
        let mut mainline = Vec::with_capacity(moves.len());
        for value in moves {
            let movement = position.parse_uci(value)?;
            position = position.play(movement)?;
            mainline.push(movement);
        }
        Ok(mainline)
    }

    fn hash(identity: &NotationGameIdentity) -> u64 {
        let mut hasher = DefaultHasher::new();
        identity.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Default)]
struct NotationGameIdentityCollisionHasher;

impl Hasher for NotationGameIdentityCollisionHasher {
    fn finish(&self) -> u64 {
        0
    }

    fn write(&mut self, _bytes: &[u8]) {}
}

#[test]
fn normalizes_fixed_unicode_examples_on_both_players() -> Result<(), Box<dyn std::error::Error>> {
    for (left, right, expected) in [
        ("\u{2003}ÉLODIE\u{a0}", "E\u{301}lodie", "élodie"),
        ("Straße", "STRASSE", "strasse"),
        ("Σσς", "σσσ", "σσσ"),
        ("I", "i", "i"),
        ("İ", "i\u{307}", "i\u{307}"),
        ("ı", "ı", "ı"),
    ] {
        let mut fixture = NotationGameIdentityFixture::complete()?;
        fixture.white = Some(left);
        fixture.black = Some(left);
        let first = fixture.standard()?;
        fixture.white = Some(right);
        fixture.black = Some(right);
        let second = fixture.standard()?;
        assert_eq!(first.white(), Some(expected));
        assert_eq!(first.black(), Some(expected));
        assert_eq!(first, second);
        assert_eq!(first.compare(&second), NotationGameIdentityEnumMatch::Exact);
        assert_eq!(
            NotationGameIdentityFixture::hash(&first),
            NotationGameIdentityFixture::hash(&second)
        );
    }
    Ok(())
}

#[test]
fn keeps_non_turkic_i_classes_and_internal_whitespace_distinct()
-> Result<(), Box<dyn std::error::Error>> {
    for (left, right) in [
        ("I", "İ"),
        ("i", "ı"),
        ("İ", "ı"),
        ("Anne  Marie", "Anne Marie"),
        ("é", "e"),
    ] {
        let mut fixture = NotationGameIdentityFixture::complete()?;
        fixture.white = Some(left);
        let first = fixture.standard()?;
        fixture.white = Some(right);
        let second = fixture.standard()?;
        assert_ne!(first, second);
        assert_eq!(
            first.compare(&second),
            NotationGameIdentityEnumMatch::Different
        );
    }
    Ok(())
}

#[test]
fn treats_empty_unicode_whitespace_and_pgn_player_placeholder_as_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationGameIdentityFixture::complete()?;
    fixture.white = None;
    fixture.black = None;
    let missing = fixture.standard()?;
    for value in ["", " \u{2003}\u{a0}\u{202f}", "?", "\u{2003}?\u{a0}"] {
        fixture.white = Some(value);
        fixture.black = Some(value);
        let candidate = fixture.standard()?;
        assert_eq!(candidate.white(), None);
        assert_eq!(candidate.black(), None);
        assert_eq!(candidate, missing);
        assert_eq!(
            candidate.compare(&missing),
            NotationGameIdentityEnumMatch::Potential
        );
    }
    fixture.white = Some("?Alice");
    assert_eq!(fixture.standard()?.white(), Some("?alice"));
    Ok(())
}

#[test]
fn preserves_borrowed_raw_metadata_and_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationGameIdentityFixture::complete()?;
    fixture.white = Some("\u{a0}E\u{301}LODIE\u{2003}");
    fixture.black = Some("Straße");
    let metadata = fixture.metadata()?;
    let original = metadata.clone();
    let identity =
        NotationGameIdentity::new(&BoardPosition::standard(), &[], &metadata, fixture.result)?;
    assert_eq!(metadata, original);
    assert_eq!(
        metadata
            .known()
            .white
            .as_ref()
            .map(GameMetadataText::as_str),
        fixture.white
    );
    assert_eq!(
        metadata
            .known()
            .black
            .as_ref()
            .map(GameMetadataText::as_str),
        fixture.black
    );
    let canonical = NotationGameIdentityFixture {
        white: identity.white(),
        black: identity.black(),
        date: identity.date(),
        result: identity.result(),
    };
    assert_eq!(identity, canonical.standard()?);
    Ok(())
}

#[test]
fn canonicalizes_fen_syntax_and_retains_full_start_counters()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    let start = BoardPosition::standard();
    let syntax = BoardPosition::from_fen(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR   w   qkQK - 000 0001",
        BoardEnumVariant::Standard,
    )?;
    let first = fixture.build(&start, &[])?;
    let equivalent = fixture.build(&syntax, &[])?;
    assert_eq!(first, equivalent);
    assert_eq!(first.structural_key().variant(), BoardEnumVariant::Standard);
    assert_eq!(
        first.structural_key().initial_fen(),
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
    );
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 1 1",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 2",
    ] {
        let changed = fixture.build(
            &BoardPosition::from_fen(fen, BoardEnumVariant::Standard)?,
            &[],
        )?;
        assert_ne!(first, changed);
        assert_eq!(first.position_identity(), changed.position_identity());
        assert_eq!(
            first.compare(&changed),
            NotationGameIdentityEnumMatch::Different
        );
    }
    Ok(())
}

#[test]
fn distinguishes_turn_castling_and_legally_available_en_passant()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    for (left, right) in [
        (
            "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/8/8/8/4K3 b - - 0 1",
        ),
        (
            "4k3/8/8/8/8/8/8/4K2R w K - 0 1",
            "4k3/8/8/8/8/8/8/4K2R w - - 0 1",
        ),
        (
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "4k3/8/8/3pP3/8/8/8/4K3 w - - 0 1",
        ),
    ] {
        let first = fixture.build(
            &BoardPosition::from_fen(left, BoardEnumVariant::Standard)?,
            &[],
        )?;
        let second = fixture.build(
            &BoardPosition::from_fen(right, BoardEnumVariant::Standard)?,
            &[],
        )?;
        assert_ne!(first.position_identity(), second.position_identity());
        assert_ne!(first.structural_key(), second.structural_key());
        assert_eq!(
            first.compare(&second),
            NotationGameIdentityEnumMatch::Different
        );
    }
    Ok(())
}

#[test]
fn position_identity_excludes_irrelevant_and_pinned_en_passant()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    for (with_ep, without_ep) in [
        (
            "4k3/8/8/3p4/8/8/8/4K3 w - d6 0 1",
            "4k3/8/8/3p4/8/8/8/4K3 w - - 0 1",
        ),
        (
            "k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "k3r3/8/8/3pP3/8/8/8/4K3 w - - 0 1",
        ),
    ] {
        let first = fixture.build(
            &BoardPosition::from_fen(with_ep, BoardEnumVariant::Standard)?,
            &[],
        )?;
        let second = fixture.build(
            &BoardPosition::from_fen(without_ep, BoardEnumVariant::Standard)?,
            &[],
        )?;
        assert_eq!(first.position_identity(), second.position_identity());
        assert_ne!(first, second);
    }
    Ok(())
}

#[test]
fn separates_standard_from_chess960_but_ignores_frc_dfrc_provenance()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    let frc = BoardStart::chess960(BoardStartIndex::STANDARD)?;
    let dfrc = BoardStart::dfrc(BoardStartIndex::STANDARD, BoardStartIndex::STANDARD)?;
    let first = fixture.build(frc.position(), &[])?;
    let second = fixture.build(dfrc.position(), &[])?;
    assert_eq!(first, second);
    assert_eq!(first.compare(&second), NotationGameIdentityEnumMatch::Exact);
    let standard = fixture.standard()?;
    assert_ne!(first.position_identity(), standard.position_identity());
    assert_eq!(
        first.compare(&standard),
        NotationGameIdentityEnumMatch::Different
    );
    let custom = BoardPosition::from_fen(frc.initial_fen(), BoardEnumVariant::Chess960)?;
    assert_eq!(first, fixture.build(&custom, &[])?);
    Ok(())
}

#[test]
fn structural_variant_stays_distinct_when_fen_and_moves_are_identical()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    let fen = "4k3/8/8/8/8/8/8/4K3 w - - 0 1";
    let standard = BoardPosition::from_fen(fen, BoardEnumVariant::Standard)?;
    let chess960 = BoardPosition::from_fen(fen, BoardEnumVariant::Chess960)?;
    let first = fixture.build(&standard, &[standard.parse_uci("e1d1")?])?;
    let second = fixture.build(&chess960, &[chess960.parse_uci("e1d1")?])?;
    assert_eq!(
        first.structural_key().initial_fen(),
        second.structural_key().initial_fen()
    );
    assert_eq!(
        first.structural_key().mainline_uci(),
        second.structural_key().mainline_uci()
    );
    assert_ne!(first.structural_key(), second.structural_key());
    assert_ne!(first, second);
    assert_eq!(
        first.compare(&second),
        NotationGameIdentityEnumMatch::Different
    );
    Ok(())
}

#[test]
fn frc_dfrc_generation_provenance_does_not_affect_same_played_line()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    let index = BoardStartIndex::try_from(42)?;
    let frc = BoardStart::chess960(index)?;
    let dfrc = BoardStart::dfrc(index, index)?;
    assert_ne!(frc.selection(), dfrc.selection());
    let first = fixture.build(
        frc.position(),
        &NotationGameIdentityFixture::line(frc.position(), &["e2e4", "e7e5"])?,
    )?;
    let second = fixture.build(
        dfrc.position(),
        &NotationGameIdentityFixture::line(dfrc.position(), &["e2e4", "e7e5"])?,
    )?;
    assert_eq!(first, second);
    assert_eq!(first.compare(&second), NotationGameIdentityEnumMatch::Exact);
    Ok(())
}

#[test]
fn different_legal_move_orders_to_same_final_position_remain_different()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    let start = BoardPosition::standard();
    let first_line = NotationGameIdentityFixture::line(&start, &["g1f3", "g8f6", "b1c3", "b8c6"])?;
    let second_line = NotationGameIdentityFixture::line(&start, &["b1c3", "b8c6", "g1f3", "g8f6"])?;
    let first_final = first_line
        .iter()
        .try_fold(start.clone(), |position, &movement| position.play(movement))?;
    let second_final = second_line
        .iter()
        .try_fold(start.clone(), |position, &movement| position.play(movement))?;
    assert_eq!(first_final, second_final);
    let first = fixture.build(&start, &first_line)?;
    let second = fixture.build(&start, &second_line)?;
    assert_ne!(first, second);
    assert_eq!(
        first.compare(&second),
        NotationGameIdentityEnumMatch::Different
    );
    Ok(())
}

#[test]
fn caches_canonical_ordered_mainline_from_real_prior_positions()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    let start = BoardPosition::standard();
    let uci = NotationGameIdentityFixture::line(&start, &["e2e4", "e7e5", "g1f3"])?;
    let first = fixture.build(&start, &uci)?;
    assert_eq!(
        first.structural_key().mainline_uci(),
        ["e2e4", "e7e5", "g1f3"]
    );
    let after_white = start.play(uci[0])?;
    let after_black = after_white.play(uci[1])?;
    let san = [
        start.parse_san("e4")?,
        after_white.parse_san("e5")?,
        after_black.parse_san("Nf3")?,
    ];
    assert_eq!(first, fixture.build(&start, &san)?);
    assert_eq!(first.position_identity(), &start.identity());
    assert_eq!(start, BoardPosition::standard());
    assert_ne!(
        first.structural_key(),
        fixture.build(&start, &uci[..2])?.structural_key()
    );
    let alternative = NotationGameIdentityFixture::line(&start, &["d2d4", "d7d5", "c1f4"])?;
    assert_eq!(
        first.compare(&fixture.build(&start, &alternative)?),
        NotationGameIdentityEnumMatch::Different
    );
    Ok(())
}

#[test]
fn retains_chess960_rook_target_castling_and_promotion_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    for (fen, variant, token) in [
        (
            "4k3/8/8/8/8/8/8/4K2R w K - 0 1",
            BoardEnumVariant::Standard,
            "e1g1",
        ),
        (
            "4k3/8/8/8/8/8/8/4K2R w H - 0 1",
            BoardEnumVariant::Chess960,
            "e1h1",
        ),
        (
            "4k3/8/8/8/8/8/8/6KR w H - 0 1",
            BoardEnumVariant::Chess960,
            "g1h1",
        ),
        (
            "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            BoardEnumVariant::Standard,
            "a7a8n",
        ),
        (
            "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            BoardEnumVariant::Chess960,
            "a7a8q",
        ),
    ] {
        let start = BoardPosition::from_fen(fen, variant)?;
        let identity = fixture.build(&start, &[start.parse_uci(token)?])?;
        assert_eq!(identity.structural_key().mainline_uci(), [token]);
    }
    Ok(())
}

#[test]
fn rejects_illegal_reordered_and_late_invalid_typed_moves() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = NotationGameIdentityFixture::complete()?;
    let metadata = fixture.metadata()?;
    let start = BoardPosition::standard();
    let line = NotationGameIdentityFixture::line(&start, &["e2e4", "e7e5", "g1f3"])?;
    for (moves, expected_index) in [
        (vec![line[1], line[0]], 0),
        (vec![line[0], line[2], line[1]], 1),
        (vec![line[0], line[1], line[0]], 2),
    ] {
        let error = NotationGameIdentity::new(&start, &moves, &metadata, fixture.result);
        assert!(
            matches!(error, Err(NotationGameIdentityError::Mainline { index, source: BoardMoveError::IllegalMove }) if index == expected_index)
        );
    }
    let pawnless =
        BoardPosition::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1", BoardEnumVariant::Standard)?;
    assert!(NotationGameIdentity::new(&pawnless, &line[..1], &metadata, fixture.result).is_err());
    Ok(())
}

#[test]
fn each_known_player_and_date_component_conflict_is_different()
-> Result<(), Box<dyn std::error::Error>> {
    let baseline = NotationGameIdentityFixture::complete()?.standard()?;
    let mut fixture = NotationGameIdentityFixture::complete()?;
    fixture.white = Some("Carol");
    assert_eq!(
        baseline.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Different
    );
    fixture.white = Some("Alice");
    fixture.black = Some("Carol");
    assert_eq!(
        baseline.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Different
    );
    fixture.black = Some("Bob");
    for (year, month, day) in [(2025, 10, 17), (2026, 9, 17), (2026, 10, 16)] {
        fixture.date = Some(GameMetadataDate::new(Some(year), Some(month), Some(day))?);
        let changed = fixture.standard()?;
        assert_ne!(baseline, changed);
        assert_eq!(
            baseline.compare(&changed),
            NotationGameIdentityEnumMatch::Different
        );
    }
    fixture.white = Some("Bob");
    fixture.black = Some("Alice");
    assert_eq!(
        baseline.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Different
    );
    Ok(())
}

#[test]
fn preserves_partial_dates_and_matches_only_compatible_known_components()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationGameIdentityFixture::complete()?;
    let full = fixture.standard()?;
    for components in [
        (Some(2026), None, None),
        (Some(2026), Some(10), None),
        (None, Some(10), Some(17)),
        (None, None, Some(17)),
        (None, Some(10), None),
    ] {
        let date = GameMetadataDate::new(components.0, components.1, components.2)?;
        fixture.date = Some(date);
        let partial = fixture.standard()?;
        assert_eq!(partial.date(), Some(date));
        assert_ne!(partial, full);
        assert_eq!(
            partial.compare(&full),
            NotationGameIdentityEnumMatch::Potential
        );
        assert_eq!(
            full.compare(&partial),
            NotationGameIdentityEnumMatch::Potential
        );
        assert_eq!(
            partial.compare(&partial),
            NotationGameIdentityEnumMatch::Potential
        );
    }
    fixture.date = Some(GameMetadataDate::new(Some(2025), None, None)?);
    assert_eq!(
        full.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Different
    );
    fixture.date = Some(GameMetadataDate::new(None, Some(9), None)?);
    assert_eq!(
        full.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Different
    );
    fixture.date = Some(GameMetadataDate::new(None, None, Some(16))?);
    assert_eq!(
        full.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Different
    );
    fixture.date = Some(GameMetadataDate::new(Some(2026), None, None)?);
    let year = fixture.standard()?;
    fixture.date = Some(GameMetadataDate::new(None, Some(10), None)?);
    assert_eq!(
        year.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Potential
    );
    Ok(())
}

#[test]
fn missing_and_entirely_unknown_dates_have_one_canonical_value()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationGameIdentityFixture::complete()?;
    fixture.date = None;
    let absent = fixture.standard()?;
    fixture.date = Some(GameMetadataDate::new(None, None, None)?);
    let unknown = fixture.standard()?;
    assert_eq!(unknown.date(), None);
    assert_eq!(absent, unknown);
    assert_eq!(
        NotationGameIdentityFixture::hash(&absent),
        NotationGameIdentityFixture::hash(&unknown)
    );
    assert_eq!(
        absent.compare(&unknown),
        NotationGameIdentityEnumMatch::Potential
    );
    Ok(())
}

#[test]
fn typed_results_own_canonical_pgn_tokens_and_all_known_conflicts()
-> Result<(), Box<dyn std::error::Error>> {
    let results = [
        (NotationGameIdentityEnumResult::Unfinished, "*"),
        (
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::White,
            )),
            "1-0",
        ),
        (
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw),
            "1/2-1/2",
        ),
        (
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::Black,
            )),
            "0-1",
        ),
    ];
    for (index, (result, token)) in results.iter().enumerate() {
        assert_eq!(result.pgn_token(), *token);
        let mut fixture = NotationGameIdentityFixture::complete()?;
        fixture.result = Some(*result);
        let first = fixture.standard()?;
        assert_eq!(first.result(), Some(*result));
        assert_eq!(first.compare(&first), NotationGameIdentityEnumMatch::Exact);
        for (other_index, (other, _)) in results.iter().enumerate() {
            fixture.result = Some(*other);
            let second = fixture.standard()?;
            assert_eq!(first == second, index == other_index);
            assert_eq!(
                first.compare(&second),
                if index == other_index {
                    NotationGameIdentityEnumMatch::Exact
                } else {
                    NotationGameIdentityEnumMatch::Different
                }
            );
        }
        fixture.result = None;
        let missing = fixture.standard()?;
        assert_ne!(first, missing);
        assert_eq!(
            first.compare(&missing),
            NotationGameIdentityEnumMatch::Potential
        );
        assert_eq!(
            missing.compare(&first),
            NotationGameIdentityEnumMatch::Potential
        );
        assert_eq!(
            missing.compare(&missing),
            NotationGameIdentityEnumMatch::Potential
        );
    }
    Ok(())
}

#[test]
fn compatible_missing_players_are_potential_and_never_mask_known_conflicts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationGameIdentityFixture::complete()?;
    let full = fixture.standard()?;
    fixture.white = None;
    let missing_white = fixture.standard()?;
    assert_ne!(full, missing_white);
    assert_eq!(
        full.compare(&missing_white),
        NotationGameIdentityEnumMatch::Potential
    );
    fixture.white = Some("Alice");
    fixture.black = None;
    assert_eq!(
        full.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Potential
    );
    fixture.white = Some("Carol");
    fixture.date = None;
    fixture.result = None;
    assert_eq!(
        full.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Different
    );
    fixture.white = None;
    fixture.black = Some("Carol");
    assert_eq!(
        full.compare(&fixture.standard()?),
        NotationGameIdentityEnumMatch::Different
    );
    Ok(())
}

#[test]
fn potential_matching_is_nontransitive_and_is_separate_from_equality()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationGameIdentityFixture::complete()?;
    let alice = fixture.standard()?;
    fixture.white = None;
    let unknown = fixture.standard()?;
    fixture.white = Some("Carol");
    let carol = fixture.standard()?;
    assert_eq!(
        alice.compare(&unknown),
        NotationGameIdentityEnumMatch::Potential
    );
    assert_eq!(
        unknown.compare(&carol),
        NotationGameIdentityEnumMatch::Potential
    );
    assert_eq!(
        alice.compare(&carol),
        NotationGameIdentityEnumMatch::Different
    );
    assert_ne!(alice, unknown);
    assert_ne!(unknown, carol);
    assert_ne!(alice, carol);
    Ok(())
}

#[test]
fn excludes_nonidentity_metadata_annotations_clocks_and_source_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationGameIdentityFixture::complete()?;
    let original = fixture.metadata()?;
    let mut known = original.known().clone();
    known.event = NotationGameIdentityFixture::text(Some("Other tournament"))?;
    known.site = NotationGameIdentityFixture::text(Some("Other site"))?;
    known.round = NotationGameIdentityFixture::text(Some("19"))?;
    known.white_elo = Some(2400);
    known.black_elo = Some(2500);
    known.opening = NotationGameIdentityFixture::text(Some("Other opening"))?;
    known.variation = NotationGameIdentityFixture::text(Some("Side line"))?;
    known.annotator = NotationGameIdentityFixture::text(Some("Editor"))?;
    let extensions = [
        ("SourceId", "another-source"),
        ("Annotation", "Changed note [%cal Ge2e4]"),
        ("Clock", "0:05:00"),
    ]
    .into_iter()
    .map(|(name, value)| {
        GameMetadataExtension::new(name.to_owned(), GameMetadataText::new(value.to_owned())?)
    })
    .collect::<Result<Vec<_>, GameMetadataError>>()?;
    let changed = GameMetadata::new(known, extensions)?;
    let start = BoardPosition::standard();
    let first = NotationGameIdentity::new(&start, &[], &original, fixture.result)?;
    let second = NotationGameIdentity::new(&start, &[], &changed, fixture.result)?;
    assert_eq!(first, second);
    assert_eq!(first.compare(&second), NotationGameIdentityEnumMatch::Exact);
    assert_eq!(
        NotationGameIdentityFixture::hash(&first),
        NotationGameIdentityFixture::hash(&second)
    );
    Ok(())
}

#[test]
fn full_equality_survives_constant_hash_collisions() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationGameIdentityFixture::complete()?;
    let first = fixture.standard()?;
    fixture.white = Some("Carol");
    let second = fixture.standard()?;
    fixture.white = None;
    let potential = fixture.standard()?;
    let mut identities =
        HashSet::with_hasher(BuildHasherDefault::<NotationGameIdentityCollisionHasher>::default());
    assert!(identities.insert(first));
    assert!(identities.insert(second));
    assert!(identities.insert(potential));
    assert!(!identities.insert(fixture.standard()?));
    assert_eq!(identities.len(), 3);
    Ok(())
}

#[test]
fn structural_candidate_bucket_reaches_missing_metadata_under_collisions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationGameIdentityFixture::complete()?;
    let full = fixture.standard()?;
    fixture.white = None;
    fixture.date = None;
    fixture.result = None;
    let missing = fixture.standard()?;
    let mut buckets =
        HashMap::with_hasher(BuildHasherDefault::<NotationGameIdentityCollisionHasher>::default());
    buckets
        .entry(full.structural_key().clone())
        .or_insert_with(Vec::new)
        .push(full);
    let different_start = BoardPosition::standard().play_uci("e2e4")?;
    let unrelated = fixture.build(&different_start, &[])?;
    buckets
        .entry(unrelated.structural_key().clone())
        .or_insert_with(Vec::new)
        .push(unrelated);
    let candidates = &buckets[missing.structural_key()];
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].compare(&missing),
        NotationGameIdentityEnumMatch::Potential
    );
    assert_eq!(buckets.len(), 2);
    Ok(())
}

#[test]
fn partial_dates_require_a_possible_common_calendar_date() -> Result<(), Box<dyn std::error::Error>>
{
    for (left, right, expected) in [
        (
            GameMetadataDate::new(Some(2024), None, Some(31))?,
            GameMetadataDate::new(Some(2024), Some(2), None)?,
            NotationGameIdentityEnumMatch::Different,
        ),
        (
            GameMetadataDate::new(None, Some(2), Some(29))?,
            GameMetadataDate::new(Some(1900), None, None)?,
            NotationGameIdentityEnumMatch::Different,
        ),
        (
            GameMetadataDate::new(None, Some(4), None)?,
            GameMetadataDate::new(None, None, Some(31))?,
            NotationGameIdentityEnumMatch::Different,
        ),
        (
            GameMetadataDate::new(None, Some(2), Some(29))?,
            GameMetadataDate::new(Some(2000), None, None)?,
            NotationGameIdentityEnumMatch::Potential,
        ),
        (
            GameMetadataDate::new(Some(2024), None, None)?,
            GameMetadataDate::new(None, Some(2), Some(29))?,
            NotationGameIdentityEnumMatch::Potential,
        ),
    ] {
        let mut fixture = NotationGameIdentityFixture::complete()?;
        fixture.date = Some(left);
        let first = fixture.standard()?;
        fixture.date = Some(right);
        let second = fixture.standard()?;
        assert_eq!(
            first.compare(&second),
            expected,
            "{left:?} versus {right:?}"
        );
        assert_eq!(
            second.compare(&first),
            expected,
            "{right:?} versus {left:?}"
        );
    }
    Ok(())
}

#[test]
fn canonical_text_is_shared_unicode_normalization_without_player_missing_semantics() {
    for (source, expected) in [
        ("\u{2003}Cafe\u{301}\t", "café"),
        (" Straße ", "strasse"),
        ("ΟΣ", "οσ"),
        (" ? ", "?"),
        (" \n\t ", ""),
    ] {
        assert_eq!(NotationGameIdentity::canonical_text(source), expected);
    }
}

#[test]
fn canonical_player_retains_its_missing_policy_and_preserves_the_display_value()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(NotationGameIdentity::canonical_player(None), None);
    for source in ["", " \n\t ", " ? "] {
        let value = GameMetadataText::new(source.to_owned())?;
        assert_eq!(NotationGameIdentity::canonical_player(Some(&value)), None);
        assert_eq!(value.as_str(), source);
    }
    let source = "\u{2003}Straße Cafe\u{301}\t";
    let value = GameMetadataText::new(source.to_owned())?;
    assert_eq!(
        NotationGameIdentity::canonical_player(Some(&value)),
        Some(NotationGameIdentity::canonical_text(source))
    );
    assert_eq!(value.as_str(), source);
    Ok(())
}
