use thiserror::Error;

#[derive(Debug, Error)]
pub enum PuzzleDefinitionIdentityError {
    #[error("A puzzle source identity must not be empty")]
    EmptySource,
    #[error("A puzzle source version must not be empty")]
    EmptyVersion,
    #[error("A source puzzle identifier must not be empty")]
    EmptyPuzzleId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PuzzleDefinitionIdentity {
    source: String,
    version: String,
    puzzle_id: String,
}

impl PuzzleDefinitionIdentity {
    pub fn new(
        source: String,
        version: String,
        puzzle_id: String,
    ) -> Result<Self, PuzzleDefinitionIdentityError> {
        if source.is_empty() {
            return Err(PuzzleDefinitionIdentityError::EmptySource);
        }
        if version.is_empty() {
            return Err(PuzzleDefinitionIdentityError::EmptyVersion);
        }
        if puzzle_id.is_empty() {
            return Err(PuzzleDefinitionIdentityError::EmptyPuzzleId);
        }
        Ok(Self {
            source,
            version,
            puzzle_id,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn puzzle_id(&self) -> &str {
        &self.puzzle_id
    }
}
