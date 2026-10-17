use std::future::poll_fn;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::Poll;

use chess::capacity::budget::CapacityBudget;
use chess::desktop::native::game_engine::{
    DesktopNativeGameEngineAllocation, DesktopNativeGameEngineResources,
};
use chess::desktop::native::game_resources::{
    DesktopNativeGameResources, DesktopNativeGameResourcesMemoryProvider,
    DesktopNativeGameResourcesOptions, DesktopNativeGameResourcesPolicy,
    DesktopNativeGameResourcesRegistration,
};
use chess::game::engine_provider::GameEngineContext;
use chess::game::enums::GameRevision;

type CapacityConcurrentGamesResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct CapacityConcurrentGamesMemory(AtomicU64);

impl DesktopNativeGameResourcesMemoryProvider for CapacityConcurrentGamesMemory {
    type Error = std::io::Error;

    async fn available_bytes(&self) -> Result<u64, Self::Error> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct CapacityConcurrentGamesFixture {
    resources: DesktopNativeGameResources<CapacityConcurrentGamesMemory>,
    budget: CapacityBudget,
    registrations: Vec<DesktopNativeGameResourcesRegistration>,
    contexts: Vec<GameEngineContext>,
}

impl CapacityConcurrentGamesFixture {
    fn new(workers: usize, hash_mib: u32) -> CapacityConcurrentGamesResult<Self> {
        let budget = CapacityBudget::new(NonZeroUsize::new(workers).ok_or("empty budget")?)?;
        let memory = Arc::new(CapacityConcurrentGamesMemory(AtomicU64::new(
            64 * 1024 * 1024 * 1024,
        )));
        Ok(Self {
            resources: DesktopNativeGameResources::new(DesktopNativeGameResourcesOptions {
                budget: budget.clone(),
                policy: DesktopNativeGameResourcesPolicy::new(workers, hash_mib)?,
                memory,
            })?,
            budget,
            registrations: Vec::new(),
            contexts: Vec::new(),
        })
    }

    fn context(index: u8) -> CapacityConcurrentGamesResult<GameEngineContext> {
        let game = format!("00000000-0000-4000-8000-{index:012}").parse()?;
        Ok(GameEngineContext::new(game, GameRevision::ZERO))
    }

    async fn admit(&mut self, index: u8) -> CapacityConcurrentGamesResult {
        let context = Self::context(index)?;
        let pending = self.resources.register(context);
        tokio::pin!(pending);
        let mut ready = None;
        poll_fn(|cx| {
            if let Poll::Ready(result) = pending.as_mut().poll(cx) {
                ready = Some(result);
            }
            Poll::Ready(())
        })
        .await;
        if ready.is_none() {
            for existing in self.contexts.clone() {
                let update = self.resources.next_update(existing).await?;
                self.resources
                    .acknowledge(existing, update.config())
                    .await?;
            }
        }
        let registration = match ready {
            Some(result) => result?,
            None => pending.await?,
        };
        self.registrations.push(registration);
        self.contexts.push(context);
        let allocation = self.resources.allocate(context).await?;
        self.resources
            .acknowledge(context, allocation.config())
            .await?;
        Ok(())
    }

    async fn simultaneous(
        &self,
    ) -> CapacityConcurrentGamesResult<Vec<DesktopNativeGameEngineAllocation>> {
        let blocker = self.budget.lease(self.budget.workers()).await?;
        let mut pending = Vec::new();
        for context in &self.contexts {
            pending.push(Box::pin(self.resources.allocate(*context)));
        }
        poll_fn(|cx| {
            for future in &mut pending {
                assert!(future.as_mut().poll(cx).is_pending());
            }
            Poll::Ready(())
        })
        .await;
        drop(blocker);
        let mut ready: Vec<_> = pending.iter().map(|_future| None).collect();
        poll_fn(|cx| {
            for (future, slot) in pending.iter_mut().zip(ready.iter_mut()) {
                if slot.is_none()
                    && let Poll::Ready(result) = future.as_mut().poll(cx)
                {
                    *slot = Some(result);
                }
            }
            if ready.iter().all(Option::is_some) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        ready
            .into_iter()
            .map(|slot| Ok(slot.ok_or("missing simultaneous allocation")??))
            .collect()
    }
}

#[tokio::test(start_paused = true)]
async fn three_games_split_seven_threads_with_the_remainder_rotating_through_every_game()
-> CapacityConcurrentGamesResult {
    let mut fixture = CapacityConcurrentGamesFixture::new(7, 64)?;
    for index in 1..=3 {
        fixture.admit(index).await?;
    }
    let mut recipients = Vec::new();
    for _round in 0..3 {
        let allocations = fixture.simultaneous().await?;
        let workers: Vec<_> = allocations.iter().map(|value| value.workers()).collect();
        let mut sorted = workers.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, [2, 2, 3]);
        assert_eq!(fixture.budget.available(), 0);
        recipients.push(
            workers
                .iter()
                .position(|count| *count == 3)
                .ok_or("missing remainder holder")?,
        );
        drop(allocations);
        assert_eq!(fixture.budget.available(), 7);
    }
    recipients.sort_unstable();
    assert_eq!(recipients, [0, 1, 2]);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_exact_multiple_has_no_remainder_and_every_game_keeps_a_whole_share()
-> CapacityConcurrentGamesResult {
    let mut fixture = CapacityConcurrentGamesFixture::new(6, 64)?;
    for index in 1..=3 {
        fixture.admit(index).await?;
    }
    for _round in 0..2 {
        let allocations = fixture.simultaneous().await?;
        assert!(allocations.iter().all(|value| value.workers() == 2));
        assert_eq!(fixture.budget.available(), 0);
        drop(allocations);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_lone_search_borrows_the_idle_reservations_of_human_turn_games()
-> CapacityConcurrentGamesResult {
    let mut fixture = CapacityConcurrentGamesFixture::new(7, 64)?;
    for index in 1..=3 {
        fixture.admit(index).await?;
    }
    let lone = fixture
        .resources
        .allocate(CapacityConcurrentGamesFixture::context(1)?)
        .await?;
    assert_eq!(lone.workers(), 7);
    assert_eq!(fixture.budget.available(), 0);
    assert!(fixture.budget.try_lease(1).is_err());
    drop(lone);
    assert_eq!(fixture.budget.available(), 7);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn no_rotation_round_ever_leases_more_than_the_budget() -> CapacityConcurrentGamesResult {
    let mut fixture = CapacityConcurrentGamesFixture::new(5, 64)?;
    for index in 1..=2 {
        fixture.admit(index).await?;
    }
    for _round in 0..6 {
        let allocations = fixture.simultaneous().await?;
        let leased: usize = allocations.iter().map(|value| value.workers()).sum();
        assert_eq!(leased + fixture.budget.available(), 5);
        assert!(allocations.iter().all(|value| value.workers() >= 1));
        drop(allocations);
        assert_eq!(fixture.budget.available(), 5);
    }
    Ok(())
}
