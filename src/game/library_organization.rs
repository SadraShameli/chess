use std::fmt::{Display, Formatter};
use std::str::FromStr;

use thiserror::Error;

use crate::game::enums::{GameId, GameIdentityError};

#[derive(Debug, Error)]
pub enum GameLibraryOrganizationError {
    #[error(transparent)]
    Identity(#[from] GameIdentityError),
    #[error("Library revisions must be canonical unsigned decimal strings")]
    Revision,
    #[error("The library organization revision is exhausted")]
    Exhausted,
    #[error("Library organization changed: expected {expected}, found {actual}")]
    Conflict {
        expected: GameLibraryOrganizationRevision,
        actual: GameLibraryOrganizationRevision,
    },
    #[error("Library timestamps must be nonnegative Unix milliseconds")]
    Timestamp,
    #[error("A library name must contain 1..1024 UTF-8 bytes without control characters")]
    Name,
    #[error("A tag set must contain at most 256 distinct tag identifiers")]
    Tags,
    #[error("A folder cannot contain itself or an ancestor")]
    FolderCycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GameLibraryOrganizationRevision(u64);

impl GameLibraryOrganizationRevision {
    pub const ZERO: Self = Self(0);
    pub const fn value(self) -> u64 {
        self.0
    }
    pub fn next(self) -> Result<Self, GameLibraryOrganizationError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(GameLibraryOrganizationError::Exhausted)
    }
    pub fn check(self, expected: Self) -> Result<(), GameLibraryOrganizationError> {
        if self == expected {
            Ok(())
        } else {
            Err(GameLibraryOrganizationError::Conflict {
                expected,
                actual: self,
            })
        }
    }
}

impl FromStr for GameLibraryOrganizationRevision {
    type Err = GameLibraryOrganizationError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed: u64 = value
            .parse()
            .map_err(|_error| GameLibraryOrganizationError::Revision)?;
        if parsed.to_string() != value {
            return Err(GameLibraryOrganizationError::Revision);
        }
        Ok(Self(parsed))
    }
}

impl Display for GameLibraryOrganizationRevision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GameLibraryOrganizationTimestamp(i64);

impl GameLibraryOrganizationTimestamp {
    pub fn new(value: i64) -> Result<Self, GameLibraryOrganizationError> {
        if value < 0 {
            return Err(GameLibraryOrganizationError::Timestamp);
        }
        Ok(Self(value))
    }
    pub const fn value(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameLibraryOrganizationName(String);

impl GameLibraryOrganizationName {
    pub const MAX_BYTES: usize = 1024;
    pub fn new(value: String) -> Result<Self, GameLibraryOrganizationError> {
        let value = value.trim();
        if value.is_empty() || value.len() > Self::MAX_BYTES || value.chars().any(char::is_control)
        {
            return Err(GameLibraryOrganizationError::Name);
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameLibraryOrganizationFolderId(GameId);

impl GameLibraryOrganizationFolderId {
    pub const fn as_uuid(self) -> uuid::Uuid {
        self.0.as_uuid()
    }
}

impl FromStr for GameLibraryOrganizationFolderId {
    type Err = GameLibraryOrganizationError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}

impl Display for GameLibraryOrganizationFolderId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0.as_uuid(), formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameLibraryOrganizationTagId(GameId);

impl GameLibraryOrganizationTagId {
    pub const fn as_uuid(self) -> uuid::Uuid {
        self.0.as_uuid()
    }
}

impl FromStr for GameLibraryOrganizationTagId {
    type Err = GameLibraryOrganizationError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}

impl Display for GameLibraryOrganizationTagId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0.as_uuid(), formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameLibraryOrganizationGameTarget {
    id: GameId,
    expected: GameLibraryOrganizationRevision,
}

impl GameLibraryOrganizationGameTarget {
    pub const fn new(id: GameId, expected: GameLibraryOrganizationRevision) -> Self {
        Self { id, expected }
    }
    pub const fn id(self) -> GameId {
        self.id
    }
    pub const fn expected(self) -> GameLibraryOrganizationRevision {
        self.expected
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameLibraryOrganizationFolderTarget {
    id: GameLibraryOrganizationFolderId,
    expected: GameLibraryOrganizationRevision,
}

impl GameLibraryOrganizationFolderTarget {
    pub const fn new(
        id: GameLibraryOrganizationFolderId,
        expected: GameLibraryOrganizationRevision,
    ) -> Self {
        Self { id, expected }
    }
    pub const fn id(self) -> GameLibraryOrganizationFolderId {
        self.id
    }
    pub const fn expected(self) -> GameLibraryOrganizationRevision {
        self.expected
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameLibraryOrganizationTagTarget {
    id: GameLibraryOrganizationTagId,
    expected: GameLibraryOrganizationRevision,
}

impl GameLibraryOrganizationTagTarget {
    pub const fn new(
        id: GameLibraryOrganizationTagId,
        expected: GameLibraryOrganizationRevision,
    ) -> Self {
        Self { id, expected }
    }
    pub const fn id(self) -> GameLibraryOrganizationTagId {
        self.id
    }
    pub const fn expected(self) -> GameLibraryOrganizationRevision {
        self.expected
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameLibraryOrganizationTagSet(Vec<GameLibraryOrganizationTagId>);

impl GameLibraryOrganizationTagSet {
    pub const MAX_TAGS: usize = 256;
    pub fn new(
        mut tags: Vec<GameLibraryOrganizationTagId>,
    ) -> Result<Self, GameLibraryOrganizationError> {
        if tags.len() > Self::MAX_TAGS {
            return Err(GameLibraryOrganizationError::Tags);
        }
        tags.sort_unstable_by_key(|id| id.as_uuid());
        if tags.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(GameLibraryOrganizationError::Tags);
        }
        Ok(Self(tags))
    }
    pub fn ids(&self) -> &[GameLibraryOrganizationTagId] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameLibraryOrganizationEnumMutation {
    CreateFolder(
        GameLibraryOrganizationFolderId,
        Option<GameLibraryOrganizationFolderTarget>,
        GameLibraryOrganizationName,
    ),
    RenameFolder(
        GameLibraryOrganizationFolderTarget,
        GameLibraryOrganizationName,
    ),
    ReparentFolder(
        GameLibraryOrganizationFolderTarget,
        Option<GameLibraryOrganizationFolderTarget>,
    ),
    CreateTag(GameLibraryOrganizationTagId, GameLibraryOrganizationName),
    RenameTag(
        GameLibraryOrganizationTagTarget,
        GameLibraryOrganizationName,
    ),
    DeleteTag(GameLibraryOrganizationTagTarget),
    SetFavorite(GameLibraryOrganizationGameTarget, bool),
    MoveToFolder(
        GameLibraryOrganizationGameTarget,
        Option<GameLibraryOrganizationFolderTarget>,
    ),
    ReplaceTags(
        GameLibraryOrganizationGameTarget,
        GameLibraryOrganizationTagSet,
    ),
}

#[derive(Debug, Clone)]
pub struct GameLibraryOrganizationRequest {
    timestamp: GameLibraryOrganizationTimestamp,
    mutation: GameLibraryOrganizationEnumMutation,
}

impl GameLibraryOrganizationRequest {
    pub fn new(
        timestamp: GameLibraryOrganizationTimestamp,
        mutation: GameLibraryOrganizationEnumMutation,
    ) -> Result<Self, GameLibraryOrganizationError> {
        if let GameLibraryOrganizationEnumMutation::CreateFolder(id, Some(parent), _) = &mutation
            && *id == parent.id()
        {
            return Err(GameLibraryOrganizationError::FolderCycle);
        }
        if let GameLibraryOrganizationEnumMutation::ReparentFolder(target, Some(parent)) = &mutation
            && target.id() == parent.id()
        {
            return Err(GameLibraryOrganizationError::FolderCycle);
        }
        Ok(Self {
            timestamp,
            mutation,
        })
    }
    pub const fn timestamp(&self) -> GameLibraryOrganizationTimestamp {
        self.timestamp
    }
    pub fn mutation(&self) -> &GameLibraryOrganizationEnumMutation {
        &self.mutation
    }
    pub fn transport_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match &self.mutation {
                GameLibraryOrganizationEnumMutation::CreateFolder(_, _, name)
                | GameLibraryOrganizationEnumMutation::RenameFolder(_, name)
                | GameLibraryOrganizationEnumMutation::CreateTag(_, name)
                | GameLibraryOrganizationEnumMutation::RenameTag(_, name) => name.as_str().len(),
                GameLibraryOrganizationEnumMutation::ReplaceTags(_, tags) => {
                    std::mem::size_of_val(tags.ids())
                }
                GameLibraryOrganizationEnumMutation::ReparentFolder(_, _)
                | GameLibraryOrganizationEnumMutation::DeleteTag(_)
                | GameLibraryOrganizationEnumMutation::SetFavorite(_, _)
                | GameLibraryOrganizationEnumMutation::MoveToFolder(_, _) => 0,
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameLibraryOrganizationState {
    revision: GameLibraryOrganizationRevision,
    favorite: bool,
    folder: Option<GameLibraryOrganizationFolderId>,
    tags: GameLibraryOrganizationTagSet,
}

impl GameLibraryOrganizationState {
    pub fn new(
        revision: GameLibraryOrganizationRevision,
        favorite: bool,
        folder: Option<GameLibraryOrganizationFolderId>,
        tags: GameLibraryOrganizationTagSet,
    ) -> Self {
        Self {
            revision,
            favorite,
            folder,
            tags,
        }
    }
    pub const fn revision(&self) -> GameLibraryOrganizationRevision {
        self.revision
    }
    pub const fn is_favorite(&self) -> bool {
        self.favorite
    }
    pub const fn folder(&self) -> Option<GameLibraryOrganizationFolderId> {
        self.folder
    }
    pub fn tags(&self) -> &GameLibraryOrganizationTagSet {
        &self.tags
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameLibraryOrganizationEnumTarget {
    Game(GameId),
    Folder(GameLibraryOrganizationFolderId),
    Tag(GameLibraryOrganizationTagId),
}

#[derive(Debug)]
pub struct GameLibraryOrganizationReceipt {
    target: GameLibraryOrganizationEnumTarget,
    revision: GameLibraryOrganizationRevision,
    affected_games: u64,
    changed: bool,
}

impl GameLibraryOrganizationReceipt {
    pub const fn new(
        target: GameLibraryOrganizationEnumTarget,
        revision: GameLibraryOrganizationRevision,
        affected_games: u64,
        changed: bool,
    ) -> Self {
        Self {
            target,
            revision,
            affected_games,
            changed,
        }
    }
    pub const fn target(&self) -> GameLibraryOrganizationEnumTarget {
        self.target
    }
    pub const fn revision(&self) -> GameLibraryOrganizationRevision {
        self.revision
    }
    pub const fn affected_games(&self) -> u64 {
        self.affected_games
    }
    pub const fn is_changed(&self) -> bool {
        self.changed
    }
}
