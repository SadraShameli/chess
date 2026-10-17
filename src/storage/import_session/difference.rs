use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::board::enums::BoardEnumColor;
use crate::game::enums::GameMoveId;
use crate::game::move_node::GameMoveNode;
use crate::game::tree::GameTree;
use crate::notation::game::NotationGameEnumWarning;
use crate::notation::game_identity::{
    NotationGameIdentity, NotationGameIdentityEnumMatch, NotationGameIdentityEnumResult,
};
use crate::storage::game_document::StorageGameDocument;
use crate::storage::import_session::StorageImportSessionRuntime;
use crate::storage::import_session::disk::StorageImportSessionDiskEnumFile;
use crate::storage::import_session::index::StorageImportSessionIndex;
use crate::storage::import_session::model::{
    StorageImportSessionCandidate, StorageImportSessionCandidateBody,
    StorageImportSessionCandidateCursor, StorageImportSessionCandidateQuery,
    StorageImportSessionError, StorageImportSessionFilesystemProvider,
    StorageImportSessionIdentityProvider, StorageImportSessionPageRequest,
    StorageImportSessionRepository, StorageImportSessionRepositoryError,
};
use crate::storage::import_session::stage::StorageImportSessionStage;
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorageImportSessionDetailEnumCursor {
    Staged(u64),
    Library {
        page: StorageImportSessionCandidateCursor,
        offset: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageImportSessionDetailRequest {
    ordinal: u64,
    cursor: StorageImportSessionDetailEnumCursor,
    bounds: StorageImportSessionPageRequest,
}

impl StorageImportSessionDetailRequest {
    pub fn new(
        ordinal: u64,
        limit: usize,
        byte_limit: usize,
    ) -> Result<Self, StorageImportSessionError> {
        if ordinal == 0 {
            return Err(StorageImportSessionError::State);
        }
        Ok(Self {
            ordinal,
            cursor: StorageImportSessionDetailEnumCursor::Staged(0),
            bounds: StorageImportSessionPageRequest::new(0, limit, byte_limit)?,
        })
    }
    pub const fn ordinal(self) -> u64 {
        self.ordinal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageImportSessionEnumMatchSource {
    Staged { ordinal: u64 },
    Library(StorageImportSessionCandidate),
}

#[derive(Debug)]
pub struct StorageImportSessionRelationship {
    source: StorageImportSessionEnumMatchSource,
    relation: NotationGameIdentityEnumMatch,
    differences: StorageImportSessionDifferences,
}

impl StorageImportSessionRelationship {
    pub const fn source(&self) -> StorageImportSessionEnumMatchSource {
        self.source
    }
    pub const fn relation(&self) -> NotationGameIdentityEnumMatch {
        self.relation
    }
    pub fn differences(&self) -> &StorageImportSessionDifferences {
        &self.differences
    }
}

#[derive(Debug)]
pub struct StorageImportSessionDetailPage {
    relationships: Vec<StorageImportSessionRelationship>,
    next: Option<StorageImportSessionDetailRequest>,
}

impl StorageImportSessionDetailPage {
    pub fn relationships(&self) -> &[StorageImportSessionRelationship] {
        &self.relationships
    }
    pub const fn next_request(&self) -> Option<StorageImportSessionDetailRequest> {
        self.next
    }
}

#[derive(Debug)]
pub struct StorageImportSessionDifferences {
    tree: bool,
    selected_mainline: bool,
    notes: bool,
    nags: bool,
    markings: bool,
    metadata: bool,
    warnings: bool,
    source_clocks: bool,
}

impl StorageImportSessionDifferences {
    pub const fn tree(&self) -> bool {
        self.tree
    }
    pub const fn selected_mainline(&self) -> bool {
        self.selected_mainline
    }
    pub const fn notes(&self) -> bool {
        self.notes
    }
    pub const fn nags(&self) -> bool {
        self.nags
    }
    pub const fn markings(&self) -> bool {
        self.markings
    }
    pub const fn metadata(&self) -> bool {
        self.metadata
    }
    pub const fn warnings(&self) -> bool {
        self.warnings
    }
    pub const fn source_clocks(&self) -> bool {
        self.source_clocks
    }
    pub const fn has_changes(&self) -> bool {
        self.tree
            || self.selected_mainline
            || self.notes
            || self.nags
            || self.markings
            || self.metadata
            || self.warnings
            || self.source_clocks
    }

    pub(super) fn between(
        left: &StorageLibraryRecord,
        right: &StorageLibraryRecord,
    ) -> Result<Self, StorageImportSessionError> {
        let left_tree = StorageImportSessionDifferenceTree::new(left)?;
        let right_tree = StorageImportSessionDifferenceTree::new(right)?;
        let left_notation = left_tree.notation_fields(left)?;
        let right_notation = right_tree.notation_fields(right)?;
        let same_start = left.start().position() == right.start().position();
        let mut differences = Self {
            tree: !same_start
                || !left_tree
                    .nodes
                    .iter()
                    .map(|node| (&node.parent, node.child, &node.uci))
                    .eq(right_tree
                        .nodes
                        .iter()
                        .map(|node| (&node.parent, node.child, &node.uci))),
            selected_mainline: left_notation.mainline_uci != right_notation.mainline_uci,
            notes: false,
            nags: false,
            markings: false,
            metadata: left.metadata() != right.metadata()
                || left.result() != right.result()
                || left_notation.time_control != right_notation.time_control
                || left_notation.result_header != right_notation.result_header,
            warnings: left_tree.ratings != right_tree.ratings,
            source_clocks: false,
        };
        left_tree.compare_annotations(&right_tree, same_start, &mut differences)?;
        Ok(differences)
    }
}

struct StorageImportSessionDifferenceNotation<'a> {
    mainline_uci: Vec<String>,
    time_control: Option<&'a str>,
    result_header: Option<NotationGameIdentityEnumResult>,
}

struct StorageImportSessionDifferenceNode<'a> {
    parent: Option<usize>,
    child: usize,
    uci: Option<String>,
    node: &'a GameMoveNode,
    warnings: Vec<&'a str>,
    clock: Option<u64>,
}

struct StorageImportSessionDifferenceTree<'a> {
    tree: &'a GameTree,
    nodes: Vec<StorageImportSessionDifferenceNode<'a>>,
    ordinals: HashMap<GameMoveId, usize>,
    ratings: Vec<(BoardEnumColor, &'a str)>,
}

impl<'a> StorageImportSessionDifferenceTree<'a> {
    fn new(record: &'a StorageLibraryRecord) -> Result<Self, StorageImportSessionError> {
        let tree = match record.content() {
            StorageLibraryEnumContent::Played(session) => session.tree(),
            StorageLibraryEnumContent::Notation(game) => game.tree(),
        };
        let mut result = Self {
            tree,
            nodes: Vec::with_capacity(tree.len()),
            ordinals: HashMap::with_capacity(tree.len()),
            ratings: Vec::new(),
        };
        let mut pending = vec![(tree.root(), None, 0)];
        while let Some((id, parent, child)) = pending.pop() {
            let node = tree.node(id)?;
            let ordinal = result.nodes.len();
            if result.ordinals.insert(id, ordinal).is_some() {
                return Err(StorageImportSessionError::Proof);
            }
            result.nodes.push(StorageImportSessionDifferenceNode {
                parent,
                child,
                uci: node
                    .movement()
                    .map(|movement| movement.uci(record.start().position().variant())),
                node,
                warnings: Vec::new(),
                clock: match record.content() {
                    StorageLibraryEnumContent::Notation(game) => {
                        game.clock(id).map(|clock| clock.as_nanos())
                    }
                    StorageLibraryEnumContent::Played(_) => None,
                },
            });
            pending.extend(
                node.children()
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(child, id)| (*id, Some(ordinal), child)),
            );
        }
        if result.nodes.len() != tree.len() {
            return Err(StorageImportSessionError::Proof);
        }
        if let StorageLibraryEnumContent::Notation(game) = record.content() {
            for warning in game.warnings() {
                match warning {
                    NotationGameEnumWarning::UnknownDirective { node, directive } => {
                        let index = result.ordinal(*node)?;
                        result
                            .nodes
                            .get_mut(index)
                            .ok_or(StorageImportSessionError::Proof)?
                            .warnings
                            .push(directive);
                    }
                    NotationGameEnumWarning::UnavailableRating { color, value } => {
                        result.ratings.push((*color, value.as_str()));
                    }
                }
            }
        }
        Ok(result)
    }

    fn ordinal(&self, node: GameMoveId) -> Result<usize, StorageImportSessionError> {
        self.ordinals
            .get(&node)
            .copied()
            .ok_or(StorageImportSessionError::Proof)
    }

    fn notation_fields<'b>(
        &self,
        record: &'b StorageLibraryRecord,
    ) -> Result<StorageImportSessionDifferenceNotation<'b>, StorageImportSessionError> {
        let (tip, control, result) = match record.content() {
            StorageLibraryEnumContent::Notation(game) => (
                game.mainline_tip(),
                game.time_control().map(|text| text.as_str()),
                game.result_header(),
            ),
            StorageLibraryEnumContent::Played(session) => {
                (session.study_mainline_tip(), None, None)
            }
        };
        let mut line = Vec::new();
        for id in self.tree.path_to(tip)? {
            if let Some(movement) = self.tree.node(id)?.movement() {
                line.push(movement.uci(record.start().position().variant()));
            }
        }
        Ok(StorageImportSessionDifferenceNotation {
            mainline_uci: line,
            time_control: control,
            result_header: result,
        })
    }

    fn children(
        &self,
        parent: Option<usize>,
    ) -> Result<BTreeMap<&str, usize>, StorageImportSessionError> {
        let mut children = BTreeMap::new();
        if let Some(parent) = parent {
            let parent = self
                .nodes
                .get(parent)
                .ok_or(StorageImportSessionError::Proof)?;
            for id in parent.node.children() {
                let index = self.ordinal(*id)?;
                let uci = self
                    .nodes
                    .get(index)
                    .and_then(|node| node.uci.as_deref())
                    .ok_or(StorageImportSessionError::Proof)?;
                if children.insert(uci, index).is_some() {
                    return Err(StorageImportSessionError::Proof);
                }
            }
        }
        Ok(children)
    }

    fn compare_annotations(
        &self,
        other: &Self,
        same_start: bool,
        differences: &mut StorageImportSessionDifferences,
    ) -> Result<(), StorageImportSessionError> {
        let mut pending = if same_start {
            vec![(Some(0), Some(0))]
        } else {
            vec![(Some(0), None), (None, Some(0))]
        };
        while let Some((left_index, right_index)) = pending.pop() {
            let left = left_index
                .map(|index| {
                    self.nodes
                        .get(index)
                        .ok_or(StorageImportSessionError::Proof)
                })
                .transpose()?;
            let right = right_index
                .map(|index| {
                    other
                        .nodes
                        .get(index)
                        .ok_or(StorageImportSessionError::Proof)
                })
                .transpose()?;
            differences.notes |= left.map_or("", |node| node.node.annotations().notes())
                != right.map_or("", |node| node.node.annotations().notes());
            differences.nags |= left.map_or(&[][..], |node| node.node.annotations().nags())
                != right.map_or(&[][..], |node| node.node.annotations().nags());
            differences.markings |= left.map_or(&[][..], |node| node.node.annotations().markings())
                != right.map_or(&[][..], |node| node.node.annotations().markings());
            differences.warnings |= left.map_or(&[][..], |node| node.warnings.as_slice())
                != right.map_or(&[][..], |node| node.warnings.as_slice());
            differences.source_clocks |=
                left.and_then(|node| node.clock) != right.and_then(|node| node.clock);
            let left_children = self.children(left_index)?;
            let mut right_children = other.children(right_index)?;
            for (uci, left_child) in left_children {
                pending.push((Some(left_child), right_children.remove(uci)));
            }
            pending.extend(
                right_children
                    .into_values()
                    .map(|index| (None, Some(index))),
            );
        }
        Ok(())
    }
}

impl StorageImportSessionRelationship {
    fn between(
        record: &StorageLibraryRecord,
        identity: &NotationGameIdentity,
        other: &StorageLibraryRecord,
        source: StorageImportSessionEnumMatchSource,
    ) -> Result<Option<Self>, StorageImportSessionError> {
        let relation = identity.compare(&other.identity()?);
        if relation == NotationGameIdentityEnumMatch::Different {
            return Ok(None);
        }
        Ok(Some(Self {
            source,
            relation,
            differences: StorageImportSessionDifferences::between(record, other)?,
        }))
    }
}

impl StorageImportSessionStage {
    fn staged_details(
        &mut self,
        request: StorageImportSessionDetailRequest,
        token: &crate::capacity::executor::CapacityExecutorCancellation,
        cancellation: &crate::storage::import_session::model::StorageImportSessionCancellation,
    ) -> Result<StorageImportSessionDetailPage, StorageImportSessionError> {
        let StorageImportSessionDetailEnumCursor::Staged(cursor) = request.cursor else {
            return Err(StorageImportSessionError::State);
        };
        let record = self.disk.record(request.ordinal)?;
        let (begin, end) =
            StorageImportSessionIndex::bucket(&mut self.disk, record.structural_digest)?;
        let mut index = begin
            .checked_add(cursor)
            .ok_or(StorageImportSessionError::Arithmetic)?;
        if index > end {
            return Err(StorageImportSessionError::Proof);
        }
        let target = self.read_record(request.ordinal, token)?;
        let identity = target.identity()?;
        let mut relationships = Vec::new();
        while index < end {
            Self::check(token, cancellation)?;
            let entry = StorageImportSessionIndex::entry(
                &mut self.disk,
                StorageImportSessionDiskEnumFile::StructuralIndex,
                index,
            )?;
            if entry.ordinal < request.ordinal {
                let other = self.read_record(entry.ordinal, token)?;
                if let Some(relationship) = StorageImportSessionRelationship::between(
                    &target,
                    &identity,
                    &other,
                    StorageImportSessionEnumMatchSource::Staged {
                        ordinal: entry.ordinal,
                    },
                )? {
                    relationships.push(relationship);
                }
            }
            index = index
                .checked_add(1)
                .ok_or(StorageImportSessionError::Arithmetic)?;
            if Self::detail_full(request, relationships.len())? {
                break;
            }
        }
        let cursor = if index < end {
            StorageImportSessionDetailEnumCursor::Staged(index - begin)
        } else {
            StorageImportSessionDetailEnumCursor::Library {
                page: StorageImportSessionCandidateCursor::start(),
                offset: 0,
            }
        };
        Ok(StorageImportSessionDetailPage {
            relationships,
            next: Some(StorageImportSessionDetailRequest { cursor, ..request }),
        })
    }

    fn detail_full(
        request: StorageImportSessionDetailRequest,
        count: usize,
    ) -> Result<bool, StorageImportSessionError> {
        let next = count
            .checked_add(1)
            .and_then(|value| {
                value.checked_mul(std::mem::size_of::<StorageImportSessionRelationship>())
            })
            .ok_or(StorageImportSessionError::Arithmetic)?;
        if count == 0 && next > request.bounds.byte_limit() {
            return Err(StorageImportSessionError::PageBudget);
        }
        Ok(count >= request.bounds.limit() || next > request.bounds.byte_limit())
    }

    fn candidate_detail(
        &mut self,
        ordinal: u64,
        body: StorageImportSessionCandidateBody,
        token: &crate::capacity::executor::CapacityExecutorCancellation,
    ) -> Result<Option<StorageImportSessionRelationship>, StorageImportSessionError> {
        let candidate = StorageGameDocument::recover_record_cancellable(body.body(), token)?;
        if candidate.id() != body.candidate().game() {
            return Err(StorageImportSessionRepositoryError::InvalidResponse.into());
        }
        let target = self.read_record(ordinal, token)?;
        StorageImportSessionRelationship::between(
            &target,
            &target.identity()?,
            &candidate,
            StorageImportSessionEnumMatchSource::Library(body.candidate()),
        )
    }
}

impl<
    R: StorageImportSessionRepository,
    F: StorageImportSessionFilesystemProvider,
    I: StorageImportSessionIdentityProvider,
> StorageImportSessionRuntime<R, F, I>
{
    pub(super) async fn detail_page(
        &self,
        request: StorageImportSessionDetailRequest,
    ) -> Result<StorageImportSessionDetailPage, StorageImportSessionError> {
        let (snapshot, key) = self
            .worker(move |work, token, cancellation| {
                work.require_ready()?;
                StorageImportSessionStage::check(token, cancellation)?;
                let record = work.stage.disk.record(request.ordinal)?;
                if record.game.is_none() {
                    return Err(StorageImportSessionError::State);
                }
                Ok((
                    work.stage
                        .snapshot
                        .ok_or(StorageImportSessionError::State)?,
                    Arc::new(work.stage.read_record(request.ordinal, token)?.identity()?),
                ))
            })
            .await?;
        if self.read(self.resources.repository.snapshot()).await? != snapshot {
            return Err(StorageImportSessionRepositoryError::StaleCatalog.into());
        }
        let mut result = match request.cursor {
            StorageImportSessionDetailEnumCursor::Staged(_) => {
                self.worker(move |work, token, cancellation| {
                    work.stage.staged_details(request, token, cancellation)
                })
                .await?
            }
            StorageImportSessionDetailEnumCursor::Library { .. } => {
                StorageImportSessionDetailPage {
                    relationships: Vec::new(),
                    next: Some(request),
                }
            }
        };
        while !StorageImportSessionStage::detail_full(request, result.relationships.len())? {
            let Some(next) = result.next else {
                break;
            };
            let StorageImportSessionDetailEnumCursor::Library {
                page: cursor,
                offset,
            } = next.cursor
            else {
                break;
            };
            self.cancellation.check()?;
            let query = StorageImportSessionCandidateQuery::new(
                snapshot.catalog_stamp(),
                Arc::clone(&key),
                cursor,
            )?;
            let page = self
                .read(self.resources.repository.candidates(query.clone()))
                .await?;
            page.validate(&query)?;
            if offset > page.candidates().len() {
                return Err(StorageImportSessionRepositoryError::InvalidResponse.into());
            }
            for (index, candidate) in page.candidates().iter().enumerate().skip(offset) {
                self.cancellation.check()?;
                let body = self
                    .read(self.resources.repository.read_candidate(*candidate))
                    .await?;
                if body.candidate() != *candidate {
                    return Err(StorageImportSessionRepositoryError::InvalidResponse.into());
                }
                if let Some(relationship) = self
                    .worker(move |work, token, cancellation| {
                        StorageImportSessionStage::check(token, cancellation)?;
                        work.stage.candidate_detail(request.ordinal, body, token)
                    })
                    .await?
                {
                    result.relationships.push(relationship);
                }
                let offset = index
                    .checked_add(1)
                    .ok_or(StorageImportSessionError::Arithmetic)?;
                result.next = Some(StorageImportSessionDetailRequest {
                    cursor: StorageImportSessionDetailEnumCursor::Library {
                        page: cursor,
                        offset,
                    },
                    ..request
                });
                if StorageImportSessionStage::detail_full(request, result.relationships.len())? {
                    break;
                }
            }
            if StorageImportSessionStage::detail_full(request, result.relationships.len())? {
                break;
            }
            result.next = page
                .next_cursor()
                .map(|page| StorageImportSessionDetailRequest {
                    cursor: StorageImportSessionDetailEnumCursor::Library { page, offset: 0 },
                    ..request
                });
        }
        if self.read(self.resources.repository.snapshot()).await? != snapshot {
            return Err(StorageImportSessionRepositoryError::StaleCatalog.into());
        }
        Ok(result)
    }
}
