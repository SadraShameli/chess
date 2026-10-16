use std::collections::VecDeque;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::game_supervisor::{
    DesktopGameSupervisorEngineFactory, DesktopGameSupervisorEngineProvider,
};
use chess::desktop::native::game_engine::{
    DesktopNativeGameEngineAllocation, DesktopNativeGameEngineFactory,
    DesktopNativeGameEngineFactoryOptions, DesktopNativeGameEngineResources,
};
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::resource_config::EngineResourceConfig;
use chess::game::engine_provider::{
    GameEngineContext, GameEngineEnumEvent, GameEngineProvider, GameEngineRequest,
    GameEngineSearchConfig,
};
use chess::game::enums::GameRevision;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::engine_package_selection::EnginePackageFixture;

type DesktopNativeGameEngineTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct DesktopNativeGameEngineFixtureClock;

impl ClockTimeProvider for DesktopNativeGameEngineFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(0))
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
        match command {
            "uci" => {
                self.observations
                    .emit("id name Reckless 0.9.0".to_owned())?;
                self.observations
                    .emit(format!("id author {}", EngineIdentity::AUTHOR))?;
                for option in EngineEnumOption::ALL {
                    self.observations.emit(format!(
                        "option name {} type {}",
                        option.name(),
                        option.descriptor()
                    ))?;
                }
                self.observations.emit("uciok".to_owned())?;
            }
            "isready" => self.observations.emit("readyok".to_owned())?,
            "stop" => self.observations.emit("bestmove e2e4".to_owned())?,
            value if self.observations.acknowledge.load(Ordering::SeqCst) => {
                for option in EngineEnumOption::ALL {
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
                clock: Arc::new(DesktopNativeGameEngineFixtureClock),
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
