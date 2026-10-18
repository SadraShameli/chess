use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleDefinitionIdentityEnumField {
    Source,
    Version,
    PuzzleId,
}

#[derive(Debug, Error)]
pub enum PuzzleDefinitionIdentityError {
    #[error("A puzzle source identity must not be empty")]
    EmptySource,
    #[error("A puzzle source version must not be empty")]
    EmptyVersion,
    #[error("A source puzzle identifier must not be empty")]
    EmptyPuzzleId,
    #[error("Puzzle identity {field:?} has {actual} bytes, exceeding the limit {limit}")]
    TokenTooLong {
        field: PuzzleDefinitionIdentityEnumField,
        actual: usize,
        limit: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PuzzleDefinitionIdentity {
    source: String,
    version: String,
    puzzle_id: String,
}

impl PuzzleDefinitionIdentity {
    pub const MAX_TOKEN_BYTES: usize = 1_024;

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
        Self::validate_token_lengths(&source, &version, &puzzle_id)?;
        Ok(Self {
            source,
            version,
            puzzle_id,
        })
    }

    pub(crate) fn validate_token_lengths(
        source: &str,
        version: &str,
        puzzle_id: &str,
    ) -> Result<(), PuzzleDefinitionIdentityError> {
        for (field, value) in [
            (PuzzleDefinitionIdentityEnumField::Source, source),
            (PuzzleDefinitionIdentityEnumField::Version, version),
            (PuzzleDefinitionIdentityEnumField::PuzzleId, puzzle_id),
        ] {
            if value.len() > Self::MAX_TOKEN_BYTES {
                return Err(PuzzleDefinitionIdentityError::TokenTooLong {
                    field,
                    actual: value.len(),
                    limit: Self::MAX_TOKEN_BYTES,
                });
            }
        }
        Ok(())
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
