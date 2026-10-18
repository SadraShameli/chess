use std::future::poll_fn;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::Poll;

use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockDuration;
use chess::desktop::engine_commands::{
    DesktopEngineCommandsAdmissionProvider, DesktopEngineCommandsEnumWorkload,
    DesktopEngineCommandsResourceRequest,
};
use chess::desktop::messages::DesktopMessagesEntityId;
use chess::desktop::native::analysis_resources::DesktopNativeAnalysisResources;
use chess::desktop::native::game_engine::DesktopNativeGameEngineResources;
use chess::desktop::native::game_resources::{
    DesktopNativeGameResources, DesktopNativeGameResourcesMemoryProvider,
    DesktopNativeGameResourcesOptions, DesktopNativeGameResourcesPolicy,
};
use chess::engine::package::manifest::EnginePackageManifest;
use chess::engine::resource_config::EngineResourceConfig;
use chess::game::engine_provider::GameEngineContext;
use chess::game::enums::GameRevision;

type DesktopNativeAnalysisResourcesTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct DesktopNativeAnalysisResourcesFixtureMemory {
    bytes: AtomicU64,
    failed: AtomicBool,
}

impl DesktopNativeGameResourcesMemoryProvider for DesktopNativeAnalysisResourcesFixtureMemory {
    type Error = std::io::Error;

    async fn available_bytes(&self) -> Result<u64, Self::Error> {
        if self.failed.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("Memory observation failed"));
        }
        Ok(self.bytes.load(Ordering::SeqCst))
    }
}

struct DesktopNativeAnalysisResourcesFixture {
    canonical: Arc<DesktopNativeGameResources<DesktopNativeAnalysisResourcesFixtureMemory>>,
    analysis: DesktopNativeAnalysisResources<DesktopNativeAnalysisResourcesFixtureMemory>,
    budget: CapacityBudget,
    memory: Arc<DesktopNativeAnalysisResourcesFixtureMemory>,
}

impl DesktopNativeAnalysisResourcesFixture {
    fn new(workers: usize, hash_mib: u32) -> DesktopNativeAnalysisResourcesTestResult<Self> {
        let budget = CapacityBudget::new(NonZeroUsize::new(workers).ok_or("Empty capacity")?)?;
        let memory = Arc::new(DesktopNativeAnalysisResourcesFixtureMemory {
            bytes: AtomicU64::new(64 * 1024 * 1024 * 1024),
            failed: AtomicBool::new(false),
        });
        let canonical = Arc::new(DesktopNativeGameResources::new(
            DesktopNativeGameResourcesOptions {
                budget: budget.clone(),
                policy: DesktopNativeGameResourcesPolicy::new(workers, hash_mib)?,
                memory: Arc::clone(&memory),
            },
        )?);
        Ok(Self {
            analysis: DesktopNativeAnalysisResources::new(
                DesktopMessagesEntityId::new(),
                Arc::clone(&canonical),
            ),
            canonical,
            budget,
            memory,
        })
    }

    fn config(
        workers: usize,
        hash_mib: u32,
    ) -> DesktopNativeAnalysisResourcesTestResult<EngineResourceConfig> {
        Ok(EngineResourceConfig::new(
            workers,
            hash_mib,
            ClockDuration::from_millis(100)?,
        )?)
    }

    fn request(
        workload: DesktopEngineCommandsEnumWorkload,
        config: EngineResourceConfig,
    ) -> DesktopEngineCommandsResourceRequest {
        DesktopEngineCommandsResourceRequest::new(
            workload,
            config,
            match workload {
                DesktopEngineCommandsEnumWorkload::Retry
                | DesktopEngineCommandsEnumWorkload::Diagnostic => Some(16),
                DesktopEngineCommandsEnumWorkload::Analysis
                | DesktopEngineCommandsEnumWorkload::Configuration => None,
            },
        )
    }

    fn game() -> DesktopNativeAnalysisResourcesTestResult<GameEngineContext> {
        Ok(GameEngineContext::new(
            "123e4567-e89b-42d3-a456-426614174000".parse()?,
            GameRevision::ZERO,
        ))
    }
}

#[tokio::test]
async fn startup_peak_and_idle_configured_hash_share_the_canonical_game_budget()
-> DesktopNativeAnalysisResourcesTestResult {
    let mut fixture = DesktopNativeAnalysisResourcesFixture::new(2, 24)?;
    let (lease, reservation) = fixture
        .analysis
        .initial(DesktopNativeAnalysisResourcesFixture::config(1, 4)?)
        .await?;
    assert_eq!(fixture.budget.available(), 1);
    let game = DesktopNativeAnalysisResourcesFixture::game()?;
    assert!(fixture.canonical.register(game).await.is_err());
    fixture.analysis.acknowledge(&reservation).await?;
    drop(lease);
    let registration = fixture.canonical.register(game).await?;
    let allocation = fixture.canonical.allocate(game).await?;
    assert_eq!(allocation.config().hash_mib(), 20);
    fixture
        .canonical
        .acknowledge(game, allocation.config())
        .await?;
    drop(allocation);
    assert!(
        fixture
            .analysis
            .reserve(DesktopNativeAnalysisResourcesFixture::request(
                DesktopEngineCommandsEnumWorkload::Diagnostic,
                DesktopNativeAnalysisResourcesFixture::config(1, 24)?
            ))
            .await
            .is_err()
    );
    drop(registration);
    let diagnostic = fixture
        .analysis
        .reserve(DesktopNativeAnalysisResourcesFixture::request(
            DesktopEngineCommandsEnumWorkload::Diagnostic,
            DesktopNativeAnalysisResourcesFixture::config(1, 1)?,
        ))
        .await?;
    drop(diagnostic);
    drop(reservation);
    let registration = fixture.canonical.register(game).await?;
    let allocation = fixture.canonical.allocate(game).await?;
    assert_eq!(allocation.config().hash_mib(), 24);
    drop(allocation);
    drop(registration);
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[tokio::test]
async fn process_memory_allowances_cover_the_idle_owner_and_separate_diagnostic_child()
-> DesktopNativeAnalysisResourcesTestResult {
    let mut fixture = DesktopNativeAnalysisResourcesFixture::new(2, 64)?;
    let policy = fixture.canonical.policy();
    let per_process = EnginePackageManifest::NETWORK_BYTES
        + DesktopNativeGameResourcesPolicy::PROCESS_ALLOWANCE_BYTES;
    let required = u64::from(policy.hash_mib()) * DesktopNativeGameResourcesPolicy::MIB
        + per_process
        + DesktopNativeGameResourcesPolicy::HEADROOM_BYTES;
    let config = DesktopNativeAnalysisResourcesFixture::config(1, 16)?;
    fixture.memory.bytes.store(required - 1, Ordering::SeqCst);
    assert!(fixture.analysis.initial(config).await.is_err());
    fixture.memory.bytes.store(required, Ordering::SeqCst);
    let (lease, reservation) = fixture.analysis.initial(config).await?;
    fixture.analysis.acknowledge(&reservation).await?;
    drop(lease);
    let request = DesktopNativeAnalysisResourcesFixture::request(
        DesktopEngineCommandsEnumWorkload::Diagnostic,
        config,
    );
    assert!(fixture.analysis.reserve(request).await.is_err());
    fixture
        .memory
        .bytes
        .store(required + per_process, Ordering::SeqCst);
    let diagnostic = fixture.analysis.reserve(request).await?;
    assert_eq!(fixture.budget.available(), 1);
    drop(diagnostic);
    drop(reservation);
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[tokio::test]
async fn configured_hash_replacement_waits_for_acknowledgment_and_retired_owner_release()
-> DesktopNativeAnalysisResourcesTestResult {
    let mut fixture = DesktopNativeAnalysisResourcesFixture::new(2, 64)?;
    let (lease, previous) = fixture
        .analysis
        .initial(DesktopNativeAnalysisResourcesFixture::config(1, 32)?)
        .await?;
    fixture.analysis.acknowledge(&previous).await?;
    drop(lease);
    let (lease, replacement) = fixture
        .analysis
        .reserve(DesktopNativeAnalysisResourcesFixture::request(
            DesktopEngineCommandsEnumWorkload::Configuration,
            DesktopNativeAnalysisResourcesFixture::config(1, 8)?,
        ))
        .await?;
    let mut other = DesktopNativeAnalysisResources::new(
        DesktopMessagesEntityId::new(),
        Arc::clone(&fixture.canonical),
    );
    assert!(other.acknowledge(&replacement).await.is_err());
    fixture.analysis.acknowledge(&replacement).await?;
    drop(lease);
    assert!(
        other
            .initial(DesktopNativeAnalysisResourcesFixture::config(1, 40)?)
            .await
            .is_err()
    );
    drop(previous);
    let other_allocation = other
        .initial(DesktopNativeAnalysisResourcesFixture::config(1, 40)?)
        .await?;
    drop(other_allocation);
    let search = fixture
        .analysis
        .reserve(DesktopNativeAnalysisResourcesFixture::request(
            DesktopEngineCommandsEnumWorkload::Analysis,
            DesktopNativeAnalysisResourcesFixture::config(1, 8)?,
        ))
        .await?;
    assert!(
        fixture
            .analysis
            .reserve(DesktopNativeAnalysisResourcesFixture::request(
                DesktopEngineCommandsEnumWorkload::Analysis,
                DesktopNativeAnalysisResourcesFixture::config(1, 32)?
            ))
            .await
            .is_err()
    );
    drop(search);
    drop(replacement);
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[tokio::test]
async fn failed_observation_pressure_cpu_and_forged_startup_hints_refuse_without_leaks()
-> DesktopNativeAnalysisResourcesTestResult {
    let fixture = DesktopNativeAnalysisResourcesFixture::new(1, 16)?;
    let config = DesktopNativeAnalysisResourcesFixture::config(1, 1)?;
    for hint in [None, Some(0), Some(1), Some(17)] {
        assert!(
            fixture
                .analysis
                .reserve(DesktopEngineCommandsResourceRequest::new(
                    DesktopEngineCommandsEnumWorkload::Retry,
                    config,
                    hint
                ))
                .await
                .is_err()
        );
    }
    fixture.memory.failed.store(true, Ordering::SeqCst);
    assert!(fixture.analysis.initial(config).await.is_err());
    fixture.memory.failed.store(false, Ordering::SeqCst);
    fixture.memory.bytes.store(0, Ordering::SeqCst);
    assert!(fixture.analysis.initial(config).await.is_err());
    fixture
        .memory
        .bytes
        .store(64 * 1024 * 1024 * 1024, Ordering::SeqCst);
    let blocker = fixture.budget.try_lease(1)?;
    assert!(fixture.analysis.initial(config).await.is_err());
    drop(blocker);
    let allocation = fixture.analysis.initial(config).await?;
    drop(allocation);
    assert_eq!(fixture.budget.available(), 1);
    Ok(())
}

#[tokio::test]
async fn live_search_demand_wakes_preemption_and_keeps_cpu_until_observed_stop()
-> DesktopNativeAnalysisResourcesTestResult {
    let mut fixture = DesktopNativeAnalysisResourcesFixture::new(2, 64)?;
    let config = DesktopNativeAnalysisResourcesFixture::config(2, 16)?;
    let (lease, reservation) = fixture.analysis.initial(config).await?;
    fixture.analysis.acknowledge(&reservation).await?;
    drop(lease);
    let game = DesktopNativeAnalysisResourcesFixture::game()?;
    let registration = fixture.canonical.register(game).await?;
    let allocation = fixture.canonical.allocate(game).await?;
    fixture
        .canonical
        .acknowledge(game, allocation.config())
        .await?;
    drop(allocation);
    let search = fixture
        .analysis
        .reserve(DesktopNativeAnalysisResourcesFixture::request(
            DesktopEngineCommandsEnumWorkload::Analysis,
            config,
        ))
        .await?;
    let mut demand = fixture.analysis.live_demand();
    let live = fixture.canonical.allocate(game);
    tokio::pin!(live);
    poll_fn(|cx| {
        assert!(live.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    demand.changed().await?;
    assert_eq!(
        demand.borrow().as_ref().map(|context| context.game()),
        Some(game.game())
    );
    assert_eq!(fixture.budget.available(), 0);
    assert!(
        fixture
            .analysis
            .reserve(DesktopNativeAnalysisResourcesFixture::request(
                DesktopEngineCommandsEnumWorkload::Diagnostic,
                config
            ))
            .await
            .is_err()
    );
    drop(search);
    let live = live.await?;
    assert_eq!(live.workers(), 2);
    drop(live);
    drop(registration);
    drop(reservation);
    Ok(())
}

#[tokio::test]
async fn canceled_hash_rebalance_releases_pending_reservation_and_wakes_live_owner()
-> DesktopNativeAnalysisResourcesTestResult {
    let fixture = DesktopNativeAnalysisResourcesFixture::new(2, 64)?;
    let game = DesktopNativeAnalysisResourcesFixture::game()?;
    let registration = fixture.canonical.register(game).await?;
    let allocation = fixture.canonical.allocate(game).await?;
    fixture
        .canonical
        .acknowledge(game, allocation.config())
        .await?;
    drop(allocation);
    {
        let pending = fixture
            .analysis
            .initial(DesktopNativeAnalysisResourcesFixture::config(1, 16)?);
        tokio::pin!(pending);
        poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let shrink = fixture.canonical.next_update(game).await?;
        assert_eq!(shrink.config().hash_mib(), 48);
        fixture.canonical.acknowledge(game, shrink.config()).await?;
        drop(shrink);
    }
    let expand = fixture.canonical.next_update(game).await?;
    assert_eq!(expand.config().hash_mib(), 64);
    drop(expand);
    drop(registration);
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}
