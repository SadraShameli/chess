use std::mem::{size_of, size_of_val};
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::OwnedSemaphorePermit;

use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::desktop::dto::{DesktopBoardMove, DesktopBoardPiece, DesktopBoardSnapshot};
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorContext, DesktopGameEditorEnumInput,
    DesktopGameEditorError, DesktopGameEditorSnapshot, DesktopGameEditorSnapshotEnumState,
};
use crate::game::enums::GameMoveId;
use crate::game::move_node::GameMoveNodeAnnotations;
use crate::game::session::{GameMetadata, GameMetadataText};
use crate::storage::document_store::{StorageDocumentClient, StorageDocumentError};

#[derive(Debug, Error)]
pub enum DesktopGameEditorPublicationError {
    #[error(transparent)]
    Editor(#[from] DesktopGameEditorError),
    #[error(transparent)]
    Memory(#[from] StorageDocumentError),
    #[error("The game editor publication memory footprint exceeds its representable range")]
    FootprintOverflow,
}

#[derive(Debug)]
struct DesktopGameEditorPublicationBacking {
    snapshot: DesktopGameEditorSnapshot,
    _memory: OwnedSemaphorePermit,
}

#[derive(Debug, Clone)]
pub struct DesktopGameEditorPublication {
    backing: Arc<DesktopGameEditorPublicationBacking>,
}

impl DesktopGameEditorPublication {
    pub(super) fn capture(
        editor: &DesktopGameEditor<'_>,
        expected: DesktopGameEditorContext,
        client: &StorageDocumentClient,
    ) -> Result<Self, DesktopGameEditorPublicationError> {
        editor.context().check_expected(expected)?;
        let footprint = DesktopGameEditorPublicationFootprint::measure(editor)?;
        let memory = client.reserve_library_memory(footprint.bytes)?;
        let snapshot = editor.snapshot(expected)?;
        Ok(Self {
            backing: Arc::new(DesktopGameEditorPublicationBacking {
                snapshot,
                _memory: memory,
            }),
        })
    }

    pub fn context(&self) -> DesktopGameEditorContext {
        self.backing.snapshot.context()
    }

    pub fn root(&self) -> GameMoveId {
        self.backing.snapshot.root()
    }

    pub fn mainline_tip(&self) -> GameMoveId {
        self.backing.snapshot.mainline_tip()
    }

    pub fn board(&self) -> &DesktopBoardSnapshot {
        self.backing.snapshot.board()
    }

    pub fn annotations(&self) -> &GameMoveNodeAnnotations {
        self.backing.snapshot.annotations()
    }

    pub fn metadata(&self) -> &GameMetadata {
        self.backing.snapshot.metadata()
    }

    pub fn state(&self) -> &DesktopGameEditorSnapshotEnumState {
        self.backing.snapshot.state()
    }
}

struct DesktopGameEditorPublicationFootprint {
    bytes: usize,
}

impl DesktopGameEditorPublicationFootprint {
    fn measure(editor: &DesktopGameEditor<'_>) -> Result<Self, DesktopGameEditorPublicationError> {
        let mut footprint = Self {
            bytes: size_of::<DesktopGameEditorPublicationBacking>() + 2 * size_of::<usize>(),
        };
        let selected = editor
            .tree()
            .node(editor.context().selected())
            .map_err(DesktopGameEditorError::from)?;
        let annotations = selected.annotations();
        footprint.add(annotations.notes().len())?;
        footprint.add(size_of_val(annotations.nags()))?;
        footprint.add(size_of_val(annotations.markings()))?;
        footprint.metadata(editor.input.metadata())?;
        match editor.input {
            DesktopGameEditorEnumInput::Played(_) => {}
            DesktopGameEditorEnumInput::Notation(game) => {
                footprint.text(game.time_control())?;
            }
        }
        footprint.add(2 * BoardPosition::MAX_FEN_BYTES)?;
        footprint.add(2 * shakmaty::Square::ALL.len() * size_of::<DesktopBoardPiece>())?;
        footprint.add(
            2 * shakmaty::MoveList::new().capacity()
                * (size_of::<BoardMove>() + size_of::<DesktopBoardMove>()),
        )?;
        Ok(footprint)
    }

    fn metadata(
        &mut self,
        metadata: &GameMetadata,
    ) -> Result<(), DesktopGameEditorPublicationError> {
        let known = metadata.known();
        for value in [
            known.event.as_ref(),
            known.site.as_ref(),
            known.round.as_ref(),
            known.white.as_ref(),
            known.black.as_ref(),
            known.opening.as_ref(),
            known.variation.as_ref(),
            known.annotator.as_ref(),
        ] {
            self.text(value)?;
        }
        if let Some(eco) = &known.eco {
            self.add(eco.as_str().len())?;
        }
        self.add(size_of_val(metadata.extensions()))?;
        for extension in metadata.extensions() {
            self.add(extension.name().len())?;
            self.text(Some(extension.value()))?;
        }
        Ok(())
    }

    fn text(
        &mut self,
        value: Option<&GameMetadataText>,
    ) -> Result<(), DesktopGameEditorPublicationError> {
        self.add(value.map_or(0, |text| text.as_str().len()))
    }

    fn add(&mut self, bytes: usize) -> Result<(), DesktopGameEditorPublicationError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(DesktopGameEditorPublicationError::FootprintOverflow)?;
        Ok(())
    }
}
