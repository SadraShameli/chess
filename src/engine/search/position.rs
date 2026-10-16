use std::sync::Arc;

use thiserror::Error;

use crate::board::r#move::BoardMove;

use crate::board::r#move::BoardMoveError;
use crate::board::position::BoardPosition;
use crate::engine::process_provider::EngineProcessLimits;

#[derive(Debug, Error)]
pub enum EngineSearchPositionError {
    #[error("A search history contains an invalid move: {0}")]
    Move(#[from] BoardMoveError),
    #[error("The complete position and history exceed the engine command bound")]
    OversizedHistory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSearchPosition(Arc<EngineSearchPositionState>);

#[derive(Debug, PartialEq, Eq)]
struct EngineSearchPositionState {
    initial: BoardPosition,
    current: BoardPosition,
    history: Box<[BoardMove]>,
    command: Box<str>,
}

impl EngineSearchPosition {
    pub fn new(
        initial: BoardPosition,
        history: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, EngineSearchPositionError> {
        let mut command = if initial == BoardPosition::standard() {
            "position startpos".to_owned()
        } else {
            format!("position fen {}", initial.fen())
        };
        let mut current = initial.clone();
        let mut moves = Vec::new();
        let mut history = history.into_iter().peekable();
        if history.peek().is_some() {
            command.push_str(" moves");
        }
        for value in history {
            let value = value.as_ref();
            let size = command
                .len()
                .checked_add(value.len())
                .and_then(|size| size.checked_add(1))
                .ok_or(EngineSearchPositionError::OversizedHistory)?;
            if size > EngineProcessLimits::LINE_BYTES {
                return Err(EngineSearchPositionError::OversizedHistory);
            }
            let movement = current.parse_uci(value)?;
            current = current.play(movement)?;
            moves.push(movement);
            command.push(' ');
            command.push_str(value);
        }
        Ok(Self(Arc::new(EngineSearchPositionState {
            initial,
            current,
            history: moves.into_boxed_slice(),
            command: command.into_boxed_str(),
        })))
    }

    pub fn initial(&self) -> &BoardPosition {
        &self.0.initial
    }

    pub fn history(&self) -> &[BoardMove] {
        &self.0.history
    }

    pub fn current(&self) -> &BoardPosition {
        &self.0.current
    }

    pub fn command(&self) -> &str {
        &self.0.command
    }
}
