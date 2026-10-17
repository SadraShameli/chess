use thiserror::Error;

use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PuzzleDefinitionNodeId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PuzzleDefinitionLineId(usize);

#[derive(Debug, Error)]
pub enum PuzzleDefinitionTreeError {
    #[error("A puzzle must contain at least one accepted solution line")]
    EmptySolutions,
    #[error("A puzzle has {actual} solution lines, exceeding the limit {limit}")]
    TooManyLines { actual: usize, limit: usize },
    #[error("Puzzle solution line {line} has {actual} plies, exceeding the limit {limit}")]
    LineTooLong {
        line: usize,
        actual: usize,
        limit: usize,
    },
    #[error("Puzzle solution line {line} contains no moves")]
    EmptyLine { line: usize },
    #[error("Puzzle solution line {line} duplicates line {previous:?}")]
    DuplicateLine {
        line: usize,
        previous: PuzzleDefinitionLineId,
    },
    #[error("Puzzle solution line {line} conflicts with a terminal prefix at node {node:?}")]
    TerminalPrefix {
        line: usize,
        node: PuzzleDefinitionNodeId,
    },
    #[error("Puzzle solution line {line}, ply {ply} cannot be played: {source}")]
    IllegalMove {
        line: usize,
        ply: usize,
        #[source]
        source: BoardMoveError,
    },
    #[error("The selected puzzle solution node {node:?} does not exist")]
    UnknownNode { node: PuzzleDefinitionNodeId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleDefinitionNode {
    id: PuzzleDefinitionNodeId,
    parent: Option<PuzzleDefinitionNodeId>,
    movement: Option<BoardMove>,
    position: BoardPosition,
    children: Vec<PuzzleDefinitionNodeId>,
    terminal_line: Option<PuzzleDefinitionLineId>,
}

impl PuzzleDefinitionNode {
    pub fn id(&self) -> PuzzleDefinitionNodeId {
        self.id
    }

    pub fn parent(&self) -> Option<PuzzleDefinitionNodeId> {
        self.parent
    }

    pub fn movement(&self) -> Option<BoardMove> {
        self.movement
    }

    pub fn position(&self) -> &BoardPosition {
        &self.position
    }

    pub fn children(&self) -> &[PuzzleDefinitionNodeId] {
        &self.children
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal_line.is_some()
    }

    pub fn terminal_line(&self) -> Option<PuzzleDefinitionLineId> {
        self.terminal_line
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleDefinitionLine {
    id: PuzzleDefinitionLineId,
    nodes: Vec<PuzzleDefinitionNodeId>,
}

impl PuzzleDefinitionLine {
    pub fn id(&self) -> PuzzleDefinitionLineId {
        self.id
    }

    pub fn nodes(&self) -> &[PuzzleDefinitionNodeId] {
        &self.nodes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleDefinitionTree {
    nodes: Vec<PuzzleDefinitionNode>,
    lines: Vec<PuzzleDefinitionLine>,
}

impl PuzzleDefinitionTree {
    const ROOT: PuzzleDefinitionNodeId = PuzzleDefinitionNodeId(0);

    pub(super) fn new(
        initial: BoardPosition,
        lines: Vec<Vec<BoardMove>>,
    ) -> Result<Self, PuzzleDefinitionTreeError> {
        if lines.is_empty() {
            return Err(PuzzleDefinitionTreeError::EmptySolutions);
        }
        let mut tree = Self {
            nodes: vec![PuzzleDefinitionNode {
                id: Self::ROOT,
                parent: None,
                movement: None,
                position: initial,
                children: Vec::new(),
                terminal_line: None,
            }],
            lines: Vec::with_capacity(lines.len()),
        };
        for (line, movements) in lines.into_iter().enumerate() {
            tree.append_line(line, movements)?;
        }
        Ok(tree)
    }

    pub fn root(&self) -> PuzzleDefinitionNodeId {
        Self::ROOT
    }

    pub fn starting_position(&self) -> &BoardPosition {
        &self.nodes[Self::ROOT.0].position
    }

    pub fn node(&self, id: PuzzleDefinitionNodeId) -> Option<&PuzzleDefinitionNode> {
        self.nodes.get(id.0)
    }

    pub fn lines(&self) -> &[PuzzleDefinitionLine] {
        &self.lines
    }

    pub fn continuation(
        &self,
        parent: PuzzleDefinitionNodeId,
        movement: BoardMove,
    ) -> Result<Option<&PuzzleDefinitionNode>, PuzzleDefinitionTreeError> {
        let parent = self
            .node(parent)
            .ok_or(PuzzleDefinitionTreeError::UnknownNode { node: parent })?;
        Ok(parent
            .children
            .iter()
            .map(|id| &self.nodes[id.0])
            .find(|node| node.movement == Some(movement)))
    }

    fn append_line(
        &mut self,
        line: usize,
        movements: Vec<BoardMove>,
    ) -> Result<(), PuzzleDefinitionTreeError> {
        if movements.is_empty() {
            return Err(PuzzleDefinitionTreeError::EmptyLine { line });
        }
        let id = PuzzleDefinitionLineId(line);
        let mut path = Vec::with_capacity(movements.len());
        let mut parent = Self::ROOT;
        for (ply, movement) in movements.into_iter().enumerate() {
            if self.nodes[parent.0].is_terminal() {
                return Err(PuzzleDefinitionTreeError::TerminalPrefix { line, node: parent });
            }
            let position = self.nodes[parent.0]
                .position
                .play(movement)
                .map_err(|source| PuzzleDefinitionTreeError::IllegalMove { line, ply, source })?;
            let child = match self.continuation(parent, movement)? {
                Some(node) => node.id,
                None => {
                    let child = PuzzleDefinitionNodeId(self.nodes.len());
                    self.nodes.push(PuzzleDefinitionNode {
                        id: child,
                        parent: Some(parent),
                        movement: Some(movement),
                        position,
                        children: Vec::new(),
                        terminal_line: None,
                    });
                    self.nodes[parent.0].children.push(child);
                    child
                }
            };
            path.push(child);
            parent = child;
        }
        let terminal = &mut self.nodes[parent.0];
        if let Some(previous) = terminal.terminal_line {
            return Err(PuzzleDefinitionTreeError::DuplicateLine { line, previous });
        }
        if !terminal.children.is_empty() {
            return Err(PuzzleDefinitionTreeError::TerminalPrefix { line, node: parent });
        }
        terminal.terminal_line = Some(id);
        self.lines.push(PuzzleDefinitionLine { id, nodes: path });
        Ok(())
    }
}
