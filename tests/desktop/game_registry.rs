use std::num::NonZeroUsize;
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::desktop::game_bridge::DesktopGameBridgeProvider;
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryError, DesktopGameRegistryOptions,
};
use chess::game::enums::GameEnumPhase;
use chess::game::storage_repository::GameStorageRepository;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::game_document::StorageGameDocumentRepository;
use rand::{SeedableRng, rngs::StdRng};
use tokio::time::Instant;

use crate::desktop_game_registry_fixture::{
    DesktopGameRegistryFixture, DesktopGameRegistryFixtureClock,
    DesktopGameRegistryFixtureFactories, DesktopGameRegistryFixtureGate,
    DesktopGameRegistryFixtureProof, DesktopGameRegistryFixtureRuntime,
    DesktopGameRegistryTestResult,
};
use crate::desktop_game_supervisor::DesktopGameSupervisorFixtureStorage;

#[derive(Clone, Copy)]
enum DesktopGameRegistryEnumSaveRecovery {
    RetrySave,
    RetryQuit,
}

#[tokio::test]
async fn failed_quit_retains_explicit_save_and_quit_recovery_without_losing_the_latest_move()
-> DesktopGameRegistryTestResult {
    for recovery in [
        DesktopGameRegistryEnumSaveRecovery::RetrySave,
        DesktopGameRegistryEnumSaveRecovery::RetryQuit,
    ] {
        let fixture = DesktopGameSupervisorFixtureStorage::new(4).await?;
        let (mut registry, port) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
            factory_provider: DesktopGameRegistryFixtureFactories::default(),
            storage: fixture.store.client(),
            executor: Arc::clone(&fixture.executor),
            clock: Arc::new(DesktopGameRegistryFixtureClock::default()),
            proof: Arc::new(DesktopGameRegistryFixtureProof::default()),
            random: StdRng::seed_from_u64(81),
        });
        let worker = tokio::spawn(async move {
            let result = registry.run().await;
            (registry, result)
        });
        let first = port
            .execute(DesktopGameRegistryFixture::create())
            .await
            .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
        fixture.set_save_failure(true).await?;
        assert!(
            port.execute(DesktopGameRegistryFixture::session(
                &first,
                DesktopGameCommandsEnumAction::Play {
                    node: "78042ad8-a09e-4500-b12d-d66f41a69668".parse()?,
                    origin: "e2".parse()?,
                    destination: "e4".parse()?,
                    promotion: None,
                    color: BoardEnumColor::White,
                },
            ))
            .await
            .is_err()
        );
        let mut snapshots = port.snapshots();
        let interrupted = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let snapshot = snapshots.borrow()[0].clone();
                if snapshot.is_save_required() {
                    break Ok::<_, tokio::sync::watch::error::RecvError>(snapshot);
                }
                snapshots.changed().await?;
            }
        })
        .await??;
        assert!(
            port.shutdown_all(Instant::now() + Duration::from_secs(3))?
                .wait()
                .await
                .is_err()
        );
        let (mut registry, result) = worker.await?;
        assert!(result.is_err());
        assert!(!registry.is_shutdown_complete());
        assert!(
            port.execute(DesktopGameRegistryFixture::create())
                .await
                .is_err()
        );
        fixture.set_save_failure(false).await?;
        match recovery {
            DesktopGameRegistryEnumSaveRecovery::RetrySave => {
                let saved = port
                    .execute(DesktopGameRegistryFixture::session(
                        &interrupted,
                        DesktopGameCommandsEnumAction::RetrySave,
                    ))
                    .await
                    .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
                assert!(!saved.is_save_required());
            }
            DesktopGameRegistryEnumSaveRecovery::RetryQuit => {}
        }
        registry
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await?;
        assert!(registry.is_shutdown_complete());
        let repository =
            StorageGameDocumentRepository::new(fixture.store.client(), &fixture.executor);
        let restored = repository
            .load(first.id())
            .await?
            .ok_or("missing recovered game")?;
        assert_eq!(restored.session().tree().len(), 2);
        assert_eq!(restored.session().revision(), interrupted.revision());
        assert!(!restored.session().clock().is_running());
        drop((registry, port, snapshots, repository));
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn independent_local_games_route_moves_publish_snapshots_and_shutdown_durably()
-> DesktopGameRegistryTestResult {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(4).ok_or("invalid capacity")?)?;
    let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    let (mut registry, port) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
        factory_provider: DesktopGameRegistryFixtureFactories::default(),
        storage: store.client(),
        executor: Arc::clone(&executor),
        clock: Arc::new(DesktopGameRegistryFixtureClock::default()),
        proof: Arc::new(DesktopGameRegistryFixtureProof::default()),
        random: StdRng::seed_from_u64(81),
    });
    let worker = tokio::spawn(async move {
        let result = registry.run().await;
        (registry, result)
    });
    let mut snapshots = port.snapshots();
    let (first, second) = tokio::join!(
        port.execute(DesktopGameRegistryFixture::create()),
        port.execute(DesktopGameRegistryFixture::create())
    );
    let first = first.map_err(|error| std::io::Error::other(error.message().to_owned()))?;
    let second = second.map_err(|error| std::io::Error::other(error.message().to_owned()))?;
    assert_ne!(first.id(), second.id());
    assert_eq!(first.phase(), GameEnumPhase::Active);
    assert_eq!(second.phase(), GameEnumPhase::Active);
    let moved = port
        .execute(DesktopGameRegistryFixture::session(
            &first,
            DesktopGameCommandsEnumAction::Play {
                node: "cb1c91bf-1e75-4ad8-a5c6-dd3f96146c50".parse()?,
                origin: "e2".parse()?,
                destination: "e4".parse()?,
                promotion: None,
                color: BoardEnumColor::White,
            },
        ))
        .await
        .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
    assert_eq!(moved.live_turn(), BoardEnumColor::Black);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if snapshots.borrow().iter().any(|snapshot| {
                snapshot.id() == first.id() && snapshot.revision() == moved.revision()
            }) {
                break;
            }
            snapshots.changed().await?;
        }
        Ok::<_, tokio::sync::watch::error::RecvError>(())
    })
    .await??;
    assert_eq!(
        snapshots
            .borrow()
            .iter()
            .find(|snapshot| snapshot.id() == second.id())
            .ok_or("missing second game")?
            .live_turn(),
        BoardEnumColor::White
    );
    port.shutdown_all(Instant::now() + Duration::from_secs(3))?
        .wait()
        .await?;
    let (registry, result) = worker.await?;
    result?;
    assert!(registry.is_shutdown_complete());
    let repository = StorageGameDocumentRepository::new(store.client(), &executor);
    for game in [first.id(), second.id()] {
        let restored = repository
            .load(game)
            .await?
            .ok_or("missing saved registry game")?;
        assert_eq!(restored.session().phase(), GameEnumPhase::Interrupted);
        assert!(!restored.session().clock().is_running());
    }
    drop((registry, port, snapshots, repository));
    store.close().await?;
    Arc::try_unwrap(executor)
        .map_err(|_owner| "executor still borrowed")?
        .shutdown()
        .await?;
    Ok(())
}

#[tokio::test]
async fn shutdown_rebases_quit_after_an_inflight_move_changes_the_game_revision()
-> DesktopGameRegistryTestResult {
    let gate = Arc::new(DesktopGameRegistryFixtureGate::default());
    let fixture = DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof(Some(
        Arc::clone(&gate),
    )))
    .await?;
    let (mut registry, port) = fixture.registry();
    let worker = tokio::spawn(async move {
        let result = registry.run().await;
        (registry, result)
    });
    let first = port
        .execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
    gate.armed.store(true, Ordering::SeqCst);
    let movement = tokio::spawn(port.execute(DesktopGameRegistryFixture::session(
        &first,
        DesktopGameCommandsEnumAction::Play {
            node: "6dc258a4-1f27-47c7-9d45-a5bc1d4c8065".parse()?,
            origin: "e2".parse()?,
            destination: "e4".parse()?,
            promotion: None,
            color: BoardEnumColor::White,
        },
    )));
    gate.entered.notified().await;
    let deadline = Instant::now() + Duration::from_secs(3);
    let shutdown = port.shutdown_all(deadline)?;
    let entered = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match port.shutdown_all(deadline) {
                Err(DesktopGameRegistryError::Closed) => break,
                Err(DesktopGameRegistryError::Busy) => tokio::task::yield_now().await,
                Err(error) => return Err(error),
                Ok(_) => return Err(DesktopGameRegistryError::MissingCreation),
            }
        }
        Ok(())
    })
    .await;
    gate.release()?;
    entered??;
    let moved = movement
        .await?
        .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
    assert_eq!(moved.live_turn(), BoardEnumColor::Black);
    let shutdown_result = shutdown.wait().await;
    let (mut registry, result) = worker.await?;
    if !registry.is_shutdown_complete() {
        registry
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await?;
    }
    let succeeded = shutdown_result.is_ok() && result.is_ok();
    drop((registry, port));
    fixture.close().await?;
    assert!(
        succeeded,
        "shutdown failed after a racing accepted move: {shutdown_result:?}, {result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn shutdown_completes_queued_reserved_controls_before_returning_its_owner()
-> DesktopGameRegistryTestResult {
    use chess::capacity::phase::CapacityPhase;
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let (mut registry, port) = fixture.registry();
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let shutdown = port.shutdown_all(deadline)?;
    let quiesce = port.quiesce_all(generation, deadline)?;
    let resume = port.resume_all(generation, deadline)?;
    let worker = tokio::spawn(async move {
        let result = registry.run().await;
        (registry, result)
    });
    shutdown.wait().await?;
    let (registry, result) = worker.await?;
    result?;
    let quiesce = tokio::time::timeout(Duration::ZERO, quiesce.wait()).await;
    let resume = tokio::time::timeout(Duration::ZERO, resume.wait()).await;
    let quiesce_closed = matches!(quiesce, Ok(Err(ref error)) if matches!(error.as_ref(), DesktopGameRegistryError::Closed));
    let resume_closed = matches!(resume, Ok(Err(ref error)) if matches!(error.as_ref(), DesktopGameRegistryError::Closed));
    drop((registry, port, phase));
    fixture.close().await?;
    assert!(
        quiesce_closed,
        "queued quiesce receipt remained unresolved: {quiesce:?}"
    );
    assert!(
        resume_closed,
        "queued resume receipt remained unresolved: {resume:?}"
    );
    Ok(())
}

#[tokio::test]
async fn creation_deadline_still_requests_existing_game_cleanup_and_retains_pending_creation()
-> DesktopGameRegistryTestResult {
    use chess::capacity::phase::CapacityPhase;
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let (mut registry, port) = fixture.registry();
    let worker = tokio::spawn(async move {
        let result = registry.run().await;
        (registry, result)
    });
    let first = port
        .execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
    fixture.admitted.notified().await;
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    let pending_creation = tokio::spawn(port.execute(DesktopGameRegistryFixture::create()));
    fixture.admitted.notified().await;
    let shutdown = port.shutdown_all(Instant::now())?.wait().await;
    assert!(shutdown.is_err());
    let (mut registry, result) = worker.await?;
    assert!(result.is_err());
    assert!(!registry.is_shutdown_complete());
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    let repository = StorageGameDocumentRepository::new(fixture.store.client(), &fixture.executor);
    let paused = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let loaded = repository
                .load(first.id())
                .await?
                .ok_or("missing existing game")?;
            if loaded.session().revision() > first.revision() {
                break;
            }
            tokio::task::yield_now().await;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await;
    registry
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(registry.is_shutdown_complete());
    let creation_result = pending_creation.await?;
    assert!(creation_result.is_err());
    drop((registry, port, repository, phase));
    fixture.close().await?;
    paused??;
    Ok(())
}
