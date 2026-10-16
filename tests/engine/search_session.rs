use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::task::Poll;
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockDuration;
use chess::engine::analysis::EngineAnalysisEnumScore;
use chess::engine::options::{EngineOptionsEnumValue, EngineOptionsSetting};
use chess::engine::process_provider::EngineProcessLimits;
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::enums::EngineSearchEnumState;
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumCompletion, EngineSearchSessionEnumEvent,
    EngineSearchSessionError, EngineSearchSessionRequest,
};
use chess::engine::search::stop::EngineSearchStop;
use chess::engine::session::EngineSessionError;
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_lifecycle::{EngineLifecycleProcess, EngineLifecycleProvider};
use crate::engine_package_selection::EnginePackageFixture;

type EngineSearchSessionTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct EngineSearchSessionFixture;

impl EngineSearchSessionFixture {
    fn budget(workers: usize) -> EngineSearchSessionTestResult<CapacityBudget> {
        Ok(CapacityBudget::new(
            NonZeroUsize::new(workers).ok_or("empty fixture CPU budget")?,
        )?)
    }

    fn position() -> EngineSearchSessionTestResult<EngineSearchPosition> {
        Ok(EngineSearchPosition::new(
            BoardPosition::standard(),
            std::iter::empty::<&str>(),
        )?)
    }

    fn request(
        budget: &CapacityBudget,
        revision: u64,
        position: EngineSearchPosition,
    ) -> EngineSearchSessionTestResult<EngineSearchSessionRequest> {
        Ok(EngineSearchSessionRequest::new(
            EngineSearchContext::new(
                EngineSearchGameId::try_from(Uuid::from_u128(1))?,
                EngineSearchRevision::new(revision),
            ),
            position,
            EngineSearchLimit::infinite(),
            budget.try_lease(2)?,
        ))
    }

    async fn session(
        provider: &mut EngineLifecycleProvider,
        package: &EnginePackageFixture,
    ) -> EngineSearchSessionTestResult<EngineSearchSession<EngineLifecycleProcess>> {
        let mut engine = provider.start(package).await?;
        for line in [
            "info string set Threads to 2",
            "info string set Hash to 32 MB",
            "info string set MoveOverhead to 0 ms",
        ] {
            provider.emit(line).await?;
        }
        engine
            .configure_resources(EngineResourceConfig::new(2, 32, ClockDuration::ZERO)?)
            .await?;
        for command in [
            "setoption name Threads value 2",
            "setoption name Hash value 32",
            "setoption name MoveOverhead value 0",
            "isready",
        ] {
            provider.expect(command).await?;
        }
        let setting = EngineOptionsSetting::new(
            EngineOptionsEnumValue::MultiPv(2),
            engine.identity().capabilities(),
        )?;
        provider.emit("info string set MultiPV to 2").await?;
        engine.configure_option(setting).await?;
        provider.expect("setoption name MultiPV value 2").await?;
        provider.expect("isready").await?;
        Ok(EngineSearchSession::new(engine))
    }

    async fn start(
        session: &mut EngineSearchSession<EngineLifecycleProcess>,
        provider: &mut EngineLifecycleProvider,
        budget: &CapacityBudget,
        revision: u64,
    ) -> EngineSearchSessionTestResult<chess::engine::search::identity::EngineSearchTag> {
        let tag = session
            .start(Self::request(budget, revision, Self::position()?)?)
            .await?;
        provider.expect("position startpos").await?;
        provider.expect("go infinite").await?;
        Ok(tag)
    }

    async fn close(
        session: &mut EngineSearchSession<EngineLifecycleProcess>,
    ) -> EngineSearchSessionTestResult {
        session
            .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await?;
        assert!(session.engine().is_shutdown_complete());
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn metadata_keeps_authoritative_history_shared_and_rejects_thread_lease_mismatch_before_io()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let position = EngineSearchPosition::new(BoardPosition::standard(), ["e2e4", "e7e5"])?;
    let original = position.clone();
    let request = EngineSearchSessionRequest::new(
        EngineSearchContext::new(
            EngineSearchGameId::try_from(Uuid::from_u128(1))?,
            EngineSearchRevision::new(4),
        ),
        position.clone(),
        EngineSearchLimit::depth(6)?,
        budget.try_lease(1)?,
    );
    assert!(matches!(
        session.start(request).await,
        Err(EngineSearchSessionError::ThreadLeaseMismatch { .. })
    ));
    assert_eq!(budget.available(), 2);
    provider.assert_no_command();
    let started = Instant::now();
    let tag = session
        .start(EngineSearchSessionFixture::request(&budget, 4, position)?)
        .await?;
    provider.expect("position startpos moves e2e4 e7e5").await?;
    provider.expect("go infinite").await?;
    let metadata = session.metadata().ok_or("missing search metadata")?;
    assert_eq!(metadata.tag(), tag);
    assert_eq!(metadata.started_at(), started);
    assert_eq!(metadata.position().initial(), &BoardPosition::standard());
    assert_eq!(metadata.position().history().len(), 2);
    assert_eq!(
        metadata.position().command().as_ptr(),
        original.command().as_ptr()
    );
    assert_eq!(
        metadata.position().history().as_ptr(),
        original.history().as_ptr()
    );
    assert_eq!(metadata.limit(), EngineSearchLimit::infinite());
    assert_eq!(budget.available(), 0);
    provider.emit("bestmove g1f3").await?;
    assert!(matches!(
        session.next_event(tag).await?,
        EngineSearchSessionEnumEvent::Finished(_)
    ));
    assert_eq!(budget.available(), 2);
    assert_eq!(session.engine().state(), EngineSearchEnumState::Idle);
    assert!(!session.engine().is_shutdown_complete());
    EngineSearchSessionFixture::close(&mut session).await
}

#[tokio::test(start_paused = true)]
async fn coalescing_orders_whole_ranked_lines_without_merging_optional_statistics()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    provider
        .emit("info depth 8 multipv 2 score cp -9 nodes 99 pv d2d4")
        .await?;
    provider
        .emit("info depth 9 multipv 1 score cp 40 lowerbound pv e2e4")
        .await?;
    provider.emit("info nodes 150 nps 1234").await?;
    let published = {
        let event = session.next_event(tag);
        tokio::pin!(event);
        assert!(
            poll_fn(|context| Poll::Ready(event.as_mut().poll(context)))
                .await
                .is_pending()
        );
        tokio::time::advance(Duration::from_millis(49)).await;
        assert!(
            poll_fn(|context| Poll::Ready(event.as_mut().poll(context)))
                .await
                .is_pending()
        );
        tokio::time::advance(Duration::from_millis(1)).await;
        event.await?
    };
    let EngineSearchSessionEnumEvent::Snapshot(snapshot) = published else {
        return Err("expected coalesced snapshot".into());
    };
    assert_eq!(snapshot.metadata().tag(), tag);
    assert_eq!(snapshot.lines().len(), 2);
    assert_eq!(
        snapshot.lines()[0]
            .rank()
            .ok_or("missing first rank")?
            .get(),
        1
    );
    assert_eq!(
        snapshot.lines()[1]
            .rank()
            .ok_or("missing second rank")?
            .get(),
        2
    );
    assert_eq!(
        snapshot.lines()[0]
            .score()
            .ok_or("missing score")?
            .reported(),
        EngineAnalysisEnumScore::Centipawns(40)
    );
    assert_eq!(snapshot.lines()[0].nodes(), None);
    assert_eq!(snapshot.lines()[1].nodes(), Some(99));
    assert_eq!(
        snapshot
            .latest_update()
            .ok_or("missing statistics")?
            .nodes(),
        Some(150)
    );
    assert_eq!(
        snapshot
            .latest_update()
            .ok_or("missing statistics")?
            .variation(),
        None
    );
    provider.emit("bestmove e2e4").await?;
    let terminal_at = Instant::now();
    let EngineSearchSessionEnumEvent::Finished(terminal) = session.next_event(tag).await? else {
        return Err("expected completion".into());
    };
    assert!(matches!(
        terminal.completion(),
        EngineSearchSessionEnumCompletion::BestMove(_)
    ));
    assert_eq!(Instant::now(), terminal_at);
    assert_eq!(terminal.snapshot().lines().len(), 2);
    assert_eq!(budget.available(), 2);
    EngineSearchSessionFixture::close(&mut session).await
}

#[tokio::test(start_paused = true)]
async fn large_bursts_keep_newest_legal_candidates_and_do_not_create_extra_terminal_telemetry()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    let started = Instant::now();
    {
        let event = session.next_event(tag);
        tokio::pin!(event);
        for value in 0..512 {
            provider
                .emit(&format!("info depth 8 multipv 1 score cp {value} pv e2e4"))
                .await?;
            assert!(
                poll_fn(|context| Poll::Ready(event.as_mut().poll(context)))
                    .await
                    .is_pending()
            );
        }
        provider.emit("bestmove e2e4").await?;
        let EngineSearchSessionEnumEvent::Finished(terminal) = event.await? else {
            return Err("completion was delayed behind telemetry".into());
        };
        assert_eq!(
            terminal.snapshot().lines()[0]
                .score()
                .ok_or("missing final score")?
                .reported(),
            EngineAnalysisEnumScore::Centipawns(511)
        );
        assert_eq!(Instant::now(), started);
    }
    assert_eq!(budget.available(), 2);
    EngineSearchSessionFixture::close(&mut session).await
}

#[tokio::test(start_paused = true)]
async fn replacement_waits_for_stop_barrier_and_rejects_old_search_and_generation_requests()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(4)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let old = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    assert!(matches!(
        session
            .start(EngineSearchSessionFixture::request(
                &budget,
                2,
                EngineSearchSessionFixture::position()?
            )?)
            .await,
        Err(EngineSearchSessionError::Busy)
    ));
    assert_eq!(budget.available(), 2);
    let stop = EngineSearchStop::received_at(old, Instant::now())?;
    {
        let waiting = session.stop(stop);
        tokio::pin!(waiting);
        assert!(
            poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context)))
                .await
                .is_pending()
        );
        provider.expect("stop").await?;
    }
    assert_eq!(budget.available(), 2);
    provider.emit("info depth 8 score cp 5 pv e2e4").await?;
    provider.emit("bestmove e2e4").await?;
    let terminal = session.stop(stop).await?;
    assert!(matches!(
        terminal.completion(),
        EngineSearchSessionEnumCompletion::Stopped
    ));
    assert_eq!(terminal.snapshot().lines().len(), 1);
    assert_eq!(budget.available(), 4);
    let current =
        EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 2).await?;
    assert_ne!(current.id(), old.id());
    assert_eq!(current.generation(), old.generation());
    assert_eq!(current.context().revision(), EngineSearchRevision::new(2));
    assert!(matches!(
        session.next_event(old).await,
        Err(EngineSearchSessionError::StaleSearch)
    ));
    assert!(matches!(
        session.stop(stop).await,
        Err(EngineSearchSessionError::StaleSearch)
    ));
    provider.assert_no_command();
    let mut other_provider = EngineLifecycleProvider::new();
    let mut other = EngineSearchSessionFixture::session(&mut other_provider, &package).await?;
    let foreign =
        EngineSearchSessionFixture::start(&mut other, &mut other_provider, &budget, 2).await?;
    assert_ne!(foreign.generation(), current.generation());
    assert!(matches!(
        session
            .stop(EngineSearchStop::received_at(foreign, Instant::now())?)
            .await,
        Err(EngineSearchSessionError::StaleSearch)
    ));
    assert_eq!(budget.available(), 0);
    provider.emit("bestmove d2d4").await?;
    let EngineSearchSessionEnumEvent::Finished(current_result) =
        session.next_event(current).await?
    else {
        return Err("missing replacement completion".into());
    };
    assert!(current_result.snapshot().lines().is_empty());
    assert_eq!(current_result.snapshot().metadata().tag(), current);
    assert_eq!(budget.available(), 2);
    EngineSearchSessionFixture::close(&mut session).await?;
    EngineSearchSessionFixture::close(&mut other).await?;
    assert_eq!(budget.available(), 4);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn malformed_output_fails_immediately_with_original_cause_and_releases_only_observed_cleanup()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    provider.emit("info depth 8 score cp 5 pv e2e4").await?;
    provider.emit("unexpected search response").await?;
    let now = Instant::now();
    let EngineSearchSessionEnumEvent::Finished(terminal) = session.next_event(tag).await? else {
        return Err("failure was throttled".into());
    };
    assert_eq!(Instant::now(), now);
    assert!(
        matches!(terminal.completion(), EngineSearchSessionEnumCompletion::Failed(error) if matches!(error.as_ref(), EngineSearchSessionError::Engine(source) if matches!(source.operation(), EngineSessionError::SearchResponse)))
    );
    assert_eq!(budget.available(), 2);
    assert!(session.engine().is_shutdown_complete());
    assert_eq!(terminal.snapshot().lines().len(), 1);
    assert!(
        session
            .engine()
            .diagnostics()
            .entries()
            .any(|line| line.text() == "unexpected search response")
    );
    provider.expect("terminate").await?;
    assert!(
        session
            .start(EngineSearchSessionFixture::request(
                &budget,
                2,
                EngineSearchSessionFixture::position()?
            )?)
            .await
            .is_err()
    );
    provider.assert_no_command();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_start_and_retained_shutdown_keep_the_cpu_allocation_until_reaping()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    provider.set_position_blocked(true);
    {
        let starting = session.start(EngineSearchSessionFixture::request(
            &budget,
            1,
            EngineSearchSessionFixture::position()?,
        )?);
        tokio::pin!(starting);
        assert!(
            poll_fn(|context| Poll::Ready(starting.as_mut().poll(context)))
                .await
                .is_pending()
        );
        provider.expect("position startpos").await?;
    }
    assert_eq!(budget.available(), 0);
    assert_eq!(session.engine().state(), EngineSearchEnumState::Preparing);
    assert!(session.metadata().is_some());
    provider.set_cleanup_allowed(false);
    assert!(
        session
            .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await
            .is_err()
    );
    assert_eq!(budget.available(), 0);
    assert!(!session.engine().is_shutdown_complete());
    provider.expect("terminate").await?;
    provider.assert_no_command();
    provider.set_cleanup_allowed(true);
    session
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    assert_eq!(budget.available(), 2);
    assert!(session.engine().is_shutdown_complete());
    provider.expect("terminate").await?;
    provider.assert_no_command();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn interrupted_error_cleanup_preserves_its_cause_and_lease_for_explicit_retry()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    provider.set_cleanup_allowed(false);
    provider.emit("unexpected search response").await?;
    {
        let event = session.next_event(tag);
        tokio::pin!(event);
        assert!(
            poll_fn(|context| Poll::Ready(event.as_mut().poll(context)))
                .await
                .is_pending()
        );
        provider.expect("terminate").await?;
    }
    assert_eq!(budget.available(), 0);
    let EngineSearchSessionEnumEvent::Finished(terminal) = session.next_event(tag).await? else {
        return Err("missing retained failure".into());
    };
    assert!(
        matches!(terminal.completion(), EngineSearchSessionEnumCompletion::Failed(error) if matches!(error.as_ref(), EngineSearchSessionError::Engine(source) if matches!(source.operation(), EngineSessionError::SearchResponse)))
    );
    assert_eq!(budget.available(), 0);
    provider.set_cleanup_allowed(true);
    session
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    assert_eq!(budget.available(), 2);
    assert!(session.engine().is_shutdown_complete());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn stop_receipt_deadline_and_cancelled_delivery_never_release_live_capacity_early()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    provider.block_stop_write();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    let request = EngineSearchStop::received_at(tag, Instant::now())?;
    {
        let stop = session.stop(request);
        tokio::pin!(stop);
        assert!(
            poll_fn(|context| Poll::Ready(stop.as_mut().poll(context)))
                .await
                .is_pending()
        );
        provider.expect("stop").await?;
    }
    assert_eq!(budget.available(), 0);
    let terminal = session.stop(request).await?;
    assert!(
        matches!(terminal.completion(), EngineSearchSessionEnumCompletion::Failed(error) if matches!(error.as_ref(), EngineSearchSessionError::Engine(source) if matches!(source.operation(), EngineSessionError::AmbiguousStopDelivery)))
    );
    assert_eq!(budget.available(), 2);
    provider.expect("terminate").await?;
    provider.assert_no_command();
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 2).await?;
    let request = EngineSearchStop::received_at(tag, Instant::now())?;
    tokio::time::advance(Duration::from_secs(3)).await;
    let now = Instant::now();
    let terminal = session.stop(request).await?;
    assert!(
        matches!(terminal.completion(), EngineSearchSessionEnumCompletion::Failed(error) if matches!(error.as_ref(), EngineSearchSessionError::Engine(source) if matches!(source.operation(), EngineSessionError::StopDeadline)))
    );
    assert_eq!(Instant::now(), now);
    assert_eq!(budget.available(), 2);
    provider.expect("terminate").await?;
    provider.assert_no_command();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn publication_spacing_caps_visual_updates_at_twenty_per_second()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    let started = Instant::now();
    for tick in 1..=20 {
        provider
            .emit(&format!("info depth 8 score cp {tick} pv e2e4"))
            .await?;
        let event = {
            let next = session.next_event(tag);
            tokio::pin!(next);
            assert!(
                poll_fn(|context| Poll::Ready(next.as_mut().poll(context)))
                    .await
                    .is_pending()
            );
            tokio::time::advance(Duration::from_millis(49)).await;
            assert!(
                poll_fn(|context| Poll::Ready(next.as_mut().poll(context)))
                    .await
                    .is_pending()
            );
            tokio::time::advance(Duration::from_millis(1)).await;
            next.await?
        };
        let EngineSearchSessionEnumEvent::Snapshot(snapshot) = event else {
            return Err("expected cadence snapshot".into());
        };
        assert_eq!(
            Instant::now() - started,
            Duration::from_millis(u64::try_from(tick)? * 50)
        );
        assert_eq!(
            snapshot.lines()[0]
                .score()
                .ok_or("missing snapshot score")?
                .reported(),
            EngineAnalysisEnumScore::Centipawns(tick)
        );
    }
    provider.emit("bestmove e2e4").await?;
    assert!(matches!(
        session.next_event(tag).await?,
        EngineSearchSessionEnumEvent::Finished(_)
    ));
    assert_eq!(Instant::now() - started, Duration::from_secs(1));
    EngineSearchSessionFixture::close(&mut session).await
}

#[tokio::test(start_paused = true)]
async fn continuously_ready_output_does_not_starve_the_due_publication_or_other_tasks()
-> EngineSearchSessionTestResult {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    let produced = Arc::new(AtomicUsize::new(0));
    let observations = Arc::clone(&produced);
    let writer = tokio::spawn(async move {
        for index in 0..4_096 {
            provider
                .emit(&format!("info depth 8 score cp {index} pv e2e4"))
                .await
                .map_err(|error| error.to_string())?;
            observations.fetch_add(1, Ordering::SeqCst);
        }
        provider
            .emit("bestmove e2e4")
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(provider)
    });
    let timer = tokio::spawn(async {
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(Duration::from_millis(50)).await;
    });
    assert!(matches!(
        session.next_event(tag).await?,
        EngineSearchSessionEnumEvent::Snapshot(_)
    ));
    assert!(produced.load(Ordering::SeqCst) < 4_096);
    timer.await?;
    let terminal = loop {
        if let EngineSearchSessionEnumEvent::Finished(terminal) = session.next_event(tag).await? {
            break terminal;
        }
    };
    assert_eq!(
        terminal.snapshot().lines()[0]
            .score()
            .ok_or("missing latest burst score")?
            .reported(),
        EngineAnalysisEnumScore::Centipawns(4_095)
    );
    let mut provider = writer.await??;
    assert_eq!(budget.available(), 2);
    EngineSearchSessionFixture::close(&mut session).await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn black_perspective_and_readiness_remain_distinct_from_completion()
-> EngineSearchSessionTestResult {
    use chess::engine::analysis::EngineAnalysisEnumBound;

    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let position = EngineSearchPosition::new(BoardPosition::standard(), ["e2e4"])?;
    let tag = session
        .start(EngineSearchSessionFixture::request(&budget, 1, position)?)
        .await?;
    provider.expect("position startpos moves e2e4").await?;
    provider.expect("go infinite").await?;
    provider
        .emit("info depth 8 score cp 12 upperbound pv e7e5")
        .await?;
    provider.emit("readyok").await?;
    assert!(
        matches!(session.next_event(tag).await?, EngineSearchSessionEnumEvent::Readiness { tag: received } if received == tag)
    );
    assert_eq!(budget.available(), 0);
    assert!(session.terminal().is_none());
    provider.emit("bestmove e7e5").await?;
    let EngineSearchSessionEnumEvent::Finished(terminal) = session.next_event(tag).await? else {
        return Err("missing natural completion".into());
    };
    let score = terminal.snapshot().lines()[0]
        .score()
        .ok_or("missing black score")?;
    assert_eq!(score.reported(), EngineAnalysisEnumScore::Centipawns(12));
    assert_eq!(score.white_value(), -12);
    assert_eq!(score.bound(), EngineAnalysisEnumBound::Upper);
    assert_eq!(score.white_bound(), EngineAnalysisEnumBound::Lower);
    assert_eq!(budget.available(), 2);
    EngineSearchSessionFixture::close(&mut session).await
}

#[tokio::test(start_paused = true)]
async fn terminal_roots_and_unconfigured_resources_refuse_before_position_or_go()
-> EngineSearchSessionTestResult {
    use chess::board::enums::BoardEnumVariant;

    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let engine = provider.start(&package).await?;
    let mut session = EngineSearchSession::new(engine);
    assert!(matches!(
        session
            .start(EngineSearchSessionFixture::request(
                &budget,
                1,
                EngineSearchSessionFixture::position()?
            )?)
            .await,
        Err(EngineSearchSessionError::ResourcesRequired)
    ));
    provider.assert_no_command();
    assert_eq!(budget.available(), 2);
    EngineSearchSessionFixture::close(&mut session).await?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let position =
        BoardPosition::from_fen("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1", BoardEnumVariant::Standard)?;
    assert!(position.legal_moves().is_empty());
    let position = EngineSearchPosition::new(position, std::iter::empty::<&str>())?;
    assert!(
        matches!(session.start(EngineSearchSessionFixture::request(&budget, 1, position)?).await, Err(EngineSearchSessionError::Engine(source)) if matches!(source.operation(), EngineSessionError::TerminalPosition))
    );
    provider.assert_no_command();
    assert_eq!(budget.available(), 2);
    EngineSearchSessionFixture::close(&mut session).await
}

#[tokio::test(start_paused = true)]
async fn configured_rank_failure_survives_cancelled_cleanup_without_releasing_capacity()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    provider
        .emit("info depth 8 multipv 3 score cp 5 pv c2c4")
        .await?;
    provider.set_cleanup_allowed(false);
    {
        let event = session.next_event(tag);
        tokio::pin!(event);
        assert!(
            poll_fn(|context| Poll::Ready(event.as_mut().poll(context)))
                .await
                .is_pending()
        );
        provider.expect("stop").await?;
        provider.expect("shutdown").await?;
    }
    assert_eq!(budget.available(), 0);
    let EngineSearchSessionEnumEvent::Finished(terminal) = session.next_event(tag).await? else {
        return Err("missing rank failure".into());
    };
    assert!(
        matches!(terminal.completion(), EngineSearchSessionEnumCompletion::Failed(error) if matches!(error.as_ref(), EngineSearchSessionError::CandidateRank))
    );
    provider.set_cleanup_allowed(true);
    session
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    assert_eq!(budget.available(), 2);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn completed_stop_does_not_reuse_its_expired_deadline_for_later_idle_shutdown()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    provider.emit("bestmove e2e4").await?;
    let stopped = session
        .stop(EngineSearchStop::received_at(tag, Instant::now())?)
        .await?;
    assert!(matches!(
        stopped.completion(),
        EngineSearchSessionEnumCompletion::Stopped
    ));
    provider.expect("stop").await?;
    assert_eq!(budget.available(), 2);
    tokio::time::advance(Duration::from_secs(10)).await;
    provider.set_cleanup_allowed(false);
    {
        let cleanup = session.shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN);
        tokio::pin!(cleanup);
        assert!(
            poll_fn(|context| Poll::Ready(cleanup.as_mut().poll(context)))
                .await
                .is_pending()
        );
        provider.expect("shutdown").await?;
        provider.set_cleanup_allowed(true);
        cleanup.await?;
    }
    assert!(session.engine().is_shutdown_complete());
    assert_eq!(budget.available(), 2);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn readiness_flood_does_not_starve_due_analysis_or_lose_the_consumed_signal()
-> EngineSearchSessionTestResult {
    let package = EnginePackageFixture::new()?;
    let budget = EngineSearchSessionFixture::budget(2)?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = EngineSearchSessionFixture::session(&mut provider, &package).await?;
    let tag = EngineSearchSessionFixture::start(&mut session, &mut provider, &budget, 1).await?;
    provider.emit("info depth 8 score cp 5 pv e2e4").await?;
    {
        let event = session.next_event(tag);
        tokio::pin!(event);
        assert!(
            poll_fn(|context| Poll::Ready(event.as_mut().poll(context)))
                .await
                .is_pending()
        );
    }
    tokio::time::advance(Duration::from_millis(50)).await;
    for _ in 0..16 {
        provider.emit("readyok").await?;
    }
    assert!(matches!(
        session.next_event(tag).await?,
        EngineSearchSessionEnumEvent::Snapshot(_)
    ));
    for _ in 0..16 {
        assert!(
            matches!(session.next_event(tag).await?, EngineSearchSessionEnumEvent::Readiness { tag: received } if received == tag)
        );
    }
    provider.emit("bestmove e2e4").await?;
    assert!(matches!(
        session.next_event(tag).await?,
        EngineSearchSessionEnumEvent::Finished(_)
    ));
    EngineSearchSessionFixture::close(&mut session).await
}
