use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumPending, DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::game_editor::{DesktopGameEditorContext, DesktopGameEditorPublication};
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::library_service::DesktopLibraryServiceOperation;
use crate::desktop::library_service::import::DesktopLibraryServiceEnumImportResult;
use crate::game::enums::GameRevision;
use crate::game::library_organization::GameLibraryOrganizationReceipt;
use crate::game::storage_repository::GameStorageReceipt;
use crate::storage::library_metadata::StorageLibraryMetadataEnumResponse;
use crate::storage::library_query::StorageLibraryQueryPage;
use crate::storage::library_repository::StorageLibraryContentReceipt;
use crate::storage::trash::{StorageTrashPlan, StorageTrashReceipt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopLibraryServiceEnumDisposition {
    Refused,
    Retained {
        request: DesktopRequestId,
        expected: Option<DesktopGameEditorContext>,
    },
}

#[derive(Debug, Clone)]
pub enum DesktopLibraryServiceEnumSaveReceipt {
    Played(GameStorageReceipt),
    Notation {
        receipt: Arc<StorageLibraryContentReceipt>,
        covered: GameRevision,
    },
}

#[derive(Clone)]
pub enum DesktopLibraryServiceEnumResult {
    Query(Arc<StorageLibraryQueryPage>),
    Metadata(StorageLibraryMetadataEnumResponse),
    Opened(DesktopGameEditorPublication),
    Organized(GameLibraryOrganizationReceipt),
    Import(DesktopLibraryServiceEnumImportResult),
    Exported(u64),
    TrashPlan(StorageTrashPlan),
    Trashed(StorageTrashReceipt),
    Restored(StorageTrashReceipt),
    Editor(DesktopAnnotationCommandsEnumResult),
    PendingSnapshot {
        snapshot: DesktopGameEditorPublication,
        pending: DesktopAnnotationCommandsEnumPending,
    },
    Saved {
        snapshot: DesktopGameEditorPublication,
        receipt: DesktopLibraryServiceEnumSaveReceipt,
    },
}

impl DesktopLibraryServiceEnumResult {
    pub fn editor_context(&self) -> Option<DesktopGameEditorContext> {
        match self {
            Self::Opened(snapshot)
            | Self::PendingSnapshot { snapshot, .. }
            | Self::Saved { snapshot, .. } => Some(snapshot.context()),
            Self::Editor(result) => Some(Self::result_context(result)),
            Self::Query(_)
            | Self::Metadata(_)
            | Self::Organized(_)
            | Self::TrashPlan(_)
            | Self::Trashed(_)
            | Self::Restored(_)
            | Self::Import(_)
            | Self::Exported(_) => None,
        }
    }

    fn result_context(result: &DesktopAnnotationCommandsEnumResult) -> DesktopGameEditorContext {
        match result {
            DesktopAnnotationCommandsEnumResult::Snapshot(snapshot)
            | DesktopAnnotationCommandsEnumResult::Edited { snapshot, .. } => snapshot.context(),
            DesktopAnnotationCommandsEnumResult::Tree(page) => page.context(),
            DesktopAnnotationCommandsEnumResult::Children(page) => page.context(),
            DesktopAnnotationCommandsEnumResult::Variation(preview) => preview.context(),
            DesktopAnnotationCommandsEnumResult::Scope(page) => page.context(),
            DesktopAnnotationCommandsEnumResult::PendingProjection { result, .. } => {
                Self::result_context(result)
            }
        }
    }

    pub fn revision(&self) -> Option<DesktopRevision> {
        if let Some(context) = self.editor_context() {
            return Some(DesktopLibraryServiceOperation::editor_revision(context));
        }
        if let Self::Organized(receipt) = self {
            return Some(DesktopRevision::for_library_organization(
                receipt.revision(),
            ));
        }
        let catalog = match self {
            Self::Query(page) => page.catalog_revision(),
            Self::TrashPlan(plan) => plan.catalog_revision(),
            Self::Trashed(receipt) | Self::Restored(receipt) => receipt.catalog_revision(),
            Self::Metadata(response) => match response {
                StorageLibraryMetadataEnumResponse::Folders(page) => page.catalog_revision(),
                StorageLibraryMetadataEnumResponse::Tags(page) => page.catalog_revision(),
                StorageLibraryMetadataEnumResponse::Folder(read) => read.catalog_revision(),
            },
            Self::Opened(_)
            | Self::Organized(_)
            | Self::Import(_)
            | Self::Exported(_)
            | Self::Editor(_)
            | Self::PendingSnapshot { .. }
            | Self::Saved { .. } => return None,
        };
        Some(DesktopRevision::for_library_catalog(catalog))
    }
}

impl Debug for DesktopLibraryServiceEnumResult {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Query(page) => formatter
                .debug_struct("Query")
                .field("rows", &page.entries().len())
                .field("catalog", &page.catalog_revision())
                .field("continued", &page.next_cursor().is_some())
                .finish(),
            Self::Metadata(response) => match response {
                StorageLibraryMetadataEnumResponse::Folders(page) => formatter
                    .debug_struct("Folders")
                    .field("rows", &page.entries().len())
                    .field("catalog", &page.catalog_revision())
                    .finish(),
                StorageLibraryMetadataEnumResponse::Tags(page) => formatter
                    .debug_struct("Tags")
                    .field("rows", &page.entries().len())
                    .field("catalog", &page.catalog_revision())
                    .finish(),
                StorageLibraryMetadataEnumResponse::Folder(read) => formatter
                    .debug_struct("Folder")
                    .field("id", &read.folder().id())
                    .field("catalog", &read.catalog_revision())
                    .finish(),
            },
            Self::Opened(snapshot) => formatter
                .debug_tuple("Opened")
                .field(&snapshot.context())
                .finish(),
            Self::Import(result) => formatter.debug_tuple("Import").field(result).finish(),
            Self::Exported(bytes) => formatter.debug_tuple("Exported").field(bytes).finish(),
            Self::TrashPlan(plan) => formatter.debug_tuple("TrashPlan").field(plan).finish(),
            Self::Trashed(receipt) => formatter.debug_tuple("Trashed").field(receipt).finish(),
            Self::Restored(receipt) => formatter.debug_tuple("Restored").field(receipt).finish(),
            Self::Organized(receipt) => formatter.debug_tuple("Organized").field(receipt).finish(),
            Self::Editor(result) => formatter
                .debug_struct("Editor")
                .field("context", &Self::result_context(result))
                .field("result", &std::mem::discriminant(result))
                .finish(),
            Self::PendingSnapshot { snapshot, pending } => formatter
                .debug_struct("PendingSnapshot")
                .field("context", &snapshot.context())
                .field("pending", pending)
                .finish(),
            Self::Saved { snapshot, receipt } => formatter
                .debug_struct("Saved")
                .field("context", &snapshot.context())
                .field("receipt", receipt)
                .finish(),
        }
    }
}
