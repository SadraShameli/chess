use std::num::NonZeroUsize;

use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockDuration;
use chess::engine::analysis::persistence::EngineAnalysisLineRecord;
use chess::engine::options::persistence::EngineOptionsAppliedRecord;
use chess::engine::options::{
    EngineOptionsEnumTablebases, EngineOptionsEnumValue, EngineOptionsSetting,
    EngineOptionsSyzygyPath,
};
use chess::engine::process_provider::{EngineProcessEnumPipe, EngineProcessLimits};
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::identity::persistence::EngineSearchTagRecord;
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::session::persistence::{
    EngineSearchSessionFailureEnumCategory, EngineSearchSessionFailureEnumProcess,
    EngineSearchSessionFailureRecord,
};
use chess::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumCompletion, EngineSearchSessionEnumEvent,
    EngineSearchSessionRequest,
};
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_lifecycle::EngineLifecycleProvider;
use crate::engine_package_selection::EnginePackageFixture;
use crate::engine_report_persistence_codecs::fixture::{
    EngineReportPersistenceFailureProvider, EngineReportPersistenceRuntimeFixture,
};
use crate::engine_report_persistence_codecs::{
    EngineReportPersistenceFixture, EngineReportPersistenceTestResult,
};

#[tokio::test(start_paused = true)]
async fn applied_options_preserve_acknowledged_states_after_the_package_disappears()
-> EngineReportPersistenceTestResult {
    let package = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    provider.advertise_threads_maximum(2048);
    let mut engine = provider.start(&package).await?;
    let identity = engine.package().identity();
    let capabilities = engine.identity().capabilities().clone();
    let mut captures = vec![engine.applied_options().ok_or("missing options")?.clone()];
    for line in [
        "info string set Threads to 1024",
        "info string set Hash to 32 MB",
        "info string set MoveOverhead to 17 ms",
    ] {
        provider.emit(line).await?;
    }
    engine
        .configure_resources(EngineResourceConfig::new(
            1024,
            32,
            ClockDuration::from_millis(17)?,
        )?)
        .await?;
    for command in [
        "setoption name Threads value 1024",
        "setoption name Hash value 32",
        "setoption name MoveOverhead value 17",
        "isready",
    ] {
        provider.expect(command).await?;
    }
    for (value, acknowledgment, command) in [
        (
            EngineOptionsEnumValue::Minimal(true),
            None,
            "setoption name Minimal value true",
        ),
        (
            EngineOptionsEnumValue::Chess960(BoardEnumVariant::Chess960),
            None,
            "setoption name UCI_Chess960 value true",
        ),
        (
            EngineOptionsEnumValue::MultiPv(7),
            Some("info string set MultiPV to 7"),
            "setoption name MultiPV value 7",
        ),
    ] {
        if let Some(text) = acknowledgment {
            provider.emit(text).await?;
        }
        engine
            .configure_option(EngineOptionsSetting::new(value, &capabilities)?)
            .await?;
        provider.expect(command).await?;
        provider.expect("isready").await?;
    }
    for response in [
        "info string Loaded Syzygy tablebases with 7 pieces",
        "Failed to load Syzygy tablebases",
    ] {
        provider
            .emit_pipe(
                if response.starts_with("info") {
                    EngineProcessEnumPipe::Stdout
                } else {
                    EngineProcessEnumPipe::Stderr
                },
                response,
            )
            .await?;
        engine
            .configure_option(EngineOptionsSetting::new(
                EngineOptionsEnumValue::SyzygyPath(EngineOptionsSyzygyPath::new(
                    "/historical/missing/tables".to_owned(),
                )?),
                &capabilities,
            )?)
            .await?;
        provider
            .expect("setoption name SyzygyPath value /historical/missing/tables")
            .await?;
        provider.expect("isready").await?;
        captures.push(engine.applied_options().ok_or("missing options")?.clone());
    }
    let records = captures
        .iter()
        .map(EngineOptionsAppliedRecord::capture)
        .collect::<Result<Vec<_>, _>>()?;
    engine
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    let shutdown = engine.is_shutdown_complete();
    drop(package);
    assert!(shutdown);
    assert_eq!(
        EngineReportPersistenceFixture::roundtrip(&identity)?,
        identity
    );
    for (expected, record) in captures.iter().zip(records.iter()) {
        assert_eq!(
            EngineReportPersistenceFixture::roundtrip(record)?.restore(&capabilities)?,
            *expected
        );
    }
    assert!(matches!(
        captures[0].tablebases(),
        EngineOptionsEnumTablebases::Unconfigured
    ));
    assert!(
        matches!(captures[1].tablebases(), EngineOptionsEnumTablebases::Loaded { pieces, .. } if pieces.get() == 7)
    );
    assert!(matches!(
        captures[2].tablebases(),
        EngineOptionsEnumTablebases::Unavailable { .. }
    ));
    assert!(
        records[1]
            .restore(&EngineReportPersistenceFixture::capabilities(512)?)
            .is_err()
    );
    let json = serde_json::to_string(&records[1])?;
    assert!(
        EngineOptionsAppliedRecord::parse(
            format!(
                "{}{}",
                " ".repeat(EngineOptionsAppliedRecord::MAX_BYTES),
                json
            )
            .as_bytes()
        )
        .is_err()
    );
    for malformed in [
        json.replace("\"version\":1", "\"version\":2"),
        json.replace("\"pieces\":7", "\"pieces\":0"),
        json.replace("\"threads\":1024", "\"threads\":0"),
        json.replace("\"multi_pv\":7", "\"multi_pv\":257"),
        json.replace(
            "\"move_overhead_millis\":17",
            "\"move_overhead_millis\":17.5",
        ),
        json.replace("/historical/missing/tables", "bad path"),
        json.replace("\"data\":{", "\"data\":{\"unknown\":true,"),
    ] {
        assert!(serde_json::from_str::<EngineOptionsAppliedRecord>(&malformed).is_err());
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn actual_search_tags_and_independent_terminal_lines_survive_archival_roundtrips()
-> EngineReportPersistenceTestResult {
    let package = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut engine = provider.start(&package).await?;
    EngineReportPersistenceRuntimeFixture::configure(&mut engine, &mut provider, 1).await?;
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("empty budget")?)?;
    let mut session = EngineSearchSession::new(engine);
    let root = BoardPosition::standard();
    let context = EngineSearchContext::new(
        EngineSearchGameId::try_from(Uuid::from_u128(41))?,
        EngineSearchRevision::new(u64::MAX),
    );
    let tag = session
        .start(EngineSearchSessionRequest::new(
            context,
            EngineSearchPosition::new(root.clone(), std::iter::empty::<&str>())?,
            EngineSearchLimit::depth(4)?,
            budget.try_lease(1)?,
        ))
        .await?;
    provider.expect("position startpos").await?;
    provider.expect("go depth 4").await?;
    provider.emit("info depth 3 score cp 30 pv e2e4").await?;
    provider.emit("info score cp 35 lowerbound").await?;
    provider.emit("info nodes 900 time 1").await?;
    provider.emit("bestmove e2e4").await?;
    let terminal = loop {
        if let EngineSearchSessionEnumEvent::Finished(terminal) = session.next_event(tag).await? {
            break terminal;
        }
    };
    let primary = terminal
        .snapshot()
        .primary_evaluation()
        .ok_or("missing primary")?;
    let latest = terminal
        .snapshot()
        .latest_update()
        .ok_or("missing latest")?;
    let candidate = terminal
        .snapshot()
        .lines()
        .first()
        .ok_or("missing candidate")?;
    let records = [
        EngineAnalysisLineRecord::capture(primary, &root)?,
        EngineAnalysisLineRecord::capture(latest, &root)?,
        EngineAnalysisLineRecord::capture(candidate, &root)?,
    ];
    let tag_record = EngineSearchTagRecord::capture(tag);
    let completion_observed = session
        .next_event(tag)
        .await
        .err()
        .ok_or("expected observed completion")?;
    let completion_record = EngineSearchSessionFailureRecord::capture(&completion_observed)?;
    session
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    assert_eq!(budget.available(), 1);
    assert!(matches!(
        terminal.completion(),
        EngineSearchSessionEnumCompletion::BestMove(_)
    ));
    assert_eq!(
        EngineReportPersistenceFixture::roundtrip(&tag_record)?.restore()?,
        tag
    );
    assert_eq!(tag.context(), context);
    assert_eq!(
        completion_record.category(),
        EngineSearchSessionFailureEnumCategory::CompletionObserved
    );
    assert_eq!(records[0].restore(&root)?, **primary);
    assert_eq!(records[1].restore(&root)?, *latest);
    assert_eq!(records[2].restore(&root)?, **candidate);
    assert!(records[0].restore(&root)?.variation().is_none());
    assert!(records[1].restore(&root)?.score().is_none());
    assert!(records[2].restore(&root)?.nodes().is_none());
    let json = serde_json::to_string(&tag_record)?;
    for actual in [
        tag.id().value(),
        tag.generation().value(),
        tag.context().game().value(),
    ] {
        assert!(
            serde_json::from_str::<EngineSearchTagRecord>(
                &json.replace(&actual.to_string(), &Uuid::nil().to_string())
            )
            .is_err()
        );
    }
    assert!(
        serde_json::from_str::<EngineSearchTagRecord>(
            &json.replace("\"version\":1", "\"version\":2")
        )
        .is_err()
    );
    assert!(
        EngineSearchTagRecord::parse(
            format!("{}{}", " ".repeat(EngineSearchTagRecord::MAX_BYTES), json).as_bytes()
        )
        .is_err()
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn actual_nested_io_and_cleanup_failures_keep_portable_context_and_the_live_arc()
-> EngineReportPersistenceTestResult {
    for cleanup_failure in [false, true] {
        let package = EnginePackageFixture::new()?;
        let mut provider = EngineReportPersistenceFailureProvider::new();
        let mut engine = provider.start(&package).await?;
        EngineReportPersistenceRuntimeFixture::configure(&mut engine, &mut provider.lifecycle, 1)
            .await?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("empty budget")?)?;
        let mut session = EngineSearchSession::new(engine);
        let context = EngineSearchContext::new(
            EngineSearchGameId::try_from(Uuid::from_u128(1))?,
            EngineSearchRevision::new(1),
        );
        let tag = session
            .start(EngineSearchSessionRequest::new(
                context,
                EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
                EngineSearchLimit::infinite(),
                budget.try_lease(1)?,
            ))
            .await?;
        provider.lifecycle.expect("position startpos").await?;
        provider.lifecycle.expect("go infinite").await?;
        if cleanup_failure {
            provider.fail_cleanup();
            provider
                .lifecycle
                .emit("info multipv 2 score cp 10 pv e2e4")
                .await?;
        } else {
            provider.fail_receive();
        }
        let terminal = loop {
            if let EngineSearchSessionEnumEvent::Finished(terminal) =
                session.next_event(tag).await?
            {
                break terminal;
            }
        };
        let EngineSearchSessionEnumCompletion::Failed(failure) = terminal.completion() else {
            return Err("expected actual failed terminal".into());
        };
        let captured = EngineSearchSessionFailureRecord::capture(failure)?;
        let restored = EngineReportPersistenceFixture::roundtrip(&captured)?;
        assert!(session.engine().is_shutdown_complete());
        assert_eq!(budget.available(), 1);
        assert_eq!(restored, captured);
        assert_eq!(restored.message(), failure.to_string());
        assert!(restored.sources().next().is_some());
        let retained = session.terminal().ok_or("missing live terminal")?;
        let EngineSearchSessionEnumCompletion::Failed(retained_failure) = retained.completion()
        else {
            return Err("missing live failure".into());
        };
        assert!(std::sync::Arc::ptr_eq(failure, retained_failure));
        if cleanup_failure {
            assert_eq!(
                restored.category(),
                EngineSearchSessionFailureEnumCategory::Cleanup
            );
            assert_eq!(
                restored.operation_category(),
                Some(EngineSearchSessionFailureEnumCategory::CandidateRank)
            );
            assert_eq!(
                restored
                    .cleanup_context()
                    .ok_or("missing cleanup context")?
                    .process(),
                Some(EngineSearchSessionFailureEnumProcess::Io {
                    raw_os_error: Some(5)
                })
            );
            assert!(restored.cleanup_message().is_some());
            assert!(restored.cleanup_sources().next().is_some());
        } else {
            assert_eq!(
                restored.category(),
                EngineSearchSessionFailureEnumCategory::Engine
            );
            assert_eq!(
                restored
                    .engine_context()
                    .ok_or("missing engine context")?
                    .process(),
                Some(EngineSearchSessionFailureEnumProcess::Io {
                    raw_os_error: Some(5)
                })
            );
        }
        let json = serde_json::to_string(&captured)?;
        let too_many_sources = std::iter::repeat_n(
            "\"cause\"",
            EngineSearchSessionFailureRecord::MAX_SOURCES + 1,
        )
        .collect::<Vec<_>>()
        .join(",");
        let sources_start =
            json.find("\"sources\":[").ok_or("missing source array")? + "\"sources\":[".len();
        let sources_end = json[sources_start..]
            .find(']')
            .ok_or("missing source terminator")?
            + sources_start;
        let oversized = format!(
            "{}{}{}",
            &json[..sources_start],
            too_many_sources,
            &json[sources_end..]
        );
        assert!(serde_json::from_str::<EngineSearchSessionFailureRecord>(&oversized).is_err());
        assert!(
            EngineSearchSessionFailureRecord::parse(
                format!(
                    "{}{}",
                    " ".repeat(EngineSearchSessionFailureRecord::MAX_BYTES),
                    json
                )
                .as_bytes()
            )
            .is_err()
        );
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn thread_lease_failures_capture_the_actual_requested_allocation()
-> EngineReportPersistenceTestResult {
    let package = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut engine = provider.start(&package).await?;
    EngineReportPersistenceRuntimeFixture::configure(&mut engine, &mut provider, 2).await?;
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("empty budget")?)?;
    let mut session = EngineSearchSession::new(engine);
    let request = EngineSearchSessionRequest::new(
        EngineSearchContext::new(
            EngineSearchGameId::try_from(Uuid::from_u128(1))?,
            EngineSearchRevision::new(1),
        ),
        EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
        EngineSearchLimit::depth(1)?,
        budget.try_lease(1)?,
    );
    let failure = session
        .start(request)
        .await
        .err()
        .ok_or("expected thread lease mismatch")?;
    let record = EngineSearchSessionFailureRecord::capture(&failure)?;
    session
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    let restored = EngineReportPersistenceFixture::roundtrip(&record)?;
    assert_eq!(
        restored.category(),
        EngineSearchSessionFailureEnumCategory::ThreadLeaseMismatch
    );
    assert_eq!(restored.thread_lease(), Some((2, 1)));
    assert_eq!(budget.available(), 1);
    let json = serde_json::to_string(&record)?;
    for malformed in [
        json.replace("\"workers\":1", "\"workers\":2"),
        json.replace("\"workers\":1", "\"workers\":0"),
        json.replace("\"threads\":2", "\"threads\":0"),
    ] {
        assert!(serde_json::from_str::<EngineSearchSessionFailureRecord>(&malformed).is_err());
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn portable_failures_capture_actual_runtime_errors_without_recreating_live_ownership()
-> EngineReportPersistenceTestResult {
    let package = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let engine = provider.start(&package).await?;
    let mut session = EngineSearchSession::new(engine);
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("empty budget")?)?;
    let request = EngineSearchSessionRequest::new(
        EngineSearchContext::new(
            EngineSearchGameId::try_from(Uuid::from_u128(1))?,
            EngineSearchRevision::new(1),
        ),
        EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
        EngineSearchLimit::infinite(),
        budget.try_lease(1)?,
    );
    let failure = session
        .start(request)
        .await
        .err()
        .ok_or("expected resource failure")?;
    let captured = EngineSearchSessionFailureRecord::capture(&failure)?;
    session
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    let restored = EngineReportPersistenceFixture::roundtrip(&captured)?;
    assert_eq!(restored, captured);
    assert_eq!(
        restored.category(),
        EngineSearchSessionFailureEnumCategory::ResourcesRequired
    );
    assert_eq!(restored.message(), failure.to_string());
    assert_eq!(budget.available(), 1);
    let json = serde_json::to_string(&captured)?;
    for malformed in [
        json.replace("\"version\":1", "\"version\":2"),
        json.replace("resources_required", "future_failure"),
        json.replace(restored.message(), ""),
        json.replace("\"data\":{", "\"data\":{\"unknown\":true,"),
        json.replace(
            restored.message(),
            &"x".repeat(EngineSearchSessionFailureRecord::MAX_MESSAGE_BYTES + 1),
        ),
    ] {
        assert!(serde_json::from_str::<EngineSearchSessionFailureRecord>(&malformed).is_err());
    }
    Ok(())
}
