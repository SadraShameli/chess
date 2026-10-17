use std::collections::VecDeque;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;
use strum::IntoEnumIterator;

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use chess::clock::state::{ClockState, ClockStateEnumStatus};
use chess::clock::time_control::ClockTimeControl;
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::game_supervisor::{
    DesktopGameSupervisor, DesktopGameSupervisorEngineFactory, DesktopGameSupervisorEngineProvider,
    DesktopGameSupervisorError, DesktopGameSupervisorOptions, DesktopGameSupervisorPort,
};
use chess::desktop::game_transition::DesktopGameTransitionError;
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::native::game_engine::{
    DesktopNativeGameEngineAllocation, DesktopNativeGameEngineError,
    DesktopNativeGameEngineFactory, DesktopNativeGameEngineFactoryOptions,
    DesktopNativeGameEngineResources,
};
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::live_game::EngineLiveGameError;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::session::EngineSessionError;
use chess::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use chess::game::engine_provider::{
    GameEngineContext, GameEngineEnumEvent, GameEngineProvider, GameEngineRequest,
    GameEngineSearchConfig,
};
use chess::game::enums::{GameEnumMode, GameEnumPhase, GameEnumSeat, GameRevision};
use chess::game::pause::{GamePauseEnumFault, GamePauseError};
use chess::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionEnumInterruption, GameSessionError,
    GameSessionOptions,
};
use chess::game::storage_repository::GameStorageRevision;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::desktop_game_supervisor_storage::DesktopGameSupervisorFixtureStorage;
use crate::engine_package_selection::EnginePackageFixture;

type DesktopNativeGameEngineTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct DesktopNativeGameEngineFixtureClock(AtomicU64);

impl ClockTimeProvider for DesktopNativeGameEngineFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}

#[derive(Default)]
struct DesktopNativeGameEngineFixtureObservations {
    commands: Mutex<Vec<String>>,
    output: Mutex<VecDeque<String>>,
    ready: Notify,
    live: AtomicUsize,
    incomplete_drop: AtomicBool,
    acknowledge: AtomicBool,
    changed: Notify,
    block_cleanup: AtomicBool,
    cleanup_entered: Notify,
    cleanup_ready: Notify,
}

impl DesktopNativeGameEngineFixtureObservations {
    fn emit(&self, text: String) -> Result<(), EngineProcessError> {
        self.output
            .lock()
            .map_err(|_poisoned| std::io::Error::other("output lock unavailable"))?
            .push_back(text);
        self.ready.notify_waiters();
        Ok(())
    }

    fn commands(&self) -> DesktopNativeGameEngineTestResult<Vec<String>> {
        Ok(self
            .commands
            .lock()
            .map_err(|_poisoned| "command lock unavailable")?
            .clone())
    }
}

struct DesktopNativeGameEngineFixtureProcesses(Arc<DesktopNativeGameEngineFixtureObservations>);

impl EngineProcessProvider for DesktopNativeGameEngineFixtureProcesses {
    type Process = DesktopNativeGameEngineFixtureProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        self.0.live.fetch_add(1, Ordering::SeqCst);
        Ok(DesktopNativeGameEngineFixtureProcess {
            observations: Arc::clone(&self.0),
            complete: false,
        })
    }
}

struct DesktopNativeGameEngineFixtureProcess {
    observations: Arc<DesktopNativeGameEngineFixtureObservations>,
    complete: bool,
}

impl EngineProcess for DesktopNativeGameEngineFixtureProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        self.observations
            .commands
            .lock()
            .map_err(|_poisoned| std::io::Error::other("command lock unavailable"))?
            .push(command.to_owned());
        self.observations.changed.notify_one();
        match command {
            "uci" => {
                self.observations
                    .emit("id name Reckless 0.9.0".to_owned())?;
                self.observations
                    .emit(format!("id author {}", EngineIdentity::AUTHOR))?;
                for option in EngineEnumOption::iter() {
                    self.observations.emit(format!(
                        "option name {} type {}",
                        option.name(),
                        option.descriptor()
                    ))?;
                }
                self.observations.emit("uciok".to_owned())?;
            }
            "isready" => self.observations.emit("readyok".to_owned())?,
            "stop" => {
                self.observations.emit("bestmove g1f3".to_owned())?;
            }
            value if self.observations.acknowledge.load(Ordering::SeqCst) => {
                for option in EngineEnumOption::iter() {
                    if let Some(number) =
                        value.strip_prefix(&format!("setoption name {} value ", option.name()))
                    {
                        let suffix = match option {
                            EngineEnumOption::Hash => " MB",
                            EngineEnumOption::MoveOverhead => " ms",
                            EngineEnumOption::Threads
                            | EngineEnumOption::Minimal
                            | EngineEnumOption::ClearHash
                            | EngineEnumOption::Chess960
                            | EngineEnumOption::MultiPv
                            | EngineEnumOption::SyzygyPath => "",
                        };
                        self.observations.emit(format!(
                            "info string set {} to {number}{suffix}",
                            option.name()
                        ))?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        loop {
            let ready = self.observations.ready.notified();
            tokio::pin!(ready);
            ready.as_mut().enable();
            let line = self
                .observations
                .output
                .lock()
                .map_err(|_poisoned| std::io::Error::other("output lock unavailable"))?
                .pop_front();
            if let Some(text) = line {
                return EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text);
            }
            ready.await;
        }
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.observations.cleanup_entered.notify_one();
        while self.observations.block_cleanup.load(Ordering::SeqCst) {
            self.observations.cleanup_ready.notified().await;
        }
        self.complete = true;
        Ok(())
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }

    fn is_shutdown_complete(&self) -> bool {
        self.complete
    }

    async fn terminate(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }
}

impl Drop for DesktopNativeGameEngineFixtureProcess {
    fn drop(&mut self) {
        self.observations.live.fetch_sub(1, Ordering::SeqCst);
        if !self.complete {
            self.observations
                .incomplete_drop
                .store(true, Ordering::SeqCst);
        }
    }
}

struct DesktopNativeGameEngineFixtureRegistration(Arc<AtomicUsize>);

impl Drop for DesktopNativeGameEngineFixtureRegistration {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct DesktopNativeGameEngineFixtureResources {
    budget: CapacityBudget,
    registrations: Arc<AtomicUsize>,
    threads: AtomicUsize,
    hash_mib: AtomicUsize,
    updated: Notify,
    applied: Mutex<Vec<(GameEngineContext, EngineResourceConfig)>>,
    block_registration: AtomicBool,
    registration_ready: Notify,
}

impl DesktopNativeGameEngineResources for DesktopNativeGameEngineFixtureResources {
    type Error = std::io::Error;
    type Registration = DesktopNativeGameEngineFixtureRegistration;

    async fn register(
        &self,
        _context: GameEngineContext,
    ) -> Result<Self::Registration, Self::Error> {
        while self.block_registration.load(Ordering::SeqCst) {
            self.registration_ready.notified().await;
        }
        self.registrations.fetch_add(1, Ordering::SeqCst);
        Ok(DesktopNativeGameEngineFixtureRegistration(Arc::clone(
            &self.registrations,
        )))
    }

    async fn allocate(
        &self,
        _context: GameEngineContext,
    ) -> Result<DesktopNativeGameEngineAllocation, Self::Error> {
        let threads = self.threads.load(Ordering::SeqCst);
        let lease = self
            .budget
            .lease(threads)
            .await
            .map_err(std::io::Error::other)?;
        let hash =
            u32::try_from(self.hash_mib.load(Ordering::SeqCst)).map_err(std::io::Error::other)?;
        let config = EngineResourceConfig::new(threads, hash, ClockDuration::ZERO)
            .map_err(std::io::Error::other)?;
        DesktopNativeGameEngineAllocation::new(config, lease).map_err(std::io::Error::other)
    }

    async fn next_update(
        &self,
        context: GameEngineContext,
    ) -> Result<DesktopNativeGameEngineAllocation, Self::Error> {
        self.updated.notified().await;
        self.allocate(context).await
    }

    async fn acknowledge(
        &self,
        context: GameEngineContext,
        config: EngineResourceConfig,
    ) -> Result<(), Self::Error> {
        self.applied
            .lock()
            .map_err(|_poisoned| std::io::Error::other("applied configuration lock unavailable"))?
            .push((context, config));
        Ok(())
    }
}

struct DesktopNativeGameEngineFixture;

impl DesktopNativeGameEngineFixture {
    fn resources() -> DesktopNativeGameEngineTestResult<Arc<DesktopNativeGameEngineFixtureResources>>
    {
        Ok(Arc::new(DesktopNativeGameEngineFixtureResources {
            budget: CapacityBudget::new(NonZeroUsize::new(4).ok_or("empty CPU budget")?)?,
            registrations: Arc::new(AtomicUsize::new(0)),
            threads: AtomicUsize::new(4),
            hash_mib: AtomicUsize::new(64),
            updated: Notify::new(),
            applied: Mutex::new(Vec::new()),
            block_registration: AtomicBool::new(false),
            registration_ready: Notify::new(),
        }))
    }

    fn context(revision: GameRevision) -> DesktopNativeGameEngineTestResult<GameEngineContext> {
        Ok(GameEngineContext::new(
            uuid::Uuid::from_u128(100).to_string().parse()?,
            revision,
        ))
    }

    fn factory(
        package: &EnginePackageFixture,
        resources: Arc<DesktopNativeGameEngineFixtureResources>,
        observations: Arc<DesktopNativeGameEngineFixtureObservations>,
    ) -> DesktopNativeGameEngineTestResult<
        DesktopNativeGameEngineFactory<
            DesktopNativeGameEngineFixtureProcesses,
            DesktopNativeGameEngineFixtureClock,
            DesktopNativeGameEngineFixtureResources,
        >,
    > {
        Ok(DesktopNativeGameEngineFactory::new(
            DesktopNativeGameEngineFactoryOptions {
                processes: DesktopNativeGameEngineFixtureProcesses(observations),
                package: package.select()?,
                clock: Arc::new(DesktopNativeGameEngineFixtureClock::default()),
                resources,
            },
        ))
    }

    fn request(revision: GameRevision) -> DesktopNativeGameEngineTestResult<GameEngineRequest> {
        let mut clock = ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White);
        clock.resume(ClockInstant::from_nanos(0))?;
        Ok(GameEngineRequest::new(
            Self::context(revision)?,
            BoardPosition::standard(),
            Vec::new(),
            clock,
            GameEngineSearchConfig::new(
                ClockDuration::from_millis(3_000)?,
                ClockDuration::from_millis(25)?,
            )?,
        )?)
    }

    fn observations() -> Arc<DesktopNativeGameEngineFixtureObservations> {
        Arc::new(DesktopNativeGameEngineFixtureObservations {
            acknowledge: AtomicBool::new(true),
            ..Default::default()
        })
    }
}

#[tokio::test(start_paused = true)]
async fn initialization_and_idle_engine_keep_hash_registration_but_return_cpu_capacity()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    let mut engine = factory
        .create(DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?)
        .await?;
    assert!(engine.is_idle());
    assert_eq!(resources.budget.available(), 4);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 1);
    assert_eq!(observations.live.load(Ordering::SeqCst), 1);
    assert_eq!(
        engine
            .resources()
            .ok_or("missing engine resources")?
            .threads(),
        4
    );
    engine
        .quiesce(
            DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?,
            Instant::now().into_std(),
        )
        .await?;
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 1);
    engine
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert_eq!(observations.live.load(Ordering::SeqCst), 0);
    assert!(!observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn search_charges_all_assigned_threads_and_rebalances_only_at_next_boundary()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    let mut engine = factory
        .create(DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?)
        .await?;
    let identity = engine
        .start(DesktopNativeGameEngineFixture::request(GameRevision::ZERO)?)
        .await?;
    assert_eq!(resources.budget.available(), 0);
    assert_eq!(
        resources
            .applied
            .lock()
            .map_err(|_poisoned| "applied configuration lock unavailable")?
            .last()
            .copied(),
        Some((
            identity.context(),
            engine
                .resources()
                .ok_or("missing acknowledged search resources")?
        ))
    );
    assert!(
        observations
            .commands()?
            .contains(&"setoption name MoveOverhead value 25".to_owned())
    );
    resources.threads.store(2, Ordering::SeqCst);
    observations.emit("info depth 1 score cp 0 pv e2e4".to_owned())?;
    assert!(matches!(
        engine.next_event(identity.context()).await?,
        GameEngineEnumEvent::Info { .. }
    ));
    assert_eq!(resources.budget.available(), 0);
    observations.emit("bestmove e2e4".to_owned())?;
    assert!(matches!(
        engine.next_event(identity.context()).await?,
        GameEngineEnumEvent::Move(_)
    ));
    assert_eq!(resources.budget.available(), 4);
    let next = engine
        .start(DesktopNativeGameEngineFixture::request(
            GameRevision::ZERO.next()?,
        )?)
        .await?;
    assert_eq!(resources.budget.available(), 2);
    assert_eq!(
        engine
            .resources()
            .ok_or("missing rebalanced resources")?
            .threads(),
        2
    );
    engine.stop(next, Instant::now().into_std()).await?;
    assert_eq!(resources.budget.available(), 4);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 1);
    engine
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_factory_configuration_retains_child_registration_and_cpu_until_cleanup()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    observations.acknowledge.store(false, Ordering::SeqCst);
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    {
        let creating = factory.create(DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?);
        tokio::pin!(creating);
        assert!(
            std::future::poll_fn(|context| Poll::Ready(creating.as_mut().poll(context)))
                .await
                .is_pending()
        );
    }
    assert_eq!(resources.budget.available(), 0);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 1);
    assert_eq!(observations.live.load(Ordering::SeqCst), 1);
    factory
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(resources.budget.available(), 4);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert_eq!(observations.live.load(Ordering::SeqCst), 0);
    assert!(!observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_search_setup_resumes_without_repeating_resource_commands()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    let mut engine = factory
        .create(DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?)
        .await?;
    observations.acknowledge.store(false, Ordering::SeqCst);
    {
        let starting = engine.start(DesktopNativeGameEngineFixture::request(GameRevision::ZERO)?);
        tokio::pin!(starting);
        assert!(
            std::future::poll_fn(|context| Poll::Ready(starting.as_mut().poll(context)))
                .await
                .is_pending()
        );
    }
    assert_eq!(resources.budget.available(), 0);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 1);
    assert!(!engine.is_idle());
    let count = observations
        .commands()?
        .iter()
        .filter(|command| command.as_str() == "setoption name Threads value 4")
        .count();
    observations.acknowledge.store(true, Ordering::SeqCst);
    observations.emit("info string set Threads to 4".to_owned())?;
    let identity = engine
        .start(DesktopNativeGameEngineFixture::request(GameRevision::ZERO)?)
        .await?;
    assert_eq!(
        observations
            .commands()?
            .iter()
            .filter(|command| command.as_str() == "setoption name Threads value 4")
            .count(),
        count
    );
    assert_eq!(resources.budget.available(), 0);
    engine.stop(identity, Instant::now().into_std()).await?;
    engine
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(resources.budget.available(), 4);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert!(!observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn allocation_cannot_claim_more_engine_threads_than_its_cpu_lease()
-> DesktopNativeGameEngineTestResult {
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let lease = resources.budget.lease(2).await?;
    let config = EngineResourceConfig::new(4, 64, ClockDuration::ZERO)?;
    assert!(DesktopNativeGameEngineAllocation::new(config, lease).is_err());
    assert_eq!(resources.budget.available(), 4);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_factory_registration_resumes_before_spawning_exactly_one_process()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    resources.block_registration.store(true, Ordering::SeqCst);
    let observations = DesktopNativeGameEngineFixture::observations();
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    let context = DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?;
    {
        let creating = factory.create(context);
        tokio::pin!(creating);
        assert!(
            std::future::poll_fn(|context| Poll::Ready(creating.as_mut().poll(context)))
                .await
                .is_pending()
        );
    }
    assert_eq!(observations.live.load(Ordering::SeqCst), 0);
    resources.block_registration.store(false, Ordering::SeqCst);
    resources.registration_ready.notify_one();
    let mut engine = factory.create(context).await?;
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 1);
    assert_eq!(observations.live.load(Ordering::SeqCst), 1);
    assert_eq!(
        resources
            .applied
            .lock()
            .map_err(|_poisoned| "applied configuration lock unavailable")?
            .len(),
        1
    );
    engine
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert!(!observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_factory_configuration_resumes_the_retained_acknowledgment()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    observations.acknowledge.store(false, Ordering::SeqCst);
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    let context = DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?;
    {
        let creating = factory.create(context);
        tokio::pin!(creating);
        assert!(
            std::future::poll_fn(|context| Poll::Ready(creating.as_mut().poll(context)))
                .await
                .is_pending()
        );
    }
    observations.acknowledge.store(true, Ordering::SeqCst);
    observations.emit("info string set Threads to 4".to_owned())?;
    let mut engine = factory.create(context).await?;
    assert_eq!(
        observations
            .commands()?
            .iter()
            .filter(|command| command.as_str() == "uci")
            .count(),
        1
    );
    assert_eq!(
        observations
            .commands()?
            .iter()
            .filter(|command| command.as_str() == "setoption name Threads value 4")
            .count(),
        1
    );
    assert_eq!(
        resources
            .applied
            .lock()
            .map_err(|_poisoned| "applied configuration lock unavailable")?
            .len(),
        1
    );
    assert!(engine.is_idle());
    engine
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(!observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn quiescence_cancels_pending_capacity_admission_without_starting_a_search()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    let context = DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?;
    let mut engine = factory.create(context).await?;
    let reserved = resources.budget.lease(4).await?;
    {
        let starting = engine.start(DesktopNativeGameEngineFixture::request(GameRevision::ZERO)?);
        tokio::pin!(starting);
        assert!(
            std::future::poll_fn(|context| Poll::Ready(starting.as_mut().poll(context)))
                .await
                .is_pending()
        );
    }
    engine.quiesce(context, Instant::now().into_std()).await?;
    assert!(engine.is_idle());
    assert!(
        !observations
            .commands()?
            .iter()
            .any(|command| command.starts_with("go "))
    );
    drop(reserved);
    let identity = engine
        .start(DesktopNativeGameEngineFixture::request(GameRevision::ZERO)?)
        .await?;
    engine.stop(identity, Instant::now().into_std()).await?;
    engine
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(resources.budget.available(), 4);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn idle_hash_refresh_survives_cancellation_and_acknowledges_only_applied_resources()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    let context = DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?;
    let mut engine = factory.create(context).await?;
    resources.hash_mib.store(32, Ordering::SeqCst);
    resources.updated.notify_one();
    observations.acknowledge.store(false, Ordering::SeqCst);
    {
        let refreshing = engine.refresh_resources();
        tokio::pin!(refreshing);
        assert!(
            std::future::poll_fn(|context| Poll::Ready(refreshing.as_mut().poll(context)))
                .await
                .is_pending()
        );
    }
    assert_eq!(resources.budget.available(), 0);
    assert_eq!(
        resources
            .applied
            .lock()
            .map_err(|_poisoned| "applied configuration lock unavailable")?
            .len(),
        1
    );
    observations.acknowledge.store(true, Ordering::SeqCst);
    observations.emit("info string set Threads to 4".to_owned())?;
    engine.refresh_resources().await?;
    assert_eq!(resources.budget.available(), 4);
    assert!(engine.is_idle());
    let config = engine
        .resources()
        .ok_or("missing refreshed configuration")?;
    assert_eq!(config.hash_mib(), 32);
    assert_eq!(
        resources
            .applied
            .lock()
            .map_err(|_poisoned| "applied configuration lock unavailable")?
            .as_slice(),
        &[
            (
                context,
                EngineResourceConfig::new(4, 64, ClockDuration::ZERO)?
            ),
            (context, config)
        ]
    );
    assert!(
        !observations
            .commands()?
            .iter()
            .any(|command| command.starts_with("go "))
    );
    engine
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_pending_idle_update_wait_does_not_block_the_next_engine_turn()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    let mut engine = factory
        .create(DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?)
        .await?;
    {
        let refreshing = engine.refresh_resources();
        tokio::pin!(refreshing);
        assert!(
            std::future::poll_fn(|context| Poll::Ready(refreshing.as_mut().poll(context)))
                .await
                .is_pending()
        );
    }
    let identity = engine
        .start(DesktopNativeGameEngineFixture::request(GameRevision::ZERO)?)
        .await?;
    assert_eq!(resources.budget.available(), 0);
    engine.stop(identity, Instant::now().into_std()).await?;
    engine
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn shutdown_cancels_factory_capacity_admission_before_a_process_exists()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    let reserved = resources.budget.lease(4).await?;
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    {
        let creating = factory.create(DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?);
        tokio::pin!(creating);
        assert!(
            std::future::poll_fn(|context| Poll::Ready(creating.as_mut().poll(context)))
                .await
                .is_pending()
        );
    }
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 1);
    factory
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert_eq!(observations.live.load(Ordering::SeqCst), 0);
    assert!(factory.is_shutdown_complete());
    drop(reserved);
    assert_eq!(resources.budget.available(), 4);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn failed_initialization_cannot_replace_a_child_before_retained_cleanup()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let observations = DesktopNativeGameEngineFixture::observations();
    observations.emit("unexpected startup response".to_owned())?;
    let mut factory = DesktopNativeGameEngineFixture::factory(
        &package,
        Arc::clone(&resources),
        Arc::clone(&observations),
    )?;
    let context = DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?;
    assert!(factory.create(context).await.is_err());
    assert!(factory.create(context).await.is_err());
    assert_eq!(observations.live.load(Ordering::SeqCst), 1);
    assert!(!observations.incomplete_drop.load(Ordering::SeqCst));
    factory
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(observations.live.load(Ordering::SeqCst), 0);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert_eq!(resources.budget.available(), 4);
    Ok(())
}

struct DesktopNativeGameEngineFixtureSequence {
    processes: Mutex<VecDeque<Arc<DesktopNativeGameEngineFixtureObservations>>>,
}

impl EngineProcessProvider for DesktopNativeGameEngineFixtureSequence {
    type Process = DesktopNativeGameEngineFixtureProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        let observations = self
            .processes
            .lock()
            .map_err(|_poisoned| std::io::Error::other("process sequence lock unavailable"))?
            .pop_front()
            .ok_or_else(|| std::io::Error::other("unexpected automatic engine replacement"))?;
        observations.live.fetch_add(1, Ordering::SeqCst);
        Ok(DesktopNativeGameEngineFixtureProcess {
            observations,
            complete: false,
        })
    }
}

struct DesktopNativeGameEngineFixtureProof;

impl GameAdjudicationMateProofProvider for DesktopNativeGameEngineFixtureProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

type DesktopNativeGameEngineFixtureSupervisor = DesktopGameSupervisor<
    DesktopNativeGameEngineFactory<
        DesktopNativeGameEngineFixtureSequence,
        DesktopNativeGameEngineFixtureClock,
        DesktopNativeGameEngineFixtureResources,
    >,
    DesktopNativeGameEngineFixtureClock,
    DesktopNativeGameEngineFixtureProof,
>;

impl DesktopNativeGameEngineFixture {
    fn sequence_factory(
        package: &EnginePackageFixture,
        resources: Arc<DesktopNativeGameEngineFixtureResources>,
        observations: Vec<Arc<DesktopNativeGameEngineFixtureObservations>>,
        clock: Arc<DesktopNativeGameEngineFixtureClock>,
    ) -> DesktopNativeGameEngineTestResult<
        DesktopNativeGameEngineFactory<
            DesktopNativeGameEngineFixtureSequence,
            DesktopNativeGameEngineFixtureClock,
            DesktopNativeGameEngineFixtureResources,
        >,
    > {
        Ok(DesktopNativeGameEngineFactory::new(
            DesktopNativeGameEngineFactoryOptions {
                processes: DesktopNativeGameEngineFixtureSequence {
                    processes: Mutex::new(observations.into()),
                },
                package: package.select()?,
                clock,
                resources,
            },
        ))
    }

    async fn supervisor(
        storage: &DesktopGameSupervisorFixtureStorage,
        factory: DesktopNativeGameEngineFactory<
            DesktopNativeGameEngineFixtureSequence,
            DesktopNativeGameEngineFixtureClock,
            DesktopNativeGameEngineFixtureResources,
        >,
        clock: Arc<DesktopNativeGameEngineFixtureClock>,
    ) -> DesktopNativeGameEngineTestResult<(
        DesktopNativeGameEngineFixtureSupervisor,
        DesktopGameSupervisorPort,
    )> {
        let session = GameSession::new(GameSessionOptions {
            id: Self::context(GameRevision::ZERO)?.game(),
            root: uuid::Uuid::from_u128(101).to_string().parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::HumanVersusEngine,
                GameEnumSeat::Reckless,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(ClockTimeControl::initial_default(), BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?;
        Ok(DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
            session,
            storage_revision: GameStorageRevision::INITIAL,
            storage: storage.store.client(),
            executor: Arc::clone(&storage.executor),
            clock,
            wall_time: crate::game_storage_wall_time_fixture::GameStorageWallTimeFixture::shared(),
            proof: Arc::new(DesktopNativeGameEngineFixtureProof),
            factory,
            search_config: GameEngineSearchConfig::initial_default(),
        })
        .await?)
    }

    fn command(
        port: &DesktopGameSupervisorPort,
        action: DesktopGameCommandsEnumAction,
    ) -> DesktopGameCommandsRequest {
        DesktopGameCommandsRequest {
            request_id: DesktopRequestId::new(),
            expected_revision: port.snapshot().snapshot.revision(),
            received_at: ClockInstant::from_nanos(0),
            action,
        }
    }

    async fn wait_commands(
        observations: &DesktopNativeGameEngineFixtureObservations,
        prefix: &str,
        count: usize,
    ) -> DesktopNativeGameEngineTestResult {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let changed = observations.changed.notified();
                if observations
                    .commands()?
                    .iter()
                    .filter(|command| command.starts_with(prefix))
                    .count()
                    >= count
                {
                    return Ok(());
                }
                changed.await;
            }
        })
        .await?
    }

    async fn wait_snapshot(
        port: &DesktopGameSupervisorPort,
        predicate: impl Fn(&DesktopGameSnapshot) -> bool,
    ) -> DesktopNativeGameEngineTestResult<DesktopGameSnapshot> {
        let mut events = port.events();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let snapshot = events.borrow().snapshot.clone();
                if predicate(&snapshot) {
                    return Ok(snapshot);
                }
                events.changed().await?;
            }
        })
        .await?
    }

    async fn playing(
        port: &DesktopGameSupervisorPort,
        observations: &DesktopNativeGameEngineFixtureObservations,
    ) -> DesktopNativeGameEngineTestResult<DesktopGameSnapshot> {
        port.try_submit(Self::command(port, DesktopGameCommandsEnumAction::Start))?
            .wait()
            .await?;
        Self::wait_commands(observations, "go ", 1).await?;
        observations.emit("bestmove e2e4".to_owned())?;
        Self::wait_snapshot(port, |snapshot| snapshot.moves().len() == 1).await?;
        let moved = port
            .try_submit(Self::command(
                port,
                DesktopGameCommandsEnumAction::Play {
                    node: uuid::Uuid::from_u128(102).to_string().parse()?,
                    origin: "e7".parse()?,
                    destination: "e5".parse()?,
                    promotion: None,
                    color: BoardEnumColor::Black,
                },
            ))?
            .wait()
            .await?;
        Self::wait_commands(observations, "go ", 2).await?;
        Ok(moved)
    }

    fn preserved(before: &DesktopGameSnapshot, after: &DesktopGameSnapshot) {
        assert_eq!(after.board().fen(), before.board().fen());
        assert_eq!(after.root(), before.root());
        assert_eq!(after.live(), before.live());
        assert_eq!(after.view(), before.view());
        assert_eq!(after.outcome(), before.outcome());
        assert_eq!(
            after
                .moves()
                .iter()
                .map(|movement| (movement.id(), movement.san()))
                .collect::<Vec<_>>(),
            before
                .moves()
                .iter()
                .map(|movement| (movement.id(), movement.san()))
                .collect::<Vec<_>>()
        );
    }

    async fn quit(
        port: DesktopGameSupervisorPort,
        worker: tokio::task::JoinHandle<(
            DesktopNativeGameEngineFixtureSupervisor,
            Result<(), DesktopGameSupervisorError>,
        )>,
    ) -> DesktopNativeGameEngineTestResult {
        port.try_submit(Self::command(&port, DesktopGameCommandsEnumAction::Quit))?
            .wait()
            .await?;
        let (supervisor, result) = worker.await?;
        result?;
        assert!(supervisor.is_shutdown_complete());
        drop((supervisor, port));
        Ok(())
    }
}

#[tokio::test]
async fn failed_search_retry_reaps_validation_process_before_explicit_resume_recreates_play()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let storage = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let old = DesktopNativeGameEngineFixture::observations();
    let validation = DesktopNativeGameEngineFixture::observations();
    let resumed = DesktopNativeGameEngineFixture::observations();
    let clock = Arc::new(DesktopNativeGameEngineFixtureClock::default());
    let factory = DesktopNativeGameEngineFixture::sequence_factory(
        &package,
        Arc::clone(&resources),
        vec![
            Arc::clone(&old),
            Arc::clone(&validation),
            Arc::clone(&resumed),
        ],
        Arc::clone(&clock),
    )?;
    let (mut supervisor, port) =
        DesktopNativeGameEngineFixture::supervisor(&storage, factory, clock).await?;
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    let playing = DesktopNativeGameEngineFixture::playing(&port, &old).await?;
    old.emit("unexpected search response".to_owned())?;
    let failed = DesktopNativeGameEngineFixture::wait_snapshot(&port, |snapshot| {
        snapshot.interruption()
            == Some(GameSessionEnumInterruption::RecoveryRequired(
                GamePauseEnumFault::Engine,
            ))
    })
    .await?;
    DesktopNativeGameEngineFixture::preserved(&playing, &failed);
    assert_eq!(failed.clock().status(), ClockStateEnumStatus::Paused);
    assert_eq!(old.live.load(Ordering::SeqCst), 0);
    assert_eq!(resources.budget.available(), 4);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert!(validation.commands()?.is_empty());
    let notice = port
        .snapshot()
        .notice
        .ok_or("missing typed engine failure")?;
    let DesktopGameSupervisorError::Engine { source } = notice.as_ref() else {
        return Err("wrong supervisor fault".into());
    };
    assert!(
        matches!(source.downcast_ref::<DesktopNativeGameEngineError<std::io::Error>>(), Some(DesktopNativeGameEngineError::Engine(EngineLiveGameError::Session(error))) if matches!(error.operation(), EngineSessionError::SearchResponse))
    );
    let mut stale =
        DesktopNativeGameEngineFixture::command(&port, DesktopGameCommandsEnumAction::RetryEngine);
    stale.expected_revision = playing.revision();
    let error = port
        .try_submit(stale)?
        .wait()
        .await
        .err()
        .ok_or("stale Retry accepted")?;
    assert!(matches!(
        error.as_ref(),
        DesktopGameSupervisorError::Revision(_)
    ));
    assert!(validation.commands()?.is_empty());
    let recovered = tokio::time::timeout(
        Duration::from_secs(3),
        port.try_submit(DesktopNativeGameEngineFixture::command(
            &port,
            DesktopGameCommandsEnumAction::RetryEngine,
        ))?
        .wait(),
    )
    .await??;
    assert_eq!(
        recovered.interruption(),
        Some(GameSessionEnumInterruption::Recovered(
            GamePauseEnumFault::Engine
        ))
    );
    assert_eq!(recovered.phase(), GameEnumPhase::Interrupted);
    assert_eq!(recovered.clock(), failed.clock());
    DesktopNativeGameEngineFixture::preserved(&failed, &recovered);
    assert_eq!(validation.live.load(Ordering::SeqCst), 0);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert_eq!(resources.budget.available(), 4);
    let ready_commands = validation.commands()?;
    assert_eq!(
        ready_commands
            .iter()
            .filter(|command| command.as_str() == "uci")
            .count(),
        1
    );
    assert!(
        ready_commands
            .iter()
            .filter(|command| command.as_str() == "isready")
            .count()
            >= 2
    );
    assert!(ready_commands.contains(&"setoption name Hash value 64".to_owned()));
    assert!(ready_commands.contains(&"setoption name Threads value 4".to_owned()));
    assert!(
        !validation
            .commands()?
            .iter()
            .any(|command| command.starts_with("go "))
    );
    assert!(resumed.commands()?.is_empty());
    old.emit("bestmove g1f3".to_owned())?;
    let error = port
        .try_submit(DesktopNativeGameEngineFixture::command(
            &port,
            DesktopGameCommandsEnumAction::RetryEngine,
        ))?
        .wait()
        .await
        .err()
        .ok_or("duplicate recovery accepted")?;
    assert!(matches!(
        error.as_ref(),
        DesktopGameSupervisorError::Transition(DesktopGameTransitionError::Session(
            GameSessionError::Pause(GamePauseError::WrongRecovery)
        ))
    ));
    assert_eq!(validation.live.load(Ordering::SeqCst), 0);
    assert!(resumed.commands()?.is_empty());
    port.try_submit(DesktopNativeGameEngineFixture::command(
        &port,
        DesktopGameCommandsEnumAction::Resume,
    ))?
    .wait()
    .await?;
    DesktopNativeGameEngineFixture::wait_commands(&resumed, "go ", 1).await?;
    DesktopNativeGameEngineFixture::preserved(&recovered, &port.snapshot().snapshot);
    assert_eq!(resources.budget.available(), 0);
    assert_eq!(validation.live.load(Ordering::SeqCst), 0);
    assert_eq!(resumed.live.load(Ordering::SeqCst), 1);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 1);
    assert!(
        !validation
            .commands()?
            .iter()
            .any(|command| command.starts_with("go "))
    );
    resumed.emit("info depth 1 score cp 0 pv g1f3".to_owned())?;
    DesktopNativeGameEngineFixture::quit(port, worker).await?;
    assert!(!old.incomplete_drop.load(Ordering::SeqCst));
    assert!(!validation.incomplete_drop.load(Ordering::SeqCst));
    assert!(!resumed.incomplete_drop.load(Ordering::SeqCst));
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    drop(resources);
    assert_eq!(storage.budget.available(), 2);
    storage.close().await
}

#[tokio::test]
async fn resume_before_explicit_retry_never_spawns_a_replacement()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let storage = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let old = DesktopNativeGameEngineFixture::observations();
    let fresh = DesktopNativeGameEngineFixture::observations();
    let clock = Arc::new(DesktopNativeGameEngineFixtureClock::default());
    let factory = DesktopNativeGameEngineFixture::sequence_factory(
        &package,
        Arc::clone(&resources),
        vec![Arc::clone(&old), Arc::clone(&fresh)],
        Arc::clone(&clock),
    )?;
    let (mut supervisor, port) =
        DesktopNativeGameEngineFixture::supervisor(&storage, factory, clock).await?;
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    DesktopNativeGameEngineFixture::playing(&port, &old).await?;
    old.emit("unexpected search response".to_owned())?;
    let failed = DesktopNativeGameEngineFixture::wait_snapshot(&port, |snapshot| {
        snapshot.phase() == GameEnumPhase::Interrupted
    })
    .await?;
    let error = port
        .try_submit(DesktopNativeGameEngineFixture::command(
            &port,
            DesktopGameCommandsEnumAction::Resume,
        ))?
        .wait()
        .await
        .err()
        .ok_or("unrecovered Resume accepted")?;
    assert!(
        fresh.commands()?.is_empty(),
        "Resume created an engine before explicit Retry"
    );
    assert!(matches!(
        error.as_ref(),
        DesktopGameSupervisorError::Transition(DesktopGameTransitionError::Session(
            GameSessionError::Pause(GamePauseError::RecoveryRequired(GamePauseEnumFault::Engine))
        ))
    ));
    assert_eq!(port.snapshot().snapshot.clock(), failed.clock());
    DesktopNativeGameEngineFixture::quit(port, worker).await?;
    drop(resources);
    assert_eq!(storage.budget.available(), 2);
    storage.close().await
}

#[tokio::test]
async fn saturated_search_controls_keep_receipt_deadlines_and_settle_every_admitted_request()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let storage = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let old = DesktopNativeGameEngineFixture::observations();
    let clock = Arc::new(DesktopNativeGameEngineFixtureClock::default());
    let factory = DesktopNativeGameEngineFixture::sequence_factory(
        &package,
        Arc::clone(&resources),
        vec![Arc::clone(&old)],
        Arc::clone(&clock),
    )?;
    let (mut supervisor, port) =
        DesktopNativeGameEngineFixture::supervisor(&storage, factory, Arc::clone(&clock)).await?;
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    let playing = DesktopNativeGameEngineFixture::playing(&port, &old).await?;
    let mut queued = Vec::new();
    for _ in 0..DesktopGameSupervisorPort::ORDINARY_CAPACITY {
        queued.push(port.try_submit(DesktopNativeGameEngineFixture::command(
            &port,
            DesktopGameCommandsEnumAction::Navigate {
                node: playing.root(),
            },
        ))?);
    }
    assert!(matches!(
        port.try_submit(DesktopNativeGameEngineFixture::command(
            &port,
            DesktopGameCommandsEnumAction::RetryEngine
        )),
        Err(DesktopGameSupervisorError::Busy)
    ));
    let stop = port.try_submit(DesktopNativeGameEngineFixture::command(
        &port,
        DesktopGameCommandsEnumAction::Stop,
    ))?;
    let pause = port.try_submit(DesktopNativeGameEngineFixture::command(
        &port,
        DesktopGameCommandsEnumAction::Pause {
            cause: chess::game::pause::GamePauseEnumCause::Manual,
        },
    ))?;
    assert!(matches!(
        port.try_submit(DesktopNativeGameEngineFixture::command(
            &port,
            DesktopGameCommandsEnumAction::Stop
        )),
        Err(DesktopGameSupervisorError::Busy)
    ));
    assert!(matches!(
        port.try_submit(DesktopNativeGameEngineFixture::command(
            &port,
            DesktopGameCommandsEnumAction::Pause {
                cause: chess::game::pause::GamePauseEnumCause::Manual
            }
        )),
        Err(DesktopGameSupervisorError::Busy)
    ));
    clock.0.store(3_000_000_000, Ordering::SeqCst);
    let stopped = stop.wait().await;
    assert!(
        matches!(stopped, Err(ref error) if matches!(error.as_ref(), DesktopGameSupervisorError::Engine { source } if matches!(source.downcast_ref::<DesktopNativeGameEngineError<std::io::Error>>(), Some(DesktopNativeGameEngineError::Engine(EngineLiveGameError::Session(error))) if matches!(error.operation(), EngineSessionError::StopDeadline))))
    );
    assert!(!old.commands()?.iter().any(|command| command == "stop"));
    let paused = pause.wait().await;
    assert!(paused.is_err());
    let failed = DesktopNativeGameEngineFixture::wait_snapshot(&port, |snapshot| {
        snapshot.interruption()
            == Some(GameSessionEnumInterruption::RecoveryRequired(
                GamePauseEnumFault::Engine,
            ))
    })
    .await?;
    DesktopNativeGameEngineFixture::preserved(&playing, &failed);
    assert_eq!(resources.budget.available(), 4);
    let mut quit =
        DesktopNativeGameEngineFixture::command(&port, DesktopGameCommandsEnumAction::Quit);
    quit.received_at = clock.now()?;
    port.try_submit(quit)?.wait().await?;
    let (supervisor, result) = worker.await?;
    result?;
    assert!(supervisor.is_shutdown_complete());
    for receipt in queued {
        let result = receipt.wait().await;
        assert!(
            matches!(result, Err(ref error) if matches!(error.as_ref(), DesktopGameSupervisorError::Closed | DesktopGameSupervisorError::Revision(_)))
        );
    }
    assert!(!old.incomplete_drop.load(Ordering::SeqCst));
    drop((supervisor, port, resources));
    storage.close().await
}

#[tokio::test(start_paused = true)]
async fn retry_validation_and_resumed_session_use_distinct_generations_and_isolate_old_output()
-> DesktopNativeGameEngineTestResult {
    let package = EnginePackageFixture::new()?;
    let resources = DesktopNativeGameEngineFixture::resources()?;
    let old = DesktopNativeGameEngineFixture::observations();
    let validation = DesktopNativeGameEngineFixture::observations();
    let resumed = DesktopNativeGameEngineFixture::observations();
    let mut factory = DesktopNativeGameEngineFixture::sequence_factory(
        &package,
        Arc::clone(&resources),
        vec![
            Arc::clone(&old),
            Arc::clone(&validation),
            Arc::clone(&resumed),
        ],
        Arc::new(DesktopNativeGameEngineFixtureClock::default()),
    )?;
    let context = DesktopNativeGameEngineFixture::context(GameRevision::ZERO)?;
    let mut engine = factory.create(context).await?;
    let former = engine
        .start(DesktopNativeGameEngineFixture::request(GameRevision::ZERO)?)
        .await?;
    old.emit("unexpected search response".to_owned())?;
    assert!(
        matches!(engine.next_event(context).await, Err(DesktopNativeGameEngineError::Engine(EngineLiveGameError::Session(error))) if matches!(error.operation(), EngineSessionError::SearchResponse))
    );
    engine
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(engine.is_shutdown_complete());
    assert_eq!(old.live.load(Ordering::SeqCst), 0);
    let mut validated = factory.create(context).await?;
    assert!(validated.is_idle());
    let ready = validated
        .quiesce(context, Instant::now().into_std())
        .await?;
    assert_ne!(former.generation(), ready.generation());
    assert!(ready.stopped_search().is_none());
    assert!(
        !validation
            .commands()?
            .iter()
            .any(|command| command.starts_with("go "))
    );
    validated
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(validated.is_shutdown_complete());
    assert_eq!(validation.live.load(Ordering::SeqCst), 0);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert_eq!(resources.budget.available(), 4);
    assert!(resumed.commands()?.is_empty());
    let mut replacement = factory.create(context).await?;
    assert!(replacement.is_idle());
    assert!(
        !resumed
            .commands()?
            .iter()
            .any(|command| command.starts_with("go "))
    );
    let current = replacement
        .start(DesktopNativeGameEngineFixture::request(GameRevision::ZERO)?)
        .await?;
    assert_ne!(former.generation(), current.generation());
    assert_ne!(ready.generation(), current.generation());
    assert_ne!(former.search(), current.search());
    assert_eq!(former.context(), current.context());
    assert!(matches!(
        replacement.stop(former, Instant::now().into_std()).await,
        Err(DesktopNativeGameEngineError::Engine(
            EngineLiveGameError::StaleSearch
        ))
    ));
    assert!(!resumed.commands()?.iter().any(|command| command == "stop"));
    old.emit("bestmove e2e4".to_owned())?;
    validation.emit("bestmove e2e4".to_owned())?;
    resumed.emit("info depth 1 score cp 0 pv d2d4".to_owned())?;
    assert!(
        matches!(replacement.next_event(context).await?, GameEngineEnumEvent::Info { identity, .. } if identity == current)
    );
    replacement.stop(current, Instant::now().into_std()).await?;
    replacement
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(resources.budget.available(), 4);
    assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
    assert!(!old.incomplete_drop.load(Ordering::SeqCst));
    assert!(!validation.incomplete_drop.load(Ordering::SeqCst));
    assert!(!resumed.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn failed_and_cancelled_retry_hold_owned_resources_until_cleanup_and_allow_later_retry()
-> DesktopNativeGameEngineTestResult {
    for cancel in [false, true] {
        let package = EnginePackageFixture::new()?;
        let storage = DesktopGameSupervisorFixtureStorage::new(2).await?;
        let resources = DesktopNativeGameEngineFixture::resources()?;
        let old = DesktopNativeGameEngineFixture::observations();
        let rejected = DesktopNativeGameEngineFixture::observations();
        let fresh = DesktopNativeGameEngineFixture::observations();
        let clock = Arc::new(DesktopNativeGameEngineFixtureClock::default());
        let factory = DesktopNativeGameEngineFixture::sequence_factory(
            &package,
            Arc::clone(&resources),
            vec![Arc::clone(&old), Arc::clone(&rejected), Arc::clone(&fresh)],
            Arc::clone(&clock),
        )?;
        let (mut supervisor, port) =
            DesktopNativeGameEngineFixture::supervisor(&storage, factory, clock).await?;
        let worker = tokio::spawn(async move {
            let result = supervisor.run().await;
            (supervisor, result)
        });
        DesktopNativeGameEngineFixture::playing(&port, &old).await?;
        old.emit("unexpected search response".to_owned())?;
        let failed = DesktopNativeGameEngineFixture::wait_snapshot(&port, |snapshot| {
            snapshot.phase() == GameEnumPhase::Interrupted
        })
        .await?;
        rejected.block_cleanup.store(true, Ordering::SeqCst);
        if cancel {
            rejected.acknowledge.store(false, Ordering::SeqCst);
        } else {
            rejected.emit("unexpected startup response".to_owned())?;
        }
        let mut retry = port.try_submit(DesktopNativeGameEngineFixture::command(
            &port,
            DesktopGameCommandsEnumAction::RetryEngine,
        ))?;
        let control = if cancel {
            DesktopNativeGameEngineFixture::wait_commands(&rejected, "setoption name Threads", 1)
                .await?;
            Some(port.try_submit(DesktopNativeGameEngineFixture::command(
                &port,
                DesktopGameCommandsEnumAction::Stop,
            ))?)
        } else {
            None
        };
        tokio::time::timeout(Duration::from_secs(3), rejected.cleanup_entered.notified()).await?;
        assert!(retry.try_result()?.is_none());
        assert_eq!(rejected.live.load(Ordering::SeqCst), 1);
        assert_eq!(resources.registrations.load(Ordering::SeqCst), 1);
        assert_eq!(resources.budget.available(), 0);
        assert!(fresh.commands()?.is_empty());
        assert_eq!(port.snapshot().snapshot.clock(), failed.clock());
        rejected.block_cleanup.store(false, Ordering::SeqCst);
        rejected.cleanup_ready.notify_one();
        let error = retry.wait().await.err().ok_or("failed Retry succeeded")?;
        if let Some(control) = control {
            assert!(matches!(error.as_ref(), DesktopGameSupervisorError::Closed));
            let stopped = control.wait().await?;
            assert_eq!(stopped.interruption(), failed.interruption());
            assert_eq!(stopped.revision(), failed.revision());
        } else {
            assert!(
                matches!(error.as_ref(), DesktopGameSupervisorError::Factory { source } if matches!(source.downcast_ref::<DesktopNativeGameEngineError<std::io::Error>>(), Some(DesktopNativeGameEngineError::Startup(error)) if matches!(error.operation(), EngineSessionError::UnexpectedResponse)))
            );
        }
        assert_eq!(rejected.live.load(Ordering::SeqCst), 0);
        assert_eq!(resources.budget.available(), 4);
        assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
        assert_eq!(port.snapshot().snapshot.revision(), failed.revision());
        assert_eq!(
            port.snapshot().snapshot.interruption(),
            failed.interruption()
        );
        DesktopNativeGameEngineFixture::preserved(&failed, &port.snapshot().snapshot);
        assert!(
            !rejected
                .commands()?
                .iter()
                .any(|command| command.starts_with("go "))
        );
        let recovered = port
            .try_submit(DesktopNativeGameEngineFixture::command(
                &port,
                DesktopGameCommandsEnumAction::RetryEngine,
            ))?
            .wait()
            .await?;
        assert_eq!(recovered.phase(), GameEnumPhase::Interrupted);
        assert_eq!(recovered.clock(), failed.clock());
        assert_eq!(fresh.live.load(Ordering::SeqCst), 0);
        assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
        assert_eq!(resources.budget.available(), 4);
        assert!(!fresh.incomplete_drop.load(Ordering::SeqCst));
        assert!(
            !fresh
                .commands()?
                .iter()
                .any(|command| command.starts_with("go "))
        );
        DesktopNativeGameEngineFixture::quit(port, worker).await?;
        assert!(!rejected.incomplete_drop.load(Ordering::SeqCst));
        assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
        drop(resources);
        assert_eq!(storage.budget.available(), 2);
        storage.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn reserved_stop_pause_and_quit_progress_while_a_search_and_ordinary_queue_are_full()
-> DesktopNativeGameEngineTestResult {
    for action in [
        DesktopGameCommandsEnumAction::Stop,
        DesktopGameCommandsEnumAction::Pause {
            cause: chess::game::pause::GamePauseEnumCause::Manual,
        },
        DesktopGameCommandsEnumAction::Quit,
    ] {
        let package = EnginePackageFixture::new()?;
        let storage = DesktopGameSupervisorFixtureStorage::new(2).await?;
        let resources = DesktopNativeGameEngineFixture::resources()?;
        let old = DesktopNativeGameEngineFixture::observations();
        let clock = Arc::new(DesktopNativeGameEngineFixtureClock::default());
        let factory = DesktopNativeGameEngineFixture::sequence_factory(
            &package,
            Arc::clone(&resources),
            vec![Arc::clone(&old)],
            Arc::clone(&clock),
        )?;
        let (mut supervisor, port) =
            DesktopNativeGameEngineFixture::supervisor(&storage, factory, clock).await?;
        let worker = tokio::spawn(async move {
            let result = supervisor.run().await;
            (supervisor, result)
        });
        let playing = DesktopNativeGameEngineFixture::playing(&port, &old).await?;
        let mut queued = Vec::new();
        for _ in 0..DesktopGameSupervisorPort::ORDINARY_CAPACITY {
            queued.push(port.try_submit(DesktopNativeGameEngineFixture::command(
                &port,
                DesktopGameCommandsEnumAction::RetryEngine,
            ))?);
        }
        assert!(matches!(
            port.try_submit(DesktopNativeGameEngineFixture::command(
                &port,
                DesktopGameCommandsEnumAction::RetryEngine
            )),
            Err(DesktopGameSupervisorError::Busy)
        ));
        let interrupted = port
            .try_submit(DesktopNativeGameEngineFixture::command(
                &port,
                action.clone(),
            ))?
            .wait()
            .await?;
        DesktopNativeGameEngineFixture::preserved(&playing, &interrupted);
        assert_eq!(interrupted.phase(), GameEnumPhase::Interrupted);
        assert_eq!(interrupted.clock().status(), ClockStateEnumStatus::Paused);
        if action == DesktopGameCommandsEnumAction::Quit {
            let (supervisor, result) = worker.await?;
            result?;
            assert!(supervisor.is_shutdown_complete());
            drop(supervisor);
        } else {
            DesktopNativeGameEngineFixture::quit(port.clone(), worker).await?;
        }
        for receipt in queued {
            assert!(
                matches!(receipt.wait().await, Err(ref error) if matches!(error.as_ref(), DesktopGameSupervisorError::Closed | DesktopGameSupervisorError::Revision(_)))
            );
        }
        assert_eq!(old.live.load(Ordering::SeqCst), 0);
        assert_eq!(resources.budget.available(), 4);
        assert_eq!(resources.registrations.load(Ordering::SeqCst), 0);
        assert!(!old.incomplete_drop.load(Ordering::SeqCst));
        drop((port, resources));
        storage.close().await?;
    }
    Ok(())
}
