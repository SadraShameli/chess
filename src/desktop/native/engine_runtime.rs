use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::capacity::lease::CapacityLease;
use crate::clock::time_provider::ClockTimeProviderMonotonic;
use crate::desktop::application::DesktopApplicationWakeProvider;
use crate::desktop::engine_bridge::{
    DesktopEngineBridgeEnumEvent, DesktopEngineBridgePort, DesktopEngineBridgeStreams,
};
use crate::desktop::engine_commands::{
    DesktopEngineCommands, DesktopEngineCommandsAdmissionProvider, DesktopEngineCommandsServices,
};
use crate::desktop::engine_selection::DesktopEngineSelection;
use crate::desktop::engine_service::DesktopEngineService;
use crate::desktop::engine_tablebases::{
    DesktopEngineTablebasesPolicy, DesktopEngineTablebasesPolicyProvider,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::desktop::native::analysis_resources::DesktopNativeAnalysisResources;
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
use crate::desktop::native::game_resources::{
    DesktopNativeGameResources, DesktopNativeGameResourcesAnalysisReservation,
    DesktopNativeGameResourcesMemoryProvider,
};
use crate::desktop::workbench_commands::{
    DesktopWorkbenchCommandsObservation, DesktopWorkbenchCommandsObservationProvider,
};
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::EngineProcessProvider;
use crate::engine::resource_config::EngineResourceConfig;
use crate::engine::search::session::EngineSearchSession;
use crate::engine::session::{EngineSession, EngineSessionStartup};
use crate::game::engine_provider::{GameEngineContext, GameEngineSearchConfig};
use crate::game::enums::{GameEnumPhase, GameEnumSeat};
use crate::presentation::engine_panels::{
    PresentationEnginePanelsConnection, PresentationEnginePanelsJobUpdate,
};
use crate::presentation::workbench::controller::{
    PresentationWorkbenchControllerEnumAdmission, PresentationWorkbenchControllerEnumBusy,
};
use crate::presentation::workbench::snapshot::PresentationWorkbenchJobSnapshot;

pub struct DesktopNativeEngineRuntimeObservation {
    progress: watch::Sender<Option<PresentationEnginePanelsJobUpdate>>,
    wake: Arc<DesktopNativeEventLoopWake>,
}

impl DesktopWorkbenchCommandsObservationProvider for DesktopNativeEngineRuntimeObservation {
    fn observe(
        &mut self,
        observation: DesktopWorkbenchCommandsObservation<'_>,
    ) -> Result<(), DesktopFailure> {
        let snapshot =
            PresentationWorkbenchJobSnapshot::capture(observation.job).map_err(|error| {
                DesktopFailure::new(
                    DesktopEnumFailureCode::InternalFailure,
                    error.to_string(),
                    true,
                )
            })?;
        self.progress
            .send_replace(Some(PresentationEnginePanelsJobUpdate {
                context: observation.context,
                request: observation.request,
                snapshot,
            }));
        self.wake.wake().map_err(|error| {
            DesktopFailure::new(
                DesktopEnumFailureCode::InternalFailure,
                error.to_string(),
                true,
            )
        })
    }
}

type DesktopNativeEngineRuntimeService<P, M> = DesktopEngineService<
    P,
    DesktopNativeAnalysisResources<M>,
    DesktopEngineSelection,
    ClockTimeProviderMonotonic,
    DesktopEngineTablebasesPolicy,
    DesktopNativeEngineRuntimeObservation,
>;
type DesktopNativeEngineRuntimeTask<P, M> = JoinHandle<(
    DesktopNativeEngineRuntimeService<P, M>,
    Result<(), DesktopFailure>,
)>;

pub struct DesktopNativeEngineRuntimeOptions<P, M: DesktopNativeGameResourcesMemoryProvider> {
    pub provider: P,
    pub package: EnginePackageSelected,
    pub resources: Arc<DesktopNativeGameResources<M>>,
    pub executor: Arc<CapacityExecutor>,
    pub clock: Arc<ClockTimeProviderMonotonic>,
    pub wake: Arc<DesktopNativeEventLoopWake>,
    pub snapshots: watch::Receiver<Vec<DesktopGameSnapshot>>,
    pub budget: CapacityBudget,
    pub tablebase_parent: PathBuf,
}

pub struct DesktopNativeEngineRuntime<
    P: EngineProcessProvider,
    M: DesktopNativeGameResourcesMemoryProvider,
> {
    options: DesktopNativeEngineRuntimeOptions<Option<P>, M>,
    admission: Option<DesktopNativeAnalysisResources<M>>,
    live_demand: watch::Receiver<Option<GameEngineContext>>,
    owner: DesktopMessagesEntityId,
    startup: Option<EngineSessionStartup<P::Process>>,
    session: Option<EngineSession<P::Process>>,
    lease: Option<CapacityLease>,
    reservation: Option<DesktopNativeGameResourcesAnalysisReservation>,
    task: Option<DesktopNativeEngineRuntimeTask<P, M>>,
    retained: Option<DesktopNativeEngineRuntimeService<P, M>>,
    port: Option<DesktopEngineBridgePort>,
    delivery: Option<JoinHandle<Result<(), DesktopFailure>>>,
    drain: watch::Sender<bool>,
    stopped: bool,
    terminal_failure: Option<DesktopFailure>,
    worker_failure: Option<DesktopFailure>,
}

impl<P, M> DesktopNativeEngineRuntime<P, M>
where
    P: EngineProcessProvider + Send + Sync + 'static,
    P::Process: Sync + 'static,
    M: DesktopNativeGameResourcesMemoryProvider,
{
    pub fn new(options: DesktopNativeEngineRuntimeOptions<P, M>) -> Self {
        let owner = DesktopMessagesEntityId::new();
        let admission = DesktopNativeAnalysisResources::new(owner, Arc::clone(&options.resources));
        let live_demand = admission.live_demand();
        let (drain, _) = watch::channel(false);
        let options = DesktopNativeEngineRuntimeOptions {
            provider: Some(options.provider),
            package: options.package,
            resources: options.resources,
            executor: options.executor,
            clock: options.clock,
            wake: options.wake,
            snapshots: options.snapshots,
            budget: options.budget,
            tablebase_parent: options.tablebase_parent,
        };
        Self {
            options,
            admission: Some(admission),
            live_demand,
            owner,
            startup: None,
            session: None,
            lease: None,
            reservation: None,
            task: None,
            retained: None,
            port: None,
            delivery: None,
            drain,
            stopped: false,
            terminal_failure: None,
            worker_failure: None,
        }
    }

    pub async fn initialize(
        &mut self,
    ) -> Result<(DesktopEngineBridgePort, PresentationEnginePanelsConnection), DesktopFailure> {
        if self.task.is_some() || self.retained.is_some() || self.stopped {
            return Err(Self::failure(
                "The native analysis owner has already started or stopped",
            ));
        }
        let policy = self.options.resources.policy();
        let configuration = EngineResourceConfig::new(
            policy.workers(),
            policy.startup_hash_mib(),
            GameEngineSearchConfig::initial_default().move_overhead(),
        )
        .map_err(Self::effect)?;
        Self::check_demand(&mut self.live_demand)?;
        if self.reservation.is_none() {
            let admission = self.admission.as_ref().ok_or_else(|| {
                Self::failure("The initial analysis resource owner is unavailable")
            })?;
            let (lease, reservation) = tokio::select! {
                biased;
                demand = Self::next_demand(&mut self.live_demand) => { demand?; return Err(Self::failure("Live engine demand interrupted initial analysis admission")); }
                allocation = admission.initial(configuration) => allocation?,
            };
            self.lease = Some(lease);
            self.reservation = Some(reservation);
        }
        Self::check_demand(&mut self.live_demand)?;
        if self.startup.is_none() && self.session.is_none() {
            self.startup = Some(
                EngineSessionStartup::new(
                    self.options.provider.as_ref().ok_or_else(|| {
                        Self::failure("The analysis process provider is unavailable")
                    })?,
                    self.options.package.clone(),
                )
                .map_err(Self::effect)?,
            );
        }
        if self.session.is_none() {
            let startup = self
                .startup
                .as_mut()
                .ok_or_else(|| Self::failure("The analysis startup owner is unavailable"))?;
            let engine = tokio::select! {
                biased;
                demand = Self::next_demand(&mut self.live_demand) => { demand?; return Err(Self::failure("Live engine demand interrupted analysis startup; its child remains owned until cleanup")); }
                engine = startup.initialize() => engine.map_err(Self::effect)?,
            };
            self.session = Some(engine);
            self.startup = None;
        }
        let engine = self
            .session
            .as_mut()
            .ok_or_else(|| Self::failure("The analysis session owner is unavailable"))?;
        tokio::select! {
            biased;
            demand = Self::next_demand(&mut self.live_demand) => { demand?; return Err(Self::failure("Live engine demand interrupted analysis configuration; its child remains owned until cleanup")); }
            configured = async {
                engine.configure_resources(configuration).await.map_err(Self::effect)?;
                engine.new_game(crate::board::enums::BoardEnumVariant::Standard).await.map_err(Self::effect)?;
                Ok::<_, DesktopFailure>(())
            } => configured?,
        }
        Self::check_demand(&mut self.live_demand)?;
        let admission = self
            .admission
            .as_mut()
            .ok_or_else(|| Self::failure("The analysis resource owner is unavailable"))?;
        admission
            .acknowledge(self.reservation.as_ref().ok_or_else(|| {
                Self::failure("The configured analysis hash reservation is unavailable")
            })?)
            .await?;
        self.lease = None;
        let (selected_sender, selected) = watch::channel(None);
        let selection = DesktopEngineSelection::new(self.options.snapshots.clone(), selected);
        let engine = self
            .session
            .take()
            .ok_or_else(|| Self::failure("The configured analysis session is unavailable"))?;
        let commands =
            DesktopEngineCommands::new(
                EngineSearchSession::new(engine),
                DesktopEngineCommandsServices {
                    provider: self.options.provider.take().ok_or_else(|| {
                        Self::failure("The analysis process provider is unavailable")
                    })?,
                    admission: self.admission.take().ok_or_else(|| {
                        Self::failure("The admitted analysis owner is unavailable")
                    })?,
                    selection,
                    executor: Arc::clone(&self.options.executor),
                    reservation: self.reservation.take().ok_or_else(|| {
                        Self::failure("The admitted hash reservation is unavailable")
                    })?,
                    owner: self.owner,
                },
            );
        let initial = commands.snapshot();
        let (progress, workbench) = watch::channel(None);
        let observer = DesktopNativeEngineRuntimeObservation {
            progress,
            wake: Arc::clone(&self.options.wake),
        };
        let tablebase_policy =
            DesktopEngineTablebasesPolicy::new(self.options.tablebase_parent.clone());
        let tablebase_setup = tablebase_policy.policy().map(|_| ());
        let (service, port, streams) = DesktopEngineService::new(
            commands,
            Arc::clone(&self.options.clock),
            tablebase_policy,
            observer,
        );
        let mut service = service.with_live_demand(self.live_demand.clone());
        let wake = Arc::clone(&self.options.wake);
        let (health, health_receiver) = watch::channel(None);
        self.task = Some(tokio::spawn(async move {
            let result = service.run().await;
            if let Err(failure) = &result {
                health.send_replace(Some(failure.clone()));
            }
            let wake_result = wake.wake().map_err(Self::effect);
            let result = result.and(wake_result);
            (service, result)
        }));
        let (terminal, receiver) = mpsc::channel(DesktopEngineBridgePort::EVENT_CAPACITY);
        let telemetry = streams.telemetry.clone();
        let (admission_sender, admission) = watch::channel(Self::admission(
            &self.options.budget,
            &self.options.snapshots,
            policy.workers(),
        ));
        self.delivery = Some(tokio::spawn(Self::deliver(
            DesktopNativeEngineRuntimeDelivery {
                streams,
                terminal,
                snapshots: self.options.snapshots.clone(),
                budget: self.options.budget.clone(),
                workers: policy.workers(),
                admission: admission_sender,
                wake: Arc::clone(&self.options.wake),
                drain: self.drain.subscribe(),
            },
        )));
        self.port = Some(port.clone());
        Ok((
            port,
            PresentationEnginePanelsConnection {
                owner: self.owner,
                initial,
                selected: selected_sender,
                telemetry,
                terminal: receiver,
                workbench,
                admission,
                health: health_receiver,
                tablebase_setup,
            },
        ))
    }

    fn check_demand(
        demand: &mut watch::Receiver<Option<GameEngineContext>>,
    ) -> Result<(), DesktopFailure> {
        if demand.has_changed().map_err(Self::effect)? && demand.borrow_and_update().is_some() {
            Err(Self::failure(
                "A new live engine demand interrupted analysis startup",
            ))
        } else {
            Ok(())
        }
    }

    async fn next_demand(
        demand: &mut watch::Receiver<Option<GameEngineContext>>,
    ) -> Result<(), DesktopFailure> {
        loop {
            demand.changed().await.map_err(Self::effect)?;
            if demand.borrow_and_update().is_some() {
                return Ok(());
            }
        }
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.stopped
            && self.worker_failure.is_none()
            && self.task.is_none()
            && self.delivery.is_none()
            && self
                .retained
                .as_ref()
                .is_none_or(DesktopEngineService::is_shutdown_complete)
            && self
                .startup
                .as_ref()
                .is_none_or(EngineSessionStartup::is_shutdown_complete)
            && self
                .session
                .as_ref()
                .is_none_or(EngineSession::is_shutdown_complete)
            && self.lease.is_none()
            && self.reservation.is_none()
    }

    pub async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), DesktopFailure> {
        self.drain.send_replace(true);
        if let Some(failure) = &self.worker_failure {
            return Err(failure.clone());
        }
        if let Some(task) = &mut self.task {
            if !task.is_finished()
                && let Some(port) = &self.port
            {
                let result = timeout_at(deadline, port.shutdown(deadline))
                    .await
                    .map_err(Self::effect)?;
                if let Err(failure) = result {
                    self.terminal_failure = Some(failure);
                }
            }
            let joined = timeout_at(deadline, task).await.map_err(Self::effect)?;
            self.task = None;
            let (service, result) = match joined {
                Ok(outcome) => outcome,
                Err(error) => {
                    let failure = Self::effect(error);
                    self.worker_failure = Some(failure.clone());
                    return Err(failure);
                }
            };
            self.retained = Some(service);
            if let Err(failure) = result {
                self.terminal_failure = Some(failure);
            }
        }
        if let Some(service) = &mut self.retained {
            if !service.is_shutdown_complete() {
                let result = timeout_at(deadline, service.shutdown_retained(deadline))
                    .await
                    .map_err(Self::effect)?;
                if let Err(failure) = result {
                    self.terminal_failure = Some(failure);
                }
            }
            if !service.is_shutdown_complete() {
                return Err(Self::failure(
                    "The native engine service still retains an owned child or terminal event",
                ));
            }
            self.retained = None;
            self.port = None;
        }
        if let Some(startup) = &mut self.startup {
            let outcome = startup
                .shutdown_retained(deadline)
                .await
                .map_err(Self::effect);
            if !startup.is_shutdown_complete() {
                return outcome.and(Err(Self::failure("Analysis startup cleanup is unresolved")));
            }
            self.startup = None;
            if let Err(failure) = outcome {
                self.terminal_failure = Some(failure);
            }
        }
        if let Some(engine) = &mut self.session {
            let outcome = engine
                .shutdown_retained(deadline)
                .await
                .map_err(Self::effect);
            if !engine.is_shutdown_complete() {
                return outcome.and(Err(Self::failure("Analysis session cleanup is unresolved")));
            }
            self.session = None;
            if let Err(failure) = outcome {
                self.terminal_failure = Some(failure);
            }
        }
        self.lease = None;
        self.reservation = None;
        self.admission = None;
        if let Some(delivery) = &mut self.delivery {
            let joined = timeout_at(deadline, delivery).await.map_err(Self::effect)?;
            self.delivery = None;
            match joined {
                Ok(Ok(())) => {}
                Ok(Err(failure)) => self.terminal_failure = Some(failure),
                Err(error) => self.terminal_failure = Some(Self::effect(error)),
            }
        }
        self.stopped = true;
        self.terminal_failure.clone().map_or(Ok(()), Err)
    }

    fn admission(
        budget: &CapacityBudget,
        snapshots: &watch::Receiver<Vec<DesktopGameSnapshot>>,
        workers: usize,
    ) -> PresentationWorkbenchControllerEnumAdmission {
        if snapshots.borrow().iter().any(|snapshot| {
            snapshot.phase() == GameEnumPhase::Active
                && snapshot.seats().seat(snapshot.live_turn()) == GameEnumSeat::Reckless
        }) {
            PresentationWorkbenchControllerEnumAdmission::Busy(
                PresentationWorkbenchControllerEnumBusy::LiveGame,
            )
        } else {
            PresentationWorkbenchControllerEnumAdmission::Idle {
                available_workers: budget.available().min(workers),
            }
        }
    }

    async fn deliver(
        mut delivery: DesktopNativeEngineRuntimeDelivery,
    ) -> Result<(), DesktopFailure> {
        let mut telemetry_open = true;
        let mut snapshots_open = true;
        let mut consumer_open = true;
        let mut failure = None;
        loop {
            tokio::select! {
                biased;
                changed = delivery.drain.changed(), if !*delivery.drain.borrow() => { changed.map_err(Self::effect)?; }
                event = delivery.streams.terminal.recv() => {
                    let Some(event) = event else { return failure.map_or(Ok(()), Err); };
                    if !*delivery.drain.borrow() && consumer_open {
                        if let Err(error) = delivery.wake.wake() { failure = Some(Self::effect(error)); }
                        tokio::select! {
                            biased;
                            changed = delivery.drain.changed() => { changed.map_err(Self::effect)?; }
                            sent = delivery.terminal.send(event) => {
                                if sent.is_err() {
                                    consumer_open = false;
                                    failure = Some(Self::failure("The mounted native engine terminal consumer disconnected; remaining terminal events are drained by the root"));
                                }
                                if let Err(error) = delivery.wake.wake() { failure = Some(Self::effect(error)); }
                            }
                        }
                    }
                }
                changed = delivery.streams.telemetry.changed(), if telemetry_open => {
                    telemetry_open = changed.is_ok();
                    if telemetry_open {
                        delivery.streams.telemetry.borrow_and_update();
                        if let Err(error) = delivery.wake.wake() { failure = Some(Self::effect(error)); }
                    }
                }
                changed = delivery.snapshots.changed(), if snapshots_open => {
                    snapshots_open = changed.is_ok();
                    if snapshots_open {
                        delivery.snapshots.borrow_and_update();
                        if let Err(error) = delivery.wake.wake() { failure = Some(Self::effect(error)); }
                    }
                }
            }
            delivery.admission.send_replace(Self::admission(
                &delivery.budget,
                &delivery.snapshots,
                delivery.workers,
            ));
        }
    }

    fn failure(message: &str) -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            message.to_owned(),
            true,
        )
    }

    fn effect(error: impl std::fmt::Display) -> DesktopFailure {
        Self::failure(&error.to_string())
    }
}

struct DesktopNativeEngineRuntimeDelivery {
    streams: DesktopEngineBridgeStreams,
    terminal: mpsc::Sender<DesktopEngineBridgeEnumEvent>,
    snapshots: watch::Receiver<Vec<DesktopGameSnapshot>>,
    budget: CapacityBudget,
    workers: usize,
    admission: watch::Sender<PresentationWorkbenchControllerEnumAdmission>,
    wake: Arc<DesktopNativeEventLoopWake>,
    drain: watch::Receiver<bool>,
}
