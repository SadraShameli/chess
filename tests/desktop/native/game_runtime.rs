use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::task::{Context, Wake, Waker};
use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::capacity::phase::CapacityPhase;
use chess::desktop::error::DesktopFailure;
use chess::desktop::game_bridge::DesktopGameBridgeProvider;
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::game_registry::{DesktopGameRegistry, DesktopGameRegistryOptions};
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::native::game_runtime::{
    DesktopNativeGameRuntime, DesktopNativeGameRuntimeError, DesktopNativeGameRuntimePort,
};
use chess::game::enums::GameEnumPhase;
use chess::game::storage_repository::GameStorageRepository;
use chess::storage::library_played::StorageLibraryPlayedRepository;
use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::{Notify, watch};
use tokio::time::Instant;

use crate::desktop_game_registry_fixture::{
    DesktopGameRegistryFixture, DesktopGameRegistryFixtureClock,
    DesktopGameRegistryFixtureFactories, DesktopGameRegistryFixtureGate,
    DesktopGameRegistryFixtureProof, DesktopGameRegistryFixtureRuntime,
    DesktopGameRegistryTestResult,
};
use crate::desktop_game_supervisor_storage::DesktopGameSupervisorFixtureStorage;

struct DesktopNativeGameRuntimeFixture;

impl DesktopNativeGameRuntimeFixture {
    async fn wait_snapshot(
        snapshots: &mut watch::Receiver<Vec<DesktopGameSnapshot>>,
        accepts: impl Fn(&DesktopGameSnapshot) -> bool,
    ) -> Result<DesktopGameSnapshot, Box<dyn std::error::Error>> {
        Ok(tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let accepted = snapshots
                    .borrow()
                    .iter()
                    .find(|snapshot| accepts(snapshot))
                    .cloned();
                if let Some(snapshot) = accepted {
                    return Ok::<_, watch::error::RecvError>(snapshot);
                }
                snapshots.changed().await?;
            }
        })
        .await??)
    }

    fn failure(error: DesktopFailure) -> std::io::Error {
        std::io::Error::other(error.message().to_owned())
    }

    async fn wait_failure(
        failures: &mut watch::Receiver<Option<Arc<DesktopNativeGameRuntimeError>>>,
    ) -> Result<Arc<DesktopNativeGameRuntimeError>, Box<dyn std::error::Error>> {
        Ok(tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let failure = failures.borrow().clone();
                if let Some(failure) = failure {
                    return Ok::<_, watch::error::RecvError>(failure);
                }
                failures.changed().await?;
            }
        })
        .await??)
    }
}

#[tokio::test]
async fn quit_save_failure_keeps_snapshot_forwarding_alive_until_explicit_recovery()
-> DesktopGameRegistryTestResult {
    let fixture = DesktopGameSupervisorFixtureStorage::new(CapacityPhase::MINIMUM_WORKERS).await?;
    let (registry, registry_port) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
        factory_provider: DesktopGameRegistryFixtureFactories::default(),
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::new(DesktopGameRegistryFixtureClock::default()),
        wall_time: crate::game_storage_wall_time_fixture::GameStorageWallTimeFixture::shared(),
        proof: Arc::new(DesktopGameRegistryFixtureProof::default()),
        random: StdRng::seed_from_u64(81),
    });
    let (mut runtime, port) = DesktopNativeGameRuntime::new(registry, registry_port);
    let worker = tokio::spawn(async move {
        let result = runtime.run().await;
        (runtime, result)
    });
    let created = port
        .execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameRuntimeFixture::failure)?;
    let mut snapshots = port.snapshots();
    fixture.set_save_failure(true).await?;
    assert!(
        port.shutdown(Instant::now() + Duration::from_secs(3))
            .await
            .is_err()
    );
    assert!(!port.is_shutdown_complete());
    let interrupted = DesktopNativeGameRuntimeFixture::wait_snapshot(&mut snapshots, |snapshot| {
        snapshot.id() == created.id() && snapshot.is_save_required()
    })
    .await?;
    assert_eq!(interrupted.phase(), GameEnumPhase::Interrupted);
    assert!(!interrupted.clock().is_running());
    fixture.set_save_failure(false).await?;
    let saved = port
        .execute(DesktopGameRegistryFixture::session(
            &interrupted,
            DesktopGameCommandsEnumAction::RetrySave,
        ))
        .await
        .map_err(DesktopNativeGameRuntimeFixture::failure)?;
    assert!(!saved.is_save_required());
    let published = DesktopNativeGameRuntimeFixture::wait_snapshot(&mut snapshots, |snapshot| {
        snapshot.id() == created.id()
            && snapshot.revision() == saved.revision()
            && !snapshot.is_save_required()
    })
    .await?;
    assert!(!published.clock().is_running());
    port.shutdown(Instant::now() + Duration::from_secs(3))
        .await?;
    let (runtime, result) = worker.await?;
    result?;
    assert!(runtime.is_shutdown_complete());
    let repository = StorageLibraryPlayedRepository::new(fixture.store.client(), &fixture.executor);
    let restored = repository
        .load(created.id())
        .await?
        .ok_or("missing recovered quit save")?;
    assert_eq!(restored.session().revision(), published.revision());
    assert!(!restored.session().clock().is_running());
    drop((runtime, port, snapshots, repository));
    fixture.close().await
}

struct DesktopNativeGameRuntimeRetryObserver {
    port: DesktopNativeGameRuntimePort,
    observed: AtomicBool,
    admitted: AtomicBool,
    notified: Notify,
}

impl Wake for DesktopNativeGameRuntimeRetryObserver {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if self.observed.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut retry = Box::pin(self.port.shutdown(Instant::now() + Duration::from_secs(3)));
        let mut context = Context::from_waker(Waker::noop());
        self.admitted.store(
            retry.as_mut().poll(&mut context).is_pending(),
            Ordering::Release,
        );
        self.notified.notify_one();
    }
}

#[tokio::test]
async fn failed_shutdown_releases_its_reservation_before_notifying_a_retry_observer()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let (registry, registry_port) = fixture.registry();
    let (mut runtime, port) = DesktopNativeGameRuntime::new(registry, registry_port);
    let mut failures = port.failures();
    let worker = tokio::spawn(async move {
        let result = runtime.run().await;
        (runtime, result)
    });
    port.execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameRuntimeFixture::failure)?;
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    let observer = Arc::new(DesktopNativeGameRuntimeRetryObserver {
        port: port.clone(),
        observed: AtomicBool::new(false),
        admitted: AtomicBool::new(false),
        notified: Notify::new(),
    });
    let waker = Waker::from(Arc::clone(&observer));
    let mut changed = Box::pin(failures.changed());
    assert!(
        changed
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    assert!(port.shutdown(Instant::now()).await.is_err());
    tokio::time::timeout(Duration::from_secs(3), observer.notified.notified()).await?;
    let admitted = observer.admitted.load(Ordering::Acquire);
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    if !admitted {
        port.shutdown(Instant::now() + Duration::from_secs(3))
            .await?;
    }
    let (runtime, result) = tokio::time::timeout(Duration::from_secs(3), worker).await??;
    result?;
    assert!(runtime.is_shutdown_complete());
    drop(changed);
    drop((runtime, port, waker, observer, failures, phase));
    fixture.close().await?;
    assert!(
        admitted,
        "a completed shutdown reservation rejected observer retry"
    );
    Ok(())
}

#[tokio::test]
async fn cancelling_a_shutdown_waiter_keeps_its_reservation_until_owned_cleanup_completes()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let (registry, registry_port) = fixture.registry();
    let (mut runtime, port) = DesktopNativeGameRuntime::new(registry, registry_port);
    let worker = tokio::spawn(async move {
        let result = runtime.run().await;
        (runtime, result)
    });
    port.execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameRuntimeFixture::failure)?;
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    let mut shutdown = Box::pin(port.shutdown(Instant::now() + Duration::from_secs(3)));
    assert!(
        shutdown
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(shutdown);
    let competing = port.shutdown(Instant::now() + Duration::from_secs(3)).await;
    assert!(
        matches!(competing, Err(ref error) if matches!(error.as_ref(), DesktopNativeGameRuntimeError::Busy))
    );
    assert!(!port.is_shutdown_complete());
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    let (runtime, result) = tokio::time::timeout(Duration::from_secs(3), worker).await??;
    result?;
    assert!(runtime.is_shutdown_complete());
    drop((runtime, port, phase));
    fixture.close().await
}

#[tokio::test]
async fn timed_out_shutdown_retains_an_accepted_move_and_retries_until_it_is_durable()
-> DesktopGameRegistryTestResult {
    let gate = Arc::new(DesktopGameRegistryFixtureGate::default());
    let fixture = DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof(Some(
        Arc::clone(&gate),
    )))
    .await?;
    let (registry, registry_port) = fixture.registry();
    let (mut runtime, port) = DesktopNativeGameRuntime::new(registry, registry_port);
    let mut failures = port.failures();
    let worker = tokio::spawn(async move {
        let result = runtime.run().await;
        (runtime, result)
    });
    let first = port
        .execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameRuntimeFixture::failure)?;
    gate.armed.store(true, Ordering::SeqCst);
    let movement = tokio::spawn(port.execute(DesktopGameRegistryFixture::session(
        &first,
        DesktopGameCommandsEnumAction::Play {
            node: "840b70e2-5b46-4876-a336-245e1faec992".parse()?,
            origin: "e2".parse()?,
            destination: "e4".parse()?,
            promotion: None,
            color: BoardEnumColor::White,
        },
    )));
    gate.entered.notified().await;
    let shutdown = port.shutdown(Instant::now()).await;
    assert!(shutdown.is_err());
    DesktopNativeGameRuntimeFixture::wait_failure(&mut failures).await?;
    assert!(!port.is_shutdown_complete());
    assert!(!worker.is_finished());
    gate.release()?;
    let moved = movement
        .await?
        .map_err(DesktopNativeGameRuntimeFixture::failure)?;
    assert_eq!(moved.live_turn(), BoardEnumColor::Black);
    port.shutdown(Instant::now() + Duration::from_secs(3))
        .await?;
    let (runtime, result) = worker.await?;
    result?;
    assert!(runtime.is_shutdown_complete());
    assert!(port.is_shutdown_complete());
    let repository = StorageLibraryPlayedRepository::new(fixture.store.client(), &fixture.executor);
    let restored = repository
        .load(first.id())
        .await?
        .ok_or("missing durable runtime game")?;
    assert_eq!(restored.session().phase(), GameEnumPhase::Interrupted);
    assert_eq!(restored.session().tree().len(), 2);
    assert!(!restored.session().clock().is_running());
    port.shutdown(Instant::now()).await?;
    drop((runtime, port, failures, repository));
    fixture.close().await
}

#[tokio::test]
async fn shutdown_preserves_pending_creation_and_finishes_it_without_starting_after_closure()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let (registry, registry_port) = fixture.registry();
    let (mut runtime, port) = DesktopNativeGameRuntime::new(registry, registry_port);
    let mut failures = port.failures();
    let worker = tokio::spawn(async move {
        let result = runtime.run().await;
        (runtime, result)
    });
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    let creation = tokio::spawn(port.execute(DesktopGameRegistryFixture::create()));
    fixture.admitted.notified().await;
    assert!(port.shutdown(Instant::now()).await.is_err());
    DesktopNativeGameRuntimeFixture::wait_failure(&mut failures).await?;
    assert!(!port.is_shutdown_complete());
    assert!(!creation.is_finished());
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    port.shutdown(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(creation.await?.is_err());
    let (runtime, result) = worker.await?;
    result?;
    assert!(runtime.is_shutdown_complete());
    assert!(port.snapshots().borrow().iter().all(|snapshot| {
        snapshot.phase() != GameEnumPhase::Active && !snapshot.clock().is_running()
    }));
    drop((runtime, port, failures, phase));
    fixture.close().await
}

#[tokio::test]
async fn registry_failure_keeps_the_independent_cleanup_port_alive_and_preserves_the_error()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let (registry, registry_port) = fixture.registry();
    let (mut runtime, port) = DesktopNativeGameRuntime::new(registry, registry_port);
    let mut failures = port.failures();
    let worker = tokio::spawn(async move {
        let result = runtime.run().await;
        (runtime, result)
    });
    port.execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameRuntimeFixture::failure)?;
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    assert!(
        port.registry_port()
            .shutdown_all(Instant::now())?
            .wait()
            .await
            .is_err()
    );
    let failure = DesktopNativeGameRuntimeFixture::wait_failure(&mut failures).await?;
    assert!(matches!(
        failure.as_ref(),
        DesktopNativeGameRuntimeError::Registry(_)
    ));
    assert!(!worker.is_finished());
    assert!(!port.is_shutdown_complete());
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    port.shutdown(Instant::now() + Duration::from_secs(3))
        .await?;
    let (runtime, result) = worker.await?;
    assert!(matches!(result, Err(ref error) if Arc::ptr_eq(error, &failure)));
    assert!(runtime.is_shutdown_complete());
    drop((runtime, port, failures, phase));
    fixture.close().await
}
