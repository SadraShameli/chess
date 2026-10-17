use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use crate::board::enums::BoardEnumColor;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::phase::{
    CapacityPhase, CapacityPhaseEnumInterruption, CapacityPhaseEnumState, CapacityPhaseGeneration,
};
use crate::clock::time_control::ClockTimeControl;
use crate::desktop::application::DesktopApplicationError;
use crate::desktop::dto::{DesktopEnumEvent, DesktopEnumValue};
use crate::desktop::game_bridge::DesktopGameBridgeEnumOperation;
use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
};
use crate::desktop::native::bootstrap::{DesktopNativeBootstrap, DesktopNativeBootstrapSupervisor};
use crate::desktop::native::host::{DesktopNativeHostEvent, DesktopNativeHostServices};
use crate::desktop::native::startup::{DesktopNativeStartup, DesktopNativeStartupEnumAdmission};
use crate::desktop::profile::{DesktopProfileId, DesktopProfileOsLock, DesktopProfileSecurity};
use crate::game::configuration::{
    GameConfiguration, GameConfigurationEnumOpponent, GameConfigurationEnumStart,
    GameConfigurationOptions,
};
use crate::game::enums::{GameEnumPhase, GameId};

type DesktopNativeBootstrapGameTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct DesktopNativeBootstrapGameFixture;

impl DesktopNativeBootstrapGameFixture {
    const DEADLINE: Duration = Duration::from_secs(5);

    fn budget() -> DesktopNativeBootstrapGameTestResult<CapacityBudget> {
        Ok(CapacityBudget::new(
            NonZeroUsize::new(CapacityPhase::MINIMUM_WORKERS).ok_or("zero native workers")?,
        )?)
    }

    fn phase(
        capacity: &Mutex<CapacityPhase>,
    ) -> DesktopNativeBootstrapGameTestResult<MutexGuard<'_, CapacityPhase>> {
        capacity
            .lock()
            .map_err(|_poisoned| "native fixture phase poisoned".into())
    }

    fn run<T>(
        profile: &Path,
        exercise: impl AsyncFnOnce(
            &DesktopNativeHostServices,
        ) -> DesktopNativeBootstrapGameTestResult<T>,
    ) -> DesktopNativeBootstrapGameTestResult<T> {
        Self::run_preparing(profile, async move |services| {
            Self::ready(services).await?;
            exercise(services).await
        })
    }

    fn run_preparing<T>(
        profile: &Path,
        exercise: impl AsyncFnOnce(
            &DesktopNativeHostServices,
        ) -> DesktopNativeBootstrapGameTestResult<T>,
    ) -> DesktopNativeBootstrapGameTestResult<T> {
        let security = DesktopProfileSecurity::current()?;
        let startup = DesktopNativeStartup::new(security.clone(), DesktopProfileOsLock);
        let admission = startup.admit(
            profile,
            DesktopProfileId::from_uuid(uuid::Uuid::new_v4())?,
            DesktopRequestId::new(),
        )?;
        let DesktopNativeStartupEnumAdmission::Owned(owner) = admission else {
            return Err("native fixture did not acquire its temporary profile".into());
        };
        let budget = Self::budget()?;
        let mut result = None;
        DesktopNativeBootstrap::owned(owner, security, budget, |services| {
            result = Some(services.runtime.block_on(async {
                tokio::time::timeout(Self::DEADLINE, exercise(&services)).await?
            }));
            Ok(())
        })?;
        result.ok_or("native fixture never received host services")?
    }

    async fn queued_teardown(
        services: &DesktopNativeHostServices,
        event: impl FnOnce(CapacityPhaseGeneration) -> DesktopNativeHostEvent,
    ) -> DesktopNativeBootstrapGameTestResult {
        loop {
            let mut notification = std::pin::pin!(services.wake.notified());
            let consumed =
                poll_fn(|context| Poll::Ready(notification.as_mut().poll(context).is_ready()))
                    .await;
            if !consumed {
                break;
            }
        }
        services.wake.acknowledge();
        services.port.acknowledge_wake();
        {
            let mut phase = Self::phase(&services.capacity)?;
            let generation = phase
                .generation()
                .ok_or("native fixture missing preparation")?;
            services.events.send_replace(event(generation));
            phase.interrupt_preparation(generation, CapacityPhaseEnumInterruption::Cancelled)?;
            phase.acknowledge_preparation_stopped(generation)?;
            phase.begin_teardown()?;
        }
        services.wake.notified().await;
        let phase = Self::phase(&services.capacity)?;
        assert_eq!(phase.state(), CapacityPhaseEnumState::Teardown);
        assert!(phase.try_lease_workload(1).is_err());
        Ok(())
    }

    async fn ready(services: &DesktopNativeHostServices) -> DesktopNativeBootstrapGameTestResult {
        let generation = Self::phase(&services.capacity)?
            .generation()
            .ok_or("native fixture missing preparation")?;
        services
            .events
            .send_replace(DesktopNativeHostEvent::FrameworkReady { generation });
        loop {
            services.wake.acknowledge();
            services.port.acknowledge_wake();
            let state = Self::phase(&services.capacity)?.state();
            if state == CapacityPhaseEnumState::Steady {
                return Ok(());
            }
            services.wake.notified().await;
        }
    }

    async fn application<T>(
        mut operation: impl FnMut() -> Result<T, DesktopApplicationError>,
    ) -> DesktopNativeBootstrapGameTestResult<T> {
        Ok(tokio::time::timeout(Self::DEADLINE, async {
            loop {
                match operation() {
                    Err(DesktopApplicationError::Busy) => tokio::task::yield_now().await,
                    result => return result,
                }
            }
        })
        .await??)
    }

    async fn command(
        services: &DesktopNativeHostServices,
        command: DesktopEnumCommand,
    ) -> DesktopNativeBootstrapGameTestResult<DesktopMessagesEnumResult> {
        let revision = match &command {
            DesktopEnumCommand::Editor { operation, .. } => {
                Some(DesktopCommand::editor_expected_revision(operation))
            }
            DesktopEnumCommand::Library(operation) => operation.expected_revision(),
            DesktopEnumCommand::Engine(operation) => Some(operation.context().configuration()),
            DesktopEnumCommand::Game(operation) => Some(operation.revision()),
            DesktopEnumCommand::Puzzle { operation, .. } => Some(operation.expected_revision()),
            DesktopEnumCommand::Bootstrap
            | DesktopEnumCommand::FileSelect { .. }
            | DesktopEnumCommand::StreamSubscribe
            | DesktopEnumCommand::StreamAcknowledge { .. }
            | DesktopEnumCommand::StreamResync { .. }
            | DesktopEnumCommand::Backup(_)
            | DesktopEnumCommand::Stop { .. }
            | DesktopEnumCommand::Pause { .. }
            | DesktopEnumCommand::Quit => None,
        };
        let request = services
            .port
            .request(DesktopCommand::new(command, revision))?;
        let receipt = Self::application(|| services.port.try_submit(&request)).await?;
        loop {
            services.wake.acknowledge();
            services.port.acknowledge_wake();
            if let Some(response) = receipt.response() {
                return Ok(response.clone());
            }
            services.wake.notified().await;
        }
    }

    fn game(
        response: DesktopMessagesEnumResult,
    ) -> DesktopNativeBootstrapGameTestResult<DesktopGameSnapshot> {
        if let DesktopMessagesEnumResult::Value {
            value: DesktopEnumValue::Game(snapshot),
            ..
        } = response
        {
            Ok(*snapshot)
        } else {
            Err(format!("unexpected native game response: {response:?}").into())
        }
    }
}

#[tokio::test]
async fn native_preparation_observes_terminal_cancellation_while_waiting_for_its_phase()
-> DesktopNativeBootstrapGameTestResult {
    let budget = DesktopNativeBootstrapGameFixture::budget()?;
    let capacity = Mutex::new(CapacityPhase::new(budget)?);
    let generation = DesktopNativeBootstrapGameFixture::phase(&capacity)?
        .generation()
        .ok_or("missing preparation")?;
    let mut preparation = std::pin::pin!(DesktopNativeBootstrapSupervisor::prepare(
        &capacity, generation
    ));
    {
        let mut phase = DesktopNativeBootstrapGameFixture::phase(&capacity)?;
        assert!(
            preparation
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        phase.interrupt_preparation(generation, CapacityPhaseEnumInterruption::Cancelled)?;
        phase.begin_teardown()?;
    }
    assert!(!preparation.await?);
    Ok(())
}

#[tokio::test]
async fn native_preparation_observes_terminal_cancellation_while_waiting_for_workload_drain()
-> DesktopNativeBootstrapGameTestResult {
    let budget = DesktopNativeBootstrapGameFixture::budget()?;
    let mut phase = CapacityPhase::new(budget.clone())?;
    let initial = phase.generation().ok_or("missing initial preparation")?;
    phase.acknowledge_searches_stopped(initial)?;
    phase.try_enter_preparation(initial)?;
    phase.complete_preparation(initial)?;
    let workload = budget.try_lease(1)?;
    let generation = phase.request_preparation()?;
    let capacity = Mutex::new(phase);
    let mut preparation = std::pin::pin!(DesktopNativeBootstrapSupervisor::prepare(
        &capacity, generation
    ));
    assert!(
        preparation
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    {
        let mut phase = DesktopNativeBootstrapGameFixture::phase(&capacity)?;
        phase.interrupt_preparation(generation, CapacityPhaseEnumInterruption::Cancelled)?;
        phase.begin_teardown()?;
    }
    drop(workload);
    assert!(!preparation.await?);
    Ok(())
}

#[test]
fn native_composition_queued_framework_ready_cannot_reopen_terminal_capacity()
-> DesktopNativeBootstrapGameTestResult {
    let directory = tempfile::tempdir()?;
    DesktopNativeBootstrapGameFixture::run_preparing(
        &directory.path().join("profile"),
        async |services| {
            DesktopNativeBootstrapGameFixture::queued_teardown(services, |generation| {
                DesktopNativeHostEvent::FrameworkReady { generation }
            })
            .await
        },
    )?;
    directory.close()?;
    Ok(())
}

#[test]
fn native_composition_queued_preparation_cannot_restart_terminal_capacity()
-> DesktopNativeBootstrapGameTestResult {
    let directory = tempfile::tempdir()?;
    DesktopNativeBootstrapGameFixture::run_preparing(
        &directory.path().join("profile"),
        async |services| {
            DesktopNativeBootstrapGameFixture::queued_teardown(services, |generation| {
                DesktopNativeHostEvent::PreparationRequested { generation }
            })
            .await
        },
    )?;
    directory.close()?;
    Ok(())
}

#[test]
fn native_composition_graphics_fault_from_steady_reaches_a_fresh_preparation()
-> DesktopNativeBootstrapGameTestResult {
    let directory = tempfile::tempdir()?;
    DesktopNativeBootstrapGameFixture::run(&directory.path().join("profile"), async |services| {
        let generation =
            DesktopNativeBootstrapGameFixture::phase(&services.capacity)?.request_preparation()?;
        services
            .events
            .send_replace(DesktopNativeHostEvent::GraphicsPreparationStopped {
                generation,
                received_at: services.port.now()?,
            });
        loop {
            services.wake.acknowledge();
            services.port.acknowledge_wake();
            {
                let phase = DesktopNativeBootstrapGameFixture::phase(&services.capacity)?;
                if phase.state() == CapacityPhaseEnumState::Preparing
                    && phase.generation().is_some_and(|fresh| fresh != generation)
                {
                    return Ok(());
                }
            }
            services.wake.notified().await;
        }
    })?;
    directory.close()?;
    Ok(())
}

#[test]
fn native_composition_creates_plays_saves_and_restores_a_local_game_paused()
-> DesktopNativeBootstrapGameTestResult {
    let directory = tempfile::tempdir()?;
    let profile = directory.path().join("native-profile");
    let game: GameId = DesktopNativeBootstrapGameFixture::run(&profile, async |services| {
        let created = DesktopNativeBootstrapGameFixture::command(
            services,
            DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Create {
                owner: DesktopMessagesEntityId::new(),
                configuration: GameConfiguration::new(
                    GameConfigurationEnumStart::Standard,
                    GameConfigurationEnumOpponent::LocalTwoPlayer,
                    ClockTimeControl::initial_default(),
                    GameConfigurationOptions::initial_default(),
                ),
            }),
        )
        .await?;
        let created = DesktopNativeBootstrapGameFixture::game(created)?;
        assert_eq!(created.phase(), GameEnumPhase::Active);
        let moved = DesktopNativeBootstrapGameFixture::command(
            services,
            DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
                game: created.id(),
                revision: created.revision(),
                action: DesktopGameCommandsEnumAction::Play {
                    node: "47086030-40d9-4dbc-b74e-4c4408c0e69b".parse()?,
                    origin: "e2".parse()?,
                    destination: "e4".parse()?,
                    promotion: None,
                    color: BoardEnumColor::White,
                },
            }),
        )
        .await?;
        let moved = DesktopNativeBootstrapGameFixture::game(moved)?;
        assert_eq!(moved.moves().len(), 1);
        assert_eq!(moved.live_turn(), BoardEnumColor::Black);
        assert!(moved.clock().is_running());
        Ok(moved.id())
    })?;
    DesktopNativeBootstrapGameFixture::run(&profile, async |services| {
        let _subscribed = DesktopNativeBootstrapGameFixture::command(
            services,
            DesktopEnumCommand::StreamSubscribe,
        )
        .await?;
        loop {
            services.wake.acknowledge();
            services.port.acknowledge_wake();
            while let Some(event) =
                DesktopNativeBootstrapGameFixture::application(|| services.port.try_event()).await?
            {
                if let DesktopEnumEvent::Games(games) = event.payload() {
                    assert_eq!(games.len(), 1);
                    let restored = &games[0];
                    assert_eq!(restored.id(), game);
                    assert_eq!(restored.phase(), GameEnumPhase::Interrupted);
                    assert!(!restored.clock().is_running());
                    assert_eq!(restored.moves().len(), 1);
                    assert_eq!(restored.live_turn(), BoardEnumColor::Black);
                    return Ok(());
                }
            }
            services.wake.notified().await;
        }
    })?;
    directory.close()?;
    Ok(())
}
