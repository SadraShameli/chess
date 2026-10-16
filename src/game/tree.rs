use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::clock::state::ClockStateSnapshot;
use crate::game::adjudication::GameAdjudicationCandidate;
use crate::game::enums::GameMoveId;
use crate::game::move_node::{GameMoveNode, GameMoveNodeAnnotations};

#[derive(Debug, Error)]
pub enum GameTreeError {
    #[error("The selected move node does not exist")]
    UnknownNode,
    #[error("The supplied move identity already identifies a different node")]
    DuplicateIdentity,
    #[error("A restored continuation must use its existing stable move identity")]
    DuplicateContinuation,
    #[error("The immutable root cannot be removed or replaced as a move")]
    ImmutableRoot,
    #[error("The resolved move belongs to a different immutable parent position")]
    WrongParent,
    #[error("The move tree contains a broken parent or child relationship")]
    BrokenRelationship,
    #[error(transparent)]
    Move(#[from] BoardMoveError),
}

#[derive(Debug, Clone)]
pub struct GameTree {
    root: GameMoveId,
    nodes: Vec<GameMoveNode>,
    indices: HashMap<GameMoveId, usize>,
}

struct GameTreeResolvedMove {
    parent: GameMoveId,
    id: GameMoveId,
    movement: BoardMove,
    position: BoardPosition,
    clock: Option<ClockStateSnapshot>,
    before_clock: Option<ClockStateSnapshot>,
}

pub(super) struct GameTreeCandidateClock {
    pub before: ClockStateSnapshot,
    pub after: ClockStateSnapshot,
}

enum GameTreeEnumInsertion {
    Insert { parent_index: usize },
    Reuse { index: usize },
}

pub(crate) struct GameTreePreparedMove<'a> {
    tree: &'a mut GameTree,
    insertion: GameTreeEnumInsertion,
    node: GameMoveNode,
}

impl GameTreePreparedMove<'_> {
    pub(crate) fn id(&self) -> GameMoveId {
        self.node.id()
    }

    pub(crate) fn commit(self) -> GameMoveNode {
        match self.insertion {
            GameTreeEnumInsertion::Insert { parent_index } => {
                let receipt = self.node.clone();
                self.tree.nodes[parent_index]
                    .children_mut()
                    .push(self.node.id());
                self.tree
                    .indices
                    .insert(self.node.id(), self.tree.nodes.len());
                self.tree.nodes.push(self.node);
                receipt
            }
            GameTreeEnumInsertion::Reuse { index } => {
                self.tree.nodes[index].set_clock(self.node.clock().cloned());
                self.tree.nodes[index].set_before_clock(self.node.before_clock().cloned());
                self.node
            }
        }
    }
}

impl GameTree {
    pub fn new(
        root: GameMoveId,
        initial: BoardPosition,
        clock: Option<ClockStateSnapshot>,
    ) -> Self {
        Self {
            root,
            nodes: vec![GameMoveNode::root(root, initial, clock)],
            indices: HashMap::from([(root, 0)]),
        }
    }

    pub fn root(&self) -> GameMoveId {
        self.root
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn nodes(&self) -> impl ExactSizeIterator<Item = &GameMoveNode> {
        self.nodes.iter()
    }

    pub(super) fn restore_move(
        &mut self,
        parent: GameMoveId,
        id: GameMoveId,
        movement: BoardMove,
        clock: Option<GameTreeCandidateClock>,
    ) -> Result<GameMoveId, GameTreeError> {
        let position = self.node(parent)?.position().play(movement)?;
        let (before_clock, clock) = match clock {
            Some(clock) => (Some(clock.before), Some(clock.after)),
            None => (None, None),
        };
        let prepared = self.prepare_resolved(GameTreeResolvedMove {
            parent,
            id,
            movement,
            position,
            clock,
            before_clock,
        })?;
        if prepared.id() != id {
            return Err(GameTreeError::DuplicateContinuation);
        }
        Ok(prepared.commit().id())
    }

    pub fn node(&self, id: GameMoveId) -> Result<&GameMoveNode, GameTreeError> {
        self.indices
            .get(&id)
            .and_then(|index| self.nodes.get(*index))
            .ok_or(GameTreeError::UnknownNode)
    }

    pub fn append(
        &mut self,
        parent: GameMoveId,
        id: GameMoveId,
        movement: BoardMove,
        clock: Option<ClockStateSnapshot>,
    ) -> Result<GameMoveId, GameTreeError> {
        let position = self.node(parent)?.position().play(movement)?;
        self.prepare_resolved(GameTreeResolvedMove {
            parent,
            id,
            movement,
            position,
            clock,
            before_clock: None,
        })
        .map(|prepared| prepared.commit().id())
    }

    pub(super) fn append_study(
        &mut self,
        parent: GameMoveId,
        id: GameMoveId,
        movement: BoardMove,
    ) -> Result<GameMoveId, GameTreeError> {
        let matching = self.continuation_index(parent, movement)?;
        let (before_clock, clock) = match matching {
            Some(index) => (
                self.nodes[index].before_clock().cloned(),
                self.nodes[index].clock().cloned(),
            ),
            None => (None, None),
        };
        let position = self.node(parent)?.position().play(movement)?;
        self.prepare_resolved(GameTreeResolvedMove {
            parent,
            id,
            movement,
            position,
            clock,
            before_clock,
        })
        .map(|prepared| prepared.commit().id())
    }

    fn continuation_index(
        &self,
        parent: GameMoveId,
        movement: BoardMove,
    ) -> Result<Option<usize>, GameTreeError> {
        for child in self.node(parent)?.children() {
            if self.node(*child)?.movement() == Some(movement) {
                return self
                    .indices
                    .get(child)
                    .copied()
                    .map(Some)
                    .ok_or(GameTreeError::BrokenRelationship);
            }
        }
        Ok(None)
    }

    pub(super) fn prepare_candidate(
        &mut self,
        parent: GameMoveId,
        id: GameMoveId,
        candidate: &GameAdjudicationCandidate,
        clock: GameTreeCandidateClock,
    ) -> Result<GameTreePreparedMove<'_>, GameTreeError> {
        if self.node(parent)?.position() != candidate.previous_position() {
            return Err(GameTreeError::WrongParent);
        }
        self.prepare_resolved(GameTreeResolvedMove {
            parent,
            id,
            movement: candidate.movement(),
            position: candidate.position().clone(),
            clock: Some(clock.after),
            before_clock: Some(clock.before),
        })
    }

    fn prepare_resolved(
        &mut self,
        resolved: GameTreeResolvedMove,
    ) -> Result<GameTreePreparedMove<'_>, GameTreeError> {
        let parent_index = *self
            .indices
            .get(&resolved.parent)
            .ok_or(GameTreeError::UnknownNode)?;
        let parent = self
            .nodes
            .get(parent_index)
            .ok_or(GameTreeError::BrokenRelationship)?;
        let matching = self.continuation_index(resolved.parent, resolved.movement)?;
        if let Some(existing) = self.indices.get(&resolved.id)
            && Some(*existing) != matching
        {
            return Err(GameTreeError::DuplicateIdentity);
        }
        let (insertion, node) = match matching {
            Some(index) => {
                let mut node = self.nodes[index].clone();
                node.set_clock(resolved.clock);
                node.set_before_clock(resolved.before_clock);
                (GameTreeEnumInsertion::Reuse { index }, node)
            }
            None => {
                let san = parent
                    .position()
                    .san(&resolved.movement.uci(parent.position().variant()))?;
                let mut node = GameMoveNode::child(
                    resolved.id,
                    resolved.parent,
                    resolved.movement,
                    san,
                    resolved.position,
                );
                node.set_clock(resolved.clock);
                node.set_before_clock(resolved.before_clock);
                (GameTreeEnumInsertion::Insert { parent_index }, node)
            }
        };
        Ok(GameTreePreparedMove {
            tree: self,
            insertion,
            node,
        })
    }

    pub fn set_annotations(
        &mut self,
        id: GameMoveId,
        annotations: GameMoveNodeAnnotations,
    ) -> Result<(), GameTreeError> {
        let index = *self.indices.get(&id).ok_or(GameTreeError::UnknownNode)?;
        let node = self
            .nodes
            .get_mut(index)
            .ok_or(GameTreeError::BrokenRelationship)?;
        node.set_annotations(annotations);
        Ok(())
    }

    pub fn path_to(&self, id: GameMoveId) -> Result<Vec<GameMoveId>, GameTreeError> {
        let mut path = Vec::new();
        let mut current = Some(id);
        while let Some(id) = current {
            if path.len() >= self.nodes.len() {
                return Err(GameTreeError::BrokenRelationship);
            }
            let node = self.node(id)?;
            path.push(id);
            current = node.parent();
        }
        path.reverse();
        if path.first() != Some(&self.root) {
            return Err(GameTreeError::BrokenRelationship);
        }
        Ok(path)
    }

    pub fn delete_branch(&mut self, id: GameMoveId) -> Result<usize, GameTreeError> {
        let parent = self
            .node(id)?
            .parent()
            .ok_or(GameTreeError::ImmutableRoot)?;
        let parent_index = *self
            .indices
            .get(&parent)
            .ok_or(GameTreeError::BrokenRelationship)?;
        let removed = self.subtree(id)?;
        self.nodes[parent_index]
            .children_mut()
            .retain(|child| *child != id);
        self.remove_nodes(&removed);
        Ok(removed.len())
    }

    pub fn replace_branch(
        &mut self,
        id: GameMoveId,
        replacement: GameMoveId,
        movement: BoardMove,
    ) -> Result<GameMoveId, GameTreeError> {
        let parent = self
            .node(id)?
            .parent()
            .ok_or(GameTreeError::ImmutableRoot)?;
        let parent_index = *self
            .indices
            .get(&parent)
            .ok_or(GameTreeError::BrokenRelationship)?;
        let parent_node = self.node(parent)?;
        let position = parent_node.position().play(movement)?;
        let san = parent_node
            .position()
            .san(&movement.uci(parent_node.position().variant()))?;
        let child_index = parent_node
            .children()
            .iter()
            .position(|child| *child == id)
            .ok_or(GameTreeError::BrokenRelationship)?;
        let removed = self.subtree(id)?;
        let matching = self
            .continuation_index(parent, movement)?
            .map(|index| self.nodes[index].id());
        if self.indices.contains_key(&replacement)
            && Some(replacement) != matching
            && !removed.contains(&replacement)
        {
            return Err(GameTreeError::DuplicateIdentity);
        }
        if let Some(existing) = matching {
            if existing != id {
                self.nodes[parent_index]
                    .children_mut()
                    .retain(|child| *child != id);
                self.remove_nodes(&removed);
            }
            return Ok(existing);
        }
        let node = GameMoveNode::child(replacement, parent, movement, san, position);
        self.nodes[parent_index].children_mut()[child_index] = replacement;
        self.remove_nodes(&removed);
        self.indices.insert(replacement, self.nodes.len());
        self.nodes.push(node);
        Ok(replacement)
    }

    fn subtree(&self, id: GameMoveId) -> Result<HashSet<GameMoveId>, GameTreeError> {
        let mut pending = vec![id];
        let mut removed = HashSet::new();
        while let Some(id) = pending.pop() {
            if !removed.insert(id) {
                return Err(GameTreeError::BrokenRelationship);
            }
            pending.extend_from_slice(self.node(id)?.children());
        }
        Ok(removed)
    }

    fn remove_nodes(&mut self, removed: &HashSet<GameMoveId>) {
        self.nodes.retain(|node| !removed.contains(&node.id()));
        self.indices = self
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id(), index))
            .collect();
    }
}
