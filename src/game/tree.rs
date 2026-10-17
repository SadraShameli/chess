use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::clock::state::ClockStateSnapshot;
use crate::game::adjudication::GameAdjudicationCandidate;
use crate::game::annotations::{
    GameAnnotationsConfirmedVariation, GameAnnotationsDeletionScope, GameAnnotationsEnumChange,
    GameAnnotationsEnumVariationRequest, GameAnnotationsPromotionScope,
    GameAnnotationsVariationPreview,
};
use crate::game::enums::{GameEnumMarkingColor, GameMoveId};
use crate::game::move_node::{
    GameMoveNode, GameMoveNodeAnnotationError, GameMoveNodeAnnotations, GameMoveNodeMarking,
    GameMoveNodeNag,
};

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
    #[error("The study mainline tip must identify a leaf of the game tree")]
    InteriorStudyTip,
    #[error("The variation scope changed after its preview")]
    StaleVariation,
    #[error("The variation content exceeds the fingerprint length range")]
    ContentLengthOverflow,
    #[error(transparent)]
    Annotation(#[from] GameMoveNodeAnnotationError),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
}

#[derive(Debug, Clone)]
pub struct GameTree {
    root: GameMoveId,
    nodes: Vec<GameMoveNode>,
    indices: HashMap<GameMoveId, usize>,
}

struct GameTreeVariationContentHasher {
    digest: Sha256,
}

impl GameTreeVariationContentHasher {
    const DOMAIN: &[u8] = b"game-annotations-content-v1";
    const ABSENT: u8 = 0;
    const PRESENT: u8 = 1;

    fn new() -> Self {
        let mut digest = Sha256::new();
        digest.update(Self::DOMAIN);
        Self { digest }
    }

    fn length(&mut self, length: usize) -> Result<(), GameTreeError> {
        let length =
            u64::try_from(length).map_err(|_error| GameTreeError::ContentLengthOverflow)?;
        self.digest.update(length.to_be_bytes());
        Ok(())
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<(), GameTreeError> {
        self.length(bytes.len())?;
        self.digest.update(bytes);
        Ok(())
    }

    fn node(&mut self, node: &GameMoveNode) -> Result<(), GameTreeError> {
        self.digest.update(node.id().as_uuid().as_bytes());
        match node.parent() {
            Some(parent) => {
                self.digest.update([Self::PRESENT]);
                self.digest.update(parent.as_uuid().as_bytes());
            }
            None => self.digest.update([Self::ABSENT]),
        }
        self.length(node.children().len())?;
        for child in node.children() {
            self.digest.update(child.as_uuid().as_bytes());
        }
        match node.san() {
            Some(san) => {
                self.digest.update([Self::PRESENT]);
                self.bytes(san.as_bytes())?;
            }
            None => self.digest.update([Self::ABSENT]),
        }
        let annotations = node.annotations();
        self.bytes(annotations.notes().as_bytes())?;
        self.length(annotations.nags().len())?;
        for nag in annotations.nags() {
            self.digest.update([nag.value()]);
        }
        self.length(annotations.markings().len())?;
        for marking in annotations.markings() {
            let color = match marking.color() {
                GameEnumMarkingColor::Green => b'G',
                GameEnumMarkingColor::Red => b'R',
                GameEnumMarkingColor::Blue => b'B',
                GameEnumMarkingColor::Yellow => b'Y',
            };
            self.digest.update([color, u8::from(marking.origin())]);
            match marking.destination() {
                Some(destination) => self.digest.update([Self::PRESENT, u8::from(destination)]),
                None => self.digest.update([Self::ABSENT]),
            }
        }
        Ok(())
    }

    fn finish(self) -> [u8; 32] {
        self.digest.finalize().into()
    }
}

struct GameTreeAncestors<'a> {
    tree: &'a GameTree,
    next: Option<GameMoveId>,
    remaining: usize,
}

impl<'a> Iterator for GameTreeAncestors<'a> {
    type Item = Result<&'a GameMoveNode, GameTreeError>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next.take()?;
        if self.remaining == 0 {
            return Some(Err(GameTreeError::BrokenRelationship));
        }
        self.remaining -= 1;
        let node = match self.tree.node(id) {
            Ok(node) => node,
            Err(error) => return Some(Err(error)),
        };
        if node.parent().is_none() && node.id() != self.tree.root {
            return Some(Err(GameTreeError::BrokenRelationship));
        }
        self.next = node.parent();
        Some(Ok(node))
    }
}

struct GameTreeResolvedMove {
    parent: GameMoveId,
    id: GameMoveId,
    movement: BoardMove,
    position: BoardPosition,
    clock: Option<ClockStateSnapshot>,
    before_clock: Option<ClockStateSnapshot>,
}

pub(crate) struct GameTreeCandidateClock {
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

pub(crate) struct GameTreePreparedAnnotations<'a> {
    tree: &'a mut GameTree,
    index: usize,
    annotations: GameMoveNodeAnnotations,
    change: GameAnnotationsEnumChange,
}

impl GameTreePreparedAnnotations<'_> {
    pub(crate) const fn change(&self) -> GameAnnotationsEnumChange {
        self.change
    }

    pub(crate) fn commit(self) -> GameAnnotationsEnumChange {
        if self.change == GameAnnotationsEnumChange::Changed {
            self.tree.nodes[self.index].set_annotations(self.annotations);
        }
        self.change
    }
}

enum GameTreeEnumPreparedVariation {
    Promote {
        ordering: Vec<(usize, usize)>,
    },
    Delete {
        parent_index: usize,
        removed: HashSet<GameMoveId>,
    },
}

pub(crate) struct GameTreePreparedVariation<'a> {
    tree: &'a mut GameTree,
    operation: GameTreeEnumPreparedVariation,
    change: GameAnnotationsEnumChange,
    tip: GameMoveId,
    view: GameMoveId,
}

impl GameTreePreparedVariation<'_> {
    pub(crate) const fn change(&self) -> GameAnnotationsEnumChange {
        self.change
    }

    pub(crate) const fn tip(&self) -> GameMoveId {
        self.tip
    }

    pub(crate) const fn view(&self) -> GameMoveId {
        self.view
    }

    pub(crate) fn commit(self) -> GameAnnotationsEnumChange {
        match self.operation {
            GameTreeEnumPreparedVariation::Promote { ordering } => {
                for (parent, child) in ordering {
                    self.tree.nodes[parent].children_mut()[..=child].rotate_right(1);
                }
            }
            GameTreeEnumPreparedVariation::Delete {
                parent_index,
                removed,
            } => {
                self.tree.nodes[parent_index]
                    .children_mut()
                    .retain(|child| !removed.contains(child));
                self.tree.remove_nodes(&removed);
            }
        }
        self.change
    }
}

impl GameTreePreparedMove<'_> {
    pub(crate) fn id(&self) -> GameMoveId {
        self.node.id()
    }

    pub(crate) fn ordered_leaf(&self) -> Result<GameMoveId, GameTreeError> {
        match self.insertion {
            GameTreeEnumInsertion::Insert { .. } => Ok(self.node.id()),
            GameTreeEnumInsertion::Reuse { .. } => self.tree.ordered_leaf(self.node.id()),
        }
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

    pub(crate) fn contains(&self, id: GameMoveId) -> bool {
        self.indices.contains_key(&id)
    }

    pub fn nodes(&self) -> impl ExactSizeIterator<Item = &GameMoveNode> {
        self.nodes.iter()
    }

    pub(crate) fn restore_move(
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

    pub(super) fn prepare_study(
        &mut self,
        parent: GameMoveId,
        id: GameMoveId,
        movement: BoardMove,
    ) -> Result<GameTreePreparedMove<'_>, GameTreeError> {
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

    pub(crate) fn prepare_notes_and_nags(
        &mut self,
        id: GameMoveId,
        notes: String,
        nags: Vec<GameMoveNodeNag>,
    ) -> Result<GameTreePreparedAnnotations<'_>, GameTreeError> {
        let annotations = GameMoveNodeAnnotations::new(
            notes,
            nags,
            self.node(id)?.annotations().markings().to_vec(),
        )?;
        self.prepare_annotations(id, annotations)
    }

    pub(crate) fn prepare_markings(
        &mut self,
        id: GameMoveId,
        markings: Vec<GameMoveNodeMarking>,
    ) -> Result<GameTreePreparedAnnotations<'_>, GameTreeError> {
        let previous = self.node(id)?.annotations();
        let annotations = GameMoveNodeAnnotations::new(
            previous.notes().to_owned(),
            previous.nags().to_vec(),
            markings,
        )?;
        self.prepare_annotations(id, annotations)
    }

    fn prepare_annotations(
        &mut self,
        id: GameMoveId,
        annotations: GameMoveNodeAnnotations,
    ) -> Result<GameTreePreparedAnnotations<'_>, GameTreeError> {
        let index = *self.indices.get(&id).ok_or(GameTreeError::UnknownNode)?;
        let change = if self.node(id)?.annotations() == &annotations {
            GameAnnotationsEnumChange::Unchanged
        } else {
            GameAnnotationsEnumChange::Changed
        };
        Ok(GameTreePreparedAnnotations {
            tree: self,
            index,
            annotations,
            change,
        })
    }

    pub(crate) fn preview_variation(
        &self,
        request: GameAnnotationsEnumVariationRequest,
        previous_tip: GameMoveId,
    ) -> Result<GameAnnotationsVariationPreview, GameTreeError> {
        let previous_path = self.path_to(previous_tip)?;
        match request {
            GameAnnotationsEnumVariationRequest::Promote { tip } => {
                let path = self.path_to(tip)?;
                let mut content = GameTreeVariationContentHasher::new();
                self.fingerprint_nodes(&mut content, &previous_path)?;
                self.fingerprint_nodes(&mut content, &path)?;
                let children = path
                    .iter()
                    .map(|id| self.node(*id).map(|node| node.children().to_vec()))
                    .collect::<Result<_, _>>()?;
                Ok(GameAnnotationsVariationPreview::promotion(
                    previous_tip,
                    previous_path,
                    GameAnnotationsPromotionScope {
                        tip,
                        path,
                        children,
                        content_fingerprint: content.finish(),
                    },
                ))
            }
            GameAnnotationsEnumVariationRequest::Delete { root } => {
                let parent = self
                    .node(root)?
                    .parent()
                    .ok_or(GameTreeError::ImmutableRoot)?;
                let siblings = self.node(parent)?.children();
                if !siblings.contains(&root) {
                    return Err(GameTreeError::BrokenRelationship);
                }
                let nodes = self.ordered_subtree(root)?;
                let mut content = GameTreeVariationContentHasher::new();
                self.fingerprint_nodes(&mut content, &nodes)?;
                let replacement_tip = if nodes.contains(&previous_tip) {
                    match siblings.iter().find(|child| **child != root) {
                        Some(child) => self.ordered_leaf(*child)?,
                        None => parent,
                    }
                } else {
                    previous_tip
                };
                self.path_to(replacement_tip)?;
                Ok(GameAnnotationsVariationPreview::deletion(
                    previous_tip,
                    previous_path,
                    GameAnnotationsDeletionScope {
                        root,
                        parent,
                        siblings: siblings.to_vec(),
                        nodes,
                        replacement_tip,
                        content_fingerprint: content.finish(),
                    },
                ))
            }
        }
    }

    fn fingerprint_nodes(
        &self,
        content: &mut GameTreeVariationContentHasher,
        nodes: &[GameMoveId],
    ) -> Result<(), GameTreeError> {
        content.length(nodes.len())?;
        for id in nodes {
            content.node(self.node(*id)?)?;
        }
        Ok(())
    }

    pub(crate) fn prepare_variation(
        &mut self,
        confirmation: &GameAnnotationsConfirmedVariation,
        previous_tip: GameMoveId,
        current_view: GameMoveId,
    ) -> Result<GameTreePreparedVariation<'_>, GameTreeError> {
        self.node(current_view)?;
        let preview = confirmation.preview();
        let current = self.preview_variation(preview.request(), previous_tip)?;
        if &current != preview {
            return Err(GameTreeError::StaleVariation);
        }
        let tip = preview.resulting_tip();
        let mut view = current_view;
        let (operation, change) = match preview.request() {
            GameAnnotationsEnumVariationRequest::Promote { tip } => {
                let path = self.path_to(tip)?;
                let mut ordering = Vec::with_capacity(path.len());
                for edge in path.windows(2) {
                    let parent_index = *self
                        .indices
                        .get(&edge[0])
                        .ok_or(GameTreeError::BrokenRelationship)?;
                    let child_index = self.nodes[parent_index]
                        .children()
                        .iter()
                        .position(|child| *child == edge[1])
                        .ok_or(GameTreeError::BrokenRelationship)?;
                    if child_index != 0 {
                        ordering.push((parent_index, child_index));
                    }
                }
                let change = if tip == previous_tip && ordering.is_empty() {
                    GameAnnotationsEnumChange::Unchanged
                } else {
                    GameAnnotationsEnumChange::Changed
                };
                (GameTreeEnumPreparedVariation::Promote { ordering }, change)
            }
            GameAnnotationsEnumVariationRequest::Delete { root } => {
                let parent = self
                    .node(root)?
                    .parent()
                    .ok_or(GameTreeError::ImmutableRoot)?;
                let parent_index = *self
                    .indices
                    .get(&parent)
                    .ok_or(GameTreeError::BrokenRelationship)?;
                let removed: HashSet<_> = preview.deleted_nodes().iter().copied().collect();
                if removed.contains(&current_view) {
                    view = parent;
                }
                if removed.contains(&tip) {
                    return Err(GameTreeError::BrokenRelationship);
                }
                (
                    GameTreeEnumPreparedVariation::Delete {
                        parent_index,
                        removed,
                    },
                    GameAnnotationsEnumChange::Changed,
                )
            }
        };
        Ok(GameTreePreparedVariation {
            tree: self,
            operation,
            change,
            tip,
            view,
        })
    }

    pub fn path_to(&self, id: GameMoveId) -> Result<Vec<GameMoveId>, GameTreeError> {
        let mut path = self
            .ancestors(id)
            .map(|node| node.map(GameMoveNode::id))
            .collect::<Result<Vec<_>, _>>()?;
        path.reverse();
        Ok(path)
    }

    fn ancestors(&self, id: GameMoveId) -> GameTreeAncestors<'_> {
        GameTreeAncestors {
            tree: self,
            next: Some(id),
            remaining: self.nodes.len(),
        }
    }

    pub(crate) fn line_len(&self, id: GameMoveId) -> Result<usize, GameTreeError> {
        self.ancestors(id).try_fold(0, |length, node| {
            Ok(length + usize::from(node?.parent().is_some()))
        })
    }

    pub(crate) fn validate_study_tip(&self, id: GameMoveId) -> Result<(), GameTreeError> {
        if !self.node(id)?.children().is_empty() {
            return Err(GameTreeError::InteriorStudyTip);
        }
        self.line_len(id)?;
        Ok(())
    }

    pub(crate) fn ordered_leaf(&self, id: GameMoveId) -> Result<GameMoveId, GameTreeError> {
        let mut current = id;
        for _ in 0..self.nodes.len() {
            let node = self.node(current)?;
            let Some(&child) = node.children().first() else {
                return Ok(current);
            };
            if self.node(child)?.parent() != Some(current) {
                return Err(GameTreeError::BrokenRelationship);
            }
            current = child;
        }
        Err(GameTreeError::BrokenRelationship)
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
        self.ordered_subtree(id)
            .map(|nodes| nodes.into_iter().collect())
    }

    fn ordered_subtree(&self, id: GameMoveId) -> Result<Vec<GameMoveId>, GameTreeError> {
        let mut pending = vec![id];
        let mut visited = HashSet::new();
        let mut ordered = Vec::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                return Err(GameTreeError::BrokenRelationship);
            }
            let node = self.node(id)?;
            for child in node.children().iter().rev() {
                if self.node(*child)?.parent() != Some(id) {
                    return Err(GameTreeError::BrokenRelationship);
                }
                pending.push(*child);
            }
            ordered.push(id);
        }
        Ok(ordered)
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
