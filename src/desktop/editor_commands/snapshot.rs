use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use crate::board::editor::{
    BoardEditorDraft, BoardEditorSessionEnumSelection, BoardEditorValidationEnumFinding,
    BoardEditorValidationEnumStatus,
};
use crate::board::start::BoardStart;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsContext, DesktopEditorCommandsEnumPrompt, DesktopEditorCommandsStudy,
};
use crate::storage::editor_repository::{
    StorageEditorRepositoryPage, StorageEditorRepositoryReceipt, StorageEditorRepositoryRevision,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopEditorCommandsEnumWriteStatus {
    Idle,
    Saving {
        context: DesktopEditorCommandsContext,
    },
    Deleting,
    Unresolved {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct DesktopEditorCommandsSnapshot {
    pub(super) context: DesktopEditorCommandsContext,
    pub(super) draft: BoardEditorDraft,
    pub(super) storage_revision: StorageEditorRepositoryRevision,
    pub(super) is_dirty: bool,
    pub(super) selection: BoardEditorSessionEnumSelection,
    pub(super) has_undo: bool,
    pub(super) has_redo: bool,
    pub(super) findings: Vec<BoardEditorValidationEnumFinding>,
    pub(super) status: BoardEditorValidationEnumStatus,
    pub(super) prompt: Option<DesktopEditorCommandsEnumPrompt>,
    pub(super) write: DesktopEditorCommandsEnumWriteStatus,
}

impl DesktopEditorCommandsSnapshot {
    pub const fn context(&self) -> DesktopEditorCommandsContext {
        self.context
    }
    pub const fn draft(&self) -> &BoardEditorDraft {
        &self.draft
    }
    pub const fn storage_revision(&self) -> StorageEditorRepositoryRevision {
        self.storage_revision
    }
    pub const fn is_dirty(&self) -> bool {
        self.is_dirty
    }
    pub const fn selection(&self) -> BoardEditorSessionEnumSelection {
        self.selection
    }
    pub const fn has_undo(&self) -> bool {
        self.has_undo
    }
    pub const fn has_redo(&self) -> bool {
        self.has_redo
    }
    pub fn findings(&self) -> &[BoardEditorValidationEnumFinding] {
        &self.findings
    }
    pub const fn prompt(&self) -> Option<DesktopEditorCommandsEnumPrompt> {
        self.prompt
    }
    pub const fn write(&self) -> &DesktopEditorCommandsEnumWriteStatus {
        &self.write
    }
    pub const fn status(&self) -> BoardEditorValidationEnumStatus {
        self.status
    }
}

#[derive(Clone)]
pub struct DesktopEditorCommandsPage(Arc<StorageEditorRepositoryPage>);

impl DesktopEditorCommandsPage {
    pub(super) fn new(page: StorageEditorRepositoryPage) -> Self {
        Self(Arc::new(page))
    }
    pub fn page(&self) -> &StorageEditorRepositoryPage {
        &self.0
    }
}

impl Debug for DesktopEditorCommandsPage {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DesktopEditorCommandsPage")
            .field("entries", &self.page().entries())
            .field("next_cursor", &self.page().next_cursor())
            .finish()
    }
}

#[derive(Debug, Clone)]
pub enum DesktopEditorCommandsEnumResult {
    Snapshot(Option<DesktopEditorCommandsSnapshot>),
    WritePending(Option<DesktopEditorCommandsSnapshot>),
    Written {
        receipt: StorageEditorRepositoryReceipt,
        snapshot: Option<DesktopEditorCommandsSnapshot>,
    },
    Fen {
        context: DesktopEditorCommandsContext,
        fen: String,
    },
    Play {
        context: DesktopEditorCommandsContext,
        start: BoardStart,
    },
    Study {
        context: DesktopEditorCommandsContext,
        study: DesktopEditorCommandsStudy,
    },
    List(DesktopEditorCommandsPage),
}
