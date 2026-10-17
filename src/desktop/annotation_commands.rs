use crate::board::r#move::BoardMove;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_editor::{
    DesktopGameEditorContext, DesktopGameEditorPublication, DesktopGameEditorTreeChildrenCursor,
    DesktopGameEditorTreeChildrenPage, DesktopGameEditorTreeCursor, DesktopGameEditorTreePage,
};
use crate::desktop::identity::DesktopRequestId;
use crate::game::annotations::{GameAnnotationsEnumChange, GameAnnotationsEnumVariationRequest};
use crate::game::enums::GameMoveId;
use crate::game::move_node::{GameMoveNodeMarking, GameMoveNodeNag};
use crate::game::storage_repository::GameStorageRevision;

#[derive(Debug, Clone)]
pub enum DesktopAnnotationCommandsEnumAction {
    Snapshot,
    TreePage {
        cursor: Option<DesktopGameEditorTreeCursor>,
    },
    ChildrenPage {
        parent: GameMoveId,
        cursor: Option<DesktopGameEditorTreeChildrenCursor>,
    },
    Navigate {
        selected: GameMoveId,
    },
    SaveNotesAndNags {
        notes: String,
        nags: Vec<GameMoveNodeNag>,
    },
    SaveMarkings {
        markings: Vec<GameMoveNodeMarking>,
    },
    AppendStudyBranch {
        node: GameMoveId,
        movement: BoardMove,
    },
    PreviewVariation(GameAnnotationsEnumVariationRequest),
    VariationScope {
        token: DesktopRequestId,
        scope: DesktopAnnotationCommandsEnumScope,
        offset: usize,
    },
    ConfirmVariation {
        token: DesktopRequestId,
    },
    Checkpoint,
    RetrySave,
}

impl DesktopAnnotationCommandsEnumAction {
    pub(crate) const fn edits_content(&self) -> bool {
        match self {
            Self::SaveNotesAndNags { .. }
            | Self::SaveMarkings { .. }
            | Self::AppendStudyBranch { .. }
            | Self::ConfirmVariation { .. } => true,
            Self::Snapshot
            | Self::TreePage { .. }
            | Self::ChildrenPage { .. }
            | Self::Navigate { .. }
            | Self::PreviewVariation(_)
            | Self::VariationScope { .. }
            | Self::Checkpoint
            | Self::RetrySave => false,
        }
    }
}

pub struct DesktopAnnotationCommands {
    receipt: DesktopApplicationReceipt,
    expected: DesktopGameEditorContext,
    action: DesktopAnnotationCommandsEnumAction,
}

impl DesktopAnnotationCommands {
    pub const fn new(
        receipt: DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
    ) -> Self {
        Self {
            receipt,
            expected,
            action,
        }
    }

    pub const fn receipt(&self) -> &DesktopApplicationReceipt {
        &self.receipt
    }

    pub const fn expected(&self) -> DesktopGameEditorContext {
        self.expected
    }

    pub fn into_parts(
        self,
    ) -> (
        DesktopApplicationReceipt,
        DesktopGameEditorContext,
        DesktopAnnotationCommandsEnumAction,
    ) {
        (self.receipt, self.expected, self.action)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAnnotationCommandsEnumScope {
    PreviousPath,
    SelectedPath,
    DeletedNodes,
}

#[derive(Debug, Clone)]
pub struct DesktopAnnotationCommandsVariationPreview {
    context: DesktopGameEditorContext,
    token: DesktopRequestId,
    request: GameAnnotationsEnumVariationRequest,
    previous_tip: GameMoveId,
    resulting_tip: GameMoveId,
    parent: Option<GameMoveId>,
    previous_path_count: usize,
    selected_path_count: usize,
    deleted_count: usize,
}

impl DesktopAnnotationCommandsVariationPreview {
    pub(crate) fn capture(
        context: DesktopGameEditorContext,
        token: DesktopRequestId,
        proof: &crate::game::annotations::GameAnnotationsVariationPreview,
    ) -> Self {
        Self {
            context,
            token,
            request: proof.request(),
            previous_tip: proof.previous_tip(),
            resulting_tip: proof.resulting_tip(),
            parent: proof.parent(),
            previous_path_count: proof.previous_path().len(),
            selected_path_count: proof.selected_path().map_or(0, <[GameMoveId]>::len),
            deleted_count: proof.deleted_nodes().len(),
        }
    }

    pub const fn context(&self) -> DesktopGameEditorContext {
        self.context
    }

    pub const fn token(&self) -> DesktopRequestId {
        self.token
    }

    pub const fn request(&self) -> GameAnnotationsEnumVariationRequest {
        self.request
    }

    pub const fn previous_tip(&self) -> GameMoveId {
        self.previous_tip
    }

    pub const fn resulting_tip(&self) -> GameMoveId {
        self.resulting_tip
    }

    pub const fn parent(&self) -> Option<GameMoveId> {
        self.parent
    }

    pub const fn previous_path_count(&self) -> usize {
        self.previous_path_count
    }

    pub const fn selected_path_count(&self) -> usize {
        self.selected_path_count
    }

    pub const fn deleted_count(&self) -> usize {
        self.deleted_count
    }
}

#[derive(Debug, Clone)]
pub struct DesktopAnnotationCommandsScopePage {
    context: DesktopGameEditorContext,
    token: DesktopRequestId,
    scope: DesktopAnnotationCommandsEnumScope,
    nodes: Vec<GameMoveId>,
    total: usize,
    next: Option<usize>,
}

impl DesktopAnnotationCommandsScopePage {
    pub const MAX_ROWS: usize = DesktopGameEditorTreePage::MAX_ROWS;

    pub(crate) fn capture(
        context: DesktopGameEditorContext,
        token: DesktopRequestId,
        scope: DesktopAnnotationCommandsEnumScope,
        nodes: &[GameMoveId],
        offset: usize,
    ) -> Option<Self> {
        let remaining = nodes.get(offset..)?;
        let end = offset.checked_add(remaining.len().min(Self::MAX_ROWS))?;
        Some(Self {
            context,
            token,
            scope,
            nodes: nodes.get(offset..end)?.to_vec(),
            total: nodes.len(),
            next: (end < nodes.len()).then_some(end),
        })
    }

    pub const fn context(&self) -> DesktopGameEditorContext {
        self.context
    }

    pub const fn token(&self) -> DesktopRequestId {
        self.token
    }

    pub const fn scope(&self) -> DesktopAnnotationCommandsEnumScope {
        self.scope
    }

    pub fn nodes(&self) -> &[GameMoveId] {
        &self.nodes
    }

    pub const fn total(&self) -> usize {
        self.total
    }

    pub const fn next(&self) -> Option<usize> {
        self.next
    }
}

#[derive(Debug, Clone)]
pub enum DesktopAnnotationCommandsEnumResult {
    Snapshot(DesktopGameEditorPublication),
    Tree(DesktopGameEditorTreePage),
    Children(DesktopGameEditorTreeChildrenPage),
    Edited {
        change: GameAnnotationsEnumChange,
        snapshot: DesktopGameEditorPublication,
    },
    Variation(DesktopAnnotationCommandsVariationPreview),
    Scope(DesktopAnnotationCommandsScopePage),
    PendingProjection {
        result: Box<Self>,
        storage_revision: GameStorageRevision,
        covered: crate::game::enums::GameRevision,
    },
}

#[derive(Debug, Clone)]
pub enum DesktopAnnotationCommandsEnumAdmission {
    Ready(Box<DesktopAnnotationCommandsEnumResult>),
    PendingSnapshot {
        snapshot: DesktopGameEditorPublication,
        pending: DesktopAnnotationCommandsEnumPending,
    },
    ComputePending,
    SavePending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAnnotationCommandsEnumPending {
    Computing,
    Saving {
        computing: bool,
    },
    Publishing {
        context: DesktopGameEditorContext,
    },
    Preparing {
        computing: bool,
    },
    Committing {
        computing: bool,
    },
    Projecting {
        storage_revision: GameStorageRevision,
        covered: crate::game::enums::GameRevision,
    },
    Unresolved,
}

#[cfg(test)]
#[path = "../../tests/desktop/annotation_commands.rs"]
mod tests;
