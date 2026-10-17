use crate::board::editor::{BoardEditorDraftId, BoardEditorSessionEnumOperation};
use crate::board::enums::BoardEnumVariant;
use crate::board::position::BoardPosition;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsContext, DesktopEditorCommandsError, DesktopEditorCommandsOpening,
};
use crate::storage::editor_repository::{
    StorageEditorRepositoryCursor, StorageEditorRepositoryRevision,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopEditorCommandsEnumChoice {
    Save,
    Discard,
    Stay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopEditorCommandsEnumPrompt {
    Create,
    ImportFen,
    Read,
    Leave,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopEditorCommandsEnumOperation {
    Create {
        opening: DesktopEditorCommandsOpening,
        selected: Option<BoardPosition>,
        expected: Option<DesktopEditorCommandsContext>,
    },
    ImportFen {
        opening: DesktopEditorCommandsOpening,
        fen: String,
        variant: BoardEnumVariant,
        expected: Option<DesktopEditorCommandsContext>,
    },
    Read {
        opening: DesktopEditorCommandsOpening,
        expected: Option<DesktopEditorCommandsContext>,
    },
    Leave {
        expected: DesktopEditorCommandsContext,
    },
    Resolve {
        expected: DesktopEditorCommandsContext,
        choice: DesktopEditorCommandsEnumChoice,
    },
    Apply {
        expected: DesktopEditorCommandsContext,
        operation: BoardEditorSessionEnumOperation,
    },
    Validate {
        expected: DesktopEditorCommandsContext,
    },
    Save {
        expected: DesktopEditorCommandsContext,
    },
    ExportFen {
        expected: DesktopEditorCommandsContext,
    },
    Play {
        expected: DesktopEditorCommandsContext,
    },
    List {
        after: Option<StorageEditorRepositoryCursor>,
    },
    Delete {
        id: BoardEditorDraftId,
        storage_revision: StorageEditorRepositoryRevision,
        expected: Option<DesktopEditorCommandsContext>,
    },
    Snapshot,
}

impl DesktopEditorCommandsEnumOperation {
    pub fn validate_payload(&self) -> Result<(), DesktopEditorCommandsError> {
        match self {
            Self::ImportFen { fen, .. } => {
                if fen.capacity() > BoardPosition::MAX_FEN_BYTES {
                    return Err(DesktopEditorCommandsError::FenPayloadTooLarge {
                        bytes: fen.capacity(),
                        maximum: BoardPosition::MAX_FEN_BYTES,
                    });
                }
                Ok(())
            }
            Self::Create { .. }
            | Self::Read { .. }
            | Self::Leave { .. }
            | Self::Resolve { .. }
            | Self::Apply { .. }
            | Self::Validate { .. }
            | Self::Save { .. }
            | Self::ExportFen { .. }
            | Self::Play { .. }
            | Self::List { .. }
            | Self::Delete { .. }
            | Self::Snapshot => Ok(()),
        }
    }

    pub const fn expected(&self) -> Option<DesktopEditorCommandsContext> {
        match self {
            Self::Create { expected, .. }
            | Self::ImportFen { expected, .. }
            | Self::Read { expected, .. }
            | Self::Delete { expected, .. } => *expected,
            Self::Leave { expected }
            | Self::Resolve { expected, .. }
            | Self::Apply { expected, .. }
            | Self::Validate { expected }
            | Self::Save { expected }
            | Self::ExportFen { expected }
            | Self::Play { expected } => Some(*expected),
            Self::List { .. } | Self::Snapshot => None,
        }
    }

    pub const fn is_mutation(&self) -> bool {
        match self {
            Self::Create { .. }
            | Self::ImportFen { .. }
            | Self::Read { .. }
            | Self::Leave { .. }
            | Self::Resolve { .. }
            | Self::Apply { .. }
            | Self::Save { .. }
            | Self::Delete { .. } => true,
            Self::Validate { .. }
            | Self::ExportFen { .. }
            | Self::Play { .. }
            | Self::List { .. }
            | Self::Snapshot => false,
        }
    }
}
