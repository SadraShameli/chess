use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::clock::duration::ClockInstant;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::engine_bridge::{
    DesktopEngineBridgeEnumEvent, DesktopEngineBridgeEnumLifecycle, DesktopEngineBridgeEnumResult,
    DesktopEngineBridgeEnvelope, DesktopEngineBridgeLifecycle, DesktopEngineBridgePort,
    DesktopEngineBridgeRequest, DesktopEngineBridgeStreams,
};
use crate::desktop::engine_commands::{
    DesktopEngineCommands, DesktopEngineCommandsAdmissionProvider, DesktopEngineCommandsContext,
    DesktopEngineCommandsEnumOperation, DesktopEngineCommandsOperation,
    DesktopEngineCommandsSelectionProvider, DesktopEngineCommandsWorkbenchRequest,
};
use crate::desktop::engine_tablebases::DesktopEngineTablebasesPolicyProvider;
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::request_cache::DesktopRequestCache;
use crate::desktop::workbench_commands::{
    DesktopWorkbenchCommands, DesktopWorkbenchCommandsEnumOperation,
    DesktopWorkbenchCommandsObservationProvider, DesktopWorkbenchCommandsResult,
};
use crate::engine::process_provider::{EngineProcessLimits, EngineProcessProvider};
use crate::engine::search::enums::EngineSearchEnumState;
use crate::engine::search::identity::EngineSearchTag;
use crate::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumCompletion, EngineSearchSessionEnumEvent,
    EngineSearchSessionSnapshot,
};
use crate::engine::search::stop::EngineSearchStop;
use crate::engine::tablebase::location::EngineTablebaseLocationPolicy;
use crate::game::engine_provider::GameEngineContext;
use crate::workbench::job::WorkbenchJobId;

struct DesktopEngineServiceRequest {
    operation: DesktopEngineCommandsOperation,
    receipt: DesktopApplicationReceipt,
    response: oneshot::Sender<Result<DesktopEngineBridgeEnumResult, DesktopFailure>>,
    workbench: Option<WorkbenchJobId>,
}

struct DesktopEngineServiceExecution {
    request: DesktopEngineBridgeRequest,
    workbench: Option<WorkbenchJobId>,
    now: ClockInstant,
    tablebase_policy: Option<EngineTablebaseLocationPolicy>,
}

struct DesktopEngineServiceShutdown {
    deadline: Instant,
    response: Option<oneshot::Sender<Result<(), DesktopFailure>>>,
}

#[derive(Clone, Copy)]
enum DesktopEngineServiceEnumPreemptionAction {
    Idle,
    Stop(EngineSearchTag),
    Cancel(WorkbenchJobId),
    Cleanup,
}

struct DesktopEngineServicePreemption {
    received_at: Instant,
    deadline: Instant,
    response: Option<oneshot::Sender<Result<(), DesktopFailure>>>,
}

#[derive(Clone, Copy)]
enum DesktopEngineServiceEnumDemandSource {
    Unconfigured,
    Available,
    Unavailable,
}

impl DesktopEngineServiceEnumDemandSource {
    fn is_unavailable(self) -> bool {
        matches!(self, Self::Unavailable)
    }
}

enum DesktopEngineServiceEnumPending {
    Request(DesktopEngineServiceRequest),
    Shutdown(DesktopEngineServiceShutdown),
    Preemption(DesktopEngineServicePreemption),
}

enum DesktopEngineServiceEnumCompletion {
    Request(Box<Result<DesktopEngineBridgeEnumResult, DesktopFailure>>),
    Shutdown(Result<(), DesktopFailure>),
    Preemption(Result<(), DesktopFailure>),
    Interrupted,
}

type DesktopEngineServiceFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

struct DesktopEngineServiceOperation<
    P: EngineProcessProvider,
    A: DesktopEngineCommandsAdmissionProvider,
    S,
> {
    future: DesktopEngineServiceFuture<(
        DesktopEngineCommands<P, A, S>,
        DesktopEngineServiceEnumCompletion,
    )>,
    interrupt: Option<oneshot::Sender<()>>,
    pending: DesktopEngineServiceEnumPending,
}

struct DesktopEngineServiceOutcome {
    operation: DesktopEngineCommandsOperation,
    request: DesktopRequestId,
    result: Result<DesktopEngineBridgeEnumResult, DesktopFailure>,
}

pub struct DesktopEngineService<
    P: EngineProcessProvider,
    A: DesktopEngineCommandsAdmissionProvider,
    S,
    C,
    T,
    O,
> {
    commands: Option<DesktopEngineCommands<P, A, S>>,
    operation: Option<DesktopEngineServiceOperation<P, A, S>>,
    interrupted: VecDeque<DesktopEngineServiceRequest>,
    controls: VecDeque<DesktopEngineBridgeEnvelope>,
    requested_shutdown: Option<DesktopEngineServiceShutdown>,
    shutdown_outcome: Option<Result<(), DesktopFailure>>,
    outcomes: VecDeque<DesktopEngineServiceOutcome>,
    clock: Arc<C>,
    tablebase_policy: T,
    ordinary: mpsc::Receiver<DesktopEngineBridgeEnvelope>,
    urgent: mpsc::Receiver<DesktopEngineBridgeEnvelope>,
    lifecycle: mpsc::Receiver<DesktopEngineBridgeLifecycle>,
    live_demand: Option<watch::Receiver<Option<GameEngineContext>>>,
    demand_source: DesktopEngineServiceEnumDemandSource,
    live_received_at: Option<Instant>,
    requested_preemption: Option<DesktopEngineServicePreemption>,
    interrupted_quiesce: Option<DesktopEngineServicePreemption>,
    quiesce_outcome: Option<Result<(), DesktopFailure>>,
    pending_failure: Option<DesktopFailure>,
    telemetry: watch::Sender<Option<EngineSearchSessionSnapshot>>,
    terminal: mpsc::Sender<DesktopEngineBridgeEnumEvent>,
    pending_event: Option<DesktopEngineBridgeEnumEvent>,
    pending_analysis: Option<Arc<EngineSearchSessionEnumEvent>>,
    delivered_analysis: Option<EngineSearchTag>,
    search: Option<EngineSearchTag>,
    diagnostic_running: bool,
    pending_workbench: Option<DesktopWorkbenchCommandsResult>,
    active_workbench: Option<WorkbenchJobId>,
    delivered_workbench: Option<WorkbenchJobId>,
    cancelled_admission: Option<(DesktopEngineCommandsContext, WorkbenchJobId)>,
    observer: O,
    workbench_context: Option<(DesktopEngineCommandsContext, DesktopRequestId)>,
    next_workbench_publication: Instant,
    is_closing: bool,
}

impl<P, A, S, C, T, O> DesktopEngineService<P, A, S, C, T, O>
where
    P: EngineProcessProvider + Send + Sync + 'static,
    P::Process: Sync + 'static,
    A: DesktopEngineCommandsAdmissionProvider + 'static,
    S: DesktopEngineCommandsSelectionProvider + 'static,
    C: ClockTimeProvider,
    T: DesktopEngineTablebasesPolicyProvider,
    O: DesktopWorkbenchCommandsObservationProvider,
{
    pub fn new(
        commands: DesktopEngineCommands<P, A, S>,
        clock: Arc<C>,
        tablebase_policy: T,
        observer: O,
    ) -> (Self, DesktopEngineBridgePort, DesktopEngineBridgeStreams) {
        let (ordinary_port, ordinary) = mpsc::channel(DesktopEngineBridgePort::ORDINARY_CAPACITY);
        let (urgent_port, urgent) = mpsc::channel(DesktopEngineBridgePort::URGENT_CAPACITY);
        let (lifecycle_port, lifecycle) =
            mpsc::channel(DesktopEngineBridgePort::LIFECYCLE_CAPACITY);
        let (telemetry, telemetry_receiver) = watch::channel(None);
        let (terminal, terminal_receiver) = mpsc::channel(DesktopEngineBridgePort::EVENT_CAPACITY);
        (
            Self {
                commands: Some(commands),
                operation: None,
                interrupted: VecDeque::new(),
                controls: VecDeque::new(),
                requested_shutdown: None,
                shutdown_outcome: None,
                outcomes: VecDeque::new(),
                clock,
                tablebase_policy,
                ordinary,
                urgent,
                lifecycle,
                live_demand: None,
                demand_source: DesktopEngineServiceEnumDemandSource::Unconfigured,
                live_received_at: None,
                requested_preemption: None,
                interrupted_quiesce: None,
                quiesce_outcome: None,
                pending_failure: None,
                telemetry,
                terminal,
                pending_event: None,
                pending_analysis: None,
                delivered_analysis: None,
                search: None,
                diagnostic_running: false,
                pending_workbench: None,
                active_workbench: None,
                delivered_workbench: None,
                cancelled_admission: None,
                observer,
                workbench_context: None,
                next_workbench_publication: Instant::now(),
                is_closing: false,
            },
            DesktopEngineBridgePort {
                ordinary: ordinary_port,
                urgent: urgent_port,
                lifecycle: lifecycle_port,
            },
            DesktopEngineBridgeStreams {
                telemetry: telemetry_receiver,
                terminal: terminal_receiver,
            },
        )
    }

    pub fn with_live_demand(
        mut self,
        mut receiver: watch::Receiver<Option<GameEngineContext>>,
    ) -> Self {
        let changed = receiver.has_changed();
        let current = *receiver.borrow_and_update();
        self.demand_source = DesktopEngineServiceEnumDemandSource::Available;
        match changed {
            Ok(changed) => {
                self.live_demand = Some(receiver);
                if changed || current.is_some() {
                    self.observe_live_demand(Instant::now(), Ok(current));
                }
            }
            Err(error) => self.observe_live_demand(Instant::now(), Err(error)),
        }
        self
    }

    pub fn live_demand_received_at(&self) -> Option<Instant> {
        self.live_received_at
    }

    async fn live_demand_ready(
        receiver: &mut Option<watch::Receiver<Option<GameEngineContext>>>,
    ) -> (
        Instant,
        Result<Option<GameEngineContext>, watch::error::RecvError>,
    ) {
        match receiver {
            Some(receiver) => {
                let result = receiver.changed().await;
                let received_at = Instant::now();
                (received_at, result.map(|()| *receiver.borrow_and_update()))
            }
            None => std::future::pending().await,
        }
    }

    fn observe_live_demand(
        &mut self,
        received_at: Instant,
        current: Result<Option<GameEngineContext>, watch::error::RecvError>,
    ) {
        self.live_received_at = Some(received_at);
        self.demand_source = match current {
            Ok(Some(_)) | Ok(None) => DesktopEngineServiceEnumDemandSource::Available,
            Err(error) => {
                self.live_demand = None;
                if !self.is_closing {
                    self.pending_failure = Some(Self::effect(error));
                }
                DesktopEngineServiceEnumDemandSource::Unavailable
            }
        };
        if self.is_closing {
            return;
        }
        let deadline = received_at
            .checked_add(EngineSearchStop::SAFETY)
            .and_then(|deadline| deadline.checked_add(EngineProcessLimits::SHUTDOWN));
        match deadline {
            Some(deadline) => self.queue_preemption(DesktopEngineServicePreemption {
                received_at,
                deadline,
                response: None,
            }),
            None => {
                self.pending_failure = Some(Self::failure(
                    "The live-game preemption receipt exceeds the runtime clock range",
                ));
            }
        }
    }

    fn accept_lifecycle(&mut self, lifecycle: DesktopEngineBridgeLifecycle) {
        match lifecycle.action {
            DesktopEngineBridgeEnumLifecycle::Shutdown => {
                self.request_shutdown(DesktopEngineServiceShutdown {
                    deadline: lifecycle.deadline,
                    response: Some(lifecycle.response),
                })
            }
            DesktopEngineBridgeEnumLifecycle::Quiesce => {
                let preemption = DesktopEngineServicePreemption {
                    received_at: lifecycle.received_at,
                    deadline: lifecycle.deadline,
                    response: Some(lifecycle.response),
                };
                if self.is_closing {
                    self.complete_quiesce(
                        preemption,
                        Err(Self::failure("The native engine service is closing")),
                    );
                } else {
                    self.queue_preemption(preemption);
                }
            }
        }
    }

    fn queue_preemption(&mut self, mut preemption: DesktopEngineServicePreemption) {
        if let Some(pending) = &mut self.requested_preemption {
            if preemption.response.is_some()
                && pending
                    .response
                    .as_ref()
                    .is_some_and(|response| !response.is_closed())
            {
                self.complete_quiesce(
                    preemption,
                    Err(Self::failure("Native engine quiescence is already pending")),
                );
                return;
            }
            preemption.received_at = preemption.received_at.min(pending.received_at);
            preemption.deadline = preemption.deadline.min(pending.deadline);
            if preemption.response.is_none() {
                pending.received_at = preemption.received_at;
                pending.deadline = preemption.deadline;
                return;
            }
        }
        let mut interrupt = false;
        if let Some(operation) = &self.operation {
            match &operation.pending {
                DesktopEngineServiceEnumPending::Request(pending) => {
                    interrupt = pending.operation.action().is_mutation();
                }
                DesktopEngineServiceEnumPending::Shutdown(_) => {}
                DesktopEngineServiceEnumPending::Preemption(pending) => {
                    if preemption.response.is_some()
                        && pending
                            .response
                            .as_ref()
                            .is_some_and(|response| !response.is_closed())
                    {
                        self.complete_quiesce(
                            preemption,
                            Err(Self::failure("Native engine quiescence is already running")),
                        );
                        return;
                    }
                    preemption.received_at = preemption.received_at.min(pending.received_at);
                    interrupt =
                        preemption.response.is_some() && preemption.deadline < pending.deadline;
                }
            }
        }
        self.requested_preemption = Some(preemption);
        if interrupt {
            self.interrupt_operation();
        }
    }

    pub async fn run(&mut self) -> Result<(), DesktopFailure> {
        let terminal = self.terminal.clone();
        loop {
            self.pump()?;
            if self.is_closing && self.is_shutdown_complete() {
                return Ok(());
            }
            let ordinary_open = !self.ordinary.is_closed() || !self.ordinary.is_empty();
            let urgent_open = !self.urgent.is_closed() || !self.urgent.is_empty();
            if !ordinary_open
                && !urgent_open
                && self.lifecycle.is_closed()
                && self.lifecycle.is_empty()
                && self.operation.is_none()
            {
                return Ok(());
            }
            let search = self.search;
            let diagnostic_running = self.diagnostic_running && self.commands.is_some();
            let has_pending_event = self.has_pending_event();
            let operation_running = self.operation.is_some();
            let export_pending = self
                .commands
                .as_ref()
                .is_some_and(|commands| commands.export_pending());
            let (session, workbench, exports) = match self.commands.as_mut() {
                Some(commands) => (
                    Some(&mut commands.session),
                    Some(&mut commands.workbench),
                    Some(&mut commands.exports),
                ),
                None => (None, None, None),
            };
            tokio::select! {
                biased;
                lifecycle = self.lifecycle.recv(), if self.requested_shutdown.is_none() && (!self.lifecycle.is_closed() || !self.lifecycle.is_empty()) => {
                    if let Some(lifecycle) = lifecycle { self.accept_lifecycle(lifecycle); }
                }
                (received_at, demand) = Self::live_demand_ready(&mut self.live_demand), if self.live_demand.is_some() => { self.observe_live_demand(received_at, demand); }
                envelope = self.urgent.recv(), if urgent_open && self.controls.len() < DesktopEngineBridgePort::URGENT_CAPACITY => {
                    if let Some(envelope) = envelope { self.accept_control(envelope); }
                }
                completed = Self::operation_ready(&mut self.operation), if operation_running => { self.finish_operation(completed)?; }
                result = Self::analysis_ready(session, search), if search.is_some() && !operation_running && !has_pending_event => {
                    match result {
                        Ok(EngineSearchSessionEnumEvent::Snapshot(snapshot)) => { self.telemetry.send_replace(Some(snapshot)); }
                        Ok(EngineSearchSessionEnumEvent::Finished(_)) => { self.capture_analysis_terminal(); }
                        Ok(EngineSearchSessionEnumEvent::Readiness { .. } | EngineSearchSessionEnumEvent::Diagnostic { .. }) => {}
                        Err(failure) => { self.search = None; self.pending_event = Some(DesktopEngineBridgeEnumEvent::Failure(failure)); }
                    }
                }
                result = Self::workbench_ready(workbench), if diagnostic_running && !has_pending_event => {
                    self.diagnostic_running = false;
                    if let Err(failure) = result { self.pending_event = Some(DesktopEngineBridgeEnumEvent::Failure(failure)); }
                }
                result = Self::export_ready(exports), if export_pending && !operation_running => {
                    if let Err(failure) = result { self.pending_failure = Some(failure); }
                }
                () = tokio::time::sleep_until(self.next_workbench_publication), if diagnostic_running && !has_pending_event => { self.observe_workbench()?; }
                permit = terminal.reserve(), if self.pending_event.is_some() => {
                    let permit = permit.map_err(|_closed| Self::failure("The ordered engine event consumer is unavailable"))?;
                    if let Some(event) = self.pending_event.take() { permit.send(event); }
                }
                envelope = self.ordinary.recv(), if ordinary_open && !operation_running && !has_pending_event && !self.is_closing => {
                    if let Some(envelope) = envelope { self.start_request(envelope)?; }
                }
            }
        }
    }

    fn pump(&mut self) -> Result<(), DesktopFailure> {
        self.flush_event()?;
        if let Some(commands) = &mut self.commands {
            commands.observe_search_capacity();
        }
        self.capture_workbench_terminal()?;
        self.capture_analysis_terminal();
        self.reconcile_interrupted();
        if self.commands.is_some() {
            if let Some(shutdown) = self.requested_shutdown.take() {
                self.start_shutdown(shutdown)?;
            } else if let Some(preemption) = self.requested_preemption.take() {
                self.start_preemption(preemption)?;
            } else if let Some(envelope) = self.controls.pop_front() {
                self.start_request(envelope)?;
            }
        }
        self.flush_event()
    }

    fn start_request(
        &mut self,
        envelope: DesktopEngineBridgeEnvelope,
    ) -> Result<(), DesktopFailure> {
        let operation = envelope.request.operation.clone();
        let receipt = envelope.request.receipt.clone();
        let cached = self
            .outcomes
            .iter()
            .find(|outcome| outcome.request == receipt.request_id())
            .map(|outcome| {
                if outcome.operation == operation {
                    outcome.result.clone()
                } else {
                    Err(DesktopFailure::new(
                        DesktopEnumFailureCode::RequestConflict,
                        "The native request identity belongs to another engine operation"
                            .to_owned(),
                        false,
                    ))
                }
            });
        let workbench = if matches!(
            operation.action(),
            DesktopEngineCommandsEnumOperation::Workbench(
                DesktopWorkbenchCommandsEnumOperation::Start { .. }
            )
        ) {
            Some(WorkbenchJobId::try_from(receipt.request_id().as_uuid()).map_err(Self::effect)?)
        } else {
            None
        };
        let pending = DesktopEngineServiceRequest {
            operation,
            receipt,
            response: envelope.response,
            workbench,
        };
        if let Some(result) = cached {
            self.complete_request(pending, result);
            return Ok(());
        }
        if self.is_closing {
            self.complete_request(
                pending,
                Err(Self::failure(
                    "The native engine service is closing; reconcile before retrying",
                )),
            );
            return Ok(());
        }
        if pending.operation.action().is_mutation() && self.demand_source.is_unavailable() {
            self.complete_request(
                pending,
                Err(Self::failure(
                    "Live-game priority updates are unavailable; restore the game service before changing the engine",
                )),
            );
            return Ok(());
        }
        if !self.interrupted.is_empty()
            && pending.operation.action().is_mutation()
            && !matches!(
                pending.operation.action(),
                DesktopEngineCommandsEnumOperation::Retry
            )
        {
            self.complete_request(pending, Err(Self::failure("An interrupted engine operation retains cleanup ownership; retry cleanup before starting more work")));
            return Ok(());
        }
        if let DesktopEngineCommandsEnumOperation::Workbench(
            DesktopWorkbenchCommandsEnumOperation::Cancel { job },
        ) = pending.operation.action()
            && self.cancelled_admission == Some((pending.operation.context(), *job))
        {
            let job = *job;
            self.cancelled_admission = None;
            self.complete_request(
                pending,
                Ok(DesktopEngineBridgeEnumResult::WorkbenchCancelled(job)),
            );
            return Ok(());
        }
        let now = match self.clock.now() {
            Ok(now) => now,
            Err(error) => {
                self.complete_request(pending, Err(Self::effect(error)));
                return Ok(());
            }
        };
        let tablebase_policy = if matches!(
            pending.operation.action(),
            DesktopEngineCommandsEnumOperation::PrepareTablebases { .. }
        ) {
            match self.tablebase_policy.policy() {
                Ok(policy) => Some(policy),
                Err(failure) => {
                    self.complete_request(pending, Err(failure));
                    return Ok(());
                }
            }
        } else {
            None
        };
        let execution = DesktopEngineServiceExecution {
            request: envelope.request,
            workbench,
            now,
            tablebase_policy,
        };
        let mut commands = self
            .commands
            .take()
            .ok_or_else(|| Self::failure("An engine operation still owns its session"))?;
        let (interrupt, interrupted) = oneshot::channel();
        let future = Box::pin(async move {
            let completion = tokio::select! {
                biased;
                _signal = interrupted => DesktopEngineServiceEnumCompletion::Interrupted,
                result = Self::execute(&mut commands, execution) => DesktopEngineServiceEnumCompletion::Request(Box::new(result)),
            };
            (commands, completion)
        });
        self.operation = Some(DesktopEngineServiceOperation {
            future,
            interrupt: Some(interrupt),
            pending: DesktopEngineServiceEnumPending::Request(pending),
        });
        Ok(())
    }

    fn accept_control(&mut self, envelope: DesktopEngineBridgeEnvelope) {
        if self.is_closing {
            self.reject_request(envelope);
            return;
        }
        let mut interrupt = false;
        if let Some(operation) = &self.operation
            && let DesktopEngineServiceEnumPending::Request(pending) = &operation.pending
        {
            let same_context = envelope.request.operation.context() == pending.operation.context();
            let matching = match envelope.request.operation.action() {
                DesktopEngineCommandsEnumOperation::Stop { tag } => self.search == Some(*tag),
                DesktopEngineCommandsEnumOperation::Workbench(
                    DesktopWorkbenchCommandsEnumOperation::Cancel { job },
                ) => pending.workbench == Some(*job) || self.active_workbench == Some(*job),
                DesktopEngineCommandsEnumOperation::Snapshot
                | DesktopEngineCommandsEnumOperation::Start { .. }
                | DesktopEngineCommandsEnumOperation::Configure(_)
                | DesktopEngineCommandsEnumOperation::Retry
                | DesktopEngineCommandsEnumOperation::PrepareTablebases { .. }
                | DesktopEngineCommandsEnumOperation::LoadTablebases
                | DesktopEngineCommandsEnumOperation::Workbench(
                    DesktopWorkbenchCommandsEnumOperation::Start { .. }
                    | DesktopWorkbenchCommandsEnumOperation::Result { .. }
                    | DesktopWorkbenchCommandsEnumOperation::PreviewExport { .. }
                    | DesktopWorkbenchCommandsEnumOperation::ApplyExport { .. },
                ) => false,
            };
            if !same_context || !matching {
                let pending = Self::pending_request(envelope);
                self.complete_request(
                    pending,
                    Err(DesktopFailure::new(
                        DesktopEnumFailureCode::StaleRevision,
                        "The control belongs to another active engine operation".to_owned(),
                        true,
                    )),
                );
                return;
            }
            interrupt = !pending.operation.action().is_control();
        }
        self.controls.push_back(envelope);
        if interrupt {
            self.interrupt_operation();
        }
    }

    fn request_shutdown(&mut self, shutdown: DesktopEngineServiceShutdown) {
        self.is_closing = true;
        self.ordinary.close();
        self.urgent.close();
        self.shutdown_outcome = None;
        self.requested_shutdown = Some(shutdown);
        if let Some(preemption) = self.requested_preemption.take()
            && preemption.response.is_some()
        {
            self.interrupted_quiesce = Some(preemption);
        }
        self.reject_queued();
        if self.operation.as_ref().is_some_and(|operation| {
            matches!(
                operation.pending,
                DesktopEngineServiceEnumPending::Request(_)
                    | DesktopEngineServiceEnumPending::Preemption(_)
            )
        }) {
            self.interrupt_operation();
        }
    }

    fn interrupt_operation(&mut self) {
        if let Some(operation) = &mut self.operation
            && let Some(interrupt) = operation.interrupt.take()
        {
            match interrupt.send(()) {
                Ok(()) | Err(()) => {}
            }
        }
    }

    fn start_shutdown(
        &mut self,
        shutdown: DesktopEngineServiceShutdown,
    ) -> Result<(), DesktopFailure> {
        self.shutdown_outcome = None;
        self.diagnostic_running = false;
        let mut commands = self
            .commands
            .take()
            .ok_or_else(|| Self::failure("Engine cleanup is waiting for its retained operation"))?;
        let deadline = shutdown.deadline;
        let future = Box::pin(async move {
            let result = commands.shutdown_retained(deadline).await;
            (
                commands,
                DesktopEngineServiceEnumCompletion::Shutdown(result),
            )
        });
        self.operation = Some(DesktopEngineServiceOperation {
            future,
            interrupt: None,
            pending: DesktopEngineServiceEnumPending::Shutdown(shutdown),
        });
        Ok(())
    }

    fn preemption_action(
        &self,
        commands: &DesktopEngineCommands<P, A, S>,
    ) -> DesktopEngineServiceEnumPreemptionAction {
        if commands.is_shutdown_complete() {
            return DesktopEngineServiceEnumPreemptionAction::Idle;
        }
        let has_preparation = self.interrupted.iter().any(|pending| {
            matches!(
                pending.operation.action(),
                DesktopEngineCommandsEnumOperation::PrepareTablebases { .. }
            )
        });
        let has_ambiguous_mutation = self.interrupted.iter().any(|pending| {
            matches!(
                pending.operation.action(),
                DesktopEngineCommandsEnumOperation::Configure(_)
                    | DesktopEngineCommandsEnumOperation::Retry
                    | DesktopEngineCommandsEnumOperation::LoadTablebases
            )
        });
        if has_preparation || has_ambiguous_mutation && commands.idle().is_err() {
            return DesktopEngineServiceEnumPreemptionAction::Cleanup;
        }
        if let Some(job) = commands.workbench.active_job() {
            return DesktopEngineServiceEnumPreemptionAction::Cancel(job.id());
        }
        if commands.session.terminal().is_none()
            && let Some(metadata) = commands.session.metadata()
        {
            return DesktopEngineServiceEnumPreemptionAction::Stop(metadata.tag());
        }
        if commands.idle().is_ok() {
            DesktopEngineServiceEnumPreemptionAction::Idle
        } else {
            DesktopEngineServiceEnumPreemptionAction::Cleanup
        }
    }

    fn start_preemption(
        &mut self,
        preemption: DesktopEngineServicePreemption,
    ) -> Result<(), DesktopFailure> {
        let mut commands = self.commands.take().ok_or_else(|| {
            Self::failure("Priority quiescence is waiting for its retained engine owner")
        })?;
        let action = self.preemption_action(&commands);
        if matches!(
            action,
            DesktopEngineServiceEnumPreemptionAction::Cancel(_)
                | DesktopEngineServiceEnumPreemptionAction::Cleanup
        ) {
            self.diagnostic_running = false;
        }
        let received_at = preemption.received_at;
        let deadline = preemption.deadline;
        let (interrupt, interrupted) = oneshot::channel();
        let future = Box::pin(async move {
            let completion = tokio::select! {
                biased;
                _signal = interrupted => DesktopEngineServiceEnumCompletion::Interrupted,
                result = tokio::time::timeout_at(deadline, Self::perform_preemption(&mut commands, action, received_at, deadline)) => DesktopEngineServiceEnumCompletion::Preemption(result.map_err(Self::effect).and_then(|result| result)),
            };
            (commands, completion)
        });
        self.operation = Some(DesktopEngineServiceOperation {
            future,
            interrupt: Some(interrupt),
            pending: DesktopEngineServiceEnumPending::Preemption(preemption),
        });
        Ok(())
    }

    async fn perform_preemption(
        commands: &mut DesktopEngineCommands<P, A, S>,
        action: DesktopEngineServiceEnumPreemptionAction,
        received_at: Instant,
        deadline: Instant,
    ) -> Result<(), DesktopFailure> {
        let result = match action {
            DesktopEngineServiceEnumPreemptionAction::Idle => Ok(()),
            DesktopEngineServiceEnumPreemptionAction::Cancel(job) => {
                commands.workbench.cancel(job, received_at).await
            }
            DesktopEngineServiceEnumPreemptionAction::Cleanup => {
                return commands.shutdown_retained(deadline).await;
            }
            DesktopEngineServiceEnumPreemptionAction::Stop(tag) => {
                let stop = EngineSearchStop::received_at(tag, received_at).map_err(Self::effect)?;
                let result = commands.session.stop(stop).await;
                commands.observe_search_capacity();
                let failure = match result {
                    Ok(terminal) => match terminal.completion() {
                        EngineSearchSessionEnumCompletion::Failed(error) => {
                            Some(Self::effect(error))
                        }
                        EngineSearchSessionEnumCompletion::BestMove(_)
                        | EngineSearchSessionEnumCompletion::Stopped
                        | EngineSearchSessionEnumCompletion::Cancelled => None,
                    },
                    Err(error) => Some(Self::effect(error)),
                };
                if let Some(failure) = failure {
                    return match commands
                        .shutdown_retained(deadline.min(stop.termination_deadline()))
                        .await
                    {
                        Ok(()) => Err(failure),
                        Err(cleanup) => Err(Self::failure(&format!(
                            "{}; priority cleanup also failed: {}",
                            failure.message(),
                            cleanup.message()
                        ))),
                    };
                }
                Ok(())
            }
        };
        let mut failure = result.err();
        DesktopEngineCommands::<P, A, S>::record_cleanup(
            &mut failure,
            commands.quiesce_exports(deadline).await,
        );
        failure.map_or(Ok(()), Err)
    }

    fn complete_quiesce(
        &mut self,
        preemption: DesktopEngineServicePreemption,
        result: Result<(), DesktopFailure>,
    ) {
        if let Some(response) = preemption.response {
            self.quiesce_outcome = Some(result.clone());
            if let Err(result) = response.send(result) {
                self.quiesce_outcome = Some(result);
            }
        } else if let Err(error) = result {
            self.pending_failure = Some(error);
        }
    }

    async fn operation_ready(
        operation: &mut Option<DesktopEngineServiceOperation<P, A, S>>,
    ) -> (
        DesktopEngineCommands<P, A, S>,
        DesktopEngineServiceEnumCompletion,
    ) {
        match operation {
            Some(operation) => operation.future.as_mut().await,
            None => std::future::pending().await,
        }
    }

    fn finish_operation(
        &mut self,
        completed: (
            DesktopEngineCommands<P, A, S>,
            DesktopEngineServiceEnumCompletion,
        ),
    ) -> Result<(), DesktopFailure> {
        let operation = self
            .operation
            .take()
            .ok_or_else(|| Self::failure("The completed engine operation lost its receipt"))?;
        self.commands = Some(completed.0);
        match (operation.pending, completed.1) {
            (
                DesktopEngineServiceEnumPending::Request(pending),
                DesktopEngineServiceEnumCompletion::Interrupted,
            ) => {
                if let Some(job) = pending.workbench
                    && self
                        .commands
                        .as_ref()
                        .is_some_and(|commands| commands.workbench.active_job().is_none())
                {
                    self.cancelled_admission = Some((pending.operation.context(), job));
                }
                self.interrupted.push_back(pending);
            }
            (
                DesktopEngineServiceEnumPending::Request(pending),
                DesktopEngineServiceEnumCompletion::Request(result),
            ) => {
                let result = *result;
                if let Ok(DesktopEngineBridgeEnumResult::Started(metadata)) = &result {
                    self.search = Some(metadata.tag());
                }
                if let Ok(DesktopEngineBridgeEnumResult::WorkbenchStarted(job)) = &result {
                    self.active_workbench = Some(*job);
                    self.diagnostic_running = self.commands.as_ref().is_some_and(|commands| {
                        commands
                            .workbench
                            .active_job()
                            .is_some_and(|job| job.outcome().is_none())
                    });
                    self.workbench_context =
                        Some((pending.operation.context(), pending.receipt.request_id()));
                }
                if let Ok(DesktopEngineBridgeEnumResult::WorkbenchCancelled(_)) = &result {
                    self.diagnostic_running = false;
                }
                self.complete_request(pending, result);
                self.observe_workbench()?;
            }
            (
                DesktopEngineServiceEnumPending::Shutdown(shutdown),
                DesktopEngineServiceEnumCompletion::Shutdown(result),
            ) => {
                self.capture_analysis_terminal();
                self.capture_workbench_terminal()?;
                self.reconcile_interrupted();
                self.observe_workbench()?;
                if let Some(preemption) = self.interrupted_quiesce.take() {
                    let quiesced = self
                        .commands
                        .as_ref()
                        .is_some_and(DesktopEngineCommands::is_shutdown_complete);
                    self.complete_quiesce(
                        preemption,
                        if quiesced {
                            Ok(())
                        } else {
                            Err(Self::failure(
                                "Shutdown retains unresolved quiescence ownership",
                            ))
                        },
                    );
                }
                self.flush_event()?;
                let result = result.and_then(|()| {
                    if self.has_pending_event() {
                        Err(Self::failure(
                            "Consume the retained ordered engine events to finish shutdown",
                        ))
                    } else {
                        Ok(())
                    }
                });
                self.shutdown_outcome = Some(result.clone());
                if let Some(response) = shutdown.response {
                    self.deliver_shutdown(response, result);
                }
            }
            (
                DesktopEngineServiceEnumPending::Preemption(preemption),
                DesktopEngineServiceEnumCompletion::Interrupted,
            ) => {
                if self.is_closing {
                    if preemption.response.is_some() {
                        self.interrupted_quiesce = Some(preemption);
                    }
                } else if let Some(next) = &mut self.requested_preemption {
                    next.received_at = next.received_at.min(preemption.received_at);
                    next.deadline = next.deadline.min(preemption.deadline);
                    if preemption.response.is_some() {
                        self.complete_quiesce(preemption, Err(Self::failure("Quiescence was interrupted by a tighter retained lifecycle control")));
                    }
                } else {
                    self.requested_preemption = Some(preemption);
                }
            }
            (
                DesktopEngineServiceEnumPending::Preemption(preemption),
                DesktopEngineServiceEnumCompletion::Preemption(result),
            ) => {
                let interrupted_mutation = self
                    .interrupted
                    .iter()
                    .any(|pending| pending.operation.action().is_mutation());
                self.capture_analysis_terminal();
                self.capture_workbench_terminal()?;
                self.reconcile_interrupted();
                self.observe_workbench()?;
                let quiescent = self.commands.as_ref().is_some_and(|commands| {
                    (commands.idle().is_ok() || commands.is_shutdown_complete())
                        && commands.exports_quiescent()
                });
                let result = result.and_then(|()| {
                    if quiescent {
                        Ok(())
                    } else {
                        Err(Self::failure(
                            "Priority quiescence retains child or CPU ownership",
                        ))
                    }
                });
                if let Err(error) = &result {
                    self.pending_failure = Some(error.clone());
                }
                if interrupted_mutation
                    && self.commands.as_ref().is_some_and(|commands| {
                        commands.session.engine().state() == EngineSearchEnumState::Failed
                    })
                    && result.is_ok()
                {
                    self.pending_failure = Some(Self::failure(
                        "Priority cleanup stopped an interrupted engine mutation; explicit Retry is required",
                    ));
                }
                self.complete_quiesce(preemption, result);
            }
            (
                DesktopEngineServiceEnumPending::Request(_),
                DesktopEngineServiceEnumCompletion::Shutdown(_)
                | DesktopEngineServiceEnumCompletion::Preemption(_),
            )
            | (
                DesktopEngineServiceEnumPending::Shutdown(_),
                DesktopEngineServiceEnumCompletion::Request(_)
                | DesktopEngineServiceEnumCompletion::Interrupted
                | DesktopEngineServiceEnumCompletion::Preemption(_),
            ) => {
                return Err(Self::failure(
                    "The retained engine completion belongs to another operation",
                ));
            }
            (
                DesktopEngineServiceEnumPending::Preemption(_),
                DesktopEngineServiceEnumCompletion::Request(_)
                | DesktopEngineServiceEnumCompletion::Shutdown(_),
            ) => {
                return Err(Self::failure(
                    "The retained priority completion belongs to another operation",
                ));
            }
        }
        self.capture_analysis_terminal();
        self.reconcile_interrupted();
        Ok(())
    }

    fn reconcile_interrupted(&mut self) {
        let Some(commands) = &self.commands else {
            return;
        };
        let all_reaped = commands.is_shutdown_complete();
        let idle_without_effect = commands.idle().is_ok();
        let exports_quiescent = commands.exports_quiescent();
        let export_outcomes: HashMap<_, _> = self
            .interrupted
            .iter()
            .filter_map(|pending| {
                commands
                    .export_outcome(pending.receipt.request_id())
                    .map(|outcome| (pending.receipt.request_id(), outcome))
            })
            .collect();
        let finished_job = commands.workbench.latest_result();
        let search_terminal = commands
            .session
            .terminal()
            .cloned()
            .filter(|_terminal| commands.session.allocation().is_none());
        let mut index = 0;
        while index < self.interrupted.len() {
            let pending = &self.interrupted[index];
            let result = match pending.operation.action() {
                DesktopEngineCommandsEnumOperation::Stop { tag } => search_terminal
                    .as_ref()
                    .filter(|terminal| terminal.snapshot().metadata().tag() == *tag)
                    .map(|terminal| Ok(DesktopEngineBridgeEnumResult::Stopped(terminal.clone()))),
                DesktopEngineCommandsEnumOperation::Workbench(
                    DesktopWorkbenchCommandsEnumOperation::Cancel { job },
                ) => finished_job
                    .as_ref()
                    .filter(|result| result.job().id() == *job)
                    .map(|_result| Ok(DesktopEngineBridgeEnumResult::WorkbenchCancelled(*job))),
                DesktopEngineCommandsEnumOperation::Workbench(
                    DesktopWorkbenchCommandsEnumOperation::Start { .. },
                ) => (all_reaped
                    || self.cancelled_admission.is_some_and(|(context, job)| {
                        context == pending.operation.context() && Some(job) == pending.workbench
                    })
                    || finished_job
                        .as_ref()
                        .is_some_and(|result| Some(result.job().id()) == pending.workbench))
                .then(|| Err(Self::cancelled())),
                DesktopEngineCommandsEnumOperation::Start { .. } => {
                    (all_reaped || idle_without_effect || search_terminal.is_some())
                        .then(|| Err(Self::cancelled()))
                }
                DesktopEngineCommandsEnumOperation::PrepareTablebases { .. } => {
                    all_reaped.then(|| Err(Self::cancelled()))
                }
                DesktopEngineCommandsEnumOperation::Workbench(
                    DesktopWorkbenchCommandsEnumOperation::ApplyExport { preview, .. },
                ) => export_outcomes
                    .get(&pending.receipt.request_id())
                    .cloned()
                    .map(|result| {
                        result.map(|receipt| DesktopEngineBridgeEnumResult::Exported {
                            preview: *preview,
                            receipt,
                        })
                    })
                    .or_else(|| {
                        (exports_quiescent && (all_reaped || idle_without_effect))
                            .then(|| Err(Self::cancelled()))
                    }),
                DesktopEngineCommandsEnumOperation::Snapshot
                | DesktopEngineCommandsEnumOperation::Configure(_)
                | DesktopEngineCommandsEnumOperation::Retry
                | DesktopEngineCommandsEnumOperation::LoadTablebases
                | DesktopEngineCommandsEnumOperation::Workbench(
                    DesktopWorkbenchCommandsEnumOperation::Result { .. }
                    | DesktopWorkbenchCommandsEnumOperation::PreviewExport { .. },
                ) => (all_reaped || idle_without_effect).then(|| Err(Self::cancelled())),
            };
            if let Some(result) = result {
                if let Some(pending) = self.interrupted.remove(index) {
                    self.complete_request(pending, result);
                }
            } else {
                index += 1;
            }
        }
    }

    fn complete_request(
        &mut self,
        pending: DesktopEngineServiceRequest,
        result: Result<DesktopEngineBridgeEnumResult, DesktopFailure>,
    ) {
        let retained = result.clone();
        let result = match pending.response.send(result) {
            Ok(()) => retained,
            Err(result) => result,
        };
        if self.outcomes.len() == DesktopRequestCache::<DesktopEngineBridgeEnumResult>::CAPACITY {
            self.outcomes.pop_front();
        }
        self.outcomes.push_back(DesktopEngineServiceOutcome {
            operation: pending.operation,
            request: pending.receipt.request_id(),
            result,
        });
    }

    fn pending_request(envelope: DesktopEngineBridgeEnvelope) -> DesktopEngineServiceRequest {
        DesktopEngineServiceRequest {
            operation: envelope.request.operation,
            receipt: envelope.request.receipt,
            response: envelope.response,
            workbench: None,
        }
    }

    fn reject_request(&mut self, envelope: DesktopEngineBridgeEnvelope) {
        self.complete_request(
            Self::pending_request(envelope),
            Err(Self::failure(
                "The native engine service is closing; reconcile before retrying",
            )),
        );
    }

    fn reject_queued(&mut self) {
        while let Some(envelope) = self.controls.pop_front() {
            self.reject_request(envelope);
        }
        while let Ok(envelope) = self.ordinary.try_recv() {
            self.reject_request(envelope);
        }
        while let Ok(envelope) = self.urgent.try_recv() {
            self.reject_request(envelope);
        }
    }

    fn deliver_shutdown(
        &mut self,
        response: oneshot::Sender<Result<(), DesktopFailure>>,
        result: Result<(), DesktopFailure>,
    ) {
        if let Err(result) = response.send(result) {
            self.shutdown_outcome = Some(result);
        }
    }

    pub fn retained_outcome(
        &self,
        request: DesktopRequestId,
    ) -> Option<&Result<DesktopEngineBridgeEnumResult, DesktopFailure>> {
        self.outcomes
            .iter()
            .rev()
            .find(|outcome| outcome.request == request)
            .map(|outcome| &outcome.result)
    }

    pub fn shutdown_outcome(&self) -> Option<&Result<(), DesktopFailure>> {
        self.shutdown_outcome.as_ref()
    }

    fn capture_analysis_terminal(&mut self) {
        let Some(terminal) = self
            .commands
            .as_ref()
            .and_then(|commands| commands.session.terminal())
        else {
            return;
        };
        let tag = terminal.snapshot().metadata().tag();
        self.search = None;
        if self.delivered_analysis != Some(tag) && self.pending_analysis.is_none() {
            self.pending_analysis = Some(Arc::new(EngineSearchSessionEnumEvent::Finished(
                terminal.clone(),
            )));
            self.delivered_analysis = Some(tag);
        }
    }

    fn capture_workbench_terminal(&mut self) -> Result<(), DesktopFailure> {
        if let Some(commands) = &mut self.commands {
            let completed = commands
                .workbench
                .observe_completion()?
                .or_else(|| commands.workbench.latest_result());
            if let Some(result) = completed
                && self.delivered_workbench != Some(result.job().id())
            {
                self.diagnostic_running = false;
                self.active_workbench = None;
                self.delivered_workbench = Some(result.job().id());
                self.pending_workbench = Some(result);
            }
        }
        Ok(())
    }

    fn has_pending_event(&self) -> bool {
        self.pending_event.is_some()
            || self.pending_analysis.is_some()
            || self.pending_workbench.is_some()
            || self.pending_failure.is_some()
    }

    fn flush_event(&mut self) -> Result<(), DesktopFailure> {
        if self.pending_event.is_none() {
            if let Some(event) = self.pending_analysis.take() {
                self.pending_event = Some(DesktopEngineBridgeEnumEvent::Analysis(event));
            } else if let Some(result) = self.pending_workbench.take() {
                self.pending_event = Some(DesktopEngineBridgeEnumEvent::Workbench(result));
            } else if let Some(error) = self.pending_failure.take() {
                self.pending_event = Some(DesktopEngineBridgeEnumEvent::Failure(error));
            }
        }
        if let Some(event) = self.pending_event.take() {
            match self.terminal.try_send(event) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(event)) => self.pending_event = Some(event),
                Err(mpsc::error::TrySendError::Closed(event)) => {
                    self.pending_event = Some(event);
                    return Err(Self::failure(
                        "The ordered engine event consumer is unavailable",
                    ));
                }
            }
        }
        Ok(())
    }

    async fn analysis_ready(
        session: Option<&mut EngineSearchSession<P::Process>>,
        tag: Option<EngineSearchTag>,
    ) -> Result<EngineSearchSessionEnumEvent, DesktopFailure> {
        match (session, tag) {
            (Some(session), Some(tag)) => session.next_event(tag).await.map_err(Self::effect),
            (None, _) | (Some(_), None) => std::future::pending().await,
        }
    }

    async fn export_ready(
        exports: Option<&mut crate::desktop::workbench_export::DesktopWorkbenchExport>,
    ) -> Result<crate::workbench::export::WorkbenchExportReceipt, DesktopFailure> {
        match exports {
            Some(exports) => exports.observe().await,
            None => std::future::pending().await,
        }
    }

    async fn workbench_ready(
        workbench: Option<&mut DesktopWorkbenchCommands<P::Process, A::Reservation>>,
    ) -> Result<(), DesktopFailure> {
        match workbench {
            Some(workbench) => workbench.run().await,
            None => std::future::pending().await,
        }
    }

    async fn execute(
        commands: &mut DesktopEngineCommands<P, A, S>,
        execution: DesktopEngineServiceExecution,
    ) -> Result<DesktopEngineBridgeEnumResult, DesktopFailure> {
        let request = execution.request;
        let context = request.operation.context();
        commands.validate(context)?;
        if request.receipt.is_cancellation_requested() && !request.operation.action().is_control() {
            return Err(Self::cancelled());
        }
        match request.operation.action() {
            DesktopEngineCommandsEnumOperation::Snapshot => {
                Ok(DesktopEngineBridgeEnumResult::Snapshot(commands.snapshot()))
            }
            DesktopEngineCommandsEnumOperation::Start { position, limit } => commands
                .start(context, position.clone(), *limit)
                .await
                .map(DesktopEngineBridgeEnumResult::Started),
            DesktopEngineCommandsEnumOperation::Stop { tag } => commands
                .stop(context, *tag, request.receipt.runtime_received_at())
                .await
                .map(DesktopEngineBridgeEnumResult::Stopped),
            DesktopEngineCommandsEnumOperation::Configure(setting) => commands
                .configure(context, setting.clone())
                .await
                .map(DesktopEngineBridgeEnumResult::Snapshot),
            DesktopEngineCommandsEnumOperation::Retry => commands
                .retry(context)
                .await
                .map(DesktopEngineBridgeEnumResult::Snapshot),
            DesktopEngineCommandsEnumOperation::PrepareTablebases { .. } => {
                let selection = request
                    .selection
                    .ok_or_else(|| Self::failure("Select a native tablebase directory first"))?;
                let policy = execution.tablebase_policy.ok_or_else(|| {
                    Self::failure("The native tablebase alias policy is unavailable")
                })?;
                commands
                    .prepare_tablebases(context, selection, policy, request.receipt)
                    .await
                    .map(DesktopEngineBridgeEnumResult::Snapshot)
            }
            DesktopEngineCommandsEnumOperation::LoadTablebases => commands
                .load_tablebases(context)
                .await
                .map(DesktopEngineBridgeEnumResult::Snapshot),
            DesktopEngineCommandsEnumOperation::Workbench(action) => match action {
                DesktopWorkbenchCommandsEnumOperation::Start {
                    parameters,
                    deadline,
                } => {
                    let id = execution.workbench.ok_or_else(|| {
                        Self::failure("The admitted diagnostic request lost its job identity")
                    })?;
                    commands
                        .start_workbench(
                            context,
                            DesktopEngineCommandsWorkbenchRequest {
                                parameters: parameters.clone(),
                                deadline: *deadline,
                                id,
                                now: execution.now,
                            },
                        )
                        .await?;
                    Ok(DesktopEngineBridgeEnumResult::WorkbenchStarted(id))
                }
                DesktopWorkbenchCommandsEnumOperation::Cancel { job } => {
                    commands
                        .workbench
                        .cancel(*job, request.receipt.runtime_received_at())
                        .await?;
                    Ok(DesktopEngineBridgeEnumResult::WorkbenchCancelled(*job))
                }
                DesktopWorkbenchCommandsEnumOperation::Result { job } => commands
                    .workbench
                    .result(*job)
                    .map(DesktopEngineBridgeEnumResult::WorkbenchResult),
                DesktopWorkbenchCommandsEnumOperation::PreviewExport { job } => commands
                    .preview_export(context, request.receipt.request_id(), *job)
                    .map(DesktopEngineBridgeEnumResult::ExportPreview),
                DesktopWorkbenchCommandsEnumOperation::ApplyExport {
                    preview, digest, ..
                } => {
                    let destination = request
                        .selection
                        .ok_or_else(|| Self::failure("Select a native export destination first"))?;
                    let receipt = commands
                        .apply_export(
                            context,
                            crate::desktop::workbench_export::DesktopWorkbenchExportRequest {
                                preview: *preview,
                                digest: *digest,
                                destination,
                                receipt: request.receipt,
                            },
                        )
                        .await?;
                    Ok(DesktopEngineBridgeEnumResult::Exported {
                        preview: *preview,
                        receipt,
                    })
                }
            },
        }
    }

    fn observe_workbench(&mut self) -> Result<(), DesktopFailure> {
        if let Some((context, request)) = self.workbench_context
            && let Some(commands) = &self.commands
        {
            commands
                .workbench
                .observe(context, request, &mut self.observer)?;
        }
        self.next_workbench_publication = Instant::now()
            .checked_add(std::time::Duration::from_millis(200))
            .ok_or_else(|| {
                Self::failure("The workbench publication deadline exceeds the runtime clock range")
            })?;
        Ok(())
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.commands
            .as_ref()
            .is_some_and(DesktopEngineCommands::is_shutdown_complete)
            && self.operation.is_none()
            && self.interrupted.is_empty()
            && self.controls.is_empty()
            && self.requested_shutdown.is_none()
            && self.requested_preemption.is_none()
            && self.interrupted_quiesce.is_none()
            && !self.has_pending_event()
            && self.ordinary.is_empty()
            && self.urgent.is_empty()
            && self.lifecycle.is_empty()
    }

    pub async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), DesktopFailure> {
        self.request_shutdown(DesktopEngineServiceShutdown {
            deadline,
            response: None,
        });
        loop {
            self.pump()?;
            if let Some(result) = &self.shutdown_outcome {
                return result.clone();
            }
            if self.operation.is_some() {
                let completed = Self::operation_ready(&mut self.operation).await;
                self.finish_operation(completed)?;
            } else {
                return Err(Self::failure(
                    "Engine shutdown retains unresolved ownership",
                ));
            }
        }
    }

    fn cancelled() -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::Cancelled,
            "The interrupted native engine operation stopped and its cleanup was observed"
                .to_owned(),
            true,
        )
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
