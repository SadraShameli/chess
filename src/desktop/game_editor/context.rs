use crate::desktop::game_editor::DesktopGameEditorError;
use crate::desktop::identity::DesktopRequestId;
use crate::game::enums::{GameId, GameMoveId, GameRevision};
use crate::game::storage_repository::GameStorageRevision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopGameEditorEpoch(DesktopRequestId);

impl DesktopGameEditorEpoch {
    pub const fn from_request_id(request: DesktopRequestId) -> Self {
        Self(request)
    }

    pub const fn request_id(self) -> DesktopRequestId {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopGameEditorGeneration(u64);

impl DesktopGameEditorGeneration {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, DesktopGameEditorError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(DesktopGameEditorError::GenerationExhausted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopGameEditorBinding {
    storage_revision: GameStorageRevision,
    epoch: DesktopGameEditorEpoch,
    generation: DesktopGameEditorGeneration,
}

impl DesktopGameEditorBinding {
    pub const fn new(
        storage_revision: GameStorageRevision,
        epoch: DesktopGameEditorEpoch,
        generation: DesktopGameEditorGeneration,
    ) -> Self {
        Self {
            storage_revision,
            epoch,
            generation,
        }
    }

    pub const fn storage_revision(self) -> GameStorageRevision {
        self.storage_revision
    }

    pub const fn epoch(self) -> DesktopGameEditorEpoch {
        self.epoch
    }

    pub const fn generation(self) -> DesktopGameEditorGeneration {
        self.generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopGameEditorEnumSource {
    Played { revision: GameRevision },
    Notation { revision: GameRevision },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopGameEditorContext {
    game: GameId,
    source: DesktopGameEditorEnumSource,
    selected: GameMoveId,
    binding: DesktopGameEditorBinding,
}

impl DesktopGameEditorContext {
    pub(super) const fn new(
        game: GameId,
        source: DesktopGameEditorEnumSource,
        selected: GameMoveId,
        binding: DesktopGameEditorBinding,
    ) -> Self {
        Self {
            game,
            source,
            selected,
            binding,
        }
    }

    pub const fn game(self) -> GameId {
        self.game
    }

    pub const fn source(self) -> DesktopGameEditorEnumSource {
        self.source
    }

    pub const fn selected(self) -> GameMoveId {
        self.selected
    }

    pub const fn storage_revision(self) -> GameStorageRevision {
        self.binding.storage_revision()
    }

    pub const fn epoch(self) -> DesktopGameEditorEpoch {
        self.binding.epoch()
    }

    pub const fn generation(self) -> DesktopGameEditorGeneration {
        self.binding.generation()
    }

    pub fn check_expected(self, expected: Self) -> Result<(), DesktopGameEditorError> {
        if self == expected {
            Ok(())
        } else {
            Err(DesktopGameEditorError::StaleContext)
        }
    }
}
