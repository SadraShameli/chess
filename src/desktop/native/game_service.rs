use std::future::Future;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant as StandardInstant};

use rand::{Rng, rngs::StdRng};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};
use uuid::Uuid;

use crate::capacity::executor::CapacityExecutor;
use crate::clock::time_provider::{ClockTimeProvider, ClockTimeProviderMonotonic};
use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryError, DesktopGameRegistryFactoryProvider,
    DesktopGameRegistryOptions, DesktopGameRegistryPort, DesktopGameRegistryRestored,
};
use crate::desktop::game_supervisor::DesktopGameSupervisorEngineFactory;
use crate::desktop::native::game_factories::DesktopNativeGameFactories;
use crate::desktop::native::game_resources::{
    DesktopNativeGameResources, DesktopNativeGameResourcesMemory,
};
use crate::desktop::native::game_runtime::{
    DesktopNativeGameRuntime, DesktopNativeGameRuntimeError, DesktopNativeGameRuntimePort,
};
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::engine_provider::GameEngineProvider;
use crate::game::enums::{GameEnumPhase, GameIdentityError};
use crate::game::mate_proof::{GameMateProof, GameMateProofBudget, GameMateProofControl};
use crate::game::storage_repository::GameStorageRepository;
use crate::storage::document_store::{
    StorageDocumentClient, StorageDocumentError, StorageDocumentStore,
};
use crate::storage::game_document::{StorageGameDocumentError, StorageGameDocumentRepository};

#[derive(Debug, Error)]
pub enum DesktopNativeGameServiceError {
    #[error("The native game service has already started")]
    Started,
    #[error("The native game service is still restoring its saved games")]
    Restoring,
    #[error("The native game service has begun shutdown")]
    Closing,
    #[error("Saved game restoration failed and requires cleanup: {0}")]
    Restoration(#[source] Arc<Self>),
    #[error("A saved game disappeared while its profile was exclusively owned")]
    MissingSavedGame,
    #[error("The retained native game owner exceeded its shutdown deadline: {0}")]
    Deadline(#[source] tokio::time::error::Elapsed),
    #[error("The native game worker failed before returning its owner: {0}")]
    Worker(#[source] Arc<tokio::task::JoinError>),
    #[error("The native game runtime failed: {0}")]
    Runtime(#[source] Arc<DesktopNativeGameRuntimeError>),
    #[error("Native game cleanup failed ({operation}); additional cleanup failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error(transparent)]
    Registry(#[from] DesktopGameRegistryError),
    #[error(transparent)]
    Document(#[from] StorageDocumentError),
    #[error(transparent)]
    GameDocument(#[from] StorageGameDocumentError),
    #[error(transparent)]
    Identity(#[from] GameIdentityError),
}

impl DesktopNativeGameServiceError {
    fn record(failure: &mut Option<Self>, error: Self) {
        *failure = Some(match failure.take() {
            Some(operation) => Self::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(error),
            },
            None => error,
        });
    }
}

pub struct DesktopNativeGameServiceProofControl(StandardInstant);

impl Default for DesktopNativeGameServiceProofControl {
    fn default() -> Self {
        Self(StandardInstant::now())
    }
}

impl GameMateProofControl for DesktopNativeGameServiceProofControl {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}

pub type DesktopNativeGameServiceFactories = DesktopNativeGameFactories<
    ClockTimeProviderMonotonic,
    DesktopNativeGameResources<DesktopNativeGameResourcesMemory>,
>;
pub type DesktopNativeGameServiceProof = GameMateProof<DesktopNativeGameServiceProofControl>;
pub type DesktopNativeGameServiceOptions<
    F = DesktopNativeGameServiceFactories,
    C = ClockTimeProviderMonotonic,
    P = DesktopNativeGameServiceProof,
    R = StdRng,
> = DesktopGameRegistryOptions<F, C, P, R>;

type DesktopNativeGameServiceOutcome<F, C, P, R> = (
    DesktopNativeGameRuntime<F, C, P, R>,
    Result<(), Arc<DesktopNativeGameRuntimeError>>,
);
type DesktopNativeGameServiceRestoreOutcome<F, C, P, R> = (
    DesktopGameRegistry<F, C, P, R>,
    Option<Uuid>,
    Result<(), DesktopNativeGameServiceError>,
);
type DesktopNativeGameServiceRestoreFuture<F, C, P, R> =
    Pin<Box<dyn Future<Output = DesktopNativeGameServiceRestoreOutcome<F, C, P, R>> + Send>>;

#[derive(Clone)]
struct DesktopNativeGameServiceStorage {
    client: StorageDocumentClient,
    executor: Arc<CapacityExecutor>,
}

pub struct DesktopNativeGameService<
    F: DesktopGameRegistryFactoryProvider = DesktopNativeGameServiceFactories,
    C = ClockTimeProviderMonotonic,
    P = DesktopNativeGameServiceProof,
    R = StdRng,
> {
    startup: Option<DesktopGameRegistry<F, C, P, R>>,
    restoring: Option<DesktopNativeGameServiceRestoreFuture<F, C, P, R>>,
    storage: Option<DesktopNativeGameServiceStorage>,
    restored_after: Option<Uuid>,
    restoration_failure: Option<Arc<DesktopNativeGameServiceError>>,
    registry_port: DesktopGameRegistryPort,
    port: Option<DesktopNativeGameRuntimePort>,
    worker: Option<JoinHandle<DesktopNativeGameServiceOutcome<F, C, P, R>>>,
    retained: Option<DesktopNativeGameRuntime<F, C, P, R>>,
    worker_failure: Option<Arc<tokio::task::JoinError>>,
    is_closing: bool,
}

impl DesktopNativeGameService {
    pub fn proof() -> DesktopNativeGameServiceProof {
        GameMateProof::new(
            GameMateProofBudget::default(),
            DesktopNativeGameServiceProofControl::default(),
        )
    }
}

impl<F, C, P, R> DesktopNativeGameService<F, C, P, R>
where
    F: DesktopGameRegistryFactoryProvider + 'static,
    F::Factory: 'static,
    <F::Factory as DesktopGameSupervisorEngineFactory>::Engine: 'static,
    <<F::Factory as DesktopGameSupervisorEngineFactory>::Engine as GameEngineProvider>::Error:
        'static,
    C: ClockTimeProvider + RefUnwindSafe + 'static,
    P: GameAdjudicationMateProofProvider + Send + Sync + RefUnwindSafe + 'static,
    R: Rng + Send + UnwindSafe + 'static,
{
    pub fn new(options: DesktopNativeGameServiceOptions<F, C, P, R>) -> Self {
        let storage = DesktopNativeGameServiceStorage {
            client: options.storage.clone(),
            executor: Arc::clone(&options.executor),
        };
        let (registry, registry_port) = DesktopGameRegistry::new(options);
        Self {
            startup: Some(registry),
            restoring: None,
            storage: Some(storage),
            restored_after: None,
            restoration_failure: None,
            registry_port,
            port: None,
            worker: None,
            retained: None,
            worker_failure: None,
            is_closing: false,
        }
    }

    pub async fn restore(&mut self) -> Result<(), DesktopNativeGameServiceError> {
        if self.is_closing {
            return Err(DesktopNativeGameServiceError::Closing);
        }
        if let Some(error) = &self.restoration_failure {
            return Err(DesktopNativeGameServiceError::Restoration(Arc::clone(
                error,
            )));
        }
        if self.restoring.is_none() {
            let storage = self
                .storage
                .as_ref()
                .ok_or(DesktopNativeGameServiceError::Started)?
                .clone();
            let mut registry = self
                .startup
                .take()
                .ok_or(DesktopNativeGameServiceError::Started)?;
            let mut after = self.restored_after;
            self.restoring = Some(Box::pin(async move {
                let result = Self::restore_registry(&mut registry, storage, &mut after).await;
                (registry, after, result)
            }));
        }
        self.observe_restore().await
    }

    async fn restore_registry(
        registry: &mut DesktopGameRegistry<F, C, P, R>,
        storage: DesktopNativeGameServiceStorage,
        after: &mut Option<Uuid>,
    ) -> Result<(), DesktopNativeGameServiceError> {
        let repository =
            StorageGameDocumentRepository::new(storage.client.clone(), &storage.executor);
        loop {
            let page = storage.client.inventory(*after).await?;
            for document in &page {
                let game = document.id().to_string().parse()?;
                let restored = repository
                    .load(game)
                    .await?
                    .ok_or(DesktopNativeGameServiceError::MissingSavedGame)?;
                if matches!(
                    restored.session().phase(),
                    GameEnumPhase::Setup | GameEnumPhase::Active | GameEnumPhase::Interrupted
                ) {
                    let storage_revision = restored.storage_revision();
                    registry
                        .restore(DesktopGameRegistryRestored {
                            session: restored.into_session(),
                            storage_revision,
                        })
                        .await?;
                }
                *after = Some(document.id());
            }
            if page.len() < StorageDocumentStore::INVENTORY_PAGE_SIZE {
                return Ok(());
            }
        }
    }

    async fn observe_restore(&mut self) -> Result<(), DesktopNativeGameServiceError> {
        let Some(restoring) = &mut self.restoring else {
            return Ok(());
        };
        let (registry, after, result) = restoring.await;
        self.restoring = None;
        self.startup = Some(registry);
        self.restored_after = after;
        if let Err(error) = result {
            let error = Arc::new(error);
            self.restoration_failure = Some(Arc::clone(&error));
            return Err(DesktopNativeGameServiceError::Restoration(error));
        }
        Ok(())
    }

    pub fn start(&mut self) -> Result<DesktopNativeGameRuntimePort, DesktopNativeGameServiceError> {
        if self.is_closing {
            return Err(DesktopNativeGameServiceError::Closing);
        }
        if self.restoring.is_some() {
            return Err(DesktopNativeGameServiceError::Restoring);
        }
        if let Some(error) = &self.restoration_failure {
            return Err(DesktopNativeGameServiceError::Restoration(Arc::clone(
                error,
            )));
        }
        let registry = self
            .startup
            .take()
            .ok_or(DesktopNativeGameServiceError::Started)?;
        let (mut runtime, port) =
            DesktopNativeGameRuntime::new(registry, self.registry_port.clone());
        self.worker = Some(tokio::spawn(async move {
            let result = runtime.run().await;
            (runtime, result)
        }));
        self.storage = None;
        self.port = Some(port.clone());
        Ok(port)
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.is_closing
            && self.startup.is_none()
            && self.restoring.is_none()
            && self.retained.is_none()
            && self.worker.is_none()
            && self.worker_failure.is_none()
            && self
                .port
                .as_ref()
                .is_none_or(DesktopNativeGameRuntimePort::is_shutdown_complete)
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopNativeGameServiceError> {
        self.is_closing = true;
        let mut failure = None;
        if self.restoring.is_some() {
            let result = match timeout_at(deadline, self.observe_restore()).await {
                Ok(result) => result,
                Err(error) => Err(DesktopNativeGameServiceError::Deadline(error)),
            };
            if let Err(error) = result {
                DesktopNativeGameServiceError::record(&mut failure, error);
            }
        }
        if let Some(registry) = &mut self.startup {
            if let Err(error) = Box::pin(registry.shutdown_retained(deadline)).await {
                DesktopNativeGameServiceError::record(&mut failure, error.into());
            }
            if registry.is_shutdown_complete() {
                self.startup = None;
            }
        }
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
            && let Some(port) = &self.port
            && let Err(error) = port.shutdown(deadline).await
        {
            DesktopNativeGameServiceError::record(
                &mut failure,
                DesktopNativeGameServiceError::Runtime(error),
            );
        }
        if (self.worker.as_ref().is_some_and(JoinHandle::is_finished)
            || self
                .port
                .as_ref()
                .is_some_and(DesktopNativeGameRuntimePort::is_shutdown_complete))
            && let Err(error) = self.observe_worker(deadline).await
        {
            DesktopNativeGameServiceError::record(&mut failure, error);
        }
        if let Some(owner) = &mut self.retained {
            if let Err(error) = Box::pin(owner.shutdown_retained(deadline)).await {
                DesktopNativeGameServiceError::record(
                    &mut failure,
                    DesktopNativeGameServiceError::Runtime(error),
                );
            }
            if owner.is_shutdown_complete() {
                self.retained = None;
            }
        }
        if let Some(error) = &self.worker_failure {
            DesktopNativeGameServiceError::record(
                &mut failure,
                DesktopNativeGameServiceError::Worker(Arc::clone(error)),
            );
        }
        if self.is_shutdown_complete() {
            self.storage = None;
        }
        failure.map_or(Ok(()), Err)
    }

    async fn observe_worker(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopNativeGameServiceError> {
        let Some(worker) = &mut self.worker else {
            return Ok(());
        };
        let joined = timeout_at(deadline, worker)
            .await
            .map_err(DesktopNativeGameServiceError::Deadline)?;
        self.worker = None;
        match joined {
            Ok((owner, result)) => {
                if !owner.is_shutdown_complete() {
                    self.retained = Some(owner);
                }
                result.map_err(DesktopNativeGameServiceError::Runtime)
            }
            Err(error) => {
                self.worker_failure = Some(Arc::new(error));
                Ok(())
            }
        }
    }
}
