use thiserror::Error;

use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::{PuzzleEnumHint, PuzzleEnumHintStage, PuzzleEnumPhase};
use crate::puzzle::tree::{
    PuzzleDefinitionNode, PuzzleDefinitionNodeId, PuzzleDefinitionTreeError,
};

#[derive(Debug, Error)]
pub enum PuzzleSolutionError {
    #[error("Illegal puzzle input: {0}")]
    IllegalInput(#[from] BoardMoveError),
    #[error(transparent)]
    Tree(#[from] PuzzleDefinitionTreeError),
    #[error("The puzzle solution node {0:?} does not exist")]
    MissingNode(PuzzleDefinitionNodeId),
    #[error("The selected puzzle node has no continuation")]
    MissingContinuation,
    #[error("The selected puzzle continuation has no move")]
    MissingMove,
    #[error("The next puzzle move has no source square")]
    MissingSource,
    #[error("All hint stages for this solver move have already been revealed")]
    HintsExhausted,
    #[error("The selected node is not a continuation of the current puzzle position")]
    NotContinuation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PuzzleSolution {
    definition: PuzzleDefinition,
    cursor: PuzzleDefinitionNodeId,
    position: BoardPosition,
}

impl PuzzleSolution {
    pub(super) fn new(definition: PuzzleDefinition) -> Self {
        Self {
            cursor: definition.solution().root(),
            position: definition.starting_position().clone(),
            definition,
        }
    }

    pub(super) fn definition(&self) -> &PuzzleDefinition {
        &self.definition
    }

    pub(super) fn position(&self) -> &BoardPosition {
        &self.position
    }

    pub(super) fn cursor(&self) -> PuzzleDefinitionNodeId {
        self.cursor
    }

    pub(super) fn node(
        &self,
        id: PuzzleDefinitionNodeId,
    ) -> Result<&PuzzleDefinitionNode, PuzzleSolutionError> {
        self.definition
            .solution()
            .node(id)
            .ok_or(PuzzleSolutionError::MissingNode(id))
    }

    pub(super) fn children(&self) -> Result<&[PuzzleDefinitionNodeId], PuzzleSolutionError> {
        Ok(self.node(self.cursor)?.children())
    }

    pub(super) fn classify(
        &self,
        movement: BoardMove,
    ) -> Result<Option<PuzzleDefinitionNodeId>, PuzzleSolutionError> {
        if let Some(node) = self
            .definition
            .solution()
            .continuation(self.cursor, movement)?
        {
            return Ok(Some(node.id()));
        }
        self.position.play(movement)?;
        Ok(None)
    }

    pub(super) fn next_hint(
        &self,
        stage: PuzzleEnumHintStage,
    ) -> Result<PuzzleEnumHint, PuzzleSolutionError> {
        let id = self
            .children()?
            .first()
            .ok_or(PuzzleSolutionError::MissingContinuation)?;
        let movement = self
            .node(*id)?
            .movement()
            .ok_or(PuzzleSolutionError::MissingMove)?;
        match stage {
            PuzzleEnumHintStage::Unrequested => Ok(PuzzleEnumHint::Source(
                movement
                    .origin()
                    .ok_or(PuzzleSolutionError::MissingSource)?,
            )),
            PuzzleEnumHintStage::Source => Ok(PuzzleEnumHint::Destination(movement.destination())),
            PuzzleEnumHintStage::Destination => Ok(PuzzleEnumHint::NextMove(movement)),
            PuzzleEnumHintStage::NextMove => Err(PuzzleSolutionError::HintsExhausted),
        }
    }

    pub(super) fn advance(
        &mut self,
        id: PuzzleDefinitionNodeId,
    ) -> Result<PuzzleEnumPhase, PuzzleSolutionError> {
        let node = self.node(id)?;
        if node.parent() != Some(self.cursor) {
            return Err(PuzzleSolutionError::NotContinuation);
        }
        let phase = if node.is_terminal() {
            PuzzleEnumPhase::Review
        } else if node.position().turn() == self.definition.solver() {
            PuzzleEnumPhase::SolverMove
        } else {
            PuzzleEnumPhase::OpponentReply
        };
        self.position = node.position().clone();
        self.cursor = id;
        Ok(phase)
    }
}
