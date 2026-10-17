use std::sync::Arc;

use tokio::sync::watch;

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::annotation_owner::DesktopAnnotationOwnerResources;
use crate::desktop::game_editor::{DesktopGameEditorEnumSource, DesktopGameEditorPublication};
use crate::desktop::game_registry::{
    DesktopGameRegistryError, DesktopGameRegistryPort, DesktopGameRegistrySavedOpenReceipt,
};
use crate::desktop::game_supervisor::DesktopGameSupervisorEditorReceipt;
use crate::desktop::library_service::notation::{
    DesktopLibraryServiceNotation, DesktopLibraryServiceNotationProjection,
};
use crate::desktop::library_service::import::DesktopLibraryServiceImports;
use crate::desktop::library_service::played::DesktopLibraryServicePlayed;
use crate::desktop::library_service::port::{
    DesktopLibraryServiceEnvelope, DesktopLibraryServiceResponse,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumResult,
    DesktopLibraryServiceEnumSaveReceipt, DesktopLibraryServiceError,
};
use crate::desktop::annotation_owner::DesktopAnnotationOwnerError;
use crate::game::library_organization::GameLibraryOrganizationRequest;
use crate::desktop::game_registry::{
    DesktopGameRegistryCloseCertificate, DesktopGameRegistryCloseEnumObservation,
};
use crate::desktop::library_commands::{
    DesktopLibraryCommandsConfirmation, DesktopLibraryCommandsError,
};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::storage::trash::{
    StorageTrashConfirmedRequest, StorageTrashPlan, StorageTrashPreviewRequest,
    StorageTrashReceipt, StorageTrashRestoreRequest,
};
use crate::storage::library_metadata::StorageLibraryMetadataRepository;
use crate::storage::library_query::StorageLibraryQuery;
use crate::storage::library_record::StorageLibraryEnumContent;
use crate::storage::library_repository::{
    StorageLibraryEnumMutationReceipt, StorageLibraryError, StorageLibraryRepository,
};
use crate::storage::trash::StorageTrashEnumLocation;

pub(super) struct DesktopLibraryServiceWork {
    pub(super) envelope: DesktopLibraryServiceEnvelope,
    pub(super) notation: Option<DesktopLibraryServiceNotation>,
    pub(super) played: Option<DesktopLibraryServicePlayed>,
    pub(super) result: DesktopLibraryServiceResponse,
}

impl DesktopLibraryServiceWork {
    pub(super) fn new(
        envelope: DesktopLibraryServiceEnvelope,
        notation: Option<DesktopLibraryServiceNotation>,
        played: Option<DesktopLibraryServicePlayed>,
    ) -> Self {
        Self {
            envelope,
            notation,
            played,
            result: Err(Arc::new(DesktopLibraryServiceError::Protocol)),
        }
    }

    pub(super) async fn execute(
        mut self,
        progress: Option<watch::Sender<DesktopLibraryServiceNotationProjection>>,
        resources: DesktopAnnotationOwnerResources,
        games: DesktopGameRegistryPort,
        imports: Option<Arc<DesktopLibraryServiceImports>>,
    ) -> Self {
        let result = self.perform(progress, resources, games, imports).await;
        let retained = self
            .played
            .as_ref()
            .filter(|played| played.requires_continuation())
            .map(|played| Arc::clone(&played.completion))
            .or_else(|| {
                self.notation
                    .as_ref()
                    .and_then(DesktopLibraryServiceNotation::continuation_reply)
                    .filter(|reply| {
                        reply
                            .application
                            .shares_outcome(&self.envelope.reply.application)
                    })
            });
        self.result = result.map_err(|error| {
            Arc::new(match retained {
                Some(reply) => error.continuation(&reply),
                None => error,
            })
        });
        if let Some(played) = &self.played {
            played.complete(self.result.clone());
        }
        self
    }

    async fn perform(
        &mut self,
        progress: Option<watch::Sender<DesktopLibraryServiceNotationProjection>>,
        resources: DesktopAnnotationOwnerResources,
        games: DesktopGameRegistryPort,
        imports: Option<Arc<DesktopLibraryServiceImports>>,
    ) -> Result<DesktopLibraryServiceEnumResult, DesktopLibraryServiceError> {
        let original = self.played.as_ref().map_or_else(
            || Arc::clone(&self.envelope.reply),
            |played| Arc::clone(&played.completion),
        );
        let application = original.application.clone();
        if self.played.is_none()
            && self.notation.is_none()
            && application.is_cancellation_requested()
        {
            return Err(DesktopLibraryServiceError::Cancelled);
        }
        if let Some(played) = &self.played
            && matches!(
                original.operation.action(),
                DesktopLibraryServiceEnumAction::Open { .. }
            )
            && let Some(expected) = played.completion.editor_context()
        {
            let receipt = games.try_submit_editor(DesktopAnnotationCommands::new(
                application,
                expected,
                DesktopAnnotationCommandsEnumAction::RetrySave,
            ))?;
            self.retain_played(receipt)?;
            return self.observe_played(true).await;
        }
        let operation = Arc::clone(&original.operation);
        let action = resources
            .executor
            .submit_waiting(move |_cancellation| operation.action().clone())
            .await?
            .join()
            .await?;
        match action {
            DesktopLibraryServiceEnumAction::Query { options, cursor } => {
                let query = resources
                    .executor
                    .submit_waiting(move |_cancellation| {
                        let query = StorageLibraryQuery::new(options)?;
                        match cursor {
                            Some(cursor) => query.with_cursor(cursor),
                            None => Ok(query),
                        }
                    })
                    .await?
                    .join()
                    .await??;
                Ok(DesktopLibraryServiceEnumResult::Query(Arc::new(
                    StorageLibraryRepository::new(resources.client, &resources.executor)
                        .query(query)
                        .await?,
                )))
            }
            DesktopLibraryServiceEnumAction::Metadata(request) => {
                Ok(DesktopLibraryServiceEnumResult::Metadata(
                    StorageLibraryMetadataRepository::new(resources.client)
                        .begin(request)?
                        .wait()
                        .await?,
                ))
            }
            DesktopLibraryServiceEnumAction::Open {
                game,
                storage_revision,
            } => {
                if let Some(notation) = &mut self.notation {
                    return notation.open(Arc::clone(&self.envelope.reply)).await;
                }
                match games.try_open_editor(game, application.clone()) {
                    Ok(receipt) => {
                        self.retain_played(receipt)?;
                        return self.observe_played(true).await;
                    }
                    Err(DesktopGameRegistryError::UnknownGame) => {}
                    Err(error) => return Err(error.into()),
                }
                let recovered =
                    StorageLibraryRepository::new(resources.client.clone(), &resources.executor)
                        .load(game)
                        .await?
                        .ok_or(StorageLibraryError::MissingGame(game))?;
                if recovered.location() != StorageTrashEnumLocation::Library {
                    return Err(StorageLibraryError::InTrash(game).into());
                }
                if recovered.content_revision() != storage_revision {
                    return Err(StorageLibraryError::ContentConflict {
                        expected: storage_revision,
                        actual: recovered.content_revision(),
                    }
                    .into());
                }
                match recovered.record().content() {
                    StorageLibraryEnumContent::Played(_) => {
                        let mut receipt = games.try_open_recovered_editor(
                            recovered,
                            storage_revision,
                            application,
                        )?;
                        let result = Self::observe_saved(&mut receipt).await;
                        if let Some(editor) = receipt.editor_receipt().cloned() {
                            self.retain_played(editor)?;
                        }
                        result
                    }
                    StorageLibraryEnumContent::Notation(_) => {
                        self.notation = Some(DesktopLibraryServiceNotation::new(
                            recovered,
                            resources,
                            progress.ok_or(DesktopLibraryServiceError::Protocol)?,
                        ));
                        self.notation
                            .as_mut()
                            .ok_or(DesktopLibraryServiceError::Protocol)?
                            .open(Arc::clone(&self.envelope.reply))
                            .await
                    }
                }
            }
            DesktopLibraryServiceEnumAction::Organize(mutation) => {
                let timestamp = resources
                    .wall_time
                    .now()
                    .map_err(DesktopAnnotationOwnerError::from)?;
                let request = GameLibraryOrganizationRequest::new(timestamp, mutation)
                    .map_err(StorageLibraryError::from)?;
                Ok(DesktopLibraryServiceEnumResult::Organized(
                    StorageLibraryRepository::new(resources.client, &resources.executor)
                        .organize(request)
                        .await?,
                ))
            }
            DesktopLibraryServiceEnumAction::Import(import) => {
                let imports = imports.ok_or(DesktopLibraryServiceError::ImportUnavailable)?;
                Ok(DesktopLibraryServiceEnumResult::Import(
                    imports.run(import).await?,
                ))
            }
            DesktopLibraryServiceEnumAction::Export(export) => {
                Ok(DesktopLibraryServiceEnumResult::Exported(
                    export.run(resources).await?,
                ))
            }
            DesktopLibraryServiceEnumAction::PreviewTrash(request) => {
                Ok(DesktopLibraryServiceEnumResult::TrashPlan(
                    StorageLibraryRepository::new(resources.client, &resources.executor)
                        .preview_destruction(request)
                        .await?,
                ))
            }
            DesktopLibraryServiceEnumAction::ApplyTrash(request) => {
                match StorageLibraryRepository::new(resources.client, &resources.executor)
                    .begin_destruction(request)?
                    .wait()
                    .await?
                {
                    StorageLibraryEnumMutationReceipt::DestructionApplied(receipt) => {
                        Ok(DesktopLibraryServiceEnumResult::Trashed(receipt))
                    }
                    _ => Err(StorageLibraryError::Protocol.into()),
                }
            }
            DesktopLibraryServiceEnumAction::RemoveFolder(plan) => {
                if !matches!(plan.action(), StorageTrashPreviewRequest::RemoveFolder(_)) {
                    return Err(StorageLibraryError::LifecycleRequired.into());
                }
                let timestamp = resources
                    .wall_time
                    .now()
                    .map_err(DesktopAnnotationOwnerError::from)?;
                let request = StorageTrashConfirmedRequest::new(plan.approve(), timestamp, None)?;
                match StorageLibraryRepository::new(resources.client, &resources.executor)
                    .begin_destruction(request)?
                    .wait()
                    .await?
                {
                    StorageLibraryEnumMutationReceipt::DestructionApplied(receipt) => {
                        Ok(DesktopLibraryServiceEnumResult::Trashed(receipt))
                    }
                    _ => Err(StorageLibraryError::Protocol.into()),
                }
            }
            DesktopLibraryServiceEnumAction::ConfirmGameTrash {
                plan,
                open_revision,
            } => {
                let (StorageTrashPreviewRequest::MoveToTrash(game)
                | StorageTrashPreviewRequest::PermanentlyDelete(game)) = plan.action()
                else {
                    return Err(StorageLibraryError::LifecycleRequired.into());
                };
                let revision = plan
                    .content_revision()
                    .ok_or(StorageLibraryError::LifecycleRequired)?;
                let timestamp = resources
                    .wall_time
                    .now()
                    .map_err(DesktopAnnotationOwnerError::from)?;
                let target = match open_revision {
                    Some(live) => games.try_close_target(game, live, revision)?,
                    None => games.try_not_open_close_target(game, revision)?,
                };
                let mut closing = games.try_close_game(target)?;
                let certificate = closing
                    .wait()
                    .await
                    .map_err(DesktopLibraryServiceError::Registry)?;
                match Self::apply_closed_trash(plan, &certificate, timestamp, resources).await {
                    Ok(receipt) => {
                        games.try_finish_close(
                            &certificate,
                            DesktopGameRegistryCloseEnumObservation::TrashCommitted(&receipt),
                        )?;
                        Ok(DesktopLibraryServiceEnumResult::Trashed(receipt))
                    }
                    Err(error) => {
                        let released = games.try_finish_close(
                            &certificate,
                            DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
                        );
                        drop(released);
                        Err(error)
                    }
                }
            }
            DesktopLibraryServiceEnumAction::Restore(target) => {
                let timestamp = resources
                    .wall_time
                    .now()
                    .map_err(DesktopAnnotationOwnerError::from)?;
                let request = StorageTrashRestoreRequest::new(target, timestamp);
                match StorageLibraryRepository::new(resources.client, &resources.executor)
                    .begin_restore(request)?
                    .wait()
                    .await?
                {
                    StorageLibraryEnumMutationReceipt::Restored(receipt) => {
                        Ok(DesktopLibraryServiceEnumResult::Restored(receipt))
                    }
                    _ => Err(StorageLibraryError::Protocol.into()),
                }
            }
            DesktopLibraryServiceEnumAction::Editor { expected, action } => {
                match expected.source() {
                    DesktopGameEditorEnumSource::Notation { .. } => {
                        self.notation
                            .as_mut()
                            .ok_or(DesktopLibraryServiceError::NotOpen)?
                            .execute(Arc::clone(&self.envelope.reply), expected, action)
                            .await
                    }
                    DesktopGameEditorEnumSource::Played { .. } => {
                        let receipt = games.try_submit_editor(DesktopAnnotationCommands::new(
                            application,
                            expected,
                            action,
                        ))?;
                        self.retain_played(receipt)?;
                        self.observe_played(false).await
                    }
                }
            }
        }
    }

    async fn apply_closed_trash(
        plan: StorageTrashPlan,
        certificate: &DesktopGameRegistryCloseCertificate,
        timestamp: GameLibraryOrganizationTimestamp,
        resources: DesktopAnnotationOwnerResources,
    ) -> Result<StorageTrashReceipt, DesktopLibraryServiceError> {
        let request = DesktopLibraryCommandsConfirmation::confirm(plan)
            .prepare(certificate, timestamp)
            .map_err(|error| match error {
                DesktopLibraryCommandsError::Storage(source) => {
                    DesktopLibraryServiceError::Library(source)
                }
                DesktopLibraryCommandsError::Lifecycle(source) => {
                    DesktopLibraryServiceError::Registry(Arc::new(source))
                }
            })?;
        match StorageLibraryRepository::new(resources.client, &resources.executor)
            .begin_destruction(request)?
            .wait()
            .await?
        {
            StorageLibraryEnumMutationReceipt::DestructionApplied(receipt) => Ok(receipt),
            _ => Err(StorageLibraryError::Protocol.into()),
        }
    }

    async fn observe_saved(
        receipt: &mut DesktopGameRegistrySavedOpenReceipt,
    ) -> Result<DesktopLibraryServiceEnumResult, DesktopLibraryServiceError> {
        let result = receipt
            .wait()
            .await
            .map_err(DesktopLibraryServiceError::Registry)?;
        Ok(DesktopLibraryServiceEnumResult::Opened(Self::snapshot(
            result,
        )?))
    }

    fn retain_played(
        &mut self,
        receipt: DesktopGameSupervisorEditorReceipt,
    ) -> Result<(), DesktopLibraryServiceError> {
        if let Some(played) = &self.played {
            if !played
                .receipt
                .application_receipt()
                .shares_outcome(receipt.application_receipt())
            {
                return Err(DesktopLibraryServiceError::ForeignReceipt);
            }
        } else {
            self.played = Some(DesktopLibraryServicePlayed::new(
                Arc::clone(&self.envelope.reply),
                receipt,
            ));
        }
        Ok(())
    }

    async fn observe_played(
        &mut self,
        opening: bool,
    ) -> Result<DesktopLibraryServiceEnumResult, DesktopLibraryServiceError> {
        let receipt = self
            .played
            .as_mut()
            .ok_or(DesktopLibraryServiceError::Protocol)?;
        let result = receipt.receipt.wait().await;
        receipt.retain_publication_context()?;
        let result = result.map_err(|error| {
            DesktopLibraryServiceError::Registry(Arc::new(DesktopGameRegistryError::Supervisor(
                error,
            )))
        })?;
        if opening {
            return Ok(DesktopLibraryServiceEnumResult::Opened(Self::snapshot(
                result,
            )?));
        }
        if let Some(saved) = receipt.receipt.storage_receipt() {
            return Ok(DesktopLibraryServiceEnumResult::Saved {
                snapshot: Self::snapshot(result)?,
                receipt: DesktopLibraryServiceEnumSaveReceipt::Played(saved),
            });
        }
        Ok(DesktopLibraryServiceEnumResult::Editor(result))
    }

    fn snapshot(
        result: DesktopAnnotationCommandsEnumResult,
    ) -> Result<DesktopGameEditorPublication, DesktopLibraryServiceError> {
        match result {
            DesktopAnnotationCommandsEnumResult::Snapshot(snapshot)
            | DesktopAnnotationCommandsEnumResult::Edited { snapshot, .. } => Ok(snapshot),
            DesktopAnnotationCommandsEnumResult::Tree(_)
            | DesktopAnnotationCommandsEnumResult::Children(_)
            | DesktopAnnotationCommandsEnumResult::Variation(_)
            | DesktopAnnotationCommandsEnumResult::Scope(_)
            | DesktopAnnotationCommandsEnumResult::PendingProjection { .. } => {
                Err(DesktopLibraryServiceError::Protocol)
            }
        }
    }
}
