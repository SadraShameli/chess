use crate::board::editor::{BoardEditorDraftId, BoardEditorDraftRevision};
use crate::desktop::identity::DesktopRequestId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopEditorCommandsOpening {
    draft: BoardEditorDraftId,
    session: DesktopRequestId,
}

impl DesktopEditorCommandsOpening {
    pub const fn new(draft: BoardEditorDraftId, session: DesktopRequestId) -> Self {
        Self { draft, session }
    }

    pub const fn draft(self) -> BoardEditorDraftId {
        self.draft
    }

    pub const fn session(self) -> DesktopRequestId {
        self.session
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopEditorCommandsContext {
    opening: DesktopEditorCommandsOpening,
    revision: BoardEditorDraftRevision,
}

impl DesktopEditorCommandsContext {
    pub(super) const fn new(
        opening: DesktopEditorCommandsOpening,
        revision: BoardEditorDraftRevision,
    ) -> Self {
        Self { opening, revision }
    }

    pub const fn draft(self) -> BoardEditorDraftId {
        self.opening.draft
    }

    pub const fn session(self) -> DesktopRequestId {
        self.opening.session
    }

    pub const fn revision(self) -> BoardEditorDraftRevision {
        self.revision
    }
}
