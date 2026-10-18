use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum EngineSearchIdentityError {
    #[error("A search game identifier must not be nil")]
    NilGame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineSearchGameId(Uuid);

impl TryFrom<Uuid> for EngineSearchGameId {
    type Error = EngineSearchIdentityError;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        if value.is_nil() {
            Err(EngineSearchIdentityError::NilGame)
        } else {
            Ok(Self(value))
        }
    }
}

impl EngineSearchGameId {
    pub const fn value(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EngineSearchRevision(u64);

impl EngineSearchRevision {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineSearchContext {
    game: EngineSearchGameId,
    revision: EngineSearchRevision,
}

impl EngineSearchContext {
    pub const fn new(game: EngineSearchGameId, revision: EngineSearchRevision) -> Self {
        Self { game, revision }
    }

    pub const fn game(self) -> EngineSearchGameId {
        self.game
    }

    pub const fn revision(self) -> EngineSearchRevision {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineSearchId(Uuid);

impl EngineSearchId {
    pub const fn value(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineSearchGeneration(Uuid);

impl EngineSearchGeneration {
    pub(crate) fn fresh() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn value(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineSearchTag {
    id: EngineSearchId,
    generation: EngineSearchGeneration,
    context: EngineSearchContext,
}

impl EngineSearchTag {
    pub(crate) fn fresh(generation: EngineSearchGeneration, context: EngineSearchContext) -> Self {
        Self {
            id: EngineSearchId(Uuid::new_v4()),
            generation,
            context,
        }
    }

    pub const fn id(self) -> EngineSearchId {
        self.id
    }

    pub const fn generation(self) -> EngineSearchGeneration {
        self.generation
    }

    pub const fn context(self) -> EngineSearchContext {
        self.context
    }
}
pub mod persistence;
