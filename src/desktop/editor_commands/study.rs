use std::sync::Arc;

use thiserror::Error;

use crate::board::start::BoardStart;
use crate::capacity::executor::CapacityExecutor;
use crate::game::enums::GameIdentityError;
use crate::game::enums::{GameId, GameMoveId};
use crate::game::session::{GameMetadata, GameMetadataError, GameMetadataKnownTags};
use crate::game::storage_repository::{
    GameStorageRevision, GameStorageWallTimeError, GameStorageWallTimeProvider,
};
use crate::game::tree::GameTree;
use crate::notation::game::{NotationGame, NotationGameError, NotationGameParts};
use crate::notation::game_identity::NotationGameIdentityEnumResult;
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::library_notation::StorageLibraryNotationRepository;
use crate::storage::library_prepared::StorageLibraryEnumContentChange;
use crate::storage::library_record::{
    StorageLibraryEnumContent, StorageLibraryRecord, StorageLibraryRecordError,
};
use crate::storage::library_repository::{StorageLibraryEnumMutationReceipt, StorageLibraryError};

#[derive(Debug, Error)]
pub enum DesktopEditorCommandsStudyError {
    #[error("Opening a validated draft as a study is unavailable in this session")]
    Unavailable,
    #[error(transparent)]
    Identity(#[from] GameIdentityError),
    #[error(transparent)]
    Metadata(#[from] GameMetadataError),
    #[error(transparent)]
    Notation(#[from] NotationGameError),
    #[error(transparent)]
    Record(#[from] StorageLibraryRecordError),
    #[error(transparent)]
    WallTime(#[from] GameStorageWallTimeError),
    #[error(transparent)]
    Storage(#[from] StorageLibraryError),
    #[error("The study save returned an unexpected acknowledgement")]
    UnexpectedReceipt,
}

pub trait DesktopEditorCommandsStudyIdentities: Send + Sync {
    fn next_study(&self) -> Result<(GameId, GameMoveId), DesktopEditorCommandsStudyError>;
}

#[derive(Clone)]
pub struct DesktopEditorCommandsStudyResources {
    pub client: StorageDocumentClient,
    pub executor: Arc<CapacityExecutor>,
    pub wall_time: Arc<dyn GameStorageWallTimeProvider>,
    pub identities: Arc<dyn DesktopEditorCommandsStudyIdentities>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopEditorCommandsStudy {
    game: GameId,
    storage_revision: GameStorageRevision,
}

impl DesktopEditorCommandsStudy {
    pub const fn game(&self) -> GameId {
        self.game
    }

    pub const fn storage_revision(&self) -> GameStorageRevision {
        self.storage_revision
    }
}

impl DesktopEditorCommandsStudyResources {
    pub(super) async fn create(
        &self,
        start: BoardStart,
        before_save: impl FnOnce() -> Result<(), super::DesktopEditorCommandsError>,
    ) -> Result<DesktopEditorCommandsStudy, super::DesktopEditorCommandsError> {
        let (game, root) = self.identities.next_study()?;
        let tree = GameTree::new(root, start.position().clone(), None);
        let notation = NotationGame::new(NotationGameParts {
            start,
            tree,
            mainline_tip: root,
            metadata: GameMetadata::new(GameMetadataKnownTags::default(), Vec::new())
                .map_err(DesktopEditorCommandsStudyError::from)?,
            result: NotationGameIdentityEnumResult::Unfinished,
            result_header: None,
            time_control: None,
            clocks: Vec::new(),
            warnings: Vec::new(),
        })
        .map_err(DesktopEditorCommandsStudyError::from)?;
        let record = Arc::new(
            StorageLibraryRecord::new(
                game,
                StorageLibraryEnumContent::Notation(Box::new(notation)),
            )
            .map_err(DesktopEditorCommandsStudyError::from)?,
        );
        let timestamp = self
            .wall_time
            .now()
            .map_err(DesktopEditorCommandsStudyError::from)?;
        let repository =
            StorageLibraryNotationRepository::new(self.client.clone(), self.executor.as_ref());
        let prepared = repository
            .prepare(
                GameStorageRevision::INITIAL,
                record,
                StorageLibraryEnumContentChange::Changed(timestamp),
            )
            .await
            .map_err(DesktopEditorCommandsStudyError::from)?;
        before_save()?;
        let mut pending = repository
            .begin_save(prepared)
            .map_err(DesktopEditorCommandsStudyError::from)?;
        let StorageLibraryEnumMutationReceipt::Saved(receipt) = pending
            .wait()
            .await
            .map_err(DesktopEditorCommandsStudyError::from)?
        else {
            return Err(DesktopEditorCommandsStudyError::UnexpectedReceipt.into());
        };
        Ok(DesktopEditorCommandsStudy {
            game: receipt.id(),
            storage_revision: receipt.content_revision(),
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DesktopEditorCommandsStudyUuidIdentities;

impl DesktopEditorCommandsStudyIdentities for DesktopEditorCommandsStudyUuidIdentities {
    fn next_study(&self) -> Result<(GameId, GameMoveId), DesktopEditorCommandsStudyError> {
        Ok((
            uuid::Uuid::new_v4().to_string().parse()?,
            uuid::Uuid::new_v4().to_string().parse()?,
        ))
    }
}
