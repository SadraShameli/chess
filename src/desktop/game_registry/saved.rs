use std::future::{Future, pending};
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use rand::Rng;
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, timeout_at};

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumResult;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryActor, DesktopGameRegistryCreateFuture,
    DesktopGameRegistryError, DesktopGameRegistryFactoryProvider, DesktopGameRegistryPort,
    DesktopGameRegistryRestored,
};
use crate::desktop::game_supervisor::{
    DesktopGameSupervisor, DesktopGameSupervisorEditorReceipt, DesktopGameSupervisorEngineFactory,
    DesktopGameSupervisorOptions, DesktopGameSupervisorPort,
};
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::engine_provider::GameEngineProvider;
use crate::game::enums::GameId;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_record::StorageLibraryEnumContent;
use crate::storage::library_repository::{
    StorageLibraryError, StorageLibraryRecovered, StorageLibraryRepository,
    StorageLibrarySourceMemory,
};
use crate::storage::trash::StorageTrashEnumLocation;

type DesktopGameRegistrySavedResponse =
    Result<DesktopAnnotationCommandsEnumResult, Arc<DesktopGameRegistryError>>;
type DesktopGameRegistrySavedCreated<F, C, P> = Result<
    Box<(DesktopGameRegistryActor<F, C, P>, DesktopGameSupervisorPort)>,
    DesktopGameRegistryError,
>;
type DesktopGameRegistrySavedLoad = Pin<
    Box<
        dyn Future<Output = Result<DesktopGameRegistrySavedRestored, DesktopGameRegistryError>>
            + Send
            + 'static,
    >,
>;

pub(super) struct DesktopGameRegistrySavedRestored {
    restored: DesktopGameRegistryRestored,
    memory: StorageLibrarySourceMemory,
}

pub(super) struct DesktopGameRegistrySavedRequest {
    game: GameId,
    storage_revision: GameStorageRevision,
    application: DesktopApplicationReceipt,
    recovered: Option<StorageLibraryRecovered>,
    editor: Arc<OnceLock<DesktopGameSupervisorEditorReceipt>>,
    response: watch::Sender<Option<DesktopGameRegistrySavedResponse>>,
}

impl DesktopGameRegistrySavedRequest {
    pub(super) const fn game(&self) -> GameId {
        self.game
    }

    pub(super) fn complete(self, result: DesktopGameRegistrySavedResponse) {
        self.response.send_replace(Some(result));
    }
}

pub struct DesktopGameRegistrySavedOpenReceipt {
    application: DesktopApplicationReceipt,
    response: watch::Receiver<Option<DesktopGameRegistrySavedResponse>>,
    editor: Arc<OnceLock<DesktopGameSupervisorEditorReceipt>>,
}

impl DesktopGameRegistrySavedOpenReceipt {
    pub const fn application_receipt(&self) -> &DesktopApplicationReceipt {
        &self.application
    }

    pub fn editor_receipt(&self) -> Option<&DesktopGameSupervisorEditorReceipt> {
        self.editor.get()
    }

    pub fn try_result(&self) -> Option<DesktopGameRegistrySavedResponse> {
        self.response.borrow().clone()
    }

    pub async fn wait(&mut self) -> DesktopGameRegistrySavedResponse {
        loop {
            if let Some(result) = self.try_result() {
                return result;
            }
            self.response
                .changed()
                .await
                .map_err(|_closed| Arc::new(DesktopGameRegistryError::Closed))?;
        }
    }
}

impl DesktopGameRegistryPort {
    pub fn try_open_saved_editor(
        &self,
        game: GameId,
        storage_revision: GameStorageRevision,
        application: DesktopApplicationReceipt,
    ) -> Result<DesktopGameRegistrySavedOpenReceipt, DesktopGameRegistryError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DesktopGameRegistryError::Closed);
        }
        let (response, receiver) = watch::channel(None);
        let editor = Arc::new(OnceLock::new());
        self.saved
            .try_send(DesktopGameRegistrySavedRequest {
                game,
                storage_revision,
                application: application.clone(),
                recovered: None,
                editor: Arc::clone(&editor),
                response,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => DesktopGameRegistryError::Busy,
                mpsc::error::TrySendError::Closed(_) => DesktopGameRegistryError::Closed,
            })?;
        Ok(DesktopGameRegistrySavedOpenReceipt {
            application,
            response: receiver,
            editor,
        })
    }

    pub fn try_open_recovered_editor(
        &self,
        recovered: StorageLibraryRecovered,
        expected_storage_revision: GameStorageRevision,
        application: DesktopApplicationReceipt,
    ) -> Result<DesktopGameRegistrySavedOpenReceipt, DesktopGameRegistryError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DesktopGameRegistryError::Closed);
        }
        let game = recovered.record().id();
        let (response, receiver) = watch::channel(None);
        let editor = Arc::new(OnceLock::new());
        self.saved
            .try_send(DesktopGameRegistrySavedRequest {
                game,
                storage_revision: expected_storage_revision,
                application: application.clone(),
                recovered: Some(recovered),
                editor: Arc::clone(&editor),
                response,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => DesktopGameRegistryError::Busy,
                mpsc::error::TrySendError::Closed(_) => DesktopGameRegistryError::Closed,
            })?;
        Ok(DesktopGameRegistrySavedOpenReceipt {
            application,
            response: receiver,
            editor,
        })
    }
}

enum DesktopGameRegistrySavedEnumPhase<F: DesktopGameRegistryFactoryProvider, C, P> {
    Loading(DesktopGameRegistrySavedLoad),
    Creating {
        future: DesktopGameRegistryCreateFuture<F, C, P>,
        memory: StorageLibrarySourceMemory,
    },
    Installing(DesktopGameSupervisorPort),
    Editing(DesktopGameSupervisorEditorReceipt),
}

pub(super) struct DesktopGameRegistrySavedOpen<F: DesktopGameRegistryFactoryProvider, C, P> {
    request: DesktopGameRegistrySavedRequest,
    phase: DesktopGameRegistrySavedEnumPhase<F, C, P>,
}

impl<F: DesktopGameRegistryFactoryProvider, C, P> DesktopGameRegistrySavedOpen<F, C, P> {
    pub(super) const fn game(&self) -> GameId {
        self.request.game
    }
}

pub(super) enum DesktopGameRegistrySavedEnumReady<F: DesktopGameRegistryFactoryProvider, C, P> {
    Loaded(Result<Box<DesktopGameRegistrySavedRestored>, DesktopGameRegistryError>),
    Created(DesktopGameRegistrySavedCreated<F, C, P>),
    Edited(Result<DesktopAnnotationCommandsEnumResult, Arc<DesktopGameRegistryError>>),
}

impl<F, C, P, R> DesktopGameRegistry<F, C, P, R>
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
    pub(super) fn begin_saved_open(&mut self, mut request: DesktopGameRegistrySavedRequest) {
        if let Err(error) = self
            .closing
            .admission
            .lock()
            .and_then(|admission| admission.check_game(request.game))
        {
            request.complete(Err(Arc::new(error)));
            return;
        }
        let existing = match self.sessions.try_read() {
            Ok(sessions) => sessions.get(&request.game).cloned(),
            Err(_busy) => {
                request.complete(Err(Arc::new(DesktopGameRegistryError::Busy)));
                return;
            }
        };
        if let Some(port) = existing {
            match port.try_open_editor(request.application.clone()) {
                Ok(receipt) => {
                    if request.editor.set(receipt.clone()).is_err() {
                        request.complete(Err(Arc::new(DesktopGameRegistryError::MissingCreation)));
                        return;
                    }
                    self.saved = Some(DesktopGameRegistrySavedOpen {
                        request,
                        phase: DesktopGameRegistrySavedEnumPhase::Editing(receipt),
                    });
                }
                Err(error) => request.complete(Err(Arc::new(
                    DesktopGameRegistryError::Supervisor(Arc::new(error)),
                ))),
            }
            return;
        }
        let storage = self.storage.clone();
        let executor = Arc::clone(&self.executor);
        let game = request.game;
        let expected = request.storage_revision;
        let cancellation = request.application.cancellation();
        let recovered = request.recovered.take();
        let future = Box::pin(async move {
            if cancellation.is_requested() {
                return Err(DesktopGameRegistryError::Closed);
            }
            let recovered = match recovered {
                Some(recovered) => recovered,
                None => StorageLibraryRepository::new(storage, &executor)
                    .load(game)
                    .await
                    .map_err(|error| DesktopGameRegistryError::Library(Box::new(error)))?
                    .ok_or_else(|| {
                        DesktopGameRegistryError::Library(Box::new(
                            StorageLibraryError::MissingGame(game),
                        ))
                    })?,
            };
            if recovered.location() != StorageTrashEnumLocation::Library {
                return Err(DesktopGameRegistryError::Library(Box::new(
                    StorageLibraryError::InTrash(game),
                )));
            }
            let actual = recovered.content_revision();
            if actual != expected {
                return Err(DesktopGameRegistryError::Library(Box::new(
                    StorageLibraryError::ContentConflict { expected, actual },
                )));
            }
            let (record, memory) = recovered.into_retained_record();
            match record.into_content() {
                StorageLibraryEnumContent::Played(session) => {
                    Ok(DesktopGameRegistrySavedRestored {
                        restored: DesktopGameRegistryRestored {
                            session: *session,
                            storage_revision: actual,
                        },
                        memory,
                    })
                }
                StorageLibraryEnumContent::Notation(_) => Err(DesktopGameRegistryError::Library(
                    Box::new(StorageLibraryError::ContentKind),
                )),
            }
        });
        self.saved = Some(DesktopGameRegistrySavedOpen {
            request,
            phase: DesktopGameRegistrySavedEnumPhase::Loading(future),
        });
    }

    pub(super) async fn saved_ready(
        saved: &mut Option<DesktopGameRegistrySavedOpen<F, C, P>>,
    ) -> DesktopGameRegistrySavedEnumReady<F, C, P> {
        match saved.as_mut().map(|saved| &mut saved.phase) {
            Some(DesktopGameRegistrySavedEnumPhase::Loading(future)) => {
                DesktopGameRegistrySavedEnumReady::Loaded(future.await.map(Box::new))
            }
            Some(DesktopGameRegistrySavedEnumPhase::Creating { future, .. }) => {
                DesktopGameRegistrySavedEnumReady::Created(future.await.map(Box::new))
            }
            Some(DesktopGameRegistrySavedEnumPhase::Editing(receipt)) => {
                DesktopGameRegistrySavedEnumReady::Edited(
                    receipt
                        .wait()
                        .await
                        .map_err(|error| Arc::new(DesktopGameRegistryError::Supervisor(error))),
                )
            }
            Some(DesktopGameRegistrySavedEnumPhase::Installing(_)) | None => pending().await,
        }
    }

    pub(super) async fn finish_saved_step(
        &mut self,
        ready: DesktopGameRegistrySavedEnumReady<F, C, P>,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        match ready {
            DesktopGameRegistrySavedEnumReady::Loaded(Ok(loaded)) => {
                let DesktopGameRegistrySavedRestored { restored, memory } = *loaded;
                let factory = match self.factory_provider.factory(restored.session.id()) {
                    Ok(factory) => factory,
                    Err(source) => {
                        self.complete_saved(Err(Arc::new(DesktopGameRegistryError::Factory {
                            source: Box::new(source),
                        })))?;
                        return Ok(());
                    }
                };
                let storage = self.storage.clone();
                let executor = Arc::clone(&self.executor);
                let clock = Arc::clone(&self.clock);
                let wall_time = Arc::clone(&self.wall_time);
                let proof = Arc::clone(&self.proof);
                let search_config = restored.session.engine_search_config();
                let creating = Box::pin(async move {
                    DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
                        session: restored.session,
                        storage_revision: restored.storage_revision,
                        storage,
                        executor,
                        clock,
                        wall_time,
                        proof,
                        factory,
                        search_config,
                    })
                    .await
                    .map_err(|error| DesktopGameRegistryError::Supervisor(Arc::new(error)))
                });
                self.saved
                    .as_mut()
                    .ok_or(DesktopGameRegistryError::MissingCreation)?
                    .phase = DesktopGameRegistrySavedEnumPhase::Creating {
                    future: creating,
                    memory,
                };
            }
            DesktopGameRegistrySavedEnumReady::Created(Ok(created)) => {
                let (actor, port) = *created;
                let saved = self
                    .saved
                    .as_mut()
                    .ok_or(DesktopGameRegistryError::MissingCreation)?;
                let creating = std::mem::replace(
                    &mut saved.phase,
                    DesktopGameRegistrySavedEnumPhase::Installing(port.clone()),
                );
                let DesktopGameRegistrySavedEnumPhase::Creating { memory, .. } = creating else {
                    saved.phase = creating;
                    return Err(DesktopGameRegistryError::MissingCreation);
                };
                self.saved_sources.insert(saved.request.game, memory);
                self.install(actor, port, deadline).await?;
                self.resume_saved_installation(deadline).await?;
            }
            DesktopGameRegistrySavedEnumReady::Edited(result) => self.complete_saved(result)?,
            DesktopGameRegistrySavedEnumReady::Loaded(Err(error))
            | DesktopGameRegistrySavedEnumReady::Created(Err(error)) => {
                self.complete_saved(Err(Arc::new(error)))?
            }
        }
        Ok(())
    }

    pub(super) async fn resume_saved_installation(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        let Some(DesktopGameRegistrySavedOpen {
            phase: DesktopGameRegistrySavedEnumPhase::Installing(port),
            ..
        }) = &self.saved
        else {
            return Ok(());
        };
        let port = port.clone();
        self.finish_installations(deadline).await?;
        let application = self
            .saved
            .as_ref()
            .ok_or(DesktopGameRegistryError::MissingCreation)?
            .request
            .application
            .clone();
        match port.try_open_editor(application) {
            Ok(receipt) => {
                let saved = self
                    .saved
                    .as_mut()
                    .ok_or(DesktopGameRegistryError::MissingCreation)?;
                saved
                    .request
                    .editor
                    .set(receipt.clone())
                    .map_err(|_receipt| DesktopGameRegistryError::MissingCreation)?;
                saved.phase = DesktopGameRegistrySavedEnumPhase::Editing(receipt);
            }
            Err(error) => self.complete_saved(Err(Arc::new(
                DesktopGameRegistryError::Supervisor(Arc::new(error)),
            )))?,
        }
        Ok(())
    }

    fn complete_saved(
        &mut self,
        result: DesktopGameRegistrySavedResponse,
    ) -> Result<(), DesktopGameRegistryError> {
        let saved = self
            .saved
            .take()
            .ok_or(DesktopGameRegistryError::MissingCreation)?;
        drop(saved.phase);
        saved.request.complete(result);
        Ok(())
    }

    pub(super) async fn drain_saved_open(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        self.resume_saved_installation(deadline).await?;
        while self.saved.is_some() {
            let ready = timeout_at(deadline, Self::saved_ready(&mut self.saved))
                .await
                .map_err(DesktopGameRegistryError::Deadline)?;
            self.finish_saved_step(ready, deadline).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/desktop/game_registry/saved.rs"]
mod tests;
