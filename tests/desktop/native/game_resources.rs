use std::future::poll_fn;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::Poll;

use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockDuration;
use chess::desktop::native::game_engine::{
    DesktopNativeGameEngineAllocation, DesktopNativeGameEngineResources,
};
use chess::desktop::native::game_resources::{
    DesktopNativeGameResources, DesktopNativeGameResourcesError,
    DesktopNativeGameResourcesMemoryProvider, DesktopNativeGameResourcesOptions,
    DesktopNativeGameResourcesPolicy, DesktopNativeGameResourcesRegistration,
};
use chess::engine::resource_config::EngineResourceConfig;
use chess::game::engine_provider::GameEngineContext;
use chess::game::enums::GameRevision;

type DesktopNativeGameResourcesTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct DesktopNativeGameResourcesFixtureMemory(AtomicU64);

impl DesktopNativeGameResourcesMemoryProvider for DesktopNativeGameResourcesFixtureMemory {
    type Error = std::io::Error;

    async fn available_bytes(&self) -> Result<u64, Self::Error> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct DesktopNativeGameResourcesFixture {
    resources: DesktopNativeGameResources<DesktopNativeGameResourcesFixtureMemory>,
    memory: Arc<DesktopNativeGameResourcesFixtureMemory>,
    budget: CapacityBudget,
}

impl DesktopNativeGameResourcesFixture {
    fn new(workers: usize, hash_mib: u32) -> DesktopNativeGameResourcesTestResult<Self> {
        let budget = CapacityBudget::new(NonZeroUsize::new(workers).ok_or("empty budget")?)?;
        let memory = Arc::new(DesktopNativeGameResourcesFixtureMemory(AtomicU64::new(
            64 * 1024 * 1024 * 1024,
        )));
        Ok(Self {
            resources: DesktopNativeGameResources::new(DesktopNativeGameResourcesOptions {
                budget: budget.clone(),
                policy: DesktopNativeGameResourcesPolicy::new(workers, hash_mib)?,
                memory: Arc::clone(&memory),
            })?,
            memory,
            budget,
        })
    }

    fn context(index: u8) -> DesktopNativeGameResourcesTestResult<GameEngineContext> {
        let game = format!("00000000-0000-4000-8000-{index:012}").parse()?;
        Ok(GameEngineContext::new(game, GameRevision::ZERO))
    }

    async fn acknowledge(
        &self,
        context: GameEngineContext,
    ) -> DesktopNativeGameResourcesTestResult {
        let allocation = self.resources.allocate(context).await?;
        self.resources
            .acknowledge(context, allocation.config())
            .await?;
        Ok(())
    }

    async fn register_second(
        &self,
        first: GameEngineContext,
        second: GameEngineContext,
    ) -> DesktopNativeGameResourcesTestResult<DesktopNativeGameResourcesRegistration> {
        let pending = self.resources.register(second);
        tokio::pin!(pending);
        poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let shrink = self.resources.next_update(first).await?;
        self.resources.acknowledge(first, shrink.config()).await?;
        drop(shrink);
        Ok(pending.await?)
    }

    async fn simultaneous_searches(
        &self,
        first: GameEngineContext,
        second: GameEngineContext,
    ) -> DesktopNativeGameResourcesTestResult<[DesktopNativeGameEngineAllocation; 2]> {
        let blocker = self.budget.lease(self.budget.workers()).await?;
        let first_pending = self.resources.allocate(first);
        let second_pending = self.resources.allocate(second);
        tokio::pin!(first_pending, second_pending);
        poll_fn(|cx| {
            assert!(first_pending.as_mut().poll(cx).is_pending());
            assert!(second_pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(blocker);
        Ok([first_pending.await?, second_pending.await?])
    }
}

#[test]
fn automatic_hash_uses_ten_percent_floor_and_two_gib_cap() -> DesktopNativeGameResourcesTestResult {
    let mib = DesktopNativeGameResourcesPolicy::MIB;
    assert_eq!(
        DesktopNativeGameResourcesPolicy::automatic(6, 109 * mib + 9)?.hash_mib(),
        10
    );
    assert_eq!(
        DesktopNativeGameResourcesPolicy::automatic(6, 64 * 1024 * mib)?.hash_mib(),
        2048
    );
    assert!(DesktopNativeGameResourcesPolicy::automatic(6, 9 * mib).is_err());
    Ok(())
}

#[tokio::test]
async fn human_turns_keep_admission_reservations_until_registration_drop()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(1, 64)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let registration = fixture.resources.register(first).await?;
    fixture.acknowledge(first).await?;
    assert_eq!(fixture.budget.available(), 1);
    assert!(matches!(
        fixture
            .resources
            .register(DesktopNativeGameResourcesFixture::context(2)?)
            .await,
        Err(DesktopNativeGameResourcesError::ThreadAdmission { .. })
    ));
    drop(registration);
    let _next = fixture
        .resources
        .register(DesktopNativeGameResourcesFixture::context(2)?)
        .await?;
    Ok(())
}

#[tokio::test]
async fn memory_pressure_refuses_admission_without_shrinking_configured_hash()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(4, 2048)?;
    fixture.memory.0.store(1024 * 1024 * 1024, Ordering::SeqCst);
    assert!(matches!(
        fixture
            .resources
            .register(DesktopNativeGameResourcesFixture::context(1)?)
            .await,
        Err(DesktopNativeGameResourcesError::MemoryAdmission { .. })
    ));
    assert_eq!(fixture.resources.policy().hash_mib(), 2048);
    assert_eq!(fixture.budget.available(), 4);
    Ok(())
}

#[tokio::test]
async fn startup_default_cache_is_charged_before_engine_configuration()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(4, 8)?;
    assert!(matches!(
        fixture
            .resources
            .register(DesktopNativeGameResourcesFixture::context(1)?)
            .await,
        Err(DesktopNativeGameResourcesError::StartupHashAdmission { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn newcomer_waits_for_exact_idle_hash_shrink_acknowledgment()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(4, 65)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let second = DesktopNativeGameResourcesFixture::context(2)?;
    let _first_registration = fixture.resources.register(first).await?;
    fixture.acknowledge(first).await?;
    let pending = fixture.resources.register(second);
    tokio::pin!(pending);
    poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let update = fixture.resources.next_update(first).await?;
    assert_eq!(update.config().hash_mib(), 33);
    poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    fixture
        .resources
        .acknowledge(first, update.config())
        .await?;
    drop(update);
    let _second_registration = pending.await?;
    let second_allocation = fixture.resources.allocate(second).await?;
    assert_eq!(second_allocation.config().hash_mib(), 32);
    Ok(())
}

#[tokio::test]
async fn canceled_admission_does_not_keep_a_running_game_reservation()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(2, 64)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let _registration = fixture.resources.register(first).await?;
    fixture.acknowledge(first).await?;
    let second = DesktopNativeGameResourcesFixture::context(2)?;
    {
        let pending = fixture.resources.register(second);
        tokio::pin!(pending);
        poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    let allocation = fixture.resources.allocate(first).await?;
    assert_eq!(allocation.workers(), 2);
    assert_eq!(allocation.config().hash_mib(), 64);
    Ok(())
}

#[tokio::test]
async fn active_searches_lend_idle_capacity_then_reclaim_at_the_next_boundary()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(5, 64)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let second = DesktopNativeGameResourcesFixture::context(2)?;
    let _first_registration = fixture.resources.register(first).await?;
    fixture.acknowledge(first).await?;
    let pending_registration = fixture.resources.register(second);
    tokio::pin!(pending_registration);
    poll_fn(|cx| {
        assert!(pending_registration.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let shrink = fixture.resources.next_update(first).await?;
    fixture
        .resources
        .acknowledge(first, shrink.config())
        .await?;
    drop(shrink);
    let _second_registration = pending_registration.await?;
    fixture.acknowledge(second).await?;
    let first_allocation = fixture.resources.allocate(first).await?;
    assert_eq!(first_allocation.workers(), 5);
    let second_pending = fixture.resources.allocate(second);
    tokio::pin!(second_pending);
    poll_fn(|cx| {
        assert!(second_pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(first_allocation);
    let first_pending = fixture.resources.allocate(first);
    tokio::pin!(first_pending);
    let mut first_ready = None;
    poll_fn(|cx| {
        if let Poll::Ready(result) = first_pending.as_mut().poll(cx) {
            first_ready = Some(result);
        }
        Poll::Ready(())
    })
    .await;
    let second_allocation = second_pending.await?;
    let first_allocation = match first_ready {
        Some(result) => result?,
        None => first_pending.await?,
    };
    let mut workers = [first_allocation.workers(), second_allocation.workers()];
    workers.sort_unstable();
    assert_eq!(workers, [2, 3]);
    assert_eq!(fixture.budget.available(), 0);
    drop(first_allocation);
    drop(second_allocation);
    assert_eq!(fixture.budget.available(), 5);
    Ok(())
}

#[tokio::test]
async fn foreign_and_unreserved_hash_acknowledgments_are_rejected()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(2, 64)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let _registration = fixture.resources.register(first).await?;
    let oversized = EngineResourceConfig::new(1, 65, ClockDuration::ZERO)?;
    assert!(
        fixture
            .resources
            .acknowledge(first, oversized)
            .await
            .is_err()
    );
    assert!(
        fixture
            .resources
            .allocate(DesktopNativeGameResourcesFixture::context(2)?)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn canceled_idle_growth_releases_its_unapplied_hash_reservation()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(4, 64)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let second = DesktopNativeGameResourcesFixture::context(2)?;
    let _first_registration = fixture.resources.register(first).await?;
    fixture.acknowledge(first).await?;
    let second_registration = fixture.register_second(first, second).await?;
    fixture.acknowledge(second).await?;
    drop(second_registration);
    let lease = fixture.budget.lease(4).await?;
    {
        let growth = fixture.resources.next_update(first);
        tokio::pin!(growth);
        poll_fn(|cx| {
            assert!(growth.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    let registration = fixture.resources.register(second);
    tokio::pin!(registration);
    let mut result = None;
    poll_fn(|cx| {
        if let Poll::Ready(completed) = registration.as_mut().poll(cx) {
            result = Some(completed);
        }
        Poll::Ready(())
    })
    .await;
    assert!(result.is_some(), "canceled growth retained unapplied hash");
    let admitted = result.ok_or("missing admission result")??;
    drop(admitted);
    drop(lease);
    Ok(())
}

#[tokio::test]
async fn memory_is_rechecked_after_waiting_for_existing_cache_shrink()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(4, 64)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let second = DesktopNativeGameResourcesFixture::context(2)?;
    let _first_registration = fixture.resources.register(first).await?;
    fixture.acknowledge(first).await?;
    let pending = fixture.resources.register(second);
    tokio::pin!(pending);
    poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let update = fixture.resources.next_update(first).await?;
    fixture
        .resources
        .acknowledge(first, update.config())
        .await?;
    drop(update);
    fixture.memory.0.store(256 * 1024 * 1024, Ordering::SeqCst);
    assert!(matches!(
        pending.await,
        Err(DesktopNativeGameResourcesError::MemoryAdmission { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn remainder_worker_rotates_across_successive_shared_searches()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(5, 64)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let second = DesktopNativeGameResourcesFixture::context(2)?;
    let _first_registration = fixture.resources.register(first).await?;
    fixture.acknowledge(first).await?;
    let _second_registration = fixture.register_second(first, second).await?;
    fixture.acknowledge(second).await?;
    let allocations = fixture.simultaneous_searches(first, second).await?;
    assert_eq!([allocations[0].workers(), allocations[1].workers()], [3, 2]);
    drop(allocations);
    let allocations = fixture.simultaneous_searches(first, second).await?;
    assert_eq!([allocations[0].workers(), allocations[1].workers()], [2, 3]);
    assert_eq!(fixture.budget.available(), 0);
    Ok(())
}

#[tokio::test]
async fn canceled_search_admission_releases_its_active_membership()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(4, 64)?;
    let context = DesktopNativeGameResourcesFixture::context(1)?;
    let _registration = fixture.resources.register(context).await?;
    fixture.acknowledge(context).await?;
    let blocker = fixture.budget.lease(4).await?;
    {
        let pending = fixture.resources.allocate(context);
        tokio::pin!(pending);
        poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    drop(blocker);
    let allocation = fixture.resources.allocate(context).await?;
    assert_eq!(allocation.workers(), 4);
    Ok(())
}

#[tokio::test]
async fn high_core_budget_is_fully_assigned_shared_fairly_and_released()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(1024, 64)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let second = DesktopNativeGameResourcesFixture::context(2)?;
    let _first_registration = fixture.resources.register(first).await?;
    fixture.acknowledge(first).await?;
    let allocation = fixture.resources.allocate(first).await?;
    assert_eq!(allocation.workers(), 1024);
    assert_eq!(allocation.config().threads(), 1024);
    assert_eq!(fixture.budget.available(), 0);
    assert!(fixture.budget.try_lease(1).is_err());
    drop(allocation);
    assert_eq!(fixture.budget.available(), 1024);
    let _second_registration = fixture.register_second(first, second).await?;
    fixture.acknowledge(second).await?;
    let allocations = fixture.simultaneous_searches(first, second).await?;
    for allocation in &allocations {
        assert_eq!(allocation.workers(), 512);
        assert_eq!(allocation.config().threads(), 512);
    }
    assert_eq!(fixture.budget.available(), 0);
    assert!(fixture.budget.try_lease(1).is_err());
    drop(allocations);
    assert_eq!(fixture.budget.available(), 1024);
    let shared_work = fixture.budget.lease(1).await?;
    let pending = fixture.resources.allocate(first);
    tokio::pin!(pending);
    poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(shared_work);
    let allocation = pending.await?;
    assert_eq!(allocation.workers(), 1024);
    assert_eq!(allocation.config().threads(), 1024);
    drop(allocation);
    assert_eq!(fixture.budget.available(), 1024);
    Ok(())
}

#[tokio::test]
async fn startup_cache_can_temporarily_shrink_existing_shares_then_restore_equal_hash()
-> DesktopNativeGameResourcesTestResult {
    let fixture = DesktopNativeGameResourcesFixture::new(4, 32)?;
    let first = DesktopNativeGameResourcesFixture::context(1)?;
    let second = DesktopNativeGameResourcesFixture::context(2)?;
    let third = DesktopNativeGameResourcesFixture::context(3)?;
    let _first_registration = fixture.resources.register(first).await?;
    fixture.acknowledge(first).await?;
    let _second_registration = fixture.register_second(first, second).await?;
    fixture.acknowledge(second).await?;
    let pending = fixture.resources.register(third);
    tokio::pin!(pending);
    poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    for context in [first, second] {
        let update = fixture.resources.next_update(context).await?;
        assert_eq!(update.config().hash_mib(), 8);
        fixture
            .resources
            .acknowledge(context, update.config())
            .await?;
    }
    let _third_registration = pending.await?;
    let startup = fixture.resources.allocate(third).await?;
    assert_eq!(startup.config().hash_mib(), 10);
    fixture
        .resources
        .acknowledge(third, startup.config())
        .await?;
    drop(startup);
    for context in [first, second] {
        let update = fixture.resources.next_update(context).await?;
        assert_eq!(update.config().hash_mib(), 11);
        fixture
            .resources
            .acknowledge(context, update.config())
            .await?;
    }
    Ok(())
}
