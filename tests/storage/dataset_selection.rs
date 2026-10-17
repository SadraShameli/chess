use std::cmp::Ordering;

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataKnownTags, GameMetadataText,
};
use chess::notation::game_identity::{NotationGameIdentity, NotationGameIdentityEnumResult};
use chess::package::digest::PackageDigest;
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::storage::dataset_selection::{
    StorageDatasetSelection, StorageDatasetSelectionCandidate,
    StorageDatasetSelectionEnumComponent, StorageDatasetSelectionEnumDuplicate,
    StorageDatasetSelectionEnumGameRatingBand, StorageDatasetSelectionEnumMasterEligibility,
    StorageDatasetSelectionEnumPrimaryTheme, StorageDatasetSelectionEnumPuzzleRatingBand,
    StorageDatasetSelectionEnumRating, StorageDatasetSelectionEnumRatingReason,
    StorageDatasetSelectionEnumRebuild, StorageDatasetSelectionEnumSource,
    StorageDatasetSelectionEnumStratum, StorageDatasetSelectionEnumSubset,
    StorageDatasetSelectionEnumThemeReason, StorageDatasetSelectionEnumTimeClass,
    StorageDatasetSelectionEnumTimeReason, StorageDatasetSelectionEnumTransition,
    StorageDatasetSelectionError, StorageDatasetSelectionFootprint,
    StorageDatasetSelectionIdentity, StorageDatasetSelectionInstalledArtifact,
    StorageDatasetSelectionPrefixes, StorageDatasetSelectionSequence,
    StorageDatasetSelectionStratumCount,
};

type StorageDatasetSelectionTestResult = Result<(), Box<dyn std::error::Error>>;

struct StorageDatasetSelectionFixture;

impl StorageDatasetSelectionFixture {
    fn puzzle(id: &str) -> Result<StorageDatasetSelectionIdentity, Box<dyn std::error::Error>> {
        Ok(StorageDatasetSelectionIdentity::puzzle(
            &PuzzleDefinitionIdentity::new("source".into(), "snapshot".into(), id.into())?,
        ))
    }

    fn game(
        known: GameMetadataKnownTags,
        result: Option<NotationGameIdentityEnumResult>,
    ) -> Result<StorageDatasetSelectionIdentity, Box<dyn std::error::Error>> {
        Ok(StorageDatasetSelectionIdentity::game(
            &NotationGameIdentity::new(
                &BoardPosition::standard(),
                &[],
                &GameMetadata::new(known, vec![])?,
                result,
            )?,
        )?)
    }

    fn candidate(
        id: &'static str,
        rating: Option<&str>,
        themes: Option<&str>,
    ) -> Result<StorageDatasetSelectionCandidate<&'static str>, Box<dyn std::error::Error>> {
        Ok(StorageDatasetSelectionCandidate::new(
            id,
            Self::puzzle(id)?,
            StorageDatasetSelectionEnumStratum::Puzzle(
                StorageDatasetSelection::classify_puzzle(rating, themes)?
                    .stratum()
                    .clone(),
            ),
        )?)
    }

    fn footprint(
        sizes: [u64; 5],
        fixed: [u64; 5],
    ) -> Result<StorageDatasetSelectionFootprint, StorageDatasetSelectionError> {
        StorageDatasetSelectionFootprint::from_artifacts(
            StorageDatasetSelectionEnumComponent::ORDER
                .into_iter()
                .zip(sizes)
                .zip(fixed)
                .map(|((component, bytes), fixed_bytes)| {
                    StorageDatasetSelectionInstalledArtifact::new(
                        format!("{component:?}"),
                        Some(component),
                        bytes,
                        fixed_bytes,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

#[test]
fn game_identity_has_literal_null_date_encoding_and_independent_sha()
-> StorageDatasetSelectionTestResult {
    let identity = StorageDatasetSelectionFixture::game(GameMetadataKnownTags::default(), None)?;
    assert_eq!(identity.bytes(), br#"["standard","rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",[],null,null,[null,null,null],null]"#);
    assert_eq!(
        identity.digest().to_string(),
        "eecbacd8dab7de0b174a374e563e1e0937dca662c036d1a87280a01b90911f0b"
    );
    Ok(())
}

#[test]
fn canonical_names_partial_dates_escaping_and_unfinished_result_are_preserved()
-> StorageDatasetSelectionTestResult {
    let identity = StorageDatasetSelectionFixture::game(
        GameMetadataKnownTags {
            white: Some(GameMetadataText::new("  STRAẞE  ".into())?),
            black: Some(GameMetadataText::new("A\"\\\nB".into())?),
            date: Some(GameMetadataDate::new(Some(2025), None, Some(4))?),
            ..GameMetadataKnownTags::default()
        },
        Some(NotationGameIdentityEnumResult::Unfinished),
    )?;
    assert_eq!(identity.bytes(), br#"["standard","rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",[],"strasse","a\"\\\nb",[2025,null,4],"*"]"#);
    assert_ne!(
        identity,
        StorageDatasetSelectionFixture::game(GameMetadataKnownTags::default(), None)?
    );
    Ok(())
}

#[test]
fn variant_and_mainline_use_existing_canonical_notation() -> StorageDatasetSelectionTestResult {
    let initial = BoardPosition::from_fen(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w HAha - 0 1",
        BoardEnumVariant::Chess960,
    )?;
    let movement = initial.parse_uci("e2e4")?;
    let identity = StorageDatasetSelectionIdentity::game(&NotationGameIdentity::new(
        &initial,
        &[movement],
        &GameMetadata::default(),
        None,
    )?)?;
    assert_eq!(identity.bytes(), br#"["chess960","rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w HAha - 0 1",["e2e4"],null,null,[null,null,null],null]"#);
    Ok(())
}

#[test]
fn puzzle_identity_is_exact_utf8_without_source_version_or_quotes()
-> StorageDatasetSelectionTestResult {
    let identity = StorageDatasetSelectionFixture::puzzle("abc")?;
    assert_eq!(identity.bytes(), b"abc");
    assert_eq!(
        identity.digest().to_string(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        StorageDatasetSelectionFixture::puzzle(" é\" ")?.bytes(),
        " é\" ".as_bytes()
    );
    assert_eq!(
        identity,
        StorageDatasetSelectionIdentity::puzzle(&PuzzleDefinitionIdentity::new(
            "other".into(),
            "other-version".into(),
            "abc".into()
        )?)
    );
    Ok(())
}

#[test]
fn digest_slice_and_raw_byte_order_have_literal_sha_fixtures() -> StorageDatasetSelectionTestResult
{
    let digest = PackageDigest::hash_slice(b"abc");
    assert_eq!(
        digest,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".parse()?
    );
    assert_eq!(&digest.bytes()[..4], &[0xba, 0x78, 0x16, 0xbf]);
    assert!(PackageDigest::hash_slice(b"b") < PackageDigest::hash_slice(b"a"));
    Ok(())
}

#[test]
fn both_player_rating_bands_preserve_odd_half_point_boundaries() -> StorageDatasetSelectionTestResult
{
    use StorageDatasetSelectionEnumGameRatingBand::{
        Below1200, From1200, From1600, From2000, From2400,
    };
    for (white, black, expected) in [
        ("1199", "1200", Below1200),
        ("1200", "1200", From1200),
        ("1599", "1600", From1200),
        ("1600", "1600", From1600),
        ("1999", "2000", From1600),
        ("2000", "2000", From2000),
        ("2399", "2400", From2000),
        ("2400", "2400", From2400),
        ("0", "0", Below1200),
        ("4294967295", "4294967295", From2400),
    ] {
        assert_eq!(
            StorageDatasetSelection::classify_game(Some(white), Some(black), None)?
                .stratum()
                .rating_band(),
            expected
        );
    }
    Ok(())
}

#[test]
fn game_ratings_require_both_and_report_each_missing_invalid_value()
-> StorageDatasetSelectionTestResult {
    let classification = StorageDatasetSelection::classify_game(None, Some("+1200"), None)?;
    assert_eq!(
        classification.stratum().rating_band(),
        StorageDatasetSelectionEnumGameRatingBand::Unknown
    );
    assert_eq!(
        classification.white_rating(),
        StorageDatasetSelectionEnumRating::Unknown(
            StorageDatasetSelectionEnumRatingReason::Missing
        )
    );
    assert_eq!(
        classification.black_rating(),
        StorageDatasetSelectionEnumRating::Unknown(
            StorageDatasetSelectionEnumRatingReason::Invalid
        )
    );
    for raw in [
        "+1",
        "-1",
        "1.0",
        "4294967296",
        "١",
        "1 2",
        "\u{a0}1200",
        "1200\u{b}",
    ] {
        assert_eq!(
            StorageDatasetSelection::classify_game(Some(raw), Some("1200"), None)?.white_rating(),
            StorageDatasetSelectionEnumRating::Unknown(
                StorageDatasetSelectionEnumRatingReason::Invalid
            )
        );
    }
    assert_eq!(
        StorageDatasetSelection::classify_game(Some(" \t\n\u{c}\r001200 "), Some("1200"), None)?
            .white_rating(),
        StorageDatasetSelectionEnumRating::Known(1200)
    );
    Ok(())
}

#[test]
fn time_boundaries_and_increment_estimates_consume_the_full_tag()
-> StorageDatasetSelectionTestResult {
    use StorageDatasetSelectionEnumTimeClass::{Blitz, Bullet, Classical, Correspondence, Rapid};
    for (raw, class) in [
        ("179", Bullet),
        ("180", Blitz),
        ("479", Blitz),
        ("480", Rapid),
        ("1499", Rapid),
        ("1500", Classical),
        ("0+1", Bullet),
        ("0+12", Rapid),
        ("20+4", Blitz),
        ("4294967295+4294967295", Classical),
        ("1 day per move", Correspondence),
        ("2 days per move", Correspondence),
        ("4294967295 days per move", Correspondence),
        ("86400", Classical),
        (" \t\r\n\u{c}180+0 ", Blitz),
    ] {
        let value = StorageDatasetSelection::classify_game(None, None, Some(raw))?;
        assert_eq!(value.stratum().time_class(), class, "{raw:?}");
        assert_eq!(value.time_reason(), None);
    }
    Ok(())
}

#[test]
fn unknown_time_reasons_distinguish_unavailable_malformed_overflow_and_unsupported()
-> StorageDatasetSelectionTestResult {
    use StorageDatasetSelectionEnumTimeReason::{
        Hourglass, Malformed, MultiStage, Overflow, Slash, Unavailable,
    };
    for (raw, reason) in [
        (None, Unavailable),
        (Some(" \t"), Unavailable),
        (Some("?"), Unavailable),
        (Some("-"), Unavailable),
        (Some("0+0"), Malformed),
        (Some("0"), Malformed),
        (Some("+180"), Malformed),
        (Some("180 +1"), Malformed),
        (Some("180+1junk"), Malformed),
        (Some("١٨٠"), Malformed),
        (Some("\u{a0}180"), Malformed),
        (Some("180\u{b}"), Malformed),
        (Some("1 days per move"), Malformed),
        (Some("2 day per move"), Malformed),
        (Some("4294967296"), Overflow),
        (Some("1+4294967296"), Overflow),
        (Some("4294967296 days per move"), Overflow),
        (Some("40/7200:3600"), MultiStage),
        (Some("40/7200"), Slash),
        (Some("*180"), Hourglass),
    ] {
        let value = StorageDatasetSelection::classify_game(None, None, raw)?;
        assert_eq!(
            value.stratum().time_class(),
            StorageDatasetSelectionEnumTimeClass::Unknown
        );
        assert_eq!(value.time_reason(), Some(reason), "{raw:?}");
    }
    Ok(())
}

#[test]
fn all_exact_master_titles_require_both_players_and_exclude_broadcast() {
    for title in ["GM", "IM", "FM", "WGM", "WIM", "WFM"] {
        assert_eq!(
            StorageDatasetSelection::master_eligibility(
                StorageDatasetSelectionEnumSource::Standard,
                Some(title),
                Some("GM")
            ),
            StorageDatasetSelectionEnumMasterEligibility::Eligible
        );
        assert_eq!(
            StorageDatasetSelection::master_eligibility(
                StorageDatasetSelectionEnumSource::Standard,
                Some("GM"),
                Some(title)
            ),
            StorageDatasetSelectionEnumMasterEligibility::Eligible
        );
    }
    assert_eq!(
        StorageDatasetSelection::master_eligibility(
            StorageDatasetSelectionEnumSource::Broadcast,
            Some("GM"),
            Some("GM")
        ),
        StorageDatasetSelectionEnumMasterEligibility::Broadcast
    );
    assert_eq!(
        StorageDatasetSelection::master_eligibility(
            StorageDatasetSelectionEnumSource::Standard,
            None,
            Some("GM")
        ),
        StorageDatasetSelectionEnumMasterEligibility::MissingTitle(BoardEnumColor::White)
    );
    assert_eq!(
        StorageDatasetSelection::master_eligibility(
            StorageDatasetSelectionEnumSource::Standard,
            Some("GM"),
            None
        ),
        StorageDatasetSelectionEnumMasterEligibility::MissingTitle(BoardEnumColor::Black)
    );
    for title in ["gm", "CM", "NM", " GM", "GM ", ""] {
        assert_eq!(
            StorageDatasetSelection::master_eligibility(
                StorageDatasetSelectionEnumSource::Standard,
                Some("GM"),
                Some(title)
            ),
            StorageDatasetSelectionEnumMasterEligibility::UnsupportedTitle(BoardEnumColor::Black)
        );
    }
}

#[test]
fn puzzle_rating_bands_have_checked_endpoints_including_u32_max()
-> StorageDatasetSelectionTestResult {
    for (rating, index, endpoints) in [
        ("0", 0, (0, 249)),
        ("249", 0, (0, 249)),
        ("250", 1, (250, 499)),
        ("4294967295", 17_179_869, (4_294_967_250, 4_294_967_295)),
    ] {
        let classification = StorageDatasetSelection::classify_puzzle(Some(rating), None)?;
        let StorageDatasetSelectionEnumPuzzleRatingBand::Known(band) =
            classification.stratum().rating_band()
        else {
            return Err("expected known band".into());
        };
        assert_eq!(band.index(), index);
        assert_eq!(band.endpoints()?, endpoints);
    }
    for raw in [None, Some("4294967296"), Some("+250"), Some("\u{a0}250")] {
        assert_eq!(
            StorageDatasetSelection::classify_puzzle(raw, None)?
                .stratum()
                .rating_band(),
            StorageDatasetSelectionEnumPuzzleRatingBand::Unknown
        );
    }
    Ok(())
}

#[test]
fn themes_use_ascii_whitespace_case_sensitive_bytes_and_distinct_unknown()
-> StorageDatasetSelectionTestResult {
    for raw in [
        "zebra alpha Alpha alpha",
        "alpha\tAlpha\n\u{c}\rzebra",
        "Alpha",
    ] {
        let classification = StorageDatasetSelection::classify_puzzle(None, Some(raw))?;
        assert_eq!(
            classification.stratum().primary_theme().token(),
            Some("Alpha")
        );
    }
    for raw in ["a\u{a0}b", "a\u{b}b"] {
        assert_eq!(
            StorageDatasetSelection::classify_puzzle(None, Some(raw))?
                .stratum()
                .primary_theme()
                .token(),
            Some(raw)
        );
    }
    let unknown = StorageDatasetSelection::classify_puzzle(None, Some(" \t\r\n\u{c}"))?;
    let literal = StorageDatasetSelection::classify_puzzle(None, Some("Unknown"))?;
    assert_eq!(
        unknown.stratum().primary_theme(),
        &StorageDatasetSelectionEnumPrimaryTheme::Unknown
    );
    assert_eq!(
        unknown.theme_reason(),
        Some(StorageDatasetSelectionEnumThemeReason::Empty)
    );
    assert_eq!(
        StorageDatasetSelection::classify_puzzle(None, None)?.theme_reason(),
        Some(StorageDatasetSelectionEnumThemeReason::Missing)
    );
    assert_eq!(literal.theme_reason(), None);
    assert_ne!(unknown.stratum(), literal.stratum());
    assert!(literal.stratum() < unknown.stratum());
    Ok(())
}

#[test]
fn game_strata_use_rating_then_time_and_unknown_last() -> StorageDatasetSelectionTestResult {
    let pairs = [
        (Some("0"), Some("1")),
        (Some("0"), Some("180")),
        (Some("0"), Some("480")),
        (Some("0"), Some("1500")),
        (Some("0"), Some("1 day per move")),
        (Some("0"), None),
        (Some("1200"), Some("1")),
        (None, None),
    ];
    let keys = pairs
        .into_iter()
        .map(|(rating, time)| {
            StorageDatasetSelection::classify_game(rating, rating, time)
                .map(|value| value.stratum().clone())
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
    Ok(())
}

#[test]
fn duplicate_game_representative_uses_original_utf8_bytes_and_strict_identity()
-> StorageDatasetSelectionTestResult {
    let missing = StorageDatasetSelectionFixture::game(GameMetadataKnownTags::default(), None)?;
    let known = StorageDatasetSelectionFixture::game(
        GameMetadataKnownTags {
            white: Some(GameMetadataText::new("a".into())?),
            ..GameMetadataKnownTags::default()
        },
        None,
    )?;
    assert_eq!(
        StorageDatasetSelection::compare_duplicate(&missing, "z record", &missing, "a record")?,
        StorageDatasetSelectionEnumDuplicate::Replace
    );
    assert_eq!(
        StorageDatasetSelection::compare_duplicate(&missing, "a record", &missing, "z record")?,
        StorageDatasetSelectionEnumDuplicate::Keep
    );
    assert_eq!(
        StorageDatasetSelection::compare_duplicate(&missing, "same", &missing, "same")?,
        StorageDatasetSelectionEnumDuplicate::Identical
    );
    assert_eq!(
        StorageDatasetSelection::compare_duplicate(&missing, "a", &known, "b")?,
        StorageDatasetSelectionEnumDuplicate::Distinct
    );
    assert_eq!(
        StorageDatasetSelection::compare_duplicate(&missing, "é", &missing, "z")?,
        StorageDatasetSelectionEnumDuplicate::Replace
    );
    Ok(())
}

#[test]
fn same_puzzle_id_conflicts_unless_source_record_is_byte_identical()
-> StorageDatasetSelectionTestResult {
    let identity = StorageDatasetSelectionFixture::puzzle("id")?;
    assert_eq!(
        StorageDatasetSelection::compare_duplicate(&identity, "same", &identity, "same")?,
        StorageDatasetSelectionEnumDuplicate::Identical
    );
    assert!(matches!(
        StorageDatasetSelection::compare_duplicate(&identity, "first", &identity, "second"),
        Err(StorageDatasetSelectionError::PuzzleConflict { .. })
    ));
    Ok(())
}

#[test]
fn digest_ties_are_broken_by_identity_bytes() -> StorageDatasetSelectionTestResult {
    let digest: PackageDigest =
        "0000000000000000000000000000000000000000000000000000000000000000".parse()?;
    assert_eq!(
        StorageDatasetSelection::compare_order(digest, b"a", digest, b"b"),
        Ordering::Less
    );
    assert_eq!(
        StorageDatasetSelection::compare_order(digest, b"b", digest, b"a"),
        Ordering::Greater
    );
    assert_eq!(
        StorageDatasetSelection::compare_order(digest, b"a", digest, b"a"),
        Ordering::Equal
    );
    assert_eq!(
        StorageDatasetSelection::compare_order(
            PackageDigest::hash_slice(b"b"),
            b"b",
            PackageDigest::hash_slice(b"a"),
            b"a"
        ),
        Ordering::Less
    );
    Ok(())
}

#[test]
fn observed_flat_pairs_receive_equal_turns_and_earlier_remainder_slots()
-> StorageDatasetSelectionTestResult {
    let candidates = vec![
        StorageDatasetSelectionFixture::candidate("a", Some("0"), Some("alpha"))?,
        StorageDatasetSelectionFixture::candidate("b", Some("0"), Some("alpha"))?,
        StorageDatasetSelectionFixture::candidate("c", Some("0"), Some("beta"))?,
        StorageDatasetSelectionFixture::candidate("d", Some("250"), Some("alpha"))?,
    ];
    let sequence = StorageDatasetSelectionSequence::new(
        StorageDatasetSelectionEnumSubset::Puzzles,
        candidates,
    )?;
    assert_eq!(
        sequence
            .prefix(2)?
            .candidates()
            .iter()
            .map(|candidate| *candidate.handle())
            .collect::<Vec<_>>(),
        ["b", "c"]
    );
    assert_eq!(
        sequence
            .prefix(3)?
            .candidates()
            .iter()
            .map(|candidate| *candidate.handle())
            .collect::<Vec<_>>(),
        ["b", "c", "d"]
    );
    assert_eq!(
        sequence
            .prefix(4)?
            .candidates()
            .iter()
            .map(|candidate| *candidate.handle())
            .collect::<Vec<_>>(),
        ["b", "c", "d", "a"]
    );
    assert_eq!(
        sequence
            .strata()
            .iter()
            .map(|stratum| stratum.available())
            .collect::<Vec<_>>(),
        [2, 1, 1]
    );
    Ok(())
}

#[test]
fn master_is_one_hash_ordered_sequence_without_a_rating_quota() -> StorageDatasetSelectionTestResult
{
    let mut candidates = Vec::new();
    for name in ["d", "b", "a", "c"] {
        let identity = StorageDatasetSelectionFixture::game(
            GameMetadataKnownTags {
                white: Some(GameMetadataText::new(name.into())?),
                ..GameMetadataKnownTags::default()
            },
            None,
        )?;
        candidates.push(StorageDatasetSelectionCandidate::new(
            name,
            identity,
            StorageDatasetSelectionEnumStratum::Master,
        )?);
    }
    let sequence = StorageDatasetSelectionSequence::new(
        StorageDatasetSelectionEnumSubset::Master,
        candidates,
    )?;
    assert_eq!(
        sequence
            .prefix(100_000)?
            .candidates()
            .iter()
            .map(|candidate| *candidate.handle())
            .collect::<Vec<_>>(),
        ["a", "c", "d", "b"]
    );
    assert_eq!(sequence.strata().len(), 1);
    assert_eq!(sequence.strata()[0].available(), 4);
    Ok(())
}

#[test]
fn puzzle_strata_sort_numeric_bands_then_known_themes_before_unknown()
-> StorageDatasetSelectionTestResult {
    let mut keys = Vec::new();
    for (rating, themes) in [
        (Some("500"), Some("Alpha")),
        (Some("500"), Some("alpha")),
        (Some("500"), None),
        (Some("2500"), Some("Alpha")),
        (None, Some("Alpha")),
        (None, None),
    ] {
        keys.push(
            StorageDatasetSelection::classify_puzzle(rating, themes)?
                .stratum()
                .clone(),
        );
    }
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
    Ok(())
}

#[test]
fn broad_game_pairs_round_robin_instead_of_hashing_the_corpus_as_one_sequence()
-> StorageDatasetSelectionTestResult {
    let mut candidates = Vec::new();
    for (name, rating, time) in [
        ("a", "1200", "180"),
        ("b", "0", "1"),
        ("c", "0", "1"),
        ("d", "0", "180"),
    ] {
        let identity = StorageDatasetSelectionFixture::game(
            GameMetadataKnownTags {
                white: Some(GameMetadataText::new(name.into())?),
                ..GameMetadataKnownTags::default()
            },
            None,
        )?;
        candidates.push(StorageDatasetSelectionCandidate::new(
            name,
            identity,
            StorageDatasetSelectionEnumStratum::Game(
                StorageDatasetSelection::classify_game(Some(rating), Some(rating), Some(time))?
                    .stratum()
                    .clone(),
            ),
        )?);
    }
    let sequence =
        StorageDatasetSelectionSequence::new(StorageDatasetSelectionEnumSubset::Broad, candidates)?;
    assert_eq!(
        sequence
            .prefix(3)?
            .candidates()
            .iter()
            .map(|candidate| *candidate.handle())
            .collect::<Vec<_>>(),
        ["c", "d", "a"]
    );
    assert_eq!(
        sequence
            .prefix(250_000)?
            .candidates()
            .iter()
            .map(|candidate| *candidate.handle())
            .collect::<Vec<_>>(),
        ["c", "d", "a", "b"]
    );
    Ok(())
}

#[test]
fn permutations_and_simulated_worker_completion_orders_preserve_every_prefix()
-> StorageDatasetSelectionTestResult {
    let mut reference = None;
    for order in [
        ["a", "b", "c", "d"],
        ["d", "c", "b", "a"],
        ["b", "d", "a", "c"],
        ["c", "a", "d", "b"],
    ] {
        let sequence = StorageDatasetSelectionSequence::new(
            StorageDatasetSelectionEnumSubset::Puzzles,
            order
                .into_iter()
                .map(|id| StorageDatasetSelectionFixture::candidate(id, Some("250"), Some("theme")))
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        let full = sequence.prefix(4)?;
        let handles = full
            .candidates()
            .iter()
            .map(|candidate| *candidate.handle())
            .collect::<Vec<_>>();
        assert_eq!(handles, ["d", "c", "b", "a"]);
        for length in 0..=4 {
            let prefix = sequence.prefix(length)?;
            assert_eq!(
                prefix
                    .candidates()
                    .iter()
                    .map(|candidate| *candidate.handle())
                    .collect::<Vec<_>>(),
                handles[..length]
            );
        }
        if let Some(digest) = reference {
            assert_eq!(full.digest(), digest);
        }
        reference = Some(full.digest());
    }
    Ok(())
}

#[test]
fn exhausted_and_empty_sequences_return_available_prefix_without_padding()
-> StorageDatasetSelectionTestResult {
    let sequence = StorageDatasetSelectionSequence::<()>::new(
        StorageDatasetSelectionEnumSubset::Master,
        vec![],
    )?;
    let prefix = sequence.prefix(100_000)?;
    assert!(prefix.candidates().is_empty());
    assert_eq!(
        prefix.digest().to_string(),
        "af5570f5a1810b7af78caf4bc70a660f0df51e42baf91d4de5b2328de0e83dfc"
    );
    assert!(matches!(
        sequence.prefix(100_001),
        Err(StorageDatasetSelectionError::TargetExceeded { .. })
    ));
    assert_eq!(
        StorageDatasetSelectionEnumSubset::Master.upper_target(),
        100_000
    );
    assert_eq!(
        StorageDatasetSelectionEnumSubset::Broad.upper_target(),
        250_000
    );
    assert_eq!(
        StorageDatasetSelectionEnumSubset::Puzzles.upper_target(),
        200_000
    );
    Ok(())
}

#[test]
fn sequence_refuses_unresolved_duplicate_identities_and_wrong_strata()
-> StorageDatasetSelectionTestResult {
    assert!(matches!(
        StorageDatasetSelectionSequence::new(
            StorageDatasetSelectionEnumSubset::Puzzles,
            vec![
                StorageDatasetSelectionFixture::candidate("a", None, None)?,
                StorageDatasetSelectionFixture::candidate("a", None, None)?
            ]
        ),
        Err(StorageDatasetSelectionError::UnresolvedDuplicate)
    ));
    assert!(matches!(
        StorageDatasetSelectionSequence::new(
            StorageDatasetSelectionEnumSubset::Broad,
            vec![StorageDatasetSelectionFixture::candidate("a", None, None)?]
        ),
        Err(StorageDatasetSelectionError::SubsetMismatch)
    ));
    assert!(matches!(
        StorageDatasetSelectionCandidate::new(
            (),
            StorageDatasetSelectionFixture::puzzle("a")?,
            StorageDatasetSelectionEnumStratum::Master
        ),
        Err(StorageDatasetSelectionError::IdentityKindMismatch)
    ));
    Ok(())
}

#[test]
fn sequence_digest_is_framed_by_count_and_each_byte_length() -> StorageDatasetSelectionTestResult {
    let first = [
        StorageDatasetSelectionFixture::puzzle("a")?,
        StorageDatasetSelectionFixture::puzzle("bc")?,
    ];
    let second = [
        StorageDatasetSelectionFixture::puzzle("ab")?,
        StorageDatasetSelectionFixture::puzzle("c")?,
    ];
    let digest = StorageDatasetSelection::sequence_digest(first.iter())?;
    assert_eq!(
        digest.to_string(),
        "5eee4f0e9c8cfac2efc9945e5e600f0a57d45023af5ddc3c1dc04bfcdf9657fc"
    );
    assert_eq!(
        StorageDatasetSelection::sequence_digest(second.iter())?.to_string(),
        "8e502afb4273fb33b57a8c24a5709d6c7030c2c8626f421ddae7b4f93707455f"
    );
    assert_ne!(
        digest,
        StorageDatasetSelection::sequence_digest(first.iter().rev())?
    );
    Ok(())
}

#[test]
fn cap_acceptance_counts_all_components_at_the_exact_decimal_ceiling()
-> StorageDatasetSelectionTestResult {
    let footprint = StorageDatasetSelectionFixture::footprint(
        [
            800_000_000,
            800_000_000,
            300_000_000,
            50_000_000,
            50_000_000,
        ],
        [0; 5],
    )?;
    assert_eq!(footprint.total_bytes(), 2_000_000_000);
    assert_eq!(
        footprint.component_bytes(StorageDatasetSelectionEnumComponent::Book),
        50_000_000
    );
    assert_eq!(
        footprint.transition(StorageDatasetSelectionPrefixes::new(
            100_000, 250_000, 200_000
        )?)?,
        StorageDatasetSelectionEnumTransition::Accept
    );
    assert_eq!(StorageDatasetSelection::POLICY_VERSION, 1);
    Ok(())
}

#[test]
fn first_overflow_uses_canonical_component_order_and_floor_nine_tenths()
-> StorageDatasetSelectionTestResult {
    let prefixes = StorageDatasetSelectionPrefixes::new(19, 29, 39)?;
    for (sizes, component, after, rebuild) in [
        (
            [
                800_000_001,
                800_000_001,
                300_000_001,
                50_000_001,
                50_000_001,
            ],
            StorageDatasetSelectionEnumComponent::Master,
            (17, 29, 39),
            StorageDatasetSelectionEnumRebuild::MasterAndBook,
        ),
        (
            [0, 800_000_001, 300_000_001, 50_000_001, 50_000_001],
            StorageDatasetSelectionEnumComponent::Broad,
            (19, 26, 39),
            StorageDatasetSelectionEnumRebuild::Broad,
        ),
        (
            [0, 0, 300_000_001, 50_000_001, 50_000_001],
            StorageDatasetSelectionEnumComponent::Puzzles,
            (19, 29, 35),
            StorageDatasetSelectionEnumRebuild::Puzzles,
        ),
        (
            [0, 0, 0, 50_000_001, 50_000_001],
            StorageDatasetSelectionEnumComponent::Book,
            (17, 29, 39),
            StorageDatasetSelectionEnumRebuild::MasterAndBook,
        ),
    ] {
        let StorageDatasetSelectionEnumTransition::Reduce(reduction) =
            StorageDatasetSelectionFixture::footprint(sizes, [0; 5])?.transition(prefixes)?
        else {
            return Err("expected a reduction".into());
        };
        assert_eq!(reduction.component(), component);
        assert_eq!(reduction.before(), prefixes);
        assert_eq!(
            reduction.after(),
            StorageDatasetSelectionPrefixes::new(after.0, after.1, after.2)?
        );
        assert_eq!(reduction.rebuild(), rebuild);
    }
    Ok(())
}

#[test]
fn repeated_cap_transitions_require_rebuilt_footprints_and_refuse_required_empty()
-> StorageDatasetSelectionTestResult {
    let overflowing = StorageDatasetSelectionFixture::footprint([800_000_001, 0, 0, 0, 0], [0; 5])?;
    let StorageDatasetSelectionEnumTransition::Reduce(first) =
        overflowing.transition(StorageDatasetSelectionPrefixes::new(3, 1, 1)?)?
    else {
        return Err("expected reduction".into());
    };
    assert_eq!(first.after().master(), 2);
    let StorageDatasetSelectionEnumTransition::Reduce(second) =
        overflowing.transition(first.after())?
    else {
        return Err("expected reduction".into());
    };
    assert_eq!(second.after().master(), 1);
    assert!(matches!(
        overflowing.transition(second.after()),
        Err(StorageDatasetSelectionError::RequiredEmpty(
            StorageDatasetSelectionEnumSubset::Master
        ))
    ));
    let fitting = StorageDatasetSelectionFixture::footprint([0; 5], [0; 5])?;
    for prefixes in [
        StorageDatasetSelectionPrefixes::new(0, 1, 1)?,
        StorageDatasetSelectionPrefixes::new(1, 0, 1)?,
        StorageDatasetSelectionPrefixes::new(1, 1, 0)?,
    ] {
        assert!(matches!(
            fitting.transition(prefixes),
            Err(StorageDatasetSelectionError::RequiredEmpty(_))
        ));
    }
    Ok(())
}

#[test]
fn labels_and_nonreducible_overhead_refuse_instead_of_dropping_provenance()
-> StorageDatasetSelectionTestResult {
    let prefixes = StorageDatasetSelectionPrefixes::new(10, 10, 10)?;
    assert!(matches!(
        StorageDatasetSelectionFixture::footprint([0, 0, 0, 0, 50_000_001], [0; 5])?
            .transition(prefixes),
        Err(StorageDatasetSelectionError::LabelsOverflow { .. })
    ));
    assert!(matches!(
        StorageDatasetSelectionFixture::footprint(
            [800_000_001, 0, 0, 0, 0],
            [800_000_001, 0, 0, 0, 0]
        )?
        .transition(prefixes),
        Err(StorageDatasetSelectionError::NonreducibleOverflow(
            StorageDatasetSelectionEnumComponent::Master
        ))
    ));
    assert!(matches!(
        StorageDatasetSelectionFixture::footprint(
            [0, 0, 0, 50_000_001, 0],
            [0, 0, 0, 50_000_001, 0]
        )?
        .transition(prefixes),
        Err(StorageDatasetSelectionError::NonreducibleOverflow(
            StorageDatasetSelectionEnumComponent::Book
        ))
    ));
    Ok(())
}

#[test]
fn inventory_refuses_duplicate_unclassified_invalid_and_overflowing_artifacts()
-> StorageDatasetSelectionTestResult {
    let artifact = || {
        StorageDatasetSelectionInstalledArtifact::new(
            "index".into(),
            Some(StorageDatasetSelectionEnumComponent::Master),
            1,
            0,
        )
    };
    assert!(matches!(
        StorageDatasetSelectionFootprint::from_artifacts([artifact()?, artifact()?]),
        Err(StorageDatasetSelectionError::DuplicateArtifact(_))
    ));
    assert!(matches!(
        StorageDatasetSelectionFootprint::from_artifacts([
            StorageDatasetSelectionInstalledArtifact::new("provenance".into(), None, 1, 0)?
        ]),
        Err(StorageDatasetSelectionError::UnclassifiedArtifact(_))
    ));
    assert!(matches!(
        StorageDatasetSelectionInstalledArtifact::new(
            "bad".into(),
            Some(StorageDatasetSelectionEnumComponent::Master),
            1,
            2
        ),
        Err(StorageDatasetSelectionError::InvalidFixedBytes)
    ));
    assert!(matches!(
        StorageDatasetSelectionInstalledArtifact::new(
            String::new(),
            Some(StorageDatasetSelectionEnumComponent::Master),
            1,
            0
        ),
        Err(StorageDatasetSelectionError::EmptyArtifactIdentity)
    ));
    assert!(matches!(
        StorageDatasetSelectionFootprint::from_artifacts([
            StorageDatasetSelectionInstalledArtifact::new(
                "huge".into(),
                Some(StorageDatasetSelectionEnumComponent::Master),
                u64::MAX,
                0
            )?,
            StorageDatasetSelectionInstalledArtifact::new(
                "another".into(),
                Some(StorageDatasetSelectionEnumComponent::Broad),
                1,
                0
            )?,
        ]),
        Err(StorageDatasetSelectionError::ArithmeticOverflow)
    ));
    assert!(matches!(
        StorageDatasetSelectionPrefixes::new(u64::MAX, 1, 1),
        Err(StorageDatasetSelectionError::TargetExceeded { .. })
    ));
    Ok(())
}

#[test]
fn inventory_adds_indexes_provenance_and_other_separate_files_once()
-> StorageDatasetSelectionTestResult {
    let footprint = StorageDatasetSelectionFootprint::from_artifacts([
        StorageDatasetSelectionInstalledArtifact::new(
            "games".into(),
            Some(StorageDatasetSelectionEnumComponent::Master),
            700_000_000,
            0,
        )?,
        StorageDatasetSelectionInstalledArtifact::new(
            "index".into(),
            Some(StorageDatasetSelectionEnumComponent::Master),
            100_000_000,
            10,
        )?,
        StorageDatasetSelectionInstalledArtifact::new(
            "provenance".into(),
            Some(StorageDatasetSelectionEnumComponent::Master),
            1,
            1,
        )?,
    ])?;
    assert_eq!(
        footprint.component_bytes(StorageDatasetSelectionEnumComponent::Master),
        800_000_001
    );
    assert_eq!(footprint.total_bytes(), 800_000_001);
    assert!(matches!(
        footprint.transition(StorageDatasetSelectionPrefixes::new(10, 10, 10)?)?,
        StorageDatasetSelectionEnumTransition::Reduce(_)
    ));
    Ok(())
}

#[test]
fn fixed_overhead_equal_to_the_ceiling_still_allows_a_checked_rebuild()
-> StorageDatasetSelectionTestResult {
    let footprint = StorageDatasetSelectionFixture::footprint(
        [800_000_001, 0, 0, 0, 0],
        [800_000_000, 0, 0, 0, 0],
    )?;
    assert!(matches!(
        footprint.transition(StorageDatasetSelectionPrefixes::new(10, 10, 10)?)?,
        StorageDatasetSelectionEnumTransition::Reduce(_)
    ));
    Ok(())
}

#[test]
fn manifest_stratum_keys_parse_through_canonical_validated_owners()
-> StorageDatasetSelectionTestResult {
    let key: StorageDatasetSelectionEnumStratum =
        serde_json::from_str(r#"{"game":{"rating_band":"from1600","time_class":"rapid"}}"#)?;
    let expected = StorageDatasetSelection::classify_game(Some("1600"), Some("1600"), Some("600"))?;
    assert_eq!(
        key,
        StorageDatasetSelectionEnumStratum::Game(expected.stratum().clone())
    );
    assert_eq!(
        serde_json::to_string(&key)?,
        r#"{"game":{"rating_band":"from1600","time_class":"rapid"}}"#
    );
    for invalid in [
        r#"{"game":{"rating_band":"from1600","time_class":"rapid","unexpected":0}}"#,
        r#"{"game":{"rating_band":"1600","time_class":"rapid"}}"#,
        r#"{"game":{"rating_band":"unknown","time_class":"ultrabullet"}}"#,
        r#"{"puzzle":{"rating_band":{"known":17179870},"primary_theme":"unknown"}}"#,
        r#"{"puzzle":{"rating_band":"unknown","primary_theme":{"known":"two tokens"}}}"#,
        r#"{"puzzle":{"rating_band":"unknown","primary_theme":{"known":""}}}"#,
    ] {
        assert!(
            serde_json::from_str::<StorageDatasetSelectionEnumStratum>(invalid).is_err(),
            "{invalid}"
        );
    }
    Ok(())
}

#[test]
fn parsed_puzzle_keys_preserve_maximum_band_and_ascii_token_semantics()
-> StorageDatasetSelectionTestResult {
    let known: StorageDatasetSelectionEnumStratum = serde_json::from_str(
        r#"{"puzzle":{"rating_band":{"known":17179869},"primary_theme":{"known":"Unknown"}}}"#,
    )?;
    let expected = StorageDatasetSelection::classify_puzzle(Some("4294967295"), Some("Unknown"))?;
    assert_eq!(
        known,
        StorageDatasetSelectionEnumStratum::Puzzle(expected.stratum().clone())
    );
    let unicode: StorageDatasetSelectionEnumStratum = serde_json::from_str(
        r#"{"puzzle":{"rating_band":"unknown","primary_theme":{"known":"two\u00a0tokens"}}}"#,
    )?;
    let expected = StorageDatasetSelection::classify_puzzle(None, Some("two\u{a0}tokens"))?;
    assert_eq!(
        unicode,
        StorageDatasetSelectionEnumStratum::Puzzle(expected.stratum().clone())
    );
    let unknown: StorageDatasetSelectionEnumStratum =
        serde_json::from_str(r#"{"puzzle":{"rating_band":"unknown","primary_theme":"unknown"}}"#)?;
    assert_ne!(known, unknown);
    Ok(())
}

#[test]
fn observed_stratum_counts_reject_empty_and_unknown_fields_at_the_boundary()
-> StorageDatasetSelectionTestResult {
    let count =
        StorageDatasetSelectionStratumCount::new(StorageDatasetSelectionEnumStratum::Master, 3)?;
    assert_eq!(count.available(), 3);
    let encoded = serde_json::to_string(&count)?;
    assert_eq!(encoded, r#"{"stratum":"master","available":3}"#);
    let restored: StorageDatasetSelectionStratumCount = serde_json::from_str(&encoded)?;
    assert_eq!(restored, count);
    assert!(
        StorageDatasetSelectionStratumCount::new(StorageDatasetSelectionEnumStratum::Master, 0)
            .is_err()
    );
    for invalid in [
        r#"{"stratum":"master","available":0}"#,
        r#"{"stratum":"master","available":1,"extra":0}"#,
        r#"{"stratum":"master","available":-1}"#,
        r#"{"stratum":"master","available":18446744073709551616}"#,
    ] {
        assert!(
            serde_json::from_str::<StorageDatasetSelectionStratumCount>(invalid).is_err(),
            "{invalid}"
        );
    }
    Ok(())
}

#[test]
fn manifest_subset_and_component_keys_have_one_closed_external_vocabulary()
-> StorageDatasetSelectionTestResult {
    for subset in StorageDatasetSelectionEnumSubset::ORDER {
        let encoded = serde_json::to_string(&subset)?;
        assert_eq!(
            serde_json::from_str::<StorageDatasetSelectionEnumSubset>(&encoded)?,
            subset
        );
    }
    for component in StorageDatasetSelectionEnumComponent::ORDER {
        let encoded = serde_json::to_string(&component)?;
        assert_eq!(
            serde_json::from_str::<StorageDatasetSelectionEnumComponent>(&encoded)?,
            component
        );
    }
    assert!(serde_json::from_str::<StorageDatasetSelectionEnumSubset>(r#""Master""#).is_err());
    assert!(
        serde_json::from_str::<StorageDatasetSelectionEnumComponent>(r#""manifests""#).is_err()
    );
    Ok(())
}
