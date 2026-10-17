mod context;
mod snapshot;
mod tree;

use thiserror::Error;

pub use crate::desktop::game_editor::context::{
    DesktopGameEditorBinding, DesktopGameEditorContext, DesktopGameEditorEnumSource,
    DesktopGameEditorEpoch, DesktopGameEditorGeneration,
};
pub use crate::desktop::game_editor::snapshot::{
    DesktopGameEditorSnapshot, DesktopGameEditorSnapshotEnumState,
};
pub use crate::desktop::game_editor::tree::{
    DesktopGameEditorTreeChildrenCursor, DesktopGameEditorTreeChildrenPage,
    DesktopGameEditorTreeCursor, DesktopGameEditorTreeNode, DesktopGameEditorTreePage,
};
use crate::game::enums::{GameId, GameMoveId, GameRevision};
use crate::game::session::{GameMetadata, GameSession};
use crate::game::tree::{GameTree, GameTreeError};
use crate::notation::game::NotationGame;
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};

#[derive(Debug, Error)]
pub enum DesktopGameEditorError {
    #[error("The game editor context changed")]
    StaleContext,
    #[error("The game editor generation counter is exhausted")]
    GenerationExhausted,
    #[error("The game editor requires a notation library record")]
    ExpectedNotation,
    #[error("The game editor cursor does not identify its canonical tree position")]
    InvalidCursor,
    #[error("The game editor page position exceeds its representable range")]
    PagePositionOverflow,
    #[error(transparent)]
    Tree(#[from] GameTreeError),
}

#[derive(Clone, Copy)]
enum DesktopGameEditorEnumInput<'a> {
    Played(&'a GameSession),
    Notation(&'a NotationGame),
}

impl<'a> DesktopGameEditorEnumInput<'a> {
    fn tree(self) -> &'a GameTree {
        match self {
            Self::Played(session) => session.tree(),
            Self::Notation(game) => game.tree(),
        }
    }

    fn metadata(self) -> &'a GameMetadata {
        match self {
            Self::Played(session) => session.metadata(),
            Self::Notation(game) => game.metadata(),
        }
    }
}

pub struct DesktopGameEditor<'a> {
    input: DesktopGameEditorEnumInput<'a>,
    context: DesktopGameEditorContext,
}

impl<'a> DesktopGameEditor<'a> {
    pub fn played(
        session: &'a GameSession,
        binding: DesktopGameEditorBinding,
    ) -> Result<Self, DesktopGameEditorError> {
        Self::new(
            DesktopGameEditorEnumInput::Played(session),
            session.id(),
            DesktopGameEditorEnumSource::Played {
                revision: session.revision(),
            },
            session.view(),
            binding,
        )
    }

    pub fn notation(
        record: &'a StorageLibraryRecord,
        revision: GameRevision,
        selected: GameMoveId,
        binding: DesktopGameEditorBinding,
    ) -> Result<Self, DesktopGameEditorError> {
        let game = match record.content() {
            StorageLibraryEnumContent::Notation(game) => game,
            StorageLibraryEnumContent::Played(_) => {
                return Err(DesktopGameEditorError::ExpectedNotation);
            }
        };
        Self::new(
            DesktopGameEditorEnumInput::Notation(game),
            record.id(),
            DesktopGameEditorEnumSource::Notation { revision },
            selected,
            binding,
        )
    }

    fn new(
        input: DesktopGameEditorEnumInput<'a>,
        game: GameId,
        source: DesktopGameEditorEnumSource,
        selected: GameMoveId,
        binding: DesktopGameEditorBinding,
    ) -> Result<Self, DesktopGameEditorError> {
        input.tree().node(selected)?;
        Ok(Self {
            input,
            context: DesktopGameEditorContext::new(game, source, selected, binding),
        })
    }

    pub const fn context(&self) -> DesktopGameEditorContext {
        self.context
    }

    pub fn snapshot(
        &self,
        expected: DesktopGameEditorContext,
    ) -> Result<DesktopGameEditorSnapshot, DesktopGameEditorError> {
        DesktopGameEditorSnapshot::capture(self, expected)
    }

    pub fn tree_page(
        &self,
        expected: DesktopGameEditorContext,
        cursor: Option<DesktopGameEditorTreeCursor>,
    ) -> Result<DesktopGameEditorTreePage, DesktopGameEditorError> {
        DesktopGameEditorTreePage::capture(self, expected, cursor)
    }

    pub fn children_page(
        &self,
        expected: DesktopGameEditorContext,
        parent: GameMoveId,
        cursor: Option<DesktopGameEditorTreeChildrenCursor>,
    ) -> Result<DesktopGameEditorTreeChildrenPage, DesktopGameEditorError> {
        DesktopGameEditorTreeChildrenPage::capture(self, expected, parent, cursor)
    }

    fn tree(&self) -> &GameTree {
        self.input.tree()
    }
}

#[cfg(test)]
#[path = "../../tests/desktop/game_editor.rs"]
mod tests;
