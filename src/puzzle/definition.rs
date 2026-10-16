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
    pub fn new(
        identity: PuzzleDefinitionIdentity,
        starting_position: BoardPosition,
        lines: Vec<Vec<BoardMove>>,
        metadata: PuzzleDefinitionMetadata,
    ) -> Result<Self, PuzzleDefinitionTreeError> {
        Ok(Self {
            identity,
            solution: PuzzleDefinitionTree::new(starting_position, lines)?,
            metadata,
        })
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
