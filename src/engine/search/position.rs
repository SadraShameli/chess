use thiserror::Error;

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

#[derive(Debug)]
pub struct EngineSearchPosition {
    current: BoardPosition,
    command: String,
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
        let mut current = initial;
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
            current = current.play_uci(value)?;
            command.push(' ');
            command.push_str(value);
        }
        Ok(Self { current, command })
    }

    pub fn current(&self) -> &BoardPosition {
        &self.current
    }

    pub fn command(&self) -> &str {
        &self.command
    }
}
