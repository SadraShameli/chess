use thiserror::Error;

use crate::board::r#move::BoardMove;
use crate::board::start::BoardStart;
use crate::game::enums::{GameEnumMode, GameId};
use crate::game::session::{GameMetadata, GameSession, GameSessionError};
use crate::game::tree::GameTreeError;
use crate::notation::game::NotationGame;
use crate::notation::game_identity::{
    NotationGameIdentity, NotationGameIdentityEnumResult, NotationGameIdentityError,
};

pub enum StorageLibraryEnumContent {
    Played(Box<GameSession>),
    Notation(Box<NotationGame>),
}

#[derive(Debug, Error)]
pub enum StorageLibraryRecordError {
    #[error("Library game {id:?} differs from played session {session:?}")]
    Identity { id: GameId, session: GameId },
    #[error(transparent)]
    Session(#[from] GameSessionError),
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    NotationIdentity(#[from] NotationGameIdentityError),
}

pub struct StorageLibraryRecord {
    id: GameId,
    content: StorageLibraryEnumContent,
}

impl StorageLibraryRecord {
    pub fn new(
        id: GameId,
        content: StorageLibraryEnumContent,
    ) -> Result<Self, StorageLibraryRecordError> {
        if let StorageLibraryEnumContent::Played(session) = &content
            && session.id() != id
        {
            return Err(StorageLibraryRecordError::Identity {
                id,
                session: session.id(),
            });
        }
        Ok(Self { id, content })
    }

    pub const fn id(&self) -> GameId {
        self.id
    }

    pub fn content(&self) -> &StorageLibraryEnumContent {
        &self.content
    }

    pub fn notation_mut(&mut self) -> Option<&mut NotationGame> {
        match &mut self.content {
            StorageLibraryEnumContent::Notation(game) => Some(game),
            StorageLibraryEnumContent::Played(_) => None,
        }
    }

    pub fn into_content(self) -> StorageLibraryEnumContent {
        self.content
    }

    pub fn metadata(&self) -> &GameMetadata {
        match &self.content {
            StorageLibraryEnumContent::Played(session) => session.metadata(),
            StorageLibraryEnumContent::Notation(game) => game.metadata(),
        }
    }

    pub fn start(&self) -> &BoardStart {
        match &self.content {
            StorageLibraryEnumContent::Played(session) => session.start_position(),
            StorageLibraryEnumContent::Notation(game) => game.start(),
        }
    }

    pub fn result(&self) -> NotationGameIdentityEnumResult {
        match &self.content {
            StorageLibraryEnumContent::Played(session) => session
                .outcome()
                .map_or(NotationGameIdentityEnumResult::Unfinished, |outcome| {
                    NotationGameIdentityEnumResult::Completed(outcome.result())
                }),
            StorageLibraryEnumContent::Notation(game) => game.result(),
        }
    }

    pub fn mode(&self) -> Option<GameEnumMode> {
        match &self.content {
            StorageLibraryEnumContent::Played(session) => Some(session.seats().mode()),
            StorageLibraryEnumContent::Notation(_) => None,
        }
    }

    fn identity_line(&self) -> Result<Vec<BoardMove>, StorageLibraryRecordError> {
        match &self.content {
            StorageLibraryEnumContent::Played(session) => Ok(session.live_moves()?),
            StorageLibraryEnumContent::Notation(game) => game
                .tree()
                .path_to(game.mainline_tip())?
                .into_iter()
                .filter_map(|id| game.tree().node(id).map(|node| node.movement()).transpose())
                .collect::<Result<_, _>>()
                .map_err(Into::into),
        }
    }

    pub fn identity(&self) -> Result<NotationGameIdentity, StorageLibraryRecordError> {
        Ok(NotationGameIdentity::new(
            self.start().position(),
            &self.identity_line()?,
            self.metadata(),
            Some(self.result()),
        )?)
    }

    pub fn identity_line_len(&self) -> Result<usize, StorageLibraryRecordError> {
        Ok(match &self.content {
            StorageLibraryEnumContent::Played(session) => {
                session.tree().line_len(session.live())?
            }
            StorageLibraryEnumContent::Notation(game) => {
                game.tree().line_len(game.mainline_tip())?
            }
        })
    }
}
