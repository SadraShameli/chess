use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::capacity::phase::CapacityPhase;
use chess::desktop::error::DesktopFailure;
use chess::desktop::game_bridge::DesktopGameBridgeProvider;
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::native::game_runtime::DesktopNativeGameRuntimeError;
use chess::desktop::native::game_service::{
    DesktopNativeGameService, DesktopNativeGameServiceError,
};
use chess::game::enums::GameEnumPhase;
use tokio::sync::watch;
use tokio::time::{Instant, timeout, timeout_at};

use crate::desktop_game_registry_fixture::{
    DesktopGameRegistryFixture, DesktopGameRegistryFixtureGate, DesktopGameRegistryFixtureProof,
    DesktopGameRegistryFixtureRuntime, DesktopGameRegistryTestResult,
};

struct DesktopNativeGameServiceFixture;

impl DesktopNativeGameServiceFixture {
    fn failure(error: DesktopFailure) -> std::io::Error {
        std::io::Error::other(error.message().to_owned())
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(3)
    }

    async fn wait_failure(
        failures: &mut watch::Receiver<Option<Arc<DesktopNativeGameRuntimeError>>>,
    ) -> Result<Arc<DesktopNativeGameRuntimeError>, Box<dyn std::error::Error>> {
        timeout(Duration::from_secs(3), failures.wait_for(Option::is_some))
            .await??
            .clone()
            .ok_or_else(|| "missing published runtime failure".into())
    }
}

#[tokio::test]
async fn canceled_shutdown_preserves_the_move_and_restores_its_saved_paused_game()
-> DesktopGameRegistryTestResult {
    let gate = Arc::new(DesktopGameRegistryFixtureGate::default());
    let fixture = DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof(Some(
        Arc::clone(&gate),
    )))
    .await?;
    let mut service = DesktopNativeGameService::new(fixture.options());
    service.restore().await?;
    let port = service.start()?;
    let mut failures = port.failures();
    let first = port
        .execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameServiceFixture::failure)?;
    gate.armed.store(true, Ordering::SeqCst);
    let movement = tokio::spawn(port.execute(DesktopGameRegistryFixture::session(
        &first,
        DesktopGameCommandsEnumAction::Play {
            node: "f9fbc8bb-9899-4f60-ad4a-eed15d34ef90".parse()?,
            origin: "e2".parse()?,
            destination: "e4".parse()?,
            promotion: None,
            color: BoardEnumColor::White,
        },
    )));
    gate.entered.notified().await;
    assert!(service.shutdown_retained(Instant::now()).await.is_err());
    DesktopNativeGameServiceFixture::wait_failure(&mut failures).await?;
    assert!(!service.is_shutdown_complete());
    gate.release()?;
    movement
        .await?
        .map_err(DesktopNativeGameServiceFixture::failure)?;
    service
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    assert!(service.is_shutdown_complete());
    drop((service, port, failures));

    let mut restored = DesktopNativeGameService::new(fixture.options());
    restored.restore().await?;
    restored.restore().await?;
    let port = restored.start()?;
    let snapshots = port.snapshots().borrow().clone();
    assert_eq!(snapshots.len(), 1);
    let snapshot = snapshots.first().ok_or("missing restored game")?;
    assert_eq!(snapshot.id(), first.id());
    assert_eq!(snapshot.phase(), GameEnumPhase::Interrupted);
    assert_eq!(snapshot.moves().len(), 1);
    assert_eq!(snapshot.moves()[0].san(), "e4");
    assert_eq!(snapshot.live_turn(), BoardEnumColor::Black);
    assert!(!snapshot.clock().is_running());
    restored
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    assert!(restored.is_shutdown_complete());
    drop((restored, port));
    fixture.close().await
}

#[tokio::test]
async fn runtime_failure_is_observed_once_and_completed_worker_is_not_polled_again()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let mut service = DesktopNativeGameService::new(fixture.options());
    let port = service.start()?;
    let mut failures = port.failures();
    port.execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameServiceFixture::failure)?;
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    assert!(
        port.registry_port()
            .shutdown_all(Instant::now())?
            .wait()
            .await
            .is_err()
    );
    let failure = DesktopNativeGameServiceFixture::wait_failure(&mut failures).await?;
    assert!(!service.is_shutdown_complete());
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    let outcome = service
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await;
    assert!(matches!(
        outcome,
        Err(DesktopNativeGameServiceError::Runtime(ref error)) if Arc::ptr_eq(error, &failure)
    ));
    assert!(service.is_shutdown_complete());
    service.shutdown_retained(Instant::now()).await?;
    drop((service, port, failures, phase));
    fixture.close().await
}

#[tokio::test]
async fn canceled_restore_retains_its_registry_and_resumes_without_duplicate_installation()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let mut original = DesktopNativeGameService::new(fixture.options());
    let port = original.start()?;
    for _ in 0..2 {
        port.execute(DesktopGameRegistryFixture::create())
            .await
            .map_err(DesktopNativeGameServiceFixture::failure)?;
    }
    original
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    fixture.admitted.notified().await;
    drop((original, port));

    let mut service = DesktopNativeGameService::new(fixture.options());
    {
        let restoration = service.restore();
        tokio::pin!(restoration);
        tokio::select! {
            biased;
            () = fixture.admitted.notified() => {},
            result = &mut restoration => {
                result?;
                return Err("restoration completed before the cancellation gate".into());
            }
        }
    }
    assert!(matches!(
        service.start(),
        Err(DesktopNativeGameServiceError::Restoring)
    ));
    service.restore().await?;
    service.restore().await?;
    let port = service.start()?;
    assert_eq!(port.snapshots().borrow().len(), 2);
    assert!(
        port.snapshots()
            .borrow()
            .iter()
            .all(|game| game.phase() == GameEnumPhase::Interrupted && !game.clock().is_running())
    );
    service
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    assert!(service.is_shutdown_complete());
    drop((service, port));
    fixture.close().await
}

#[tokio::test]
async fn shutdown_retains_blocked_restoration_until_the_capacity_phase_allows_cleanup()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let mut original = DesktopNativeGameService::new(fixture.options());
    let port = original.start()?;
    port.execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameServiceFixture::failure)?;
    original
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    drop((original, port));

    let mut service = DesktopNativeGameService::new(fixture.options());
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    assert!(timeout_at(Instant::now(), service.restore()).await.is_err());
    assert!(service.shutdown_retained(Instant::now()).await.is_err());
    assert!(!service.is_shutdown_complete());
    assert!(matches!(
        service.start(),
        Err(DesktopNativeGameServiceError::Closing)
    ));
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    service
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    assert!(service.is_shutdown_complete());
    service.shutdown_retained(Instant::now()).await?;
    drop((service, phase));
    fixture.close().await
}
