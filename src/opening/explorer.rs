use std::collections::HashMap;

use thiserror::Error;

use crate::board::r#move::BoardMoveError;
use crate::board::position::{BoardPosition, BoardPositionIdentity};

#[derive(Debug, Error)]
pub enum OpeningExplorerError {
    #[error("An opening definition requires a code, a name and at least one move")]
    Definition,
    #[error(transparent)]
    Move(#[from] BoardMoveError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningExplorerDefinition {
    code: String,
    name: String,
    moves: Vec<String>,
}

impl OpeningExplorerDefinition {
    pub fn new(code: &str, name: &str, moves: &[&str]) -> Result<Self, OpeningExplorerError> {
        if code.is_empty() || name.is_empty() || moves.is_empty() {
            return Err(OpeningExplorerError::Definition);
        }
        Ok(Self {
            code: code.to_owned(),
            name: name.to_owned(),
            moves: moves.iter().map(|value| (*value).to_owned()).collect(),
        })
    }

    fn position(&self) -> Result<BoardPosition, OpeningExplorerError> {
        let mut position = BoardPosition::standard();
        for movement in &self.moves {
            position = position.play_uci(movement)?;
        }
        Ok(position)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningExplorerLabel {
    code: String,
    name: String,
    depth: usize,
}

impl OpeningExplorerLabel {
    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn depth(&self) -> usize {
        self.depth
    }

    fn precedes(&self, other: &Self) -> bool {
        (std::cmp::Reverse(self.depth), &self.code, &self.name)
            < (std::cmp::Reverse(other.depth), &other.code, &other.name)
    }
}

pub struct OpeningExplorerNames {
    labels: HashMap<BoardPositionIdentity, OpeningExplorerLabel>,
}

impl OpeningExplorerNames {
    pub fn new(definitions: &[OpeningExplorerDefinition]) -> Result<Self, OpeningExplorerError> {
        let mut labels: HashMap<BoardPositionIdentity, OpeningExplorerLabel> = HashMap::new();
        for definition in definitions {
            let label = OpeningExplorerLabel {
                code: definition.code.clone(),
                name: definition.name.clone(),
                depth: definition.moves.len(),
            };
            let key = definition.position()?.identity();
            match labels.get(&key) {
                Some(existing) if !label.precedes(existing) => {}
                _ => {
                    labels.insert(key, label);
                }
            }
        }
        Ok(Self { labels })
    }

    pub fn deepest(&self, position: &BoardPosition) -> Option<&OpeningExplorerLabel> {
        self.labels.get(&position.identity())
    }

    pub fn deepest_along(&self, line: &[BoardPosition]) -> Option<&OpeningExplorerLabel> {
        line.iter()
            .rev()
            .find_map(|position| self.deepest(position))
    }
}
