use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::game_bridge::DesktopGameBridgeProvider;
use chess::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use chess::desktop::game_registry::DesktopGameRegistryPort;
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::game_transition::{
    DesktopGameTransition, DesktopGameTransitionEnumEffect, DesktopGameTransitionOptions,
};
use chess::desktop::identity::DesktopRequestId;
use chess::game::enums::{GameEnumEngineReadiness, GameEnumPhase};
use chess::game::pause::{GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseEnumFault};
use chess::game::session::GameSessionEnumInterruption;
use chess::game::storage_repository::GameStorageRepository;
use chess::storage::game_document::StorageGameDocumentRepository;
use tokio::time::Instant;

use crate::desktop_game_registry_fixture::{
    DesktopGameRegistryFixture, DesktopGameRegistryFixtureGate, DesktopGameRegistryFixtureProof,
    DesktopGameRegistryFixtureRuntime,
};
use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

type DesktopGraphicsLifecycleTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct DesktopGraphicsLifecycleFixture;

impl DesktopGraphicsLifecycleFixture {
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(3)
    }

    async fn create(
        port: &DesktopGameRegistryPort,
    ) -> DesktopGraphicsLifecycleTestResult<DesktopGameSnapshot> {
        Ok(port
            .execute(DesktopGameRegistryFixture::create())
            .await
            .map_err(|error| std::io::Error::other(error.message().to_owned()))?)
    }

    async fn submit(
        port: &DesktopGameRegistryPort,
        snapshot: &DesktopGameSnapshot,
        action: DesktopGameCommandsEnumAction,
    ) -> DesktopGraphicsLifecycleTestResult<DesktopGameSnapshot> {
        Ok(port
            .execute(DesktopGameRegistryFixture::session(snapshot, action))
            .await
            .map_err(|error| std::io::Error::other(error.message().to_owned()))?)
    }

    fn current(
        port: &DesktopGameRegistryPort,
        previous: &DesktopGameSnapshot,
    ) -> DesktopGraphicsLifecycleTestResult<DesktopGameSnapshot> {
        port.snapshots()
            .borrow()
            .iter()
            .find(|snapshot| snapshot.id() == previous.id())
            .cloned()
            .ok_or_else(|| "missing graphics lifecycle game".into())
    }
}

#[tokio::test]
async fn graphics_controls_preserve_unrelated_states_and_durably_require_explicit_resume()
-> DesktopGraphicsLifecycleTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let (mut registry, port) = fixture.registry();
    let worker = tokio::spawn(async move {
        let result = registry.run().await;
        (registry, result)
    });
    let active = DesktopGraphicsLifecycleFixture::create(&port).await?;
    let manual = DesktopGraphicsLifecycleFixture::create(&port).await?;
    let engine_fault = DesktopGraphicsLifecycleFixture::create(&port).await?;
    let finished = DesktopGraphicsLifecycleFixture::create(&port).await?;
    let manual = DesktopGraphicsLifecycleFixture::submit(
        &port,
        &manual,
        DesktopGameCommandsEnumAction::Pause {
            cause: GamePauseEnumCause::Manual,
        },
    )
    .await?;
    let engine_fault = DesktopGraphicsLifecycleFixture::submit(
        &port,
        &engine_fault,
        DesktopGameCommandsEnumAction::Pause {
            cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Engine),
        },
    )
    .await?;
    let finished = DesktopGraphicsLifecycleFixture::submit(
        &port,
        &finished,
        DesktopGameCommandsEnumAction::Resign {
            side: BoardEnumColor::White,
        },
    )
    .await?;
    fixture
        .clock
        .advance_to(ClockInstant::from_millis(1_000)?)?;
    port.pause_graphics_all(
        ClockInstant::from_millis(1_000)?,
        DesktopGraphicsLifecycleFixture::deadline(),
    )?
    .wait()
    .await?;
    let paused = DesktopGraphicsLifecycleFixture::current(&port, &active)?;
    assert_eq!(
        paused.interruption(),
        Some(GameSessionEnumInterruption::RecoveryRequired(
            GamePauseEnumFault::Graphics
        ))
    );
    assert!(!paused.clock().is_running());
    assert_eq!(
        paused
            .clock()
            .snapshot(ClockInstant::from_millis(9_000)?)?
            .elapsed(),
        ClockDuration::from_millis(1_000)?
    );
    fixture
        .clock
        .advance_to(ClockInstant::from_millis(2_000)?)?;
    port.pause_graphics_all(
        ClockInstant::from_millis(2_000)?,
        DesktopGraphicsLifecycleFixture::deadline(),
    )?
    .wait()
    .await?;
    assert_eq!(
        DesktopGraphicsLifecycleFixture::current(&port, &paused)?.revision(),
        paused.revision()
    );
    port.recover_graphics_all(DesktopGraphicsLifecycleFixture::deadline())?
        .wait()
        .await?;
    let recovered = DesktopGraphicsLifecycleFixture::current(&port, &active)?;
    assert_eq!(
        recovered.interruption(),
        Some(GameSessionEnumInterruption::Recovered(
            GamePauseEnumFault::Graphics
        ))
    );
    assert_eq!(recovered.phase(), GameEnumPhase::Interrupted);
    assert!(!recovered.clock().is_running());
    port.recover_graphics_all(DesktopGraphicsLifecycleFixture::deadline())?
        .wait()
        .await?;
    assert_eq!(
        DesktopGraphicsLifecycleFixture::current(&port, &recovered)?.revision(),
        recovered.revision()
    );
    for original in [&manual, &engine_fault, &finished] {
        let current = DesktopGraphicsLifecycleFixture::current(&port, original)?;
        assert_eq!(current.revision(), original.revision());
        assert_eq!(current.phase(), original.phase());
        assert_eq!(current.interruption(), original.interruption());
        assert_eq!(current.clock(), original.clock());
        assert_eq!(current.outcome(), original.outcome());
    }
    let repository = StorageGameDocumentRepository::new(fixture.store.client(), &fixture.executor);
    let saved = repository
        .load(active.id())
        .await?
        .ok_or("missing graphics save")?;
    assert_eq!(saved.session().revision(), recovered.revision());
    assert_eq!(saved.session().interruption(), recovered.interruption());
    assert!(!saved.session().clock().is_running());
    port.shutdown_all(DesktopGraphicsLifecycleFixture::deadline())?
        .wait()
        .await?;
    let (registry, result) = worker.await?;
    result?;
    drop((registry, port, repository, saved));
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn graphics_pause_rebases_a_racing_move_without_rewinding_its_clock()
-> DesktopGraphicsLifecycleTestResult {
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
    let active = DesktopGraphicsLifecycleFixture::create(&port).await?;
    gate.armed.store(true, Ordering::SeqCst);
    let mut request = DesktopGameRegistryFixture::session(
        &active,
        DesktopGameCommandsEnumAction::Play {
            node: "6f3a8f45-89e3-4dfd-9bb5-3a3c9aed032a".parse()?,
            origin: "e2".parse()?,
            destination: "e4".parse()?,
            promotion: None,
            color: BoardEnumColor::White,
        },
    );
    request.received_at = ClockInstant::from_millis(2_000)?;
    fixture.clock.advance_to(request.received_at)?;
    let movement = tokio::spawn(port.execute(request));
    gate.entered.notified().await;
    let pause = port.pause_graphics_all(
        ClockInstant::from_millis(1_000)?,
        DesktopGraphicsLifecycleFixture::deadline(),
    )?;
    gate.release()?;
    let moved = movement
        .await?
        .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
    pause.wait().await?;
    let paused = DesktopGraphicsLifecycleFixture::current(&port, &active)?;
    assert_eq!(paused.live(), moved.live());
    assert_eq!(paused.live_turn(), BoardEnumColor::Black);
    assert_eq!(
        paused
            .clock()
            .snapshot(ClockInstant::from_millis(9_000)?)?
            .elapsed(),
        ClockDuration::ZERO
    );
    assert_eq!(
        paused.interruption(),
        Some(GameSessionEnumInterruption::RecoveryRequired(
            GamePauseEnumFault::Graphics
        ))
    );
    port.shutdown_all(DesktopGraphicsLifecycleFixture::deadline())?
        .wait()
        .await?;
    let (registry, result) = worker.await?;
    result?;
    drop((registry, port));
    fixture.close().await?;
    Ok(())
}

#[test]
fn late_graphics_instants_are_ordered_while_ordinary_move_timestamps_stay_strict()
-> DesktopGraphicsLifecycleTestResult {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 2_000)?;
    let revision = session.revision();
    assert!(GameTransitionFixture::play(&mut session, 3, "e7e5", 1_000).is_err());
    assert_eq!(session.revision(), revision);
    let effect = DesktopGameTransition::apply(
        &mut session,
        DesktopGameTransitionOptions {
            request: DesktopGameCommandsRequest {
                request_id: DesktopRequestId::new(),
                expected_revision: revision,
                received_at: ClockInstant::from_millis(1_000)?,
                action: DesktopGameCommandsEnumAction::Pause {
                    cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Graphics),
                },
            },
            engine: GamePauseEnumEngineBarrier::NoEngine,
            readiness: GameEnumEngineReadiness::Ready,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(effect, DesktopGameTransitionEnumEffect::Autosave);
    assert_eq!(
        session
            .clock()
            .snapshot(ClockInstant::from_millis(9_000)?)?
            .elapsed(),
        ClockDuration::ZERO
    );
    assert!(!session.clock().is_running());
    assert!(DesktopGameCommandsEnumAction::RecoverGraphics.is_control());
    Ok(())
}
