use crate::board::r#move::BoardMove;
use crate::engine::analysis::EngineAnalysis;
use crate::engine::process_provider::EngineProcessEnumPipe;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineSearchEnumState {
    Idle,
    Preparing,
    Searching,
    Stopping,
    Failed,
}

#[derive(Debug)]
pub enum EngineSearchEnumOutput {
    Info(EngineAnalysis),
    Diagnostic {
        pipe: EngineProcessEnumPipe,
        text: String,
    },
    Readiness,
    BestMove(BoardMove),
    Stopped,
}
