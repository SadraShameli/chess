use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::engine::analysis::persistence::EngineAnalysisLineRecord;
use chess::engine::analysis::{EngineAnalysis, EngineAnalysisLine};

use crate::engine_report_persistence_codecs::{
    EngineReportPersistenceFixture, EngineReportPersistenceTestResult,
};

#[test]
fn complete_and_sparse_lines_roundtrip_without_merging_statistics()
-> EngineReportPersistenceTestResult {
    let white = BoardPosition::standard();
    let black = white.play_uci("e2e4")?;
    for (position, text) in [
        (
            &white,
            "info depth 18 seldepth 31 multipv 2 score cp 43 lowerbound nodes 9007199254740993 time 789 nps 12345678 hashfull 321 tbhits 0 pv e2e4 e7e5 g1f3",
        ),
        (&white, "info score mate -2 upperbound"),
        (&white, "info nodes 0 time 0 nps 0 hashfull 0 tbhits 0"),
        (&white, "info pv e2e4"),
        (&black, "info score cp -2147483648 upperbound pv e7e5"),
        (&black, "info multipv 1 score mate 3 lowerbound"),
    ] {
        let analysis = EngineAnalysis::parse(text, position)?;
        let line = analysis.line().ok_or("missing parsed line")?;
        let record = EngineAnalysisLineRecord::capture(line, position)?;
        assert_eq!(
            EngineReportPersistenceFixture::roundtrip(&record)?.restore(position)?,
            *line
        );
    }
    let empty = EngineAnalysisLine::default();
    let record = EngineAnalysisLineRecord::capture(&empty, &white)?;
    assert_eq!(
        EngineReportPersistenceFixture::roundtrip(&record)?.restore(&white)?,
        empty
    );
    Ok(())
}

#[test]
fn legal_promotions_and_both_castling_encodings_roundtrip() -> EngineReportPersistenceTestResult {
    for (fen, variant, pv) in [
        (
            "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            BoardEnumVariant::Standard,
            "a7a8q",
        ),
        (
            "4k3/8/8/8/8/8/8/4K2R w K - 0 1",
            BoardEnumVariant::Standard,
            "e1g1",
        ),
        (
            "4k3/8/8/8/8/8/8/2K2R2 w F - 0 1",
            BoardEnumVariant::Chess960,
            "c1f1",
        ),
    ] {
        let root = BoardPosition::from_fen(fen, variant)?;
        let analysis = EngineAnalysis::parse(&format!("info pv {pv}"), &root)?;
        let line = analysis.line().ok_or("missing variation")?;
        assert_eq!(
            EngineReportPersistenceFixture::roundtrip(&EngineAnalysisLineRecord::capture(
                line, &root
            )?)?
            .restore(&root)?,
            *line
        );
    }
    Ok(())
}

#[test]
fn malformed_and_position_incompatible_records_fail_at_the_canonical_boundary()
-> EngineReportPersistenceTestResult {
    let root = BoardPosition::standard();
    let analysis =
        EngineAnalysis::parse("info multipv 1 score cp 2 time 1 hashfull 1 pv e2e4", &root)?;
    let line = analysis.line().ok_or("missing line")?;
    let record = EngineAnalysisLineRecord::capture(line, &root)?;
    let json = serde_json::to_string(&record)?;
    assert!(
        EngineAnalysisLineRecord::parse(
            format!(
                "{}{}",
                " ".repeat(EngineAnalysisLineRecord::MAX_BYTES),
                json
            )
            .as_bytes()
        )
        .is_err()
    );
    for malformed in [
        json.replace("\"version\":1", "\"version\":2"),
        json.replace("\"rank\":1", "\"rank\":0"),
        json.replace("\"elapsed_millis\":1", "\"elapsed_millis\":1.5"),
        json.replace("\"hash_occupancy\":1", "\"hash_occupancy\":1001"),
        json.replace("\"data\":{", "\"data\":{\"unexpected\":1,"),
    ] {
        assert!(serde_json::from_str::<EngineAnalysisLineRecord>(&malformed).is_err());
    }
    for incompatible in [
        json.replace("e2e4", "e2e5"),
        json.replace("\"rank\":1", "\"rank\":21"),
        json.replace("\"perspective\":\"white\"", "\"perspective\":\"black\""),
    ] {
        let restored = serde_json::from_str::<EngineAnalysisLineRecord>(&incompatible)?;
        assert!(restored.restore(&root).is_err());
    }
    let black = root.play_uci("e2e4")?;
    assert!(record.restore(&black).is_err());
    assert!(EngineAnalysisLineRecord::capture(line, &black).is_err());
    let oversized = json.replace(
        "\"e2e4\"",
        &std::iter::repeat_n("\"e2e4\"", EngineAnalysisLine::MAX_VARIATION_PLIES + 1)
            .collect::<Vec<_>>()
            .join(","),
    );
    assert!(serde_json::from_str::<EngineAnalysisLineRecord>(&oversized).is_err());
    Ok(())
}

#[test]
fn each_optional_statistic_and_the_full_legal_pv_bound_roundtrip()
-> EngineReportPersistenceTestResult {
    let root = BoardPosition::standard();
    for text in [
        "info depth 0",
        "info seldepth 4294967295",
        "info multipv 20",
        "info nodes 18446744073709551615",
        "info time 18446744073709551615",
        "info nps 18446744073709551615",
        "info hashfull 1000",
        "info tbhits 18446744073709551615",
    ] {
        let parsed = EngineAnalysis::parse(text, &root)?;
        let line = parsed.line().ok_or("missing statistics")?;
        assert_eq!(
            EngineReportPersistenceFixture::roundtrip(&EngineAnalysisLineRecord::capture(
                line, &root
            )?)?
            .restore(&root)?,
            *line
        );
    }
    let mut text = "info pv".to_owned();
    for movement in ["g1f3", "g8f6", "f3g1", "f6g8"]
        .into_iter()
        .cycle()
        .take(EngineAnalysisLine::MAX_VARIATION_PLIES)
    {
        text.push(' ');
        text.push_str(movement);
    }
    let parsed = EngineAnalysis::parse(&text, &root)?;
    let line = parsed.line().ok_or("missing full variation")?;
    assert_eq!(
        EngineReportPersistenceFixture::roundtrip(&EngineAnalysisLineRecord::capture(
            line, &root
        )?)?
        .restore(&root)?,
        *line
    );
    Ok(())
}
