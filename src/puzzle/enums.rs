use crate::board::r#move::BoardMove;
use crate::board::square::BoardSquare;
use crate::puzzle::tree::PuzzleDefinitionNodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleEnumCountdownMode {
    Session,
    PerPuzzle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleEnumAssistance {
    Unassisted,
    Assisted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleEnumPrimaryOutcome {
    Solved(PuzzleEnumAssistance),
    Failed,
    Skipped,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleEnumPhase {
    SolverMove,
    OpponentReply,
    PracticeReady,
    Review,
    Finalized,
}

impl PuzzleEnumPhase {
    pub(super) fn is_unfinished(self) -> bool {
        match self {
            Self::SolverMove | Self::OpponentReply | Self::PracticeReady => true,
            Self::Review | Self::Finalized => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleEnumPractice {
    NotStarted,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleEnumHintStage {
    Unrequested,
    Source,
    Destination,
    NextMove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleEnumHint {
    Source(BoardSquare),
    Destination(BoardSquare),
    NextMove(BoardMove),
}

impl PuzzleEnumHint {
    pub const fn stage(self) -> PuzzleEnumHintStage {
        match self {
            Self::Source(_) => PuzzleEnumHintStage::Source,
            Self::Destination(_) => PuzzleEnumHintStage::Destination,
            Self::NextMove(_) => PuzzleEnumHintStage::NextMove,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleEnumMoveFeedback {
    Accepted,
    Completed,
    LegalUnlisted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleEnumEvent {
    SolverMove {
        movement: BoardMove,
        node: PuzzleDefinitionNodeId,
    },
    OpponentReply {
        movement: BoardMove,
        node: PuzzleDefinitionNodeId,
        choice: usize,
    },
    LegalUnlisted(BoardMove),
    Hint(PuzzleEnumHint),
    PracticeRetry,
    PracticeCompleted,
    Skipped,
    TimedOut(PuzzleEnumCountdownMode),
}
