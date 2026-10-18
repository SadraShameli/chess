use crate::board::enums::BoardEnumColor;
use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::PuzzleDefinitionMetadata;
use crate::puzzle::tree::{PuzzleDefinitionTree, PuzzleDefinitionTreeError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleDefinition {
    identity: PuzzleDefinitionIdentity,
    solution: PuzzleDefinitionTree,
    metadata: PuzzleDefinitionMetadata,
}

impl PuzzleDefinition {
    pub const MAX_SOLUTION_LINES: usize = 1_024;
    pub const MAX_SOLUTION_LINE_PLIES: usize = 4_096;

    pub fn new(
        identity: PuzzleDefinitionIdentity,
        starting_position: BoardPosition,
        lines: Vec<Vec<BoardMove>>,
        metadata: PuzzleDefinitionMetadata,
    ) -> Result<Self, PuzzleDefinitionTreeError> {
        Self::validate_solution_lengths(lines.iter().map(Vec::len))?;
        Ok(Self {
            identity,
            solution: PuzzleDefinitionTree::new(starting_position, lines)?,
            metadata,
        })
    }

    pub(crate) fn validate_solution_lengths(
        lengths: impl ExactSizeIterator<Item = usize>,
    ) -> Result<(), PuzzleDefinitionTreeError> {
        if lengths.len() > Self::MAX_SOLUTION_LINES {
            return Err(PuzzleDefinitionTreeError::TooManyLines {
                actual: lengths.len(),
                limit: Self::MAX_SOLUTION_LINES,
            });
        }
        for (line, actual) in lengths.enumerate() {
            if actual > Self::MAX_SOLUTION_LINE_PLIES {
                return Err(PuzzleDefinitionTreeError::LineTooLong {
                    line,
                    actual,
                    limit: Self::MAX_SOLUTION_LINE_PLIES,
                });
            }
        }
        Ok(())
    }

    pub fn identity(&self) -> &PuzzleDefinitionIdentity {
        &self.identity
    }

    pub fn starting_position(&self) -> &BoardPosition {
        self.solution.starting_position()
    }

    pub fn solver(&self) -> BoardEnumColor {
        self.starting_position().turn()
    }

    pub fn solution(&self) -> &PuzzleDefinitionTree {
        &self.solution
    }

    pub fn metadata(&self) -> &PuzzleDefinitionMetadata {
        &self.metadata
    }
}
