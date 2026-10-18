use std::collections::HashSet;
use std::num::ParseIntError;
use std::str::FromStr;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PuzzleDefinitionMetadataError {
    #[error("Puzzle difficulty must contain unsigned ASCII decimal digits")]
    DifficultySyntax,
    #[error("Puzzle difficulty is outside the unsigned 32-bit range: {0}")]
    DifficultyRange(#[from] ParseIntError),
    #[error("A puzzle theme must be a nonempty token without ASCII whitespace")]
    InvalidTheme,
    #[error("Available puzzle themes must contain at least one theme")]
    EmptyThemes,
    #[error("The puzzle theme {theme:?} is listed more than once")]
    DuplicateTheme { theme: PuzzleDefinitionTheme },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PuzzleDefinitionDifficulty(u32);

impl PuzzleDefinitionDifficulty {
    pub const fn new(rating: u32) -> Self {
        Self(rating)
    }

    pub const fn rating(self) -> u32 {
        self.0
    }
}

impl FromStr for PuzzleDefinitionDifficulty {
    type Err = PuzzleDefinitionMetadataError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(PuzzleDefinitionMetadataError::DifficultySyntax);
        }
        Ok(Self(value.parse()?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PuzzleDefinitionTheme(String);

impl PuzzleDefinitionTheme {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PuzzleDefinitionTheme {
    type Err = PuzzleDefinitionMetadataError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(PuzzleDefinitionMetadataError::InvalidTheme);
        }
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PuzzleDefinitionMetadata {
    difficulty: Option<PuzzleDefinitionDifficulty>,
    themes: Option<Vec<PuzzleDefinitionTheme>>,
}

impl PuzzleDefinitionMetadata {
    pub fn new(
        difficulty: Option<PuzzleDefinitionDifficulty>,
        themes: Option<Vec<PuzzleDefinitionTheme>>,
    ) -> Result<Self, PuzzleDefinitionMetadataError> {
        if let Some(themes) = &themes {
            if themes.is_empty() {
                return Err(PuzzleDefinitionMetadataError::EmptyThemes);
            }
            let mut seen = HashSet::with_capacity(themes.len());
            for theme in themes {
                if !seen.insert(theme) {
                    return Err(PuzzleDefinitionMetadataError::DuplicateTheme {
                        theme: theme.clone(),
                    });
                }
            }
        }
        Ok(Self { difficulty, themes })
    }

    pub fn difficulty(&self) -> Option<PuzzleDefinitionDifficulty> {
        self.difficulty
    }

    pub fn themes(&self) -> Option<&[PuzzleDefinitionTheme]> {
        self.themes.as_deref()
    }
}
