use std::fmt::{Debug, Formatter};

use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction;
use crate::desktop::game_editor::{DesktopGameEditorContext, DesktopGameEditorEnumSource};
use crate::desktop::identity::DesktopRevision;
use crate::desktop::library_service::export::DesktopLibraryServiceExport;
use crate::desktop::library_service::file::DesktopLibraryServiceFile;
use crate::desktop::library_service::import::DesktopLibraryServiceEnumImportAction;
use crate::desktop::library_service::DesktopLibraryServiceError;
use crate::desktop::file_handle::DesktopFileEnumPurpose;
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::game::enums::{GameId, GameRevision};
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationRequest,
};
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::library_metadata::StorageLibraryMetadataEnumRequest;
use crate::storage::trash::{
    StorageTrashConfirmedRequest, StorageTrashPlan, StorageTrashPreviewRequest,
};
use crate::storage::library_query::{StorageLibraryQueryCursor, StorageLibraryQueryOptions};

#[derive(Clone, PartialEq, Eq)]
pub enum DesktopLibraryServiceEnumAction {
    Query {
        options: StorageLibraryQueryOptions,
        cursor: Option<StorageLibraryQueryCursor>,
    },
    Metadata(StorageLibraryMetadataEnumRequest),
    Open {
        game: GameId,
        storage_revision: GameStorageRevision,
    },
    Editor {
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
    },
    Organize(GameLibraryOrganizationEnumMutation),
    PreviewTrash(StorageTrashPreviewRequest),
    ApplyTrash(StorageTrashConfirmedRequest),
    Restore(GameLibraryOrganizationGameTarget),
    RemoveFolder(StorageTrashPlan),
    ConfirmGameTrash {
        plan: StorageTrashPlan,
        open_revision: Option<GameRevision>,
    },
    Import(DesktopLibraryServiceEnumImportAction),
    Export(DesktopLibraryServiceExport),
}

impl Debug for DesktopLibraryServiceEnumAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Query { cursor, .. } => formatter
                .debug_struct("Query")
                .field("continued", &cursor.is_some())
                .finish(),
            Self::Metadata(request) => formatter.debug_tuple("Metadata").field(request).finish(),
            Self::Open {
                game,
                storage_revision,
            } => formatter
                .debug_struct("Open")
                .field("game", game)
                .field("storage_revision", storage_revision)
                .finish(),
            Self::Organize(request) => formatter
                .debug_tuple("Organize")
                .field(&std::mem::discriminant(request))
                .finish(),
            Self::PreviewTrash(request) => {
                formatter.debug_tuple("PreviewTrash").field(request).finish()
            }
            Self::ApplyTrash(request) => formatter
                .debug_tuple("ApplyTrash")
                .field(&request.plan().action())
                .finish(),
            Self::Restore(request) => formatter.debug_tuple("Restore").field(request).finish(),
            Self::Import(action) => formatter.debug_tuple("Import").field(action).finish(),
            Self::Export(request) => formatter.debug_tuple("Export").field(request).finish(),
            Self::ConfirmGameTrash {
                plan,
                open_revision,
            } => formatter
                .debug_struct("ConfirmGameTrash")
                .field("action", &plan.action())
                .field("open_revision", open_revision)
                .finish(),
            Self::RemoveFolder(plan) => formatter
                .debug_tuple("RemoveFolder")
                .field(&plan.action())
                .finish(),
            Self::Editor { expected, action } => formatter
                .debug_struct("Editor")
                .field("expected", expected)
                .field("action", &std::mem::discriminant(action))
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopLibraryServiceOperation {
    workspace: DesktopMessagesEntityId,
    action: DesktopLibraryServiceEnumAction,
}

impl DesktopLibraryServiceOperation {
    pub const fn new(
        workspace: DesktopMessagesEntityId,
        action: DesktopLibraryServiceEnumAction,
    ) -> Self {
        Self { workspace, action }
    }
    pub const fn workspace(&self) -> DesktopMessagesEntityId {
        self.workspace
    }
    pub const fn action(&self) -> &DesktopLibraryServiceEnumAction {
        &self.action
    }
    pub fn into_action(self) -> DesktopLibraryServiceEnumAction {
        self.action
    }

    pub fn validate_payload(&self) -> Result<(), DesktopLibraryServiceError> {
        self.transport_bytes()
            .map(drop)
            .map_err(crate::storage::library_repository::StorageLibraryError::from)?;
        Ok(())
    }

    pub(super) fn editor_context(&self) -> Option<DesktopGameEditorContext> {
        match &self.action {
            DesktopLibraryServiceEnumAction::Editor { expected, .. } => Some(*expected),
            DesktopLibraryServiceEnumAction::Query { .. }
            | DesktopLibraryServiceEnumAction::Metadata(_)
            | DesktopLibraryServiceEnumAction::Organize(_)
            | DesktopLibraryServiceEnumAction::PreviewTrash(_)
            | DesktopLibraryServiceEnumAction::ApplyTrash(_)
            | DesktopLibraryServiceEnumAction::Restore(_)
            | DesktopLibraryServiceEnumAction::RemoveFolder(_)
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash { .. }
            | DesktopLibraryServiceEnumAction::Import(_)
            | DesktopLibraryServiceEnumAction::Export(_)
            | DesktopLibraryServiceEnumAction::Open { .. } => None,
        }
    }

    pub(super) fn transport_bytes(&self) -> Result<u32, StorageDocumentError> {
        let payload = match &self.action {
            DesktopLibraryServiceEnumAction::Editor { action, .. } => match action {
                DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { notes, nags } => notes
                    .capacity()
                    .checked_add(
                        nags.capacity()
                            .checked_mul(
                                std::mem::size_of::<crate::game::move_node::GameMoveNodeNag>(),
                            )
                            .ok_or(StorageDocumentError::DocumentTooLarge)?,
                    )
                    .ok_or(StorageDocumentError::DocumentTooLarge)?,
                DesktopAnnotationCommandsEnumAction::SaveMarkings { markings } => markings
                    .capacity()
                    .checked_mul(std::mem::size_of::<
                        crate::game::move_node::GameMoveNodeMarking,
                    >())
                    .ok_or(StorageDocumentError::DocumentTooLarge)?,
                DesktopAnnotationCommandsEnumAction::Snapshot
                | DesktopAnnotationCommandsEnumAction::TreePage { .. }
                | DesktopAnnotationCommandsEnumAction::ChildrenPage { .. }
                | DesktopAnnotationCommandsEnumAction::Navigate { .. }
                | DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
                | DesktopAnnotationCommandsEnumAction::PreviewVariation(_)
                | DesktopAnnotationCommandsEnumAction::VariationScope { .. }
                | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. }
                | DesktopAnnotationCommandsEnumAction::Checkpoint
                | DesktopAnnotationCommandsEnumAction::RetrySave => 0,
            },
            DesktopLibraryServiceEnumAction::Query { .. }
            | DesktopLibraryServiceEnumAction::Metadata(_)
            | DesktopLibraryServiceEnumAction::Open { .. } => 0,
            DesktopLibraryServiceEnumAction::Organize(mutation) => {
                std::mem::size_of::<GameLibraryOrganizationRequest>() + mutation.payload_bytes()
            }
            DesktopLibraryServiceEnumAction::PreviewTrash(_)
            | DesktopLibraryServiceEnumAction::ApplyTrash(_)
            | DesktopLibraryServiceEnumAction::Restore(_)
            | DesktopLibraryServiceEnumAction::RemoveFolder(_)
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash { .. }
            | DesktopLibraryServiceEnumAction::Import(_)
            | DesktopLibraryServiceEnumAction::Export(_) => 0,
        };
        let bytes = payload
            .checked_add(std::mem::size_of::<Self>())
            .and_then(|bytes| bytes.checked_mul(2))
            .ok_or(StorageDocumentError::DocumentTooLarge)?;
        if bytes > StorageDocumentStore::MEMORY_BUDGET {
            return Err(StorageDocumentError::DocumentTooLarge);
        }
        Ok(u32::try_from(bytes)?)
    }

    pub fn mutation_entity(&self) -> Option<DesktopMessagesEntityId> {
        match &self.action {
            DesktopLibraryServiceEnumAction::Open { game, .. } => {
                Some(DesktopMessagesEntityId::for_game(*game))
            }
            DesktopLibraryServiceEnumAction::Editor { expected, action } => match action {
                DesktopAnnotationCommandsEnumAction::Snapshot
                | DesktopAnnotationCommandsEnumAction::TreePage { .. }
                | DesktopAnnotationCommandsEnumAction::ChildrenPage { .. }
                | DesktopAnnotationCommandsEnumAction::VariationScope { .. } => None,
                DesktopAnnotationCommandsEnumAction::Navigate { .. }
                | DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { .. }
                | DesktopAnnotationCommandsEnumAction::SaveMarkings { .. }
                | DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
                | DesktopAnnotationCommandsEnumAction::PreviewVariation(_)
                | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. }
                | DesktopAnnotationCommandsEnumAction::Checkpoint
                | DesktopAnnotationCommandsEnumAction::RetrySave => {
                    Some(DesktopMessagesEntityId::for_game(expected.game()))
                }
            },
            DesktopLibraryServiceEnumAction::Query { .. }
            | DesktopLibraryServiceEnumAction::Organize(_)
            | DesktopLibraryServiceEnumAction::PreviewTrash(_)
            | DesktopLibraryServiceEnumAction::ApplyTrash(_)
            | DesktopLibraryServiceEnumAction::Restore(_)
            | DesktopLibraryServiceEnumAction::RemoveFolder(_)
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash { .. }
            | DesktopLibraryServiceEnumAction::Import(_)
            | DesktopLibraryServiceEnumAction::Export(_)
            | DesktopLibraryServiceEnumAction::Metadata(_) => None,
        }
    }

    pub fn expected_revision(&self) -> Option<DesktopRevision> {
        match &self.action {
            DesktopLibraryServiceEnumAction::Open {
                storage_revision, ..
            } => Some(DesktopRevision::for_game_storage(*storage_revision)),
            DesktopLibraryServiceEnumAction::Editor { expected, .. } => {
                Some(Self::editor_revision(*expected))
            }
            DesktopLibraryServiceEnumAction::Query { .. }
            | DesktopLibraryServiceEnumAction::Organize(_)
            | DesktopLibraryServiceEnumAction::PreviewTrash(_)
            | DesktopLibraryServiceEnumAction::ApplyTrash(_)
            | DesktopLibraryServiceEnumAction::Restore(_)
            | DesktopLibraryServiceEnumAction::RemoveFolder(_)
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash { .. }
            | DesktopLibraryServiceEnumAction::Import(_)
            | DesktopLibraryServiceEnumAction::Export(_)
            | DesktopLibraryServiceEnumAction::Metadata(_) => None,
        }
    }

    pub const fn editor_revision(context: DesktopGameEditorContext) -> DesktopRevision {
        let revision = match context.source() {
            DesktopGameEditorEnumSource::Played { revision }
            | DesktopGameEditorEnumSource::Notation { revision } => revision,
        };
        DesktopRevision::for_game(revision)
    }

    pub fn file_selection(&self) -> Option<(&DesktopLibraryServiceFile, DesktopFileEnumPurpose)> {
        match &self.action {
            DesktopLibraryServiceEnumAction::Import(DesktopLibraryServiceEnumImportAction::Prepare(
                file,
            )) => Some((file, DesktopFileEnumPurpose::ReadFile)),
            DesktopLibraryServiceEnumAction::Export(request) => {
                Some((request.destination(), DesktopFileEnumPurpose::WriteFile))
            }
            DesktopLibraryServiceEnumAction::Import(
                DesktopLibraryServiceEnumImportAction::Page(_)
                | DesktopLibraryServiceEnumImportAction::Detail(_)
                | DesktopLibraryServiceEnumImportAction::Choose { .. }
                | DesktopLibraryServiceEnumImportAction::Freeze
                | DesktopLibraryServiceEnumImportAction::Commit(_)
                | DesktopLibraryServiceEnumImportAction::Cancel,
            )
            | DesktopLibraryServiceEnumAction::Query { .. }
            | DesktopLibraryServiceEnumAction::Metadata(_)
            | DesktopLibraryServiceEnumAction::Open { .. }
            | DesktopLibraryServiceEnumAction::Editor { .. }
            | DesktopLibraryServiceEnumAction::Organize(_)
            | DesktopLibraryServiceEnumAction::PreviewTrash(_)
            | DesktopLibraryServiceEnumAction::ApplyTrash(_)
            | DesktopLibraryServiceEnumAction::Restore(_)
            | DesktopLibraryServiceEnumAction::RemoveFolder(_)
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash { .. } => None,
        }
    }

    pub fn is_available_while_closing(&self) -> bool {
        match &self.action {
            DesktopLibraryServiceEnumAction::Editor { action, .. } => matches!(
                action,
                DesktopAnnotationCommandsEnumAction::Snapshot
                    | DesktopAnnotationCommandsEnumAction::TreePage { .. }
                    | DesktopAnnotationCommandsEnumAction::ChildrenPage { .. }
                    | DesktopAnnotationCommandsEnumAction::VariationScope { .. }
                    | DesktopAnnotationCommandsEnumAction::Checkpoint
                    | DesktopAnnotationCommandsEnumAction::RetrySave
            ),
            DesktopLibraryServiceEnumAction::Query { .. }
            | DesktopLibraryServiceEnumAction::Metadata(_)
            | DesktopLibraryServiceEnumAction::Organize(_)
            | DesktopLibraryServiceEnumAction::PreviewTrash(_)
            | DesktopLibraryServiceEnumAction::ApplyTrash(_)
            | DesktopLibraryServiceEnumAction::Restore(_)
            | DesktopLibraryServiceEnumAction::RemoveFolder(_)
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash { .. }
            | DesktopLibraryServiceEnumAction::Import(_)
            | DesktopLibraryServiceEnumAction::Export(_)
            | DesktopLibraryServiceEnumAction::Open { .. } => false,
        }
    }

    pub(super) const fn game(&self) -> Option<GameId> {
        match &self.action {
            DesktopLibraryServiceEnumAction::Open { game, .. } => Some(*game),
            DesktopLibraryServiceEnumAction::Editor { expected, .. } => Some(expected.game()),
            DesktopLibraryServiceEnumAction::Query { .. }
            | DesktopLibraryServiceEnumAction::Organize(_)
            | DesktopLibraryServiceEnumAction::PreviewTrash(_)
            | DesktopLibraryServiceEnumAction::ApplyTrash(_)
            | DesktopLibraryServiceEnumAction::Restore(_)
            | DesktopLibraryServiceEnumAction::RemoveFolder(_)
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash { .. }
            | DesktopLibraryServiceEnumAction::Import(_)
            | DesktopLibraryServiceEnumAction::Export(_)
            | DesktopLibraryServiceEnumAction::Metadata(_) => None,
        }
    }
}
