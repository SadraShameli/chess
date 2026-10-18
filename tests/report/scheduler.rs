mod fixture {
    include!("fixture.rs");

    use chess::report::scheduler::dispatch::ReportSchedulerDispatch;

    struct ReportSchedulerFixtureBlockedProvider(ReportFixtureProvider);

    impl EngineProcessProvider for ReportSchedulerFixtureBlockedProvider {
        type Process = ReportSchedulerFixtureBlockedProcess;

        fn spawn(
            &self,
            package: &EnginePackageSelected,
        ) -> Result<Self::Process, EngineProcessError> {
            Ok(ReportSchedulerFixtureBlockedProcess(self.0.spawn(package)?))
        }
    }

    pub(super) struct ReportSchedulerFixtureBlockedProcess(ReportFixtureProcess);

    impl EngineProcess for ReportSchedulerFixtureBlockedProcess {
        async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
            if command.starts_with("go ") {
                std::future::pending::<()>().await;
            }
            self.0.send(command).await
        }

        async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
            self.0.receive().await
        }

        async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
            self.0.shutdown().await
        }

        async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
            self.0.shutdown_retained(deadline).await
        }

        fn is_shutdown_complete(&self) -> bool {
            self.0.is_shutdown_complete()
        }

        async fn terminate(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
            self.0.terminate(deadline).await
        }
    }

    impl ReportFixture {
        pub(super) fn shared_capacity(&self) -> CapacityBudget {
            self.capacity.clone()
        }

        pub(super) fn session_mut(&mut self) -> &mut EngineSearchSession<ReportFixtureProcess> {
            &mut self.search
        }

        pub(super) async fn configure_workers(
            &mut self,
            workers: NonZeroUsize,
        ) -> ReportFixtureResult {
            self.search
                .idle_engine()?
                .configure_resources(EngineResourceConfig::new(
                    workers.get(),
                    16,
                    ClockDuration::ZERO,
                )?)
                .await?;
            self.capacity = CapacityBudget::new(workers)?;
            Ok(())
        }

        pub(super) async fn start_dispatch(
            &mut self,
            dispatch: &mut ReportSchedulerDispatch,
        ) -> ReportFixtureResult<EngineSearchTag> {
            Ok(dispatch.start(&mut self.search).await?)
        }

        pub(super) async fn blocked_search(
            &self,
        ) -> ReportFixtureResult<EngineSearchSession<ReportSchedulerFixtureBlockedProcess>>
        {
            let (sender, receiver) = mpsc::channel(64);
            let provider = ReportSchedulerFixtureBlockedProvider(ReportFixtureProvider {
                receiver: Mutex::new(Some(receiver)),
                sender,
            });
            let mut engine =
                EngineSession::start(&provider, self.search.engine().package().clone()).await?;
            engine
                .configure_resources(EngineResourceConfig::new(1, 16, ClockDuration::ZERO)?)
                .await?;
            Ok(EngineSearchSession::new(engine))
        }

        pub(super) fn cancel_stop_wait(
            &mut self,
            tag: EngineSearchTag,
        ) -> ReportFixtureResult<bool> {
            use std::future::Future;
            use std::task::{Context, Poll, Waker};

            use chess::engine::search::stop::EngineSearchStop;

            let request = EngineSearchStop::received_at(tag, Instant::now())?;
            let mut wait = Box::pin(self.search.stop(request));
            let mut context = Context::from_waker(Waker::noop());
            let pending = matches!(wait.as_mut().poll(&mut context), Poll::Pending);
            drop(wait);
            Ok(pending)
        }
    }
}

use std::num::NonZeroUsize;

use chess::engine::search::identity::EngineSearchTag;
use chess::report::job::{
    ReportJobEnumFreshness, ReportJobEnumPositionState, ReportJobEnumState, ReportJobId,
};
use chess::report::scheduler::ReportScheduler;
use chess::report::scheduler::policy::{
    ReportSchedulerDemand, ReportSchedulerEnumActivity, ReportSchedulerEnumBlocker,
    ReportSchedulerEnumDecision,
};
use chess::report::source::ReportJobSource;

use fixture::{ReportFixture, ReportFixtureResult};

struct ReportSchedulerFixture;

impl ReportSchedulerFixture {
    fn demand(live_games: u32, interactive: ReportSchedulerEnumActivity) -> ReportSchedulerDemand {
        ReportSchedulerDemand::new(
            live_games,
            ReportSchedulerEnumActivity::Idle,
            interactive,
            ReportSchedulerEnumActivity::Idle,
        )
    }

    fn idle() -> ReportSchedulerDemand {
        Self::demand(0, ReportSchedulerEnumActivity::Idle)
    }

    fn scheduler(limit: usize) -> ReportFixtureResult<ReportScheduler> {
        Ok(ReportScheduler::new(
            NonZeroUsize::new(limit).ok_or("Empty report queue")?,
            Self::idle(),
        ))
    }

    async fn start(
        scheduler: &mut ReportScheduler,
        fixture: &mut ReportFixture,
    ) -> ReportFixtureResult<EngineSearchTag> {
        let capacity = fixture.shared_capacity();
        let mut dispatch = scheduler.prepare(fixture.session_mut(), capacity.try_lease(1)?)?;
        let tag = fixture.start_dispatch(&mut dispatch).await?;
        assert_eq!(scheduler.bind(fixture.session())?, tag);
        Ok(tag)
    }

    async fn finish(
        scheduler: &mut ReportScheduler,
        fixture: &mut ReportFixture,
        tag: EngineSearchTag,
        info: &str,
    ) -> ReportFixtureResult {
        let terminal = fixture.finish(tag, info).await?;
        scheduler.record_terminal(&terminal)?;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn fifo_order_is_the_literal_enqueued_order_with_one_shared_worker() -> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(3)?;
    let third = fixture.job_with_id(&[], 33)?;
    let first = fixture.job_with_id(&[], 11)?;
    let second = fixture.job_with_id(&[], 22)?;
    let expected = [third.id(), first.id(), second.id()];
    scheduler.enqueue(third)?;
    scheduler.enqueue(first)?;
    scheduler.enqueue(second)?;
    let mut observed = Vec::new();
    for expected_job in expected {
        let ReportSchedulerEnumDecision::Ready(actual_job) = scheduler.decision() else {
            return Err("Report queue did not admit its expected owner".into());
        };
        observed.push(actual_job);
        assert_eq!(actual_job, expected_job);
        let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
        assert_eq!(capacity.available(), 0);
        assert!(capacity.try_lease(1).is_err());
        assert_eq!(fixture.session().allocation(), Some(1));
        assert_eq!(
            scheduler.decision(),
            ReportSchedulerEnumDecision::AwaitingTerminal(tag)
        );
        assert!(scheduler.bind(fixture.session()).is_err());
        assert_eq!(scheduler.job(expected_job)?.active_search(), Some(tag));
        ReportSchedulerFixture::finish(
            &mut scheduler,
            &mut fixture,
            tag,
            "info score cp 0 pv e2e4",
        )
        .await?;
        assert_eq!(capacity.available(), 1);
        assert_eq!(
            scheduler.take(expected_job)?.state(),
            ReportJobEnumState::Completed
        );
    }
    assert_eq!(observed, expected);
    assert!(scheduler.is_empty());
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn bounded_queue_returns_rejected_jobs_and_rejects_duplicate_identity() -> ReportFixtureResult
{
    let fixture = ReportFixture::new().await?;
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job_with_id(&[], 10)?)?;
    let rejected = scheduler
        .enqueue(fixture.job_with_id(&[], 20)?)
        .err()
        .ok_or("Queue admitted excess job")?;
    assert_eq!(rejected.into_job().id().value(), uuid::Uuid::from_u128(20));
    let duplicate = scheduler
        .enqueue(fixture.job_with_id(&[], 10)?)
        .err()
        .ok_or("Queue admitted duplicate")?;
    assert_eq!(duplicate.into_job().id(), id);
    assert_eq!(scheduler.len(), 1);
    scheduler.cancel(id)?;
    assert_eq!(scheduler.take(id)?.state(), ReportJobEnumState::Cancelled);
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn every_running_live_game_blocks_even_when_all_are_on_human_turns() -> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    for count in [1, 3, 1] {
        scheduler.update_demand(ReportSchedulerFixture::demand(
            count,
            ReportSchedulerEnumActivity::Idle,
        ))?;
        assert_eq!(
            scheduler.decision(),
            ReportSchedulerEnumDecision::Blocked(ReportSchedulerEnumBlocker::LiveGames)
        );
        assert!(
            scheduler
                .prepare(fixture.session_mut(), capacity.try_lease(1)?)
                .is_err()
        );
        assert_eq!(capacity.available(), 1);
        assert_eq!(scheduler.job(id)?.state(), ReportJobEnumState::Queued);
        assert_eq!(scheduler.job(id)?.active_search(), None);
    }
    scheduler.update_demand(ReportSchedulerFixture::idle())?;
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Ready(id));
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn interactive_preemption_keeps_completed_work_full_budget_and_fifo_owner()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut scheduler = ReportSchedulerFixture::scheduler(2)?;
    let first = scheduler.enqueue(fixture.job_with_id(&["e2e4"], 11)?)?;
    let second = scheduler.enqueue(fixture.job_with_id(&[], 22)?)?;
    let first_tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    ReportSchedulerFixture::finish(
        &mut scheduler,
        &mut fixture,
        first_tag,
        "info score cp 400 pv e2e4",
    )
    .await?;
    let interrupted_tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    let original_limit = fixture
        .session()
        .metadata()
        .ok_or("Missing metadata")?
        .limit();
    scheduler.update_demand(ReportSchedulerFixture::demand(
        0,
        ReportSchedulerEnumActivity::Active,
    ))?;
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Stop(interrupted_tag)
    );
    assert_eq!(scheduler.job(first)?.state(), ReportJobEnumState::Pausing);
    ReportSchedulerFixture::finish(
        &mut scheduler,
        &mut fixture,
        interrupted_tag,
        "info score cp 0 pv e7e5",
    )
    .await?;
    assert_eq!(scheduler.job(first)?.progress().completed(), 1);
    assert_eq!(
        scheduler.job(first)?.position_state(0)?,
        ReportJobEnumPositionState::Completed
    );
    assert_eq!(
        scheduler.job(first)?.position_state(1)?,
        ReportJobEnumPositionState::Pending
    );
    scheduler.update_demand(ReportSchedulerFixture::idle())?;
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Ready(first)
    );
    let resumed_tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    assert_ne!(resumed_tag, interrupted_tag);
    assert_eq!(
        fixture
            .session()
            .metadata()
            .ok_or("Missing resumed metadata")?
            .limit(),
        original_limit
    );
    assert_eq!(
        original_limit,
        chess::engine::search::limit::EngineSearchLimit::move_time(
            chess::clock::duration::ClockDuration::from_millis(3000)?
        )?
    );
    ReportSchedulerFixture::finish(
        &mut scheduler,
        &mut fixture,
        resumed_tag,
        "info score cp 0 pv e7e5",
    )
    .await?;
    assert_eq!(scheduler.job(first)?.progress().completed(), 2);
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Ready(second)
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn canceled_stop_wait_and_cleared_demand_cannot_release_or_replace_bound_attempt()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(2)?;
    let first = scheduler.enqueue(fixture.job_with_id(&[], 11)?)?;
    let second = scheduler.enqueue(fixture.job_with_id(&[], 22)?)?;
    let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    scheduler.update_demand(ReportSchedulerFixture::demand(
        1,
        ReportSchedulerEnumActivity::Idle,
    ))?;
    assert!(fixture.cancel_stop_wait(tag)?);
    scheduler.update_demand(ReportSchedulerFixture::idle())?;
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Stop(tag));
    assert_eq!(scheduler.job(first)?.active_search(), Some(tag));
    assert_eq!(capacity.available(), 0);
    assert!(capacity.try_lease(1).is_err());
    assert!(scheduler.take(first).is_err());
    assert!(scheduler.bind(fixture.session()).is_err());
    assert_eq!(scheduler.job(second)?.state(), ReportJobEnumState::Queued);
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    assert_eq!(capacity.available(), 1);
    assert_eq!(scheduler.job(first)?.progress().completed(), 0);
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Ready(first)
    );
    let resumed = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    ReportSchedulerFixture::finish(
        &mut scheduler,
        &mut fixture,
        resumed,
        "info score cp 0 pv e2e4",
    )
    .await?;
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Ready(second)
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn source_invalidation_retains_bound_owner_and_rejects_stale_terminals() -> ReportFixtureResult
{
    let mut fixture = ReportFixture::new().await?;
    let mut scheduler = ReportSchedulerFixture::scheduler(2)?;
    let first = scheduler.enqueue(fixture.job_with_id(&[], 11)?)?;
    let second = scheduler.enqueue(fixture.job_with_id(&[], 22)?)?;
    let old_tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    let changed = ReportFixture::source(&ReportFixture::tree(&["e2e4"])?)?;
    scheduler.observe_source(first, changed)?;
    assert_eq!(
        scheduler.job(first)?.freshness(),
        ReportJobEnumFreshness::Outdated
    );
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Stop(old_tag)
    );
    let old_terminal = fixture.finish(old_tag, "info score cp 0 pv e2e4").await?;
    scheduler.record_terminal(&old_terminal)?;
    assert_eq!(scheduler.job(first)?.progress().completed(), 0);
    scheduler.observe_source(first, ReportFixture::source(&ReportFixture::tree(&[])?)?)?;
    assert_eq!(
        scheduler.job(first)?.freshness(),
        ReportJobEnumFreshness::Outdated
    );
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Ready(second)
    );
    let new_tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    assert!(scheduler.record_terminal(&old_terminal).is_err());
    assert_eq!(scheduler.job(second)?.active_search(), Some(new_tag));
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::AwaitingTerminal(new_tag)
    );
    ReportSchedulerFixture::finish(
        &mut scheduler,
        &mut fixture,
        new_tag,
        "info score cp 0 pv e2e4",
    )
    .await?;
    assert!(scheduler.select(first).is_err());
    assert_eq!(
        scheduler.take(first)?.freshness(),
        ReportJobEnumFreshness::Outdated
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn restored_jobs_remain_paused_until_explicit_selection_and_keep_prior_results()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut restored = fixture.job_with_id(&["e2e4"], 11)?;
    restored.resume()?;
    fixture
        .complete(&mut restored, 0, "info score cp 400 pv e2e4")
        .await?;
    restored.recover_after_relaunch()?;
    let mut scheduler = ReportSchedulerFixture::scheduler(2)?;
    let id = scheduler.enqueue(restored)?;
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Idle);
    scheduler.update_demand(ReportSchedulerFixture::demand(
        1,
        ReportSchedulerEnumActivity::Idle,
    ))?;
    scheduler.update_demand(ReportSchedulerFixture::idle())?;
    assert_eq!(scheduler.job(id)?.state(), ReportJobEnumState::Paused);
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Idle);
    scheduler.select(id)?;
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Ready(id));
    let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    assert_eq!(
        fixture
            .session()
            .metadata()
            .ok_or("Missing restored metadata")?
            .position()
            .current()
            .turn(),
        chess::board::enums::BoardEnumColor::Black
    );
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e7e5")
        .await?;
    assert_eq!(scheduler.job(id)?.progress().completed(), 2);
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancellation_waits_for_terminal_then_advances_without_auto_resuming_canceled_job()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut scheduler = ReportSchedulerFixture::scheduler(2)?;
    let first = scheduler.enqueue(fixture.job_with_id(&[], 11)?)?;
    let second = scheduler.enqueue(fixture.job_with_id(&[], 22)?)?;
    let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    scheduler.cancel(first)?;
    assert_eq!(
        scheduler.job(first)?.state(),
        ReportJobEnumState::Cancelling
    );
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Stop(tag));
    assert!(scheduler.select(first).is_err());
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    assert_eq!(scheduler.job(first)?.state(), ReportJobEnumState::Cancelled);
    assert_eq!(scheduler.job(first)?.progress().completed(), 0);
    scheduler.update_demand(ReportSchedulerFixture::idle())?;
    assert!(scheduler.select(first).is_err());
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Ready(second)
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn dispatch_reservation_defers_pause_until_binding_and_cannot_be_settled_while_searching()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(2)?;
    let first = scheduler.enqueue(fixture.job_with_id(&[], 11)?)?;
    scheduler.enqueue(fixture.job_with_id(&[], 22)?)?;
    let mut dispatch = scheduler.prepare(fixture.session_mut(), capacity.try_lease(1)?)?;
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Dispatching(first)
    );
    let tag = fixture.start_dispatch(&mut dispatch).await?;
    scheduler.update_demand(ReportSchedulerFixture::demand(
        1,
        ReportSchedulerEnumActivity::Idle,
    ))?;
    assert!(
        scheduler
            .settle_dispatch(&mut dispatch, fixture.session_mut())
            .is_err()
    );
    assert!(scheduler.take(first).is_err());
    assert_eq!(scheduler.bind(fixture.session())?, tag);
    let outcome = chess::game::outcome::GameOutcome::new(
        chess::game::enums::GameEnumResult::Draw,
        chess::game::enums::GameEnumOutcomeReason::Agreement,
        ReportFixture::node(1)?,
    )?;
    assert!(scheduler.record_outcome(&mut dispatch, outcome).is_err());
    assert_eq!(scheduler.job(first)?.active_search(), Some(tag));
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Stop(tag));
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    assert_eq!(scheduler.job(first)?.progress().completed(), 0);
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn abandoned_dispatch_is_settled_only_after_actual_terminal_and_releases_no_synthetic_result()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    let mut dispatch = scheduler.prepare(fixture.session_mut(), capacity.try_lease(1)?)?;
    let tag = fixture.start_dispatch(&mut dispatch).await?;
    assert!(fixture.cancel_stop_wait(tag)?);
    assert!(
        scheduler
            .settle_dispatch(&mut dispatch, fixture.session_mut())
            .is_err()
    );
    assert_eq!(capacity.available(), 0);
    fixture.finish(tag, "info score cp 0 pv e2e4").await?;
    assert!(
        scheduler
            .settle_dispatch(&mut dispatch, fixture.session_mut())?
            .is_some()
    );
    assert_eq!(capacity.available(), 1);
    assert_eq!(scheduler.job(id)?.state(), ReportJobEnumState::Paused);
    assert_eq!(scheduler.job(id)?.progress().completed(), 0);
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Idle);
    scheduler.select(id)?;
    let resumed = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    ReportSchedulerFixture::finish(
        &mut scheduler,
        &mut fixture,
        resumed,
        "info score cp 0 pv e2e4",
    )
    .await?;
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn dispatch_source_change_is_applied_after_binding_before_any_terminal_result()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    let mut dispatch = scheduler.prepare(fixture.session_mut(), capacity.try_lease(1)?)?;
    scheduler.observe_source(id, ReportFixture::source(&ReportFixture::tree(&["e2e4"])?)?)?;
    let tag = fixture.start_dispatch(&mut dispatch).await?;
    scheduler.bind(fixture.session())?;
    assert_eq!(
        scheduler.job(id)?.freshness(),
        ReportJobEnumFreshness::Outdated
    );
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Stop(tag));
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    assert_eq!(scheduler.job(id)?.progress().completed(), 0);
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Idle);
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn metadata_only_observation_keeps_search_and_other_game_is_rejected() -> ReportFixtureResult
{
    let mut fixture = ReportFixture::new().await?;
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    let tree = ReportFixture::tree(&[])?;
    let old = scheduler.job(id)?.source();
    let same = ReportJobSource::capture(
        old.game(),
        old.revision().next()?,
        &tree,
        old.tip(),
        old.limits(),
    )?;
    let wrong = ReportJobSource::capture(
        ReportFixture::game(2)?,
        old.revision(),
        &tree,
        old.tip(),
        old.limits(),
    )?;
    scheduler.observe_source(id, same)?;
    assert!(scheduler.observe_source(id, wrong).is_err());
    assert_eq!(
        scheduler.job(id)?.freshness(),
        ReportJobEnumFreshness::Current
    );
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::AwaitingTerminal(tag)
    );
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn workbench_refuses_report_interactive_and_live_conflicts_without_preempting_reports()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    assert_eq!(scheduler.workbench_admission(), Ok(()));
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    assert_eq!(
        scheduler.workbench_admission(),
        Err(ReportSchedulerEnumBlocker::Report)
    );
    let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    assert_eq!(
        scheduler.workbench_admission(),
        Err(ReportSchedulerEnumBlocker::Report)
    );
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::AwaitingTerminal(tag)
    );
    scheduler.cancel(id)?;
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    for (demand, expected) in [
        (
            ReportSchedulerFixture::demand(1, ReportSchedulerEnumActivity::Idle),
            ReportSchedulerEnumBlocker::LiveGames,
        ),
        (
            ReportSchedulerDemand::new(
                0,
                ReportSchedulerEnumActivity::Active,
                ReportSchedulerEnumActivity::Idle,
                ReportSchedulerEnumActivity::Idle,
            ),
            ReportSchedulerEnumBlocker::LiveSearch,
        ),
        (
            ReportSchedulerFixture::demand(0, ReportSchedulerEnumActivity::Active),
            ReportSchedulerEnumBlocker::InteractiveAnalysis,
        ),
        (
            ReportSchedulerDemand::new(
                0,
                ReportSchedulerEnumActivity::Idle,
                ReportSchedulerEnumActivity::Idle,
                ReportSchedulerEnumActivity::Active,
            ),
            ReportSchedulerEnumBlocker::Workbench,
        ),
    ] {
        scheduler.update_demand(demand)?;
        assert_eq!(scheduler.workbench_admission(), Err(expected));
    }
    scheduler.update_demand(ReportSchedulerFixture::idle())?;
    assert_eq!(scheduler.workbench_admission(), Ok(()));
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn unavailable_or_mismatched_configuration_never_starts_or_consumes_a_position()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    let held = capacity.try_lease(1)?;
    assert!(capacity.try_lease(1).is_err());
    assert_eq!(
        scheduler.job(id)?.position_state(0)?,
        ReportJobEnumPositionState::Pending
    );
    drop(held);
    fixture.configure_hash(32).await?;
    assert!(
        scheduler
            .prepare(fixture.session_mut(), capacity.try_lease(1)?)
            .is_err()
    );
    assert_eq!(capacity.available(), 1);
    assert_eq!(
        scheduler.job(id)?.position_state(0)?,
        ReportJobEnumPositionState::Pending
    );
    assert_eq!(scheduler.job(id)?.state(), ReportJobEnumState::Queued);
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn unrelated_terminal_cannot_settle_an_inflight_dispatch_or_replace_its_reservation()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut older = fixture.job_with_id(&[], 90)?;
    older.resume()?;
    let attempt = older.start_position(0)?;
    let old_tag = fixture.start(&attempt, None).await?;
    let binding = older.bind_engine(&attempt, fixture.session())?;
    let old_terminal = fixture.finish(old_tag, "info score cp 0 pv e2e4").await?;
    older.record_terminal(&binding, &old_terminal)?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id: ReportJobId = scheduler.enqueue(fixture.job(&[])?)?;
    let mut dispatch = scheduler.prepare(fixture.session_mut(), capacity.try_lease(1)?)?;
    let tag = fixture.start_dispatch(&mut dispatch).await?;
    assert!(scheduler.record_terminal(&old_terminal).is_err());
    assert!(
        scheduler
            .settle_dispatch(&mut dispatch, fixture.session_mut())
            .is_err()
    );
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Dispatching(id)
    );
    scheduler.bind(fixture.session())?;
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn lease_availability_without_recorded_terminal_never_grants_a_second_report()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(2)?;
    let first = scheduler.enqueue(fixture.job_with_id(&[], 11)?)?;
    let second = scheduler.enqueue(fixture.job_with_id(&[], 22)?)?;
    let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    let terminal = fixture.finish(tag, "info score cp 0 pv e2e4").await?;
    assert_eq!(capacity.available(), 1);
    assert!(
        scheduler
            .prepare(fixture.session_mut(), capacity.try_lease(1)?)
            .is_err()
    );
    assert_eq!(scheduler.job(first)?.active_search(), Some(tag));
    assert_eq!(scheduler.job(second)?.state(), ReportJobEnumState::Queued);
    scheduler.record_terminal(&terminal)?;
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Ready(second)
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn unstarted_dispatch_requires_the_original_idle_session_and_explicit_reselection()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut other = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    let mut dispatch = scheduler.prepare(fixture.session_mut(), capacity.try_lease(1)?)?;
    assert_eq!(capacity.available(), 0);
    assert!(
        scheduler
            .settle_dispatch(&mut dispatch, other.session_mut())
            .is_err()
    );
    assert_eq!(capacity.available(), 0);
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Dispatching(id)
    );
    assert!(
        scheduler
            .settle_dispatch(&mut dispatch, fixture.session_mut())?
            .is_none()
    );
    assert_eq!(capacity.available(), 1);
    assert!(fixture.start_dispatch(&mut dispatch).await.is_err());
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Idle);
    scheduler.select(id)?;
    let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    other.close().await?;
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn recorded_four_worker_allocation_consumes_the_whole_injected_budget_and_rejects_partial_leases()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    fixture
        .configure_workers(NonZeroUsize::new(4).ok_or("Empty allocation")?)
        .await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    assert!(
        scheduler
            .prepare(fixture.session_mut(), capacity.try_lease(3)?)
            .is_err()
    );
    assert_eq!(capacity.available(), 4);
    assert_eq!(
        scheduler.job(id)?.position_state(0)?,
        ReportJobEnumPositionState::Pending
    );
    let mut dispatch = scheduler.prepare(fixture.session_mut(), capacity.try_lease(4)?)?;
    let tag = fixture.start_dispatch(&mut dispatch).await?;
    scheduler.bind(fixture.session())?;
    assert_eq!(fixture.session().allocation(), Some(4));
    assert_eq!(capacity.available(), 0);
    assert!(capacity.try_lease(1).is_err());
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    assert_eq!(capacity.available(), 4);
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn canceled_start_retains_dispatch_and_lease_until_real_session_shutdown_is_observed()
-> ReportFixtureResult {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    let mut fixture = ReportFixture::new().await?;
    let mut blocked = fixture.blocked_search().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    let mut dispatch = scheduler.prepare(&mut blocked, capacity.try_lease(1)?)?;
    let mut start = Box::pin(dispatch.start(&mut blocked));
    let mut context = Context::from_waker(Waker::noop());
    let pending = matches!(start.as_mut().poll(&mut context), Poll::Pending);
    drop(start);
    assert!(pending);
    assert_eq!(capacity.available(), 0);
    assert_eq!(blocked.allocation(), Some(1));
    scheduler.update_demand(ReportSchedulerFixture::demand(
        1,
        ReportSchedulerEnumActivity::Idle,
    ))?;
    assert!(
        scheduler
            .settle_dispatch(&mut dispatch, &mut blocked)
            .is_err()
    );
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Dispatching(id)
    );
    blocked
        .shutdown_retained(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await?;
    assert!(blocked.engine().is_shutdown_complete());
    let terminal = scheduler
        .settle_dispatch(&mut dispatch, &mut blocked)?
        .ok_or("Missing canceled start terminal")?;
    assert!(matches!(
        terminal.completion(),
        chess::engine::search::session::EngineSearchSessionEnumCompletion::Cancelled
    ));
    assert_eq!(capacity.available(), 1);
    assert_eq!(scheduler.job(id)?.progress().completed(), 0);
    scheduler.update_demand(ReportSchedulerFixture::idle())?;
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Ready(id));
    let resumed = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    ReportSchedulerFixture::finish(
        &mut scheduler,
        &mut fixture,
        resumed,
        "info score cp 0 pv e2e4",
    )
    .await?;
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn engine_failure_is_retained_and_requires_explicit_selection_instead_of_automatic_retry()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&[])?)?;
    let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    let terminal = fixture
        .finish(tag, "info multipv 2 score cp 0 pv e2e4")
        .await?;
    assert!(matches!(
        terminal.completion(),
        chess::engine::search::session::EngineSearchSessionEnumCompletion::Failed(_)
    ));
    scheduler.record_terminal(&terminal)?;
    assert!(matches!(
        scheduler.job(id)?.interruption(),
        Some(chess::report::job::ReportJobEnumInterruption::EngineFailed(
            _
        ))
    ));
    assert_eq!(scheduler.job(id)?.progress().completed(), 0);
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Idle);
    scheduler.update_demand(ReportSchedulerFixture::idle())?;
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Idle);
    assert_eq!(capacity.available(), 1);
    scheduler.select(id)?;
    assert_eq!(scheduler.decision(), ReportSchedulerEnumDecision::Ready(id));
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn terminal_position_uses_actual_node_bound_outcome_without_dispatching_an_impossible_search()
-> ReportFixtureResult {
    use chess::board::enums::BoardEnumColor;
    use chess::game::enums::{GameEnumOutcomeReason, GameEnumResult};
    use chess::game::outcome::GameOutcome;

    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(1)?;
    let id = scheduler.enqueue(fixture.job(&["f2f3", "e7e5", "g2g4", "d8h4"])?)?;
    for info in [
        "info score cp 0 pv f2f3",
        "info score cp 0 pv e7e5",
        "info score cp 0 pv g2g4",
        "info score mate 1 pv d8h4",
    ] {
        let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
        ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, info).await?;
    }
    let mut dispatch = scheduler.prepare(fixture.session_mut(), capacity.try_lease(1)?)?;
    let outcome = GameOutcome::new(
        GameEnumResult::Victory(BoardEnumColor::Black),
        GameEnumOutcomeReason::Checkmate,
        ReportFixture::node(5)?,
    )?;
    scheduler.record_outcome(&mut dispatch, outcome)?;
    assert_eq!(scheduler.job(id)?.state(), ReportJobEnumState::Completed);
    assert_eq!(scheduler.job(id)?.progress().completed(), 5);
    assert_eq!(capacity.available(), 1);
    assert_eq!(fixture.session().allocation(), None);
    assert!(fixture.start_dispatch(&mut dispatch).await.is_err());
    fixture.close().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn priority_pause_before_start_withdraws_the_request_and_resumes_the_same_fifo_owner()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let capacity = fixture.shared_capacity();
    let mut scheduler = ReportSchedulerFixture::scheduler(2)?;
    let first = scheduler.enqueue(fixture.job_with_id(&[], 11)?)?;
    scheduler.enqueue(fixture.job_with_id(&[], 22)?)?;
    let mut dispatch = scheduler.prepare(fixture.session_mut(), capacity.try_lease(1)?)?;
    scheduler.update_demand(ReportSchedulerFixture::demand(
        1,
        ReportSchedulerEnumActivity::Idle,
    ))?;
    assert!(
        scheduler
            .settle_dispatch(&mut dispatch, fixture.session_mut())?
            .is_none()
    );
    assert_eq!(capacity.available(), 1);
    assert_eq!(scheduler.job(first)?.state(), ReportJobEnumState::Paused);
    scheduler.update_demand(ReportSchedulerFixture::idle())?;
    assert_eq!(
        scheduler.decision(),
        ReportSchedulerEnumDecision::Ready(first)
    );
    let tag = ReportSchedulerFixture::start(&mut scheduler, &mut fixture).await?;
    ReportSchedulerFixture::finish(&mut scheduler, &mut fixture, tag, "info score cp 0 pv e2e4")
        .await?;
    fixture.close().await?;
    Ok(())
}
