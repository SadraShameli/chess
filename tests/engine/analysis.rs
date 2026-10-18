use std::time::Duration;

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::engine::analysis::{
    EngineAnalysis, EngineAnalysisEnumBound, EngineAnalysisEnumScore, EngineAnalysisLine,
    EngineAnalysisStaticEvaluation,
};
use chess::engine::process_provider::EngineProcessLimits;

type EngineAnalysisTestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn complete_analysis_preserves_legal_continuation_and_exact_large_counters()
-> EngineAnalysisTestResult {
    let root = BoardPosition::standard();
    let text = "info depth 18 seldepth 31 multipv 2 score cp 43 lowerbound nodes 9007199254740993 time 789 nps 12345678 hashfull 321 tbhits 0 pv e2e4 e7e5 g1f3";
    let analysis = EngineAnalysis::parse(text, &root)?;
    let line = analysis.line().ok_or("missing analysis")?;
    assert_eq!(analysis.text(), text);
    assert_eq!(line.depth(), Some(18));
    assert_eq!(line.selective_depth(), Some(31));
    assert_eq!(line.rank().map(std::num::NonZeroU32::get), Some(2));
    assert_eq!(line.nodes(), Some(9_007_199_254_740_993));
    assert_eq!(line.elapsed(), Some(Duration::from_millis(789)));
    assert_eq!(line.nodes_per_second(), Some(12_345_678));
    assert_eq!(
        line.hash_occupancy().ok_or("missing hashfull")?.per_mille(),
        321
    );
    assert_eq!(line.tablebase_hits(), Some(0));
    let score = line.score().ok_or("missing score")?;
    assert_eq!(score.reported(), EngineAnalysisEnumScore::Centipawns(43));
    assert_eq!(score.perspective(), BoardEnumColor::White);
    assert_eq!(score.bound(), EngineAnalysisEnumBound::Lower);
    assert_eq!(score.white_value(), 43);
    assert_eq!(score.white_bound(), EngineAnalysisEnumBound::Lower);
    let mut position = root;
    let continuation = line.variation().ok_or("missing continuation")?;
    assert_eq!(continuation.len(), 3);
    for (&movement, text) in continuation.iter().zip(["e2e4", "e7e5", "g1f3"]) {
        assert_eq!(movement, position.parse_uci(text)?);
        position = position.play(movement)?;
    }
    Ok(())
}

#[test]
fn missing_statistics_remain_absent_while_reported_zero_is_preserved() -> EngineAnalysisTestResult {
    let root = BoardPosition::standard();
    let sparse = EngineAnalysis::parse("info depth 1 pv e2e4", &root)?;
    let sparse = sparse.line().ok_or("missing analysis")?;
    assert_eq!(sparse.depth(), Some(1));
    assert_eq!(sparse.selective_depth(), None);
    assert_eq!(sparse.rank(), None);
    assert_eq!(sparse.score(), None);
    assert_eq!(sparse.nodes(), None);
    assert_eq!(sparse.elapsed(), None);
    assert_eq!(sparse.nodes_per_second(), None);
    assert_eq!(sparse.hash_occupancy(), None);
    assert_eq!(sparse.tablebase_hits(), None);
    let zero = EngineAnalysis::parse("info nodes 0 time 0 nps 0 hashfull 0 tbhits 0", &root)?;
    let zero = zero.line().ok_or("missing statistics")?;
    assert_eq!(zero.nodes(), Some(0));
    assert_eq!(zero.elapsed(), Some(Duration::ZERO));
    assert_eq!(zero.nodes_per_second(), Some(0));
    assert_eq!(
        zero.hash_occupancy().ok_or("missing hashfull")?.per_mille(),
        0
    );
    assert_eq!(zero.tablebase_hits(), Some(0));
    assert_eq!(zero.variation(), None);
    Ok(())
}

#[test]
fn black_scores_preserve_reported_values_and_invert_display_bounds() -> EngineAnalysisTestResult {
    let root = BoardPosition::standard().play_uci("e2e4")?;
    for (text, reported, expected, bound, white_bound) in [
        (
            "info score cp 57 lowerbound",
            EngineAnalysisEnumScore::Centipawns(57),
            -57,
            EngineAnalysisEnumBound::Lower,
            EngineAnalysisEnumBound::Upper,
        ),
        (
            "info score cp -89 upperbound",
            EngineAnalysisEnumScore::Centipawns(-89),
            89,
            EngineAnalysisEnumBound::Upper,
            EngineAnalysisEnumBound::Lower,
        ),
        (
            "info score mate 3",
            EngineAnalysisEnumScore::Mate(3),
            -3,
            EngineAnalysisEnumBound::Exact,
            EngineAnalysisEnumBound::Exact,
        ),
        (
            "info score mate -4 upperbound",
            EngineAnalysisEnumScore::Mate(-4),
            4,
            EngineAnalysisEnumBound::Upper,
            EngineAnalysisEnumBound::Lower,
        ),
        (
            "info score cp -2147483648",
            EngineAnalysisEnumScore::Centipawns(i32::MIN),
            2_147_483_648,
            EngineAnalysisEnumBound::Exact,
            EngineAnalysisEnumBound::Exact,
        ),
    ] {
        let analysis = EngineAnalysis::parse(text, &root)?;
        let score = analysis
            .line()
            .and_then(|line| line.score())
            .ok_or("missing score")?;
        assert_eq!(score.reported(), reported);
        assert_eq!(score.perspective(), BoardEnumColor::Black);
        assert_eq!(score.bound(), bound);
        assert_eq!(score.white_value(), expected);
        assert_eq!(score.white_bound(), white_bound);
    }
    Ok(())
}

#[test]
fn malformed_statistics_duplicates_and_contradictory_bounds_are_rejected() {
    let root = BoardPosition::standard();
    for text in [
        "info depth",
        "info depth -1",
        "info depth one",
        "info depth 1 depth 2",
        "info seldepth 4294967296",
        "info multipv 0",
        "info multipv 21",
        "info score",
        "info score cp",
        "info score cp 2147483648",
        "info score wdl 5",
        "info score cp 1 score mate 2",
        "info lowerbound",
        "info score cp 1 lowerbound upperbound",
        "info score mate 1 upperbound upperbound",
        "info nodes -1",
        "info nodes 18446744073709551616",
        "info time nan",
        "info nps inf",
        "info hashfull 1001",
        "info hashfull 0 hashfull 1",
        "info tbhits -1",
        "info depth 1 unsupported 1",
        "info pv",
    ] {
        assert!(
            EngineAnalysis::parse(text, &root).is_err(),
            "accepted {text}"
        );
    }
}

#[test]
fn illegal_pv_at_any_ply_and_terminal_continuations_are_rejected() -> EngineAnalysisTestResult {
    let root = BoardPosition::standard();
    for text in [
        "info pv e2e5",
        "info pv e2e4 e2e3",
        "info pv e2e4 e7e5 g1g4",
        "info pv e2e4 0000",
        "info pv e2e4 pv e7e5",
    ] {
        assert!(
            EngineAnalysis::parse(text, &root).is_err(),
            "accepted {text}"
        );
    }
    let mate =
        BoardPosition::from_fen("7k/8/5KQ1/8/8/8/8/8 w - - 0 1", BoardEnumVariant::Standard)?;
    assert!(EngineAnalysis::parse("info score mate 1 pv g6g7", &mate).is_ok());
    assert!(EngineAnalysis::parse("info score mate 1 pv g6g7 h8g8", &mate).is_err());
    Ok(())
}

#[test]
fn chess960_pv_uses_authoritative_rook_square_castling() -> EngineAnalysisTestResult {
    let root = BoardPosition::from_fen(
        "4k3/8/8/8/8/8/8/2K2R2 w F - 0 1",
        BoardEnumVariant::Chess960,
    )?;
    let analysis = EngineAnalysis::parse("info multipv 1 pv c1f1", &root)?;
    let movement = analysis
        .line()
        .and_then(|line| line.variation())
        .and_then(|moves| moves.first())
        .ok_or("missing castle")?;
    assert_eq!(*movement, root.parse_uci("c1f1")?);
    assert!(EngineAnalysis::parse("info pv c1g1", &root).is_err());
    Ok(())
}

#[test]
fn diagnostics_remain_literal_and_single_line_bounds_apply() -> EngineAnalysisTestResult {
    let root = BoardPosition::standard();
    for text in [
        "info string depth broken pv e2e5 <script>",
        "info vendor-status not structured",
    ] {
        let analysis = EngineAnalysis::parse(text, &root)?;
        assert!(analysis.line().is_none());
        assert_eq!(analysis.into_text(), text);
    }
    for text in [
        "",
        "info",
        "info ",
        "bestmove e2e4",
        "info string ok\ninfo nodes 1",
        "info nodes 1\r",
    ] {
        assert!(EngineAnalysis::parse(text, &root).is_err());
    }
    let oversized = format!(
        "info string {}",
        "x".repeat(EngineProcessLimits::LINE_BYTES)
    );
    assert!(EngineAnalysis::parse(&oversized, &root).is_err());
    Ok(())
}

#[test]
fn variation_work_is_bounded_by_the_pinned_engine_output_capacity() -> EngineAnalysisTestResult {
    let root = BoardPosition::standard();
    let mut text = "info pv".to_owned();
    for movement in ["g1f3", "g8f6", "f3g1", "f6g8"]
        .into_iter()
        .cycle()
        .take(EngineAnalysisLine::MAX_VARIATION_PLIES)
    {
        text.push(' ');
        text.push_str(movement);
    }
    assert_eq!(
        EngineAnalysis::parse(&text, &root)?
            .line()
            .and_then(|line| line.variation())
            .map(<[chess::board::r#move::BoardMove]>::len),
        Some(EngineAnalysisLine::MAX_VARIATION_PLIES),
    );
    text.push_str(" f3g1");
    assert!(EngineAnalysis::parse(&text, &root).is_err());
    Ok(())
}

#[test]
fn static_evaluation_is_a_separate_white_perspective_integer() -> EngineAnalysisTestResult {
    for value in [i32::MIN, -123, 0, 456, i32::MAX] {
        assert_eq!(
            EngineAnalysisStaticEvaluation::parse(&value.to_string())?.white_value(),
            value
        );
    }
    for text in ["", "score cp 5", "NNUE 4", "2147483648", "1\n2", "1.5"] {
        assert!(EngineAnalysisStaticEvaluation::parse(text).is_err());
    }
    Ok(())
}
