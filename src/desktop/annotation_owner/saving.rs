use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumPending;
use crate::desktop::annotation_owner::{
    DesktopAnnotationOwner, DesktopAnnotationOwnerEnumSaveProgress, DesktopAnnotationOwnerError,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::game::enums::{GameId, GameRevision};
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_notation::StorageLibraryNotationRepository;
use crate::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryPreparedWrite,
};
use crate::storage::library_repository::{
    StorageLibraryContentReceipt, StorageLibraryEnumMutationReceipt, StorageLibraryError,
    StorageLibraryMutation,
};

enum DesktopAnnotationOwnerSavingEnumPhase {
    Preparing(
        Pin<
            Box<
                dyn Future<Output = Result<StorageLibraryPreparedWrite, StorageLibraryError>>
                    + Send
                    + 'static,
            >,
        >,
    ),
    Committing(StorageLibraryMutation),
    Completed(Arc<StorageLibraryContentReceipt>),
    Projecting {
        saved: Arc<StorageLibraryContentReceipt>,
        future:
            Pin<Box<dyn Future<Output = Result<(), DesktopAnnotationOwnerError>> + Send + 'static>>,
    },
    Unresolved(Arc<DesktopAnnotationOwnerError>),
}

pub(super) struct DesktopAnnotationOwnerSave {
    receipt: DesktopApplicationReceipt,
    game: GameId,
    expected: GameStorageRevision,
    next: GameStorageRevision,
    covered: GameRevision,
    phase: DesktopAnnotationOwnerSavingEnumPhase,
}

impl DesktopAnnotationOwnerSave {
    pub(super) fn pending(&self, computing: bool) -> DesktopAnnotationCommandsEnumPending {
        match &self.phase {
            DesktopAnnotationOwnerSavingEnumPhase::Preparing(_) => {
                DesktopAnnotationCommandsEnumPending::Preparing { computing }
            }
            DesktopAnnotationOwnerSavingEnumPhase::Committing(_) => {
                DesktopAnnotationCommandsEnumPending::Committing { computing }
            }
            DesktopAnnotationOwnerSavingEnumPhase::Completed(saved) => {
                if saved.id() != self.game || saved.content_revision() != self.next {
                    return DesktopAnnotationCommandsEnumPending::Unresolved;
                }
                DesktopAnnotationCommandsEnumPending::Projecting {
                    storage_revision: self.next,
                    covered: self.covered,
                }
            }
            DesktopAnnotationOwnerSavingEnumPhase::Projecting { .. } => {
                DesktopAnnotationCommandsEnumPending::Projecting {
                    storage_revision: self.next,
                    covered: self.covered,
                }
            }
            DesktopAnnotationOwnerSavingEnumPhase::Unresolved(_) => {
                DesktopAnnotationCommandsEnumPending::Unresolved
            }
        }
    }

    pub(super) fn is_projecting(&self) -> bool {
        matches!(
            self.phase,
            DesktopAnnotationOwnerSavingEnumPhase::Completed(_)
                | DesktopAnnotationOwnerSavingEnumPhase::Projecting { .. }
        )
    }
}

impl DesktopAnnotationOwner {
    pub(super) fn start_save(
        &mut self,
        receipt: DesktopApplicationReceipt,
        retry: bool,
    ) -> Result<(), DesktopAnnotationOwnerError> {
        if self.save.is_some() {
            return Err(DesktopAnnotationOwnerError::SavePending);
        }
        if self.save_failed && !retry {
            return Err(DesktopAnnotationOwnerError::RetryRequired);
        }
        let source = Self::lock(&self.source)?;
        let expected = source.storage_revision();
        let next = expected
            .value()
            .checked_add(1)
            .map(GameStorageRevision::new)
            .ok_or(StorageLibraryError::ContentExhausted)?;
        let covered = source.revision();
        let record = source.record_for_preparation();
        let game = record.id();
        let change = match source
            .accepted()
            .filter(|edit| edit.revision() > self.durable)
        {
            Some(edit) => StorageLibraryEnumContentChange::Changed(edit.timestamp()),
            None => StorageLibraryEnumContentChange::Checkpoint,
        };
        drop(source);
        let executor = Arc::clone(&self.resources.executor);
        let client = self.resources.client.clone();
        let future = Box::pin(async move {
            StorageLibraryNotationRepository::new(client, &executor)
                .prepare(expected, record, change)
                .await
        });
        self.save = Some(DesktopAnnotationOwnerSave {
            receipt,
            game,
            expected,
            next,
            covered,
            phase: DesktopAnnotationOwnerSavingEnumPhase::Preparing(future),
        });
        Ok(())
    }

    pub async fn observe_save(
        &mut self,
        receipt: &DesktopApplicationReceipt,
    ) -> Result<DesktopAnnotationOwnerEnumSaveProgress, DesktopAnnotationOwnerError> {
        let pending = self
            .save
            .as_mut()
            .ok_or(DesktopAnnotationOwnerError::NoSave)?;
        if !pending.receipt.shares_outcome(receipt) {
            return Err(DesktopAnnotationOwnerError::ForeignReceipt);
        }
        match &mut pending.phase {
            DesktopAnnotationOwnerSavingEnumPhase::Preparing(future) => {
                let prepared = match future.as_mut().await {
                    Ok(prepared) => prepared,
                    Err(error) => return Err(self.save_failure(error.into())),
                };
                let repository = StorageLibraryNotationRepository::new(
                    self.resources.client.clone(),
                    &self.resources.executor,
                );
                match repository.begin_save(prepared) {
                    Ok(mutation) => {
                        pending.phase = DesktopAnnotationOwnerSavingEnumPhase::Committing(mutation);
                        Ok(DesktopAnnotationOwnerEnumSaveProgress::Committing)
                    }
                    Err(error) => Err(self.save_failure(error.into())),
                }
            }
            DesktopAnnotationOwnerSavingEnumPhase::Committing(mutation) => {
                let result = mutation.wait().await;
                match result {
                    Ok(StorageLibraryEnumMutationReceipt::Saved(saved)) => {
                        if saved.id() != pending.game || saved.content_revision() != pending.next {
                            pending.phase =
                                DesktopAnnotationOwnerSavingEnumPhase::Completed(Arc::new(saved));
                            return Err(self.retain_failure(DesktopAnnotationOwnerError::Protocol));
                        }
                        pending.phase =
                            DesktopAnnotationOwnerSavingEnumPhase::Completed(Arc::new(saved));
                        self.finish_save().await
                    }
                    Ok(
                        StorageLibraryEnumMutationReceipt::Organized(_)
                        | StorageLibraryEnumMutationReceipt::DestructionApplied(_)
                        | StorageLibraryEnumMutationReceipt::Restored(_),
                    ) => Err(self.unresolved_save(DesktopAnnotationOwnerError::Protocol)),
                    Err(
                        error @ (StorageLibraryError::Protocol
                        | StorageLibraryError::AlreadyObserved
                        | StorageLibraryError::Channel(_)),
                    ) => Err(self.unresolved_save(error.into())),
                    Err(error) => Err(self.save_failure(error.into())),
                }
            }
            DesktopAnnotationOwnerSavingEnumPhase::Completed(_)
            | DesktopAnnotationOwnerSavingEnumPhase::Projecting { .. } => self.finish_save().await,
            DesktopAnnotationOwnerSavingEnumPhase::Unresolved(error) => {
                Err(DesktopAnnotationOwnerError::Retained(Arc::clone(error)))
            }
        }
    }

    async fn finish_save(
        &mut self,
    ) -> Result<DesktopAnnotationOwnerEnumSaveProgress, DesktopAnnotationOwnerError> {
        if self.compute.is_some() {
            return Err(DesktopAnnotationOwnerError::ComputePending);
        }
        let pending = self
            .save
            .as_mut()
            .ok_or(DesktopAnnotationOwnerError::NoSave)?;
        if let DesktopAnnotationOwnerSavingEnumPhase::Completed(saved) = &pending.phase {
            if saved.id() != pending.game || saved.content_revision() != pending.next {
                return Err(DesktopAnnotationOwnerError::Protocol);
            }
            let source = Arc::clone(&self.source);
            let executor = Arc::clone(&self.resources.executor);
            let game = pending.game;
            let expected = pending.expected;
            let next = pending.next;
            let saved = Arc::clone(saved);
            pending.phase = DesktopAnnotationOwnerSavingEnumPhase::Projecting {
                saved,
                future: Box::pin(async move {
                    executor
                        .submit_waiting(move |_cancellation| {
                            let mut source = source
                                .lock()
                                .map_err(|_error| DesktopAnnotationOwnerError::Poisoned)?;
                            if source.context()?.game() != game
                                || source.storage_revision() != expected
                            {
                                return Err(DesktopAnnotationOwnerError::Protocol);
                            }
                            source.saved(next)
                        })
                        .await?
                        .join()
                        .await?
                }),
            };
        }
        let DesktopAnnotationOwnerSavingEnumPhase::Projecting { saved, future } =
            &mut pending.phase
        else {
            return Err(DesktopAnnotationOwnerError::Protocol);
        };
        if let Err(error) = future.as_mut().await {
            if matches!(error, DesktopAnnotationOwnerError::Publication(_)) {
                pending.phase = DesktopAnnotationOwnerSavingEnumPhase::Completed(Arc::clone(saved));
                return Err(self.retain_failure(error));
            }
            return Err(self.unresolved_save(error));
        }
        let covered = pending.covered;
        let receipt = Arc::clone(saved);
        self.refresh_observation()?;
        self.durable = self.durable.max(covered);
        if !self.status().is_dirty() {
            self.accepted_receipt = None;
        }
        self.save = None;
        self.failure = None;
        self.save_failed = false;
        Ok(DesktopAnnotationOwnerEnumSaveProgress::Saved { receipt, covered })
    }

    fn save_failure(&mut self, error: DesktopAnnotationOwnerError) -> DesktopAnnotationOwnerError {
        self.save = None;
        self.save_failed = true;
        self.retain_failure(error)
    }

    fn unresolved_save(
        &mut self,
        error: DesktopAnnotationOwnerError,
    ) -> DesktopAnnotationOwnerError {
        let failure = Arc::new(error);
        if let Some(pending) = &mut self.save {
            pending.phase = DesktopAnnotationOwnerSavingEnumPhase::Unresolved(Arc::clone(&failure));
        }
        if self.failure.is_none() {
            self.failure = Some(Arc::clone(&failure));
        }
        DesktopAnnotationOwnerError::Retained(failure)
    }
}
