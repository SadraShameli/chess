use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::engine::analysis::{EngineAnalysis, EngineAnalysisScore};
use chess::report::scoring::{
    ReportScoringEnumAssessment, ReportScoringEnumCoverage, ReportScoringEnumLabel,
    ReportScoringEnumPolicyVersion, ReportScoringSample, ReportScoringSummary,
};

type ReportScoringTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn black_missed_mate_and_wrong_score_perspective_are_explicit() -> ReportScoringTestResult {
    let position = BoardPosition::standard().play_uci("e2e4")?;
    let sample = ReportScoringFixture::at(
        position.clone(),
        "e7e5",
        "info score mate 3",
        "info score mate 1",
    )?;
    let ReportScoringEnumAssessment::Scored(scored) =
        sample.assess(ReportScoringEnumPolicyVersion::ExpectedScoreV1)
    else {
        return Err("Missing black mate sample".into());
    };
    assert_eq!(scored.loss(), 1.0);
    assert!(scored.is_missed_mate());
    let white_score = ReportScoringFixture::score(&BoardPosition::standard(), "info score cp 0")?;
    assert!(
        ReportScoringSample::new(
            &position,
            position.parse_uci("e7e5")?,
            Some(white_score),
            None
        )
        .is_err()
    );
    Ok(())
}

struct ReportScoringFixture;

impl ReportScoringFixture {
    fn score(position: &BoardPosition, text: &str) -> ReportScoringTestResult<EngineAnalysisScore> {
        EngineAnalysis::parse(text, position)?
            .line()
            .and_then(|line| line.score())
            .ok_or_else(|| "Missing fixture score".into())
    }

    fn sample(before: &str, after: &str) -> ReportScoringTestResult<ReportScoringSample> {
        Self::at(BoardPosition::standard(), "e2e4", before, after)
    }

    fn at(
        position: BoardPosition,
        movement: &str,
        before: &str,
        after: &str,
    ) -> ReportScoringTestResult<ReportScoringSample> {
        let movement = position.parse_uci(movement)?;
        let next = position.play(movement)?;
        Ok(ReportScoringSample::new(
            &position,
            movement,
            Some(Self::score(&position, before)?),
            Some(Self::score(&next, after)?),
        )?)
    }

    fn loss(sample: &ReportScoringSample) -> ReportScoringTestResult<f64> {
        match sample.assess(ReportScoringEnumPolicyVersion::ExpectedScoreV1) {
            ReportScoringEnumAssessment::Scored(value) => Ok(value.loss()),
            ReportScoringEnumAssessment::Unscored { .. } => Err("Unscored fixture".into()),
        }
    }
}

#[test]
fn fixed_reference_values_distinguish_natural_exponential_from_base_ten() -> ReportScoringTestResult
{
    for (before, after, expected) in [
        (
            "info score cp 400",
            "info score cp 0",
            0.231_058_578_630_004_9,
        ),
        (
            "info score cp 0",
            "info score cp 400",
            0.231_058_578_630_004_9,
        ),
        (
            "info score cp 800",
            "info score cp -400",
            0.149_738_499_347_877_6,
        ),
        ("info score cp -400", "info score cp -800", 0.0),
    ] {
        let sample = ReportScoringFixture::sample(before, after)?;
        assert!((ReportScoringFixture::loss(&sample)? - expected).abs() < 1e-12);
    }
    Ok(())
}

#[test]
fn black_mover_uses_black_expected_score_and_preserves_engine_perspective()
-> ReportScoringTestResult {
    let sample = ReportScoringFixture::at(
        BoardPosition::standard().play_uci("e2e4")?,
        "e7e5",
        "info score cp 400",
        "info score cp 0",
    )?;
    assert_eq!(sample.mover(), BoardEnumColor::Black);
    assert!((ReportScoringFixture::loss(&sample)? - 0.231_058_578_630_004_9).abs() < 1e-12);
    Ok(())
}

#[test]
fn thresholds_are_inclusive_and_neighboring_centipawns_have_independent_labels()
-> ReportScoringTestResult {
    let version = ReportScoringEnumPolicyVersion::ExpectedScoreV1;
    for (loss, expected) in [
        (0.0, ReportScoringEnumLabel::Acceptable),
        (0.049_999, ReportScoringEnumLabel::Acceptable),
        (0.05, ReportScoringEnumLabel::Inaccuracy),
        (0.099_999, ReportScoringEnumLabel::Inaccuracy),
        (0.10, ReportScoringEnumLabel::Mistake),
        (0.199_999, ReportScoringEnumLabel::Mistake),
        (0.20, ReportScoringEnumLabel::Blunder),
        (1.0, ReportScoringEnumLabel::Blunder),
    ] {
        assert_eq!(version.classify_loss(loss)?, expected);
    }
    for (cp, expected) in [
        (80, ReportScoringEnumLabel::Acceptable),
        (81, ReportScoringEnumLabel::Inaccuracy),
        (162, ReportScoringEnumLabel::Inaccuracy),
        (163, ReportScoringEnumLabel::Mistake),
        (338, ReportScoringEnumLabel::Mistake),
        (339, ReportScoringEnumLabel::Blunder),
    ] {
        let sample =
            ReportScoringFixture::sample("info score cp 0", &format!("info score cp {cp}"))?;
        let ReportScoringEnumAssessment::Scored(scored) = sample.assess(version) else {
            return Err("Missing classification".into());
        };
        assert_eq!(scored.label(), expected);
    }
    for loss in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(version.classify_loss(loss).is_err());
    }
    Ok(())
}

#[test]
fn cp_clamp_handles_both_i32_extremes_without_overflow() -> ReportScoringTestResult {
    for (before, after) in [
        ("info score cp 2147483647", "info score cp 2147483647"),
        ("info score cp 10000", "info score cp 10000"),
    ] {
        let loss = ReportScoringFixture::loss(&ReportScoringFixture::sample(before, after)?)?;
        assert!((loss - 0.999_999_999_972_224_1).abs() < 1e-12);
    }
    let zero =
        ReportScoringFixture::sample("info score cp -2147483648", "info score cp -2147483648")?;
    assert_eq!(ReportScoringFixture::loss(&zero)?, 0.0);
    Ok(())
}

#[test]
fn bounds_missing_scores_and_nonterminal_mate_zero_remain_unclassified() -> ReportScoringTestResult
{
    let position = BoardPosition::standard();
    let movement = position.parse_uci("e2e4")?;
    for (before, after) in [
        ("info score cp 400 lowerbound", "info score cp 0"),
        ("info score mate 3 upperbound", "info score mate -2"),
        ("info score cp 0", "info score cp 0 lowerbound"),
        ("info score mate 0", "info score cp 0"),
    ] {
        let sample = ReportScoringFixture::sample(before, after)?;
        assert!(matches!(
            sample.assess(ReportScoringEnumPolicyVersion::ExpectedScoreV1),
            ReportScoringEnumAssessment::Unscored { .. }
        ));
    }
    let missing = ReportScoringSample::new(&position, movement, None, None)?;
    assert!(matches!(
        missing.assess(ReportScoringEnumPolicyVersion::ExpectedScoreV1),
        ReportScoringEnumAssessment::Unscored { .. }
    ));
    Ok(())
}

#[test]
fn mate_distance_and_lost_forced_mate_are_separate_from_cp() -> ReportScoringTestResult {
    let version = ReportScoringEnumPolicyVersion::ExpectedScoreV1;
    let maintained = ReportScoringFixture::sample("info score mate 4", "info score mate -9")?;
    let ReportScoringEnumAssessment::Scored(scored) = maintained.assess(version) else {
        return Err("Missing mate sample".into());
    };
    assert_eq!(scored.loss(), 0.0);
    assert!(!scored.is_missed_mate());
    assert_eq!(scored.before_white_mate(), Some(4));
    assert_eq!(scored.after_white_mate(), Some(9));
    let missed = ReportScoringFixture::sample("info score mate 4", "info score cp 0")?;
    let ReportScoringEnumAssessment::Scored(scored) = missed.assess(version) else {
        return Err("Missing missed-mate sample".into());
    };
    assert_eq!(scored.loss(), 0.5);
    assert!(scored.is_missed_mate());
    assert_eq!(scored.label(), ReportScoringEnumLabel::Blunder);
    Ok(())
}

#[test]
fn actual_checkmate_stalemate_and_forced_move_boards_override_missing_terminal_searches()
-> ReportScoringTestResult {
    let mate =
        BoardPosition::from_fen("7k/8/5KQ1/8/8/8/8/8 w - - 0 1", BoardEnumVariant::Standard)?;
    let movement = mate.parse_uci("g6g7")?;
    assert!(mate.play(movement)?.is_checkmate());
    let sample = ReportScoringSample::new(
        &mate,
        movement,
        Some(ReportScoringFixture::score(&mate, "info score mate 1")?),
        None,
    )?;
    assert_eq!(ReportScoringFixture::loss(&sample)?, 0.0);
    let draw =
        BoardPosition::from_fen("7k/8/5KQ1/8/8/8/8/8 w - - 0 1", BoardEnumVariant::Standard)?;
    let movement = draw.parse_uci("g6f7")?;
    assert!(draw.play(movement)?.is_stalemate());
    let sample = ReportScoringSample::new(
        &draw,
        movement,
        Some(ReportScoringFixture::score(&draw, "info score mate 1")?),
        None,
    )?;
    assert_eq!(ReportScoringFixture::loss(&sample)?, 0.5);
    let forced = BoardPosition::from_fen(
        "7k/5Br1/5K2/8/8/8/8/7R b - - 0 1",
        BoardEnumVariant::Standard,
    )?;
    assert_eq!(forced.legal_moves().len(), 1);
    let movement = *forced.legal_moves().first().ok_or("Missing forced move")?;
    let sample = ReportScoringSample::new(&forced, movement, None, None)?;
    assert!(sample.is_forced());
    let summary = ReportScoringSummary::new(
        ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        ReportScoringEnumCoverage::Partial,
        [&sample],
    )?;
    assert_eq!(summary.player(BoardEnumColor::Black).forced_moves(), 1);
    assert_eq!(summary.player(BoardEnumColor::Black).accuracy(), None);
    Ok(())
}

#[test]
fn scored_forced_loss_is_excluded_from_the_mean_and_empty_players_are_unavailable()
-> ReportScoringTestResult {
    let forced = ReportScoringFixture::at(
        BoardPosition::from_fen(
            "7k/5Br1/5K2/8/8/8/8/7R b - - 0 1",
            BoardEnumVariant::Standard,
        )?,
        "g7h7",
        "info score cp 400",
        "info score cp 400",
    )?;
    assert!(forced.is_forced());
    assert!((ReportScoringFixture::loss(&forced)? - 0.462_117_157_260_009_8).abs() < 1e-12);
    let regular = ReportScoringFixture::at(
        BoardPosition::standard().play_uci("e2e4")?,
        "e7e5",
        "info score cp 0",
        "info score cp 0",
    )?;
    let summary = ReportScoringSummary::new(
        ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        ReportScoringEnumCoverage::Complete,
        [&forced, &regular],
    )?;
    assert_eq!(summary.player(BoardEnumColor::Black).scored_moves(), 1);
    assert_eq!(summary.player(BoardEnumColor::Black).forced_moves(), 1);
    assert_eq!(
        summary.player(BoardEnumColor::Black).accuracy(),
        Some(100.0)
    );
    assert_eq!(summary.player(BoardEnumColor::White).accuracy(), None);
    Ok(())
}

#[test]
fn per_player_mean_loss_uses_only_eligible_samples_and_formats_one_decimal()
-> ReportScoringTestResult {
    let first = ReportScoringFixture::sample("info score cp 400", "info score cp 0")?;
    let second = ReportScoringFixture::sample("info score cp 0", "info score cp -400")?;
    let black = ReportScoringFixture::at(
        BoardPosition::standard().play_uci("e2e4")?,
        "e7e5",
        "info score cp 0",
        "info score cp 0",
    )?;
    let missing = ReportScoringSample::new(
        &BoardPosition::standard(),
        BoardPosition::standard().parse_uci("d2d4")?,
        None,
        None,
    )?;
    let summary = ReportScoringSummary::new(
        ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        ReportScoringEnumCoverage::Partial,
        [&first, &second, &black, &missing],
    )?;
    let white = summary.player(BoardEnumColor::White);
    assert_eq!(white.scored_moves(), 2);
    assert_eq!(white.unscored_moves(), 1);
    assert!((white.accuracy().ok_or("Missing accuracy")? - 88.447_071_068_499_75).abs() < 1e-12);
    assert_eq!(white.display_accuracy().as_deref(), Some("88.4"));
    assert_eq!(
        summary
            .player(BoardEnumColor::Black)
            .display_accuracy()
            .as_deref(),
        Some("100.0")
    );
    assert_eq!(summary.coverage(), ReportScoringEnumCoverage::Partial);
    Ok(())
}

#[test]
fn an_unscored_sample_downgrades_a_complete_summary_to_partial() -> ReportScoringTestResult {
    let scored = ReportScoringFixture::sample("info score cp 0", "info score cp 0")?;
    let missing = ReportScoringSample::new(
        &BoardPosition::standard(),
        BoardPosition::standard().parse_uci("d2d4")?,
        None,
        None,
    )?;
    let complete = ReportScoringSummary::new(
        ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        ReportScoringEnumCoverage::Complete,
        [&scored],
    )?;
    assert_eq!(complete.coverage(), ReportScoringEnumCoverage::Complete);
    let downgraded = ReportScoringSummary::new(
        ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        ReportScoringEnumCoverage::Complete,
        [&scored, &missing],
    )?;
    assert_eq!(downgraded.coverage(), ReportScoringEnumCoverage::Partial);
    Ok(())
}

#[test]
fn bound_only_moves_are_counted_unscored_and_give_no_accuracy() -> ReportScoringTestResult {
    let lower = ReportScoringFixture::sample("info score cp 100 lowerbound", "info score cp 0")?;
    let upper = ReportScoringFixture::sample("info score cp 100", "info score cp 0 upperbound")?;
    for sample in [&lower, &upper] {
        assert!(matches!(
            sample.assess(ReportScoringEnumPolicyVersion::ExpectedScoreV1),
            ReportScoringEnumAssessment::Unscored { .. }
        ));
    }
    let summary = ReportScoringSummary::new(
        ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        ReportScoringEnumCoverage::Complete,
        [&lower, &upper],
    )?;
    let white = summary.player(BoardEnumColor::White);
    assert_eq!(white.scored_moves(), 0);
    assert_eq!(white.unscored_moves(), 2);
    assert_eq!(white.accuracy(), None);
    assert_eq!(white.display_accuracy(), None);
    assert_eq!(summary.coverage(), ReportScoringEnumCoverage::Partial);
    Ok(())
}

#[test]
fn players_accumulate_independently_with_shared_sample_accounting() -> ReportScoringTestResult {
    let white = ReportScoringFixture::sample("info score cp 400", "info score cp 0")?;
    let black = ReportScoringFixture::at(
        BoardPosition::standard().play_uci("e2e4")?,
        "e7e5",
        "info score cp 0",
        "info score cp 0",
    )?;
    let summary = ReportScoringSummary::new(
        ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        ReportScoringEnumCoverage::Complete,
        [&white, &black],
    )?;
    assert_eq!(summary.player(BoardEnumColor::White).scored_moves(), 1);
    assert_eq!(summary.player(BoardEnumColor::Black).scored_moves(), 1);
    assert_eq!(
        summary
            .player(BoardEnumColor::Black)
            .display_accuracy()
            .as_deref(),
        Some("100.0")
    );
    assert!(
        summary
            .player(BoardEnumColor::White)
            .accuracy()
            .ok_or("accuracy")?
            < 100.0
    );
    Ok(())
}
