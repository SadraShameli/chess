use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use rand::Rng;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, timeout_at};

use crate::capacity::phase::CapacityPhaseGeneration;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::game_bridge::{
    DesktopGameBridgeFuture, DesktopGameBridgeProvider, DesktopGameBridgeRequest,
};
use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryError, DesktopGameRegistryFactoryProvider,
    DesktopGameRegistryPort, DesktopGameRegistryReceipt,
};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::game_supervisor::DesktopGameSupervisorEngineFactory;
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::engine_provider::GameEngineProvider;

#[derive(Debug, Error)]
pub enum DesktopNativeGameRuntimeError {
    #[error("The game runtime already owns a shutdown request")]
    Busy,
    #[error("The game runtime lifecycle channel is closed before cleanup was proved")]
    Closed,
    #[error("The game runtime cleanup returned without proving shutdown")]
    Incomplete,
    #[error("The game runtime registry failed: {0}")]
    Registry(#[source] Arc<DesktopGameRegistryError>),
    #[error("The game runtime response channel closed: {0}")]
    Response(#[from] oneshot::error::RecvError),
    #[error("The game runtime lifecycle operation exceeded its deadline: {0}")]
    Deadline(#[source] tokio::time::error::Elapsed),
}

struct DesktopNativeGameRuntimeShutdown {
    deadline: Instant,
    response: oneshot::Sender<Result<(), Arc<DesktopNativeGameRuntimeError>>>,
    _slot: DesktopNativeGameRuntimeSlot,
}

struct DesktopNativeGameRuntimeSlot(Arc<AtomicBool>);

impl Drop for DesktopNativeGameRuntimeSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone)]
pub struct DesktopNativeGameRuntimePort {
    registry: DesktopGameRegistryPort,
    shutdowns: mpsc::Sender<DesktopNativeGameRuntimeShutdown>,
    shutdown_pending: Arc<AtomicBool>,
    complete: Arc<AtomicBool>,
    failures: watch::Receiver<Option<Arc<DesktopNativeGameRuntimeError>>>,
}

impl DesktopNativeGameRuntimePort {
    pub fn registry_port(&self) -> &DesktopGameRegistryPort {
        &self.registry
    }

    pub fn failures(&self) -> watch::Receiver<Option<Arc<DesktopNativeGameRuntimeError>>> {
        self.failures.clone()
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }

    pub fn quiesce_all(
        &self,
        generation: CapacityPhaseGeneration,
        deadline: Instant,
    ) -> Result<DesktopGameRegistryReceipt, DesktopGameRegistryError> {
        self.registry.quiesce_all(generation, deadline)
    }

    pub fn resume_all(
        &self,
        generation: CapacityPhaseGeneration,
        deadline: Instant,
    ) -> Result<DesktopGameRegistryReceipt, DesktopGameRegistryError> {
        self.registry.resume_all(generation, deadline)
    }

    pub async fn shutdown(
        &self,
        deadline: Instant,
    ) -> Result<(), Arc<DesktopNativeGameRuntimeError>> {
        if self.is_shutdown_complete() {
            return Ok(());
        }
        self.shutdown_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_pending| Arc::new(DesktopNativeGameRuntimeError::Busy))?;
        let slot = DesktopNativeGameRuntimeSlot(Arc::clone(&self.shutdown_pending));
        let (response, receiver) = oneshot::channel();
        self.shutdowns
            .try_send(DesktopNativeGameRuntimeShutdown {
                deadline,
                response,
                _slot: slot,
            })
            .map_err(|error| {
                Arc::new(match error {
                    mpsc::error::TrySendError::Full(_) => DesktopNativeGameRuntimeError::Busy,
                    mpsc::error::TrySendError::Closed(_) => DesktopNativeGameRuntimeError::Closed,
                })
            })?;
        timeout_at(deadline, receiver)
            .await
            .map_err(|error| Arc::new(DesktopNativeGameRuntimeError::Deadline(error)))?
            .map_err(|error| Arc::new(DesktopNativeGameRuntimeError::Response(error)))?
    }
}

impl DesktopGameBridgeProvider for DesktopNativeGameRuntimePort {
    fn execute(&self, request: DesktopGameBridgeRequest) -> DesktopGameBridgeFuture {
        self.registry.execute(request)
    }

    fn snapshots(&self) -> watch::Receiver<Vec<DesktopGameSnapshot>> {
        self.registry.snapshots()
    }
}

enum DesktopNativeGameRuntimeEnumRegistryActivity {
    Stopped,
    Snapshot,
}

pub struct DesktopNativeGameRuntime<F: DesktopGameRegistryFactoryProvider, C, P, R> {
    registry: DesktopGameRegistry<F, C, P, R>,
    shutdowns: mpsc::Receiver<DesktopNativeGameRuntimeShutdown>,
    complete: Arc<AtomicBool>,
    failures: watch::Sender<Option<Arc<DesktopNativeGameRuntimeError>>>,
    registry_failure: Option<Arc<DesktopNativeGameRuntimeError>>,
    is_registry_running: bool,
}

impl<F, C, P, R> DesktopNativeGameRuntime<F, C, P, R>
where
    F: DesktopGameRegistryFactoryProvider,
    F::Factory: 'static,
    <F::Factory as DesktopGameSupervisorEngineFactory>::Engine: 'static,
    <<F::Factory as DesktopGameSupervisorEngineFactory>::Engine as GameEngineProvider>::Error:
        'static,
    C: ClockTimeProvider + RefUnwindSafe + 'static,
    P: GameAdjudicationMateProofProvider + Send + Sync + RefUnwindSafe + 'static,
    R: Rng + Send + UnwindSafe + 'static,
{
    const SHUTDOWN_CAPACITY: usize = 1;
    const ORPHANED_TIMEOUT: Duration = Duration::from_secs(3);

    pub fn new(
        registry: DesktopGameRegistry<F, C, P, R>,
        registry_port: DesktopGameRegistryPort,
    ) -> (Self, DesktopNativeGameRuntimePort) {
        let (shutdowns, receiver) = mpsc::channel(Self::SHUTDOWN_CAPACITY);
        let (failures, failure_receiver) = watch::channel(None);
        let complete = Arc::new(AtomicBool::new(false));
        (
            Self {
                registry,
                shutdowns: receiver,
                complete: Arc::clone(&complete),
                failures,
                registry_failure: None,
                is_registry_running: true,
            },
            DesktopNativeGameRuntimePort {
                registry: registry_port,
                shutdowns,
                shutdown_pending: Arc::new(AtomicBool::new(false)),
                complete,
                failures: failure_receiver,
            },
        )
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire) && self.registry.is_shutdown_complete()
    }

    pub async fn run(&mut self) -> Result<(), Arc<DesktopNativeGameRuntimeError>> {
        loop {
            if self.is_shutdown_complete() {
                return self
                    .registry_failure
                    .as_ref()
                    .map_or(Ok(()), |error| Err(Arc::clone(error)));
            }
            tokio::select! {
                biased;
                shutdown = self.shutdowns.recv() => {
                    match shutdown {
                        Some(shutdown) => {
                            let result = self.cleanup_retained(shutdown.deadline).await;
                            drop(shutdown._slot);
                            self.publish_shutdown_failure(&result);
                            if let Err(abandoned) = shutdown.response.send(result) {
                                drop(abandoned);
                            }
                        }
                        None => {
                            self.shutdown_retained(Instant::now() + Self::ORPHANED_TIMEOUT).await?;
                        }
                    }
                }
                result = Self::registry_activity(&mut self.registry, self.is_registry_running) => {
                    match result {
                        Ok(DesktopNativeGameRuntimeEnumRegistryActivity::Snapshot) => {}
                        Ok(DesktopNativeGameRuntimeEnumRegistryActivity::Stopped) => {
                            self.is_registry_running = false;
                            if self.registry.is_shutdown_complete() {
                                self.complete.store(true, Ordering::Release);
                            } else {
                                self.record_registry_failure(DesktopNativeGameRuntimeError::Incomplete);
                            }
                        }
                        Err(error) => {
                            self.is_registry_running = false;
                            self.record_registry_failure(
                                DesktopNativeGameRuntimeError::Registry(Arc::new(error))
                            );
                        }
                    }
                }
            }
        }
    }

    async fn registry_activity(
        registry: &mut DesktopGameRegistry<F, C, P, R>,
        is_running: bool,
    ) -> Result<DesktopNativeGameRuntimeEnumRegistryActivity, DesktopGameRegistryError> {
        if is_running {
            registry.run().await?;
            Ok(DesktopNativeGameRuntimeEnumRegistryActivity::Stopped)
        } else {
            registry.forward_next_snapshot().await;
            Ok(DesktopNativeGameRuntimeEnumRegistryActivity::Snapshot)
        }
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), Arc<DesktopNativeGameRuntimeError>> {
        let result = self.cleanup_retained(deadline).await;
        self.publish_shutdown_failure(&result);
        result
    }

    async fn cleanup_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), Arc<DesktopNativeGameRuntimeError>> {
        self.is_registry_running = false;
        let result = self
            .registry
            .shutdown_retained(deadline)
            .await
            .map_err(|error| Arc::new(DesktopNativeGameRuntimeError::Registry(Arc::new(error))))
            .and_then(|()| {
                if self.registry.is_shutdown_complete() {
                    Ok(())
                } else {
                    Err(Arc::new(DesktopNativeGameRuntimeError::Incomplete))
                }
            });
        if self.registry.is_shutdown_complete() {
            self.complete.store(true, Ordering::Release);
            self.shutdowns.close();
        }
        result
    }

    fn publish_shutdown_failure(&self, result: &Result<(), Arc<DesktopNativeGameRuntimeError>>) {
        if let Err(error) = result {
            self.failures.send_replace(Some(Arc::clone(error)));
        }
    }

    fn record_registry_failure(&mut self, failure: DesktopNativeGameRuntimeError) {
        let failure = Arc::new(failure);
        self.failures.send_replace(Some(Arc::clone(&failure)));
        self.registry_failure = Some(failure);
    }
}
