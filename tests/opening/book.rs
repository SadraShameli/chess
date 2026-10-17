use std::cell::Cell;
use std::num::NonZeroUsize;

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::{BoardPosition, BoardPositionError};
use chess::clock::duration::ClockInstant;
use chess::clock::time_control::ClockTimeControl;
use chess::game::pause::GamePauseEnumEngineBarrier;
use chess::game::result::GameResultResignRequest;
use chess::opening::book::{OpeningBook, OpeningBookError, OpeningBookLimits};
use chess::package::digest::PackageDigest;

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

#[path = "book_fixture.rs"]
mod fixture;

use fixture::OpeningBookFixture;

#[test]
fn standard_keys_match_fixed_polyglot_reference_vectors() -> Result<(), Box<dyn std::error::Error>>
{
    let references = [
        (OpeningBookFixture::START_FEN, 0x463b_9618_1691_fc9c),
        (
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
            0x823c_9b50_fd11_4196,
        ),
        (
            "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2",
            0x0756_b944_61c5_0fb0,
        ),
        (
            "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2",
            0x662f_afb9_65db_29d4,
        ),
        (
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
            0x22a4_8b5a_8e47_ff78,
        ),
        (
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPPKPPP/RNBQ1BNR b kq - 1 3",
            0x652a_607c_a3f2_42c1,
        ),
        (
            "rnbq1bnr/ppp1pkpp/8/3pPp2/8/8/PPPPKPPP/RNBQ1BNR w - - 2 4",
            0x00fd_d303_c946_bdd9,
        ),
        (
            "rnbqkbnr/p1pppppp/8/8/PpP4P/8/1P1PPPP1/RNBQKBNR b KQkq c3 0 3",
            0x3c81_23ea_7b06_7637,
        ),
        (
            "rnbqkbnr/p1pppppp/8/8/P6P/R1p5/1P1PPPP1/1NBQKBNR b Kkq - 1 4",
            0x5c3f_9b82_9b27_9560,
        ),
    ];
    for (fen, expected) in references {
        assert_eq!(
            OpeningBookFixture::position(fen)?
                .standard_polyglot_key()?
                .value(),
            expected,
            "{fen}"
        );
    }
    Ok(())
}

#[test]
fn pinned_en_passant_hashes_pseudo_legal_rights_but_never_exposes_the_illegal_move()
-> Result<(), Box<dyn std::error::Error>> {
    let position = OpeningBookFixture::position("k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 1")?;
    let without = OpeningBookFixture::position("k3r3/8/8/3pP3/8/8/8/4K3 w - - 90 200")?;
    assert_eq!(position.identity(), without.identity());
    assert_eq!(
        position.standard_polyglot_key()?.value() ^ without.standard_polyglot_key()?.value(),
        0x1c99_ded3_3cb8_90a1
    );
    let bytes = OpeningBookFixture::for_position(&position, &[(0x092b, 50)])?;
    let study = OpeningBookFixture::book(&bytes)?.study(&position, || false)?;
    assert!(study.moves().is_empty());
    assert_eq!(study.rejected_illegal_records(), 1);
    assert_eq!(study.total_legal_weight(), 0);
    Ok(())
}

#[test]
fn literal_big_endian_record_preserves_raw_weight_learning_and_actual_source_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = OpeningBookFixture::LITERAL_RECORD;
    let book = OpeningBookFixture::book(&bytes)?;
    assert_eq!(book.record_count(), 1);
    assert_eq!(book.identity().source(), &OpeningBookFixture::source()?);
    assert_eq!(
        book.identity().checksum(),
        PackageDigest::hash_slice(&bytes)
    );
    assert_eq!(
        book.identity().checksum(),
        OpeningBookFixture::LITERAL_CHECKSUM.parse()?
    );
    let position = BoardPosition::standard();
    let study = book.study(&position, || false)?;
    assert_eq!(study.identity(), book.identity());
    assert_eq!(study.position(), &position.identity());
    assert_eq!(study.key().value(), OpeningBookFixture::START_KEY);
    assert_eq!(study.moves().len(), 1);
    let entry = &study.moves()[0];
    assert_eq!(entry.uci(), "e2e4");
    assert_eq!(entry.movement(), position.parse_uci("e2e4")?);
    assert_eq!(entry.raw_weight(), 258);
    assert_eq!(entry.learn(), 0x0102_0304);
    assert_eq!(
        entry.share().ok_or("Missing normalized share")?.to_string(),
        "100.0%"
    );
    Ok(())
}

#[test]
fn legal_weights_sort_descending_with_canonical_uci_ties_and_ignore_illegal_denominators()
-> Result<(), Box<dyn std::error::Error>> {
    let key = OpeningBookFixture::START_KEY;
    let bytes = OpeningBookFixture::records(&[
        (key, OpeningBookFixture::E2E4, 20),
        (key, OpeningBookFixture::E7E5, u16::MAX),
        (key, OpeningBookFixture::G1F3, 10),
        (key, OpeningBookFixture::D2D4, 20),
    ]);
    let position = BoardPosition::standard();
    let study = OpeningBookFixture::book(&bytes)?.study(&position, || false)?;
    assert_eq!(
        study
            .moves()
            .iter()
            .map(|entry| entry.uci())
            .collect::<Vec<_>>(),
        ["d2d4", "e2e4", "g1f3"]
    );
    assert_eq!(study.total_legal_weight(), 50);
    assert_eq!(study.rejected_illegal_records(), 1);
    assert_eq!(
        study
            .moves()
            .iter()
            .map(|entry| entry.share().map(|share| share.tenths()))
            .collect::<Vec<_>>(),
        [Some(400), Some(400), Some(200)]
    );
    Ok(())
}

#[test]
fn zero_weights_remain_visible_and_zero_shares_are_available_only_with_a_positive_total()
-> Result<(), Box<dyn std::error::Error>> {
    for (weight, expected) in [(0, [None, None]), (1, [Some(1000), Some(0)])] {
        let bytes = OpeningBookFixture::for_position(
            &BoardPosition::standard(),
            &[
                (OpeningBookFixture::E2E4, weight),
                (OpeningBookFixture::D2D4, 0),
            ],
        )?;
        let study =
            OpeningBookFixture::book(&bytes)?.study(&BoardPosition::standard(), || false)?;
        assert_eq!(study.moves().len(), 2);
        assert_eq!(
            study
                .moves()
                .iter()
                .map(|entry| entry.share().map(|share| share.tenths()))
                .collect::<Vec<_>>(),
            expected
        );
    }
    Ok(())
}

#[test]
fn stored_scaled_weights_round_to_one_decimal_without_forcing_the_sum_to_one_hundred()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = OpeningBookFixture::for_position(
        &BoardPosition::standard(),
        &[
            (OpeningBookFixture::E2E4, u16::MAX),
            (OpeningBookFixture::D2D4, u16::MAX),
            (OpeningBookFixture::G1F3, u16::MAX),
        ],
    )?;
    let study = OpeningBookFixture::book(&bytes)?.study(&BoardPosition::standard(), || false)?;
    assert_eq!(study.total_legal_weight(), 196_605);
    for entry in study.moves() {
        assert_eq!(entry.raw_weight(), u16::MAX);
        assert_eq!(entry.share().ok_or("Missing share")?.to_string(), "33.3%");
    }
    Ok(())
}

#[test]
fn normalized_shares_round_half_up_from_stored_integers() -> Result<(), Box<dyn std::error::Error>>
{
    let bytes = OpeningBookFixture::for_position(
        &BoardPosition::standard(),
        &[
            (OpeningBookFixture::E2E4, 1),
            (OpeningBookFixture::D2D4, 15),
        ],
    )?;
    let study = OpeningBookFixture::book(&bytes)?.study(&BoardPosition::standard(), || false)?;
    assert_eq!(
        study.moves()[0].share().ok_or("Missing share")?.to_string(),
        "93.8%"
    );
    assert_eq!(
        study.moves()[1].share().ok_or("Missing share")?.to_string(),
        "6.3%"
    );
    Ok(())
}

#[test]
fn castling_reference_records_use_king_to_rook_encoding_and_return_canonical_board_moves()
-> Result<(), Box<dyn std::error::Error>> {
    for (turn, records, expected) in [
        (
            "w",
            [(0x0107, 30), (0x0100, 20), (0x0106, 10)],
            ["e1g1", "e1c1"],
        ),
        (
            "b",
            [(0x0f3f, 30), (0x0f38, 20), (0x0f3e, 10)],
            ["e8g8", "e8c8"],
        ),
    ] {
        let position =
            OpeningBookFixture::position(&format!("r3k2r/8/8/8/8/8/8/R3K2R {turn} KQkq - 0 1"))?;
        let bytes = OpeningBookFixture::for_position(&position, &records)?;
        let study = OpeningBookFixture::book(&bytes)?.study(&position, || false)?;
        assert_eq!(study.rejected_illegal_records(), 1);
        assert_eq!(study.moves().len(), expected.len());
        for (entry, uci) in study.moves().iter().zip(expected) {
            assert_eq!(entry.uci(), uci);
            assert_eq!(entry.movement(), position.parse_uci(uci)?);
            assert!(entry.movement().castling_rook().is_some());
            position.play(entry.movement())?;
        }
    }
    Ok(())
}

#[test]
fn promotion_codes_and_en_passant_records_match_real_legal_board_moves()
-> Result<(), Box<dyn std::error::Error>> {
    let position = OpeningBookFixture::position("4k3/P7/8/8/8/8/8/4K3 w - - 0 1")?;
    let bytes = OpeningBookFixture::for_position(
        &position,
        &[
            (0x1c38, 1),
            (0x2c38, 1),
            (0x3c38, 1),
            (0x4c38, 1),
            (0x0c38, 40),
        ],
    )?;
    let study = OpeningBookFixture::book(&bytes)?.study(&position, || false)?;
    assert_eq!(study.rejected_illegal_records(), 1);
    assert_eq!(
        study
            .moves()
            .iter()
            .map(|entry| entry.uci())
            .collect::<Vec<_>>(),
        ["a7a8b", "a7a8n", "a7a8q", "a7a8r"]
    );
    for entry in study.moves() {
        assert_eq!(entry.movement(), position.parse_uci(entry.uci())?);
        assert_eq!(
            entry.share().ok_or("Missing promotion share")?.tenths(),
            250
        );
    }
    let position = OpeningBookFixture::position(
        "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
    )?;
    let bytes = OpeningBookFixture::record(0x22a4_8b5a_8e47_ff78, 0x092d, 7, 0);
    let study = OpeningBookFixture::book(&bytes)?.study(&position, || false)?;
    assert_eq!(study.moves().len(), 1);
    assert_eq!(study.moves()[0].movement(), position.parse_uci("e5f6")?);
    Ok(())
}

#[test]
fn black_promotion_reference_records_preserve_all_four_promotion_choices()
-> Result<(), Box<dyn std::error::Error>> {
    let position = OpeningBookFixture::position("4k3/8/8/8/8/8/7p/4K3 b - - 0 1")?;
    let bytes = OpeningBookFixture::for_position(
        &position,
        &[(0x13c7, 1), (0x23c7, 1), (0x33c7, 1), (0x43c7, 1)],
    )?;
    let study = OpeningBookFixture::book(&bytes)?.study(&position, || false)?;
    assert_eq!(
        study
            .moves()
            .iter()
            .map(|entry| entry.uci())
            .collect::<Vec<_>>(),
        ["h2h1b", "h2h1n", "h2h1q", "h2h1r"]
    );
    for entry in study.moves() {
        assert_eq!(entry.movement(), position.parse_uci(entry.uci())?);
        position.play(entry.movement())?;
    }
    Ok(())
}

#[test]
fn lookup_uses_only_the_selected_key_and_reports_an_absent_key_without_fabricating_moves()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = OpeningBookFixture::records(&[
        (1, OpeningBookFixture::D2D4, 1),
        (OpeningBookFixture::START_KEY, OpeningBookFixture::E2E4, 2),
        (u64::MAX, OpeningBookFixture::D2D4, 3),
    ]);
    let book = OpeningBookFixture::book(&bytes)?;
    let position = BoardPosition::standard();
    assert_eq!(book.study(&position, || false)?.moves()[0].uci(), "e2e4");
    let absent = book.study(&position.play_uci("e2e4")?, || false)?;
    assert!(absent.moves().is_empty());
    assert_eq!(absent.rejected_illegal_records(), 0);
    Ok(())
}

#[test]
fn chess960_is_refused_with_the_original_typed_variant() -> Result<(), Box<dyn std::error::Error>> {
    let book = OpeningBookFixture::book(&OpeningBookFixture::LITERAL_RECORD)?;
    let position =
        BoardPosition::from_fen(OpeningBookFixture::START_FEN, BoardEnumVariant::Chess960)?;
    assert!(matches!(
        book.study(&position, || false),
        Err(OpeningBookError::Position(
            BoardPositionError::UnsupportedPolyglotVariant(BoardEnumVariant::Chess960)
        ))
    ));
    Ok(())
}

#[test]
fn validation_refuses_empty_truncated_unsorted_duplicate_and_malformed_records()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        OpeningBookFixture::book(&[])
            .err()
            .ok_or("Expected empty refusal")?
            .downcast_ref::<OpeningBookError>(),
        Some(OpeningBookError::Empty)
    ));
    for complete_records in 0..=1 {
        for length in 1..16 {
            let mut bytes = Vec::new();
            if complete_records == 1 {
                bytes.extend_from_slice(&OpeningBookFixture::LITERAL_RECORD);
            }
            bytes.extend_from_slice(&OpeningBookFixture::LITERAL_RECORD[..length]);
            let error = OpeningBookFixture::book(&bytes)
                .err()
                .ok_or("Expected truncation refusal")?;
            assert!(matches!(
                error.downcast_ref::<OpeningBookError>(),
                Some(OpeningBookError::Truncated { .. })
            ));
        }
    }
    let key = OpeningBookFixture::START_KEY;
    for (bytes, duplicate) in [
        (
            OpeningBookFixture::records(&[
                (key, OpeningBookFixture::E2E4, 1),
                (key - 1, OpeningBookFixture::D2D4, 1),
            ]),
            false,
        ),
        (
            OpeningBookFixture::records(&[
                (key, OpeningBookFixture::E2E4, 1),
                (key, OpeningBookFixture::D2D4, 1),
                (key, OpeningBookFixture::E2E4, 2),
            ]),
            true,
        ),
    ] {
        let error = OpeningBookFixture::book(&bytes)
            .err()
            .ok_or("Expected record refusal")?;
        if duplicate {
            assert!(matches!(
                error.downcast_ref::<OpeningBookError>(),
                Some(OpeningBookError::DuplicateMove { record: 2, .. })
            ));
        } else {
            assert!(matches!(
                error.downcast_ref::<OpeningBookError>(),
                Some(OpeningBookError::Unsorted { record: 1, .. })
            ));
        }
    }
    for movement in [0x831c, 0x531c, 0x631c, 0x731c] {
        let bytes = OpeningBookFixture::record(key, movement, 1, 0);
        let error = OpeningBookFixture::book(&bytes)
            .err()
            .ok_or("Expected encoding refusal")?;
        assert!(matches!(
            error.downcast_ref::<OpeningBookError>(),
            Some(OpeningBookError::MoveEncoding { record: 0, .. })
        ));
    }
    Ok(())
}

#[test]
fn streaming_validation_retains_order_and_duplicate_state_across_chunk_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let mut distinct_keys = (0..4_096)
        .flat_map(|key| OpeningBookFixture::record(key, OpeningBookFixture::E2E4, 1, 0))
        .collect::<Vec<_>>();
    assert_eq!(distinct_keys.len(), 65_536);
    assert_eq!(
        OpeningBookFixture::book(&distinct_keys)?.record_count(),
        4_096
    );
    distinct_keys.extend_from_slice(&OpeningBookFixture::record(
        4_094,
        OpeningBookFixture::D2D4,
        1,
        0,
    ));
    let error = OpeningBookFixture::book(&distinct_keys)
        .err()
        .ok_or("Expected boundary order refusal")?;
    assert!(matches!(
        error.downcast_ref::<OpeningBookError>(),
        Some(OpeningBookError::Unsorted { record: 4_096, .. })
    ));
    let mut same_key = (0..4_096)
        .flat_map(|movement| {
            OpeningBookFixture::record(OpeningBookFixture::START_KEY, movement, 1, 0)
        })
        .collect::<Vec<_>>();
    same_key.extend_from_slice(&OpeningBookFixture::record(
        OpeningBookFixture::START_KEY,
        0,
        2,
        0,
    ));
    let error = OpeningBookFixture::book(&same_key)
        .err()
        .ok_or("Expected boundary duplicate refusal")?;
    assert!(matches!(
        error.downcast_ref::<OpeningBookError>(),
        Some(OpeningBookError::DuplicateMove { record: 4_096, .. })
    ));
    Ok(())
}

#[test]
fn cancellation_during_a_large_key_group_does_not_poison_later_queries()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = (0..4_096)
        .flat_map(|movement| {
            OpeningBookFixture::record(OpeningBookFixture::START_KEY, movement, 1, 0)
        })
        .collect::<Vec<_>>();
    let book = OpeningBookFixture::book(&bytes)?;
    for cancel_at in [128, 1_024] {
        let calls = Cell::new(0);
        let result = book.study(&BoardPosition::standard(), || {
            let called = calls.get();
            calls.set(called + 1);
            called >= cancel_at
        });
        assert!(matches!(result, Err(OpeningBookError::Cancelled)));
    }
    let study = book.study(&BoardPosition::standard(), || false)?;
    assert_eq!(study.moves().len(), 20);
    assert_eq!(study.rejected_illegal_records(), 4_076);
    Ok(())
}

#[test]
fn streaming_validation_can_cancel_after_a_complete_chunk_without_admitting_partial_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = (0..4_097)
        .flat_map(|key| OpeningBookFixture::record(key, OpeningBookFixture::E2E4, 1, 0))
        .collect::<Vec<_>>();
    let calls = Cell::new(0);
    let result = OpeningBook::validate(
        &bytes,
        OpeningBookFixture::source()?,
        PackageDigest::hash_slice(&bytes),
        OpeningBookFixture::limits(bytes.len())?,
        || {
            let called = calls.get();
            calls.set(called + 1);
            called >= 4_100
        },
    );
    assert!(matches!(result, Err(OpeningBookError::Cancelled)));
    assert_eq!(OpeningBookFixture::book(&bytes)?.record_count(), 4_097);
    Ok(())
}

#[test]
fn explicit_byte_limits_and_expected_checksum_are_enforced_before_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = OpeningBookFixture::LITERAL_RECORD;
    for (limit, checksum, exceeds) in [
        (15, PackageDigest::hash_slice(&bytes), true),
        (16, PackageDigest::hash_slice(b"different bytes"), false),
    ] {
        let error = OpeningBook::validate(
            &bytes,
            OpeningBookFixture::source()?,
            checksum,
            OpeningBookFixture::limits(limit)?,
            || false,
        )
        .err()
        .ok_or("Expected validation refusal")?;
        if exceeds {
            assert!(matches!(
                error,
                OpeningBookError::ByteLimit {
                    actual: 16,
                    limit: 15
                }
            ));
        } else {
            assert!(
                matches!(error, OpeningBookError::Checksum { expected, actual } if expected == checksum && actual == PackageDigest::hash_slice(&bytes))
            );
        }
    }
    assert!(
        OpeningBookLimits::new(
            NonZeroUsize::new(OpeningBookLimits::MAX_DECODED_BYTES + 1).ok_or("Missing bound")?
        )
        .is_err()
    );
    assert_eq!(
        OpeningBookFixture::limits(OpeningBookLimits::MAX_DECODED_BYTES)?
            .max_bytes()
            .get(),
        OpeningBookLimits::MAX_DECODED_BYTES
    );
    Ok(())
}

#[test]
fn validation_and_lookup_cancel_deterministically_before_returning_any_partial_result()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = OpeningBookFixture::LITERAL_RECORD;
    for cancel_at in [0, 2, 5] {
        let calls = Cell::new(0);
        let result = OpeningBook::validate(
            &bytes,
            OpeningBookFixture::source()?,
            PackageDigest::hash_slice(&bytes),
            OpeningBookFixture::limits(16)?,
            || {
                let called = calls.get();
                calls.set(called + 1);
                called >= cancel_at
            },
        );
        assert!(matches!(result, Err(OpeningBookError::Cancelled)));
    }
    let book = OpeningBookFixture::book(&bytes)?;
    for cancel_at in [0, 3] {
        let calls = Cell::new(0);
        let result = book.study(&BoardPosition::standard(), || {
            let called = calls.get();
            calls.set(called + 1);
            called >= cancel_at
        });
        assert!(matches!(result, Err(OpeningBookError::Cancelled)));
    }
    assert_eq!(
        book.study(&BoardPosition::standard(), || false)?
            .moves()
            .len(),
        1
    );
    Ok(())
}

#[test]
fn prepared_study_moves_cannot_submit_a_live_move_and_preserve_the_played_mainline()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let bytes = OpeningBookFixture::for_position(
        &BoardPosition::standard(),
        &[(OpeningBookFixture::D2D4, 0)],
    )?;
    let book = OpeningBookFixture::book(&bytes)?;
    let study = book.study(session.tree().node(session.view())?.position(), || false)?;
    let revision = session.revision();
    assert!(
        session
            .append_study_variation(
                revision,
                GameTransitionFixture::id(3)?,
                study.moves()[0].movement()
            )
            .is_err()
    );
    assert_eq!(session.revision(), revision);
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    let played = session.live();
    session.resign(
        GameResultResignRequest {
            expected_revision: session.revision(),
            resigning: BoardEnumColor::Black,
            received_at: ClockInstant::from_millis(2_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let outcome = session.outcome();
    let root = session.tree().root();
    session.select_view(root)?;
    let branch = session
        .append_study_variation(
            session.revision(),
            GameTransitionFixture::id(3)?,
            study.moves()[0].movement(),
        )?
        .view();
    assert_eq!(session.tree().node(root)?.children(), &[played, branch]);
    assert_eq!(session.live(), played);
    assert_eq!(session.outcome(), outcome);
    session.select_view(root)?;
    let reused = session
        .append_study_variation(
            session.revision(),
            GameTransitionFixture::id(4)?,
            study.moves()[0].movement(),
        )?
        .view();
    assert_eq!(reused, branch);
    assert_eq!(session.tree().len(), 3);
    Ok(())
}
