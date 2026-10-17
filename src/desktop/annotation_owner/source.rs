use std::sync::Arc;

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumResult,
    DesktopAnnotationCommandsEnumScope, DesktopAnnotationCommandsScopePage,
    DesktopAnnotationCommandsVariationPreview,
};
use crate::desktop::annotation_owner::{
    DesktopAnnotationOwnerError, DesktopAnnotationOwnerOpenError,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorContext, DesktopGameEditorEpoch,
    DesktopGameEditorGeneration, DesktopGameEditorPublication,
};
use crate::desktop::identity::DesktopRequestId;
use crate::game::annotations::{
    GameAnnotationsEnumChange, GameAnnotationsVariationApplied, GameAnnotationsVariationPreview,
};
use crate::game::enums::{GameMoveId, GameRevision};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::storage_repository::{
    GameStorageContentEdit, GameStorageEnumEditCause, GameStorageRevision,
    GameStorageWallTimeProvider,
};
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};

struct DesktopAnnotationOwnerSourceProof {
    scope: DesktopAnnotationCommandsVariationPreview,
    proof: GameAnnotationsVariationPreview,
}

struct DesktopAnnotationOwnerSourceEdit {
    revision: GameRevision,
    generation: DesktopGameEditorGeneration,
    timestamp: GameLibraryOrganizationTimestamp,
}

#[derive(Clone, Copy)]
enum DesktopAnnotationOwnerSourceEnumPublication {
    Snapshot,
    Edited(GameAnnotationsEnumChange),
}

pub(super) struct DesktopAnnotationOwnerSource {
    record: Arc<StorageLibraryRecord>,
    storage_revision: GameStorageRevision,
    epoch: DesktopGameEditorEpoch,
    revision: GameRevision,
    generation: DesktopGameEditorGeneration,
    selected: GameMoveId,
    accepted: Option<GameStorageContentEdit>,
    client: StorageDocumentClient,
    snapshot: Option<DesktopGameEditorPublication>,
    publication: Option<DesktopAnnotationOwnerSourceEnumPublication>,
    proof: Option<DesktopAnnotationOwnerSourceProof>,
}

impl DesktopAnnotationOwnerSource {
    pub(super) fn new(
        record: StorageLibraryRecord,
        storage_revision: GameStorageRevision,
        receipt: &DesktopApplicationReceipt,
        client: StorageDocumentClient,
    ) -> Result<Self, DesktopAnnotationOwnerOpenError> {
        if receipt.is_cancellation_requested() {
            return Err(DesktopAnnotationOwnerOpenError::new(
                record,
                DesktopAnnotationOwnerError::Cancelled,
            ));
        }
        let selected = match record.content() {
            StorageLibraryEnumContent::Notation(game) => game.tree().root(),
            StorageLibraryEnumContent::Played(_) => {
                return Err(DesktopAnnotationOwnerOpenError::new(
                    record,
                    crate::desktop::game_editor::DesktopGameEditorError::ExpectedNotation.into(),
                ));
            }
        };
        Ok(Self {
            record: Arc::new(record),
            storage_revision,
            epoch: DesktopGameEditorEpoch::from_request_id(receipt.request_id()),
            revision: GameRevision::ZERO,
            generation: DesktopGameEditorGeneration::ZERO,
            selected,
            accepted: None,
            client,
            snapshot: None,
            publication: None,
            proof: None,
        })
    }

    fn editor(&self) -> Result<DesktopGameEditor<'_>, DesktopAnnotationOwnerError> {
        self.editor_at(self.storage_revision)
    }

    fn editor_at(
        &self,
        storage_revision: GameStorageRevision,
    ) -> Result<DesktopGameEditor<'_>, DesktopAnnotationOwnerError> {
        Ok(DesktopGameEditor::notation(
            &self.record,
            self.revision,
            self.selected,
            DesktopGameEditorBinding::new(storage_revision, self.epoch, self.generation),
        )?)
    }

    pub(super) fn context(&self) -> Result<DesktopGameEditorContext, DesktopAnnotationOwnerError> {
        Ok(self.editor()?.context())
    }

    pub(super) fn snapshot(
        &self,
    ) -> Result<&DesktopGameEditorPublication, DesktopAnnotationOwnerError> {
        self.snapshot
            .as_ref()
            .ok_or(DesktopAnnotationOwnerError::NotReady)
    }

    pub(super) fn cached_snapshot(&self) -> Option<&DesktopGameEditorPublication> {
        self.snapshot.as_ref()
    }

    pub(super) fn publication_context(
        &self,
    ) -> Result<Option<DesktopGameEditorContext>, DesktopAnnotationOwnerError> {
        self.publication
            .map(|_publication| self.context())
            .transpose()
    }

    pub(super) const fn accepted(&self) -> Option<GameStorageContentEdit> {
        self.accepted
    }

    pub(super) fn record_for_preparation(&self) -> Arc<StorageLibraryRecord> {
        Arc::clone(&self.record)
    }

    pub(super) const fn revision(&self) -> GameRevision {
        self.revision
    }

    pub(super) const fn storage_revision(&self) -> GameStorageRevision {
        self.storage_revision
    }

    pub(super) fn saved(
        &mut self,
        revision: GameStorageRevision,
    ) -> Result<(), DesktopAnnotationOwnerError> {
        let editor = self.editor_at(revision)?;
        let snapshot = editor.publish(editor.context(), &self.client)?;
        self.storage_revision = revision;
        self.snapshot = Some(snapshot);
        Ok(())
    }

    fn refresh(&mut self) -> Result<(), DesktopAnnotationOwnerError> {
        let editor = self.editor()?;
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.context() == editor.context())
        {
            return Ok(());
        }
        let snapshot = editor.publish(editor.context(), &self.client)?;
        self.snapshot = Some(snapshot);
        Ok(())
    }

    pub(super) fn publish_snapshot(
        &mut self,
    ) -> Result<DesktopAnnotationCommandsEnumResult, DesktopAnnotationOwnerError> {
        self.publish(DesktopAnnotationOwnerSourceEnumPublication::Snapshot)
    }

    fn publish(
        &mut self,
        publication: DesktopAnnotationOwnerSourceEnumPublication,
    ) -> Result<DesktopAnnotationCommandsEnumResult, DesktopAnnotationOwnerError> {
        self.publication = Some(publication);
        self.retry_publication()
    }

    pub(super) fn retry_publication(
        &mut self,
    ) -> Result<DesktopAnnotationCommandsEnumResult, DesktopAnnotationOwnerError> {
        let publication = self
            .publication
            .ok_or(DesktopAnnotationOwnerError::Protocol)?;
        self.refresh()?;
        let snapshot = self.snapshot()?.clone();
        let result = match publication {
            DesktopAnnotationOwnerSourceEnumPublication::Snapshot => {
                DesktopAnnotationCommandsEnumResult::Snapshot(snapshot)
            }
            DesktopAnnotationOwnerSourceEnumPublication::Edited(change) => {
                DesktopAnnotationCommandsEnumResult::Edited { change, snapshot }
            }
        };
        self.publication = None;
        Ok(result)
    }

    fn notation(
        &self,
    ) -> Result<&crate::notation::game::NotationGame, DesktopAnnotationOwnerError> {
        match self.record.content() {
            StorageLibraryEnumContent::Notation(game) => Ok(game),
            StorageLibraryEnumContent::Played(_) => Err(DesktopAnnotationOwnerError::Protocol),
        }
    }

    fn notation_mut(
        &mut self,
    ) -> Result<&mut crate::notation::game::NotationGame, DesktopAnnotationOwnerError> {
        Arc::get_mut(&mut self.record)
            .ok_or(DesktopAnnotationOwnerError::SaveBorrowed)?
            .notation_mut()
            .ok_or(DesktopAnnotationOwnerError::Protocol)
    }

    pub(super) fn check_action(
        &self,
        action: &DesktopAnnotationCommandsEnumAction,
    ) -> Result<(), DesktopAnnotationOwnerError> {
        if action.edits_content() && Arc::strong_count(&self.record) != 1 {
            return Err(DesktopAnnotationOwnerError::SaveBorrowed);
        }
        match action {
            DesktopAnnotationCommandsEnumAction::ConfirmVariation { token }
            | DesktopAnnotationCommandsEnumAction::VariationScope { token, .. } => {
                self.checked_proof(*token)?;
            }
            DesktopAnnotationCommandsEnumAction::Snapshot
            | DesktopAnnotationCommandsEnumAction::TreePage { .. }
            | DesktopAnnotationCommandsEnumAction::ChildrenPage { .. }
            | DesktopAnnotationCommandsEnumAction::Navigate { .. }
            | DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { .. }
            | DesktopAnnotationCommandsEnumAction::SaveMarkings { .. }
            | DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
            | DesktopAnnotationCommandsEnumAction::PreviewVariation(_)
            | DesktopAnnotationCommandsEnumAction::Checkpoint
            | DesktopAnnotationCommandsEnumAction::RetrySave => {}
        }
        Ok(())
    }

    fn checked_proof(
        &self,
        token: DesktopRequestId,
    ) -> Result<&DesktopAnnotationOwnerSourceProof, DesktopAnnotationOwnerError> {
        let proof = self
            .proof
            .as_ref()
            .ok_or(DesktopAnnotationOwnerError::VariationToken)?;
        if proof.scope.token() != token {
            return Err(DesktopAnnotationOwnerError::VariationToken);
        }
        self.context()?.check_expected(proof.scope.context())?;
        Ok(proof)
    }

    fn preflight(
        &self,
        wall_time: &dyn GameStorageWallTimeProvider,
    ) -> Result<DesktopAnnotationOwnerSourceEdit, DesktopAnnotationOwnerError> {
        Ok(DesktopAnnotationOwnerSourceEdit {
            revision: self.revision.next()?,
            generation: self.generation.next()?,
            timestamp: wall_time.now()?,
        })
    }

    fn accept(
        &mut self,
        edit: DesktopAnnotationOwnerSourceEdit,
        cause: GameStorageEnumEditCause,
        change: GameAnnotationsEnumChange,
    ) {
        match change {
            GameAnnotationsEnumChange::Changed => {
                self.revision = edit.revision;
                self.generation = edit.generation;
                self.accepted = Some(GameStorageContentEdit::from_accepted_revision(
                    self.revision,
                    cause,
                    edit.timestamp,
                ));
            }
            GameAnnotationsEnumChange::Unchanged => {}
        }
    }

    fn accept_variation(
        &mut self,
        edit: DesktopAnnotationOwnerSourceEdit,
        applied: GameAnnotationsVariationApplied,
    ) -> Result<DesktopAnnotationCommandsEnumResult, DesktopAnnotationOwnerError> {
        if applied.change() == GameAnnotationsEnumChange::Unchanged
            && self.selected != applied.view()
        {
            self.generation = edit.generation;
        }
        self.selected = applied.view();
        self.accept(edit, GameStorageEnumEditCause::StudyTree, applied.change());
        self.edited(applied.change())
    }

    fn edited(
        &mut self,
        change: GameAnnotationsEnumChange,
    ) -> Result<DesktopAnnotationCommandsEnumResult, DesktopAnnotationOwnerError> {
        self.publish(DesktopAnnotationOwnerSourceEnumPublication::Edited(change))
    }

    pub(super) fn execute(
        &mut self,
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
        token: DesktopRequestId,
        wall_time: &dyn GameStorageWallTimeProvider,
    ) -> Result<DesktopAnnotationCommandsEnumResult, DesktopAnnotationOwnerError> {
        self.context()?.check_expected(expected)?;
        self.check_action(&action)?;
        match action {
            DesktopAnnotationCommandsEnumAction::Snapshot => self.publish_snapshot(),
            DesktopAnnotationCommandsEnumAction::TreePage { cursor } => {
                Ok(DesktopAnnotationCommandsEnumResult::Tree(
                    self.editor()?.tree_page(expected, cursor)?,
                ))
            }
            DesktopAnnotationCommandsEnumAction::ChildrenPage { parent, cursor } => {
                Ok(DesktopAnnotationCommandsEnumResult::Children(
                    self.editor()?.children_page(expected, parent, cursor)?,
                ))
            }
            DesktopAnnotationCommandsEnumAction::Navigate { selected } => {
                self.notation()?
                    .tree()
                    .node(selected)
                    .map_err(crate::notation::game::NotationGameError::from)?;
                if self.selected != selected {
                    let generation = self.generation.next()?;
                    self.selected = selected;
                    self.generation = generation;
                }
                self.publish_snapshot()
            }
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { notes, nags } => {
                let edit = self.preflight(wall_time)?;
                let selected = self.selected;
                let change = self
                    .notation_mut()?
                    .save_notes_and_nags(selected, notes, nags)?;
                self.accept(edit, GameStorageEnumEditCause::Annotation, change);
                self.edited(change)
            }
            DesktopAnnotationCommandsEnumAction::SaveMarkings { markings } => {
                let edit = self.preflight(wall_time)?;
                let selected = self.selected;
                let change = self.notation_mut()?.save_markings(selected, markings)?;
                self.accept(edit, GameStorageEnumEditCause::Annotation, change);
                self.edited(change)
            }
            DesktopAnnotationCommandsEnumAction::AppendStudyBranch { node, movement } => {
                let edit = self.preflight(wall_time)?;
                let selected = self.selected;
                let applied = self
                    .notation_mut()?
                    .append_study_variation(selected, node, movement)?;
                self.accept_variation(edit, applied)
            }
            DesktopAnnotationCommandsEnumAction::PreviewVariation(request) => {
                let proof = self.notation()?.preview_variation(request)?;
                let scope =
                    DesktopAnnotationCommandsVariationPreview::capture(expected, token, &proof);
                self.proof = Some(DesktopAnnotationOwnerSourceProof {
                    scope: scope.clone(),
                    proof,
                });
                Ok(DesktopAnnotationCommandsEnumResult::Variation(scope))
            }
            DesktopAnnotationCommandsEnumAction::VariationScope {
                token,
                scope,
                offset,
            } => {
                let retained = self.checked_proof(token)?;
                let nodes = match scope {
                    DesktopAnnotationCommandsEnumScope::PreviousPath => {
                        retained.proof.previous_path()
                    }
                    DesktopAnnotationCommandsEnumScope::SelectedPath => retained
                        .proof
                        .selected_path()
                        .ok_or(DesktopAnnotationOwnerError::ScopeOffset)?,
                    DesktopAnnotationCommandsEnumScope::DeletedNodes => {
                        retained.proof.deleted_nodes()
                    }
                };
                let page = DesktopAnnotationCommandsScopePage::capture(
                    expected, token, scope, nodes, offset,
                )
                .ok_or(DesktopAnnotationOwnerError::ScopeOffset)?;
                Ok(DesktopAnnotationCommandsEnumResult::Scope(page))
            }
            DesktopAnnotationCommandsEnumAction::ConfirmVariation { token } => {
                self.checked_proof(token)?;
                let edit = self.preflight(wall_time)?;
                let retained = self
                    .proof
                    .take()
                    .ok_or(DesktopAnnotationOwnerError::VariationToken)?;
                let selected = self.selected;
                let applied = self
                    .notation_mut()?
                    .apply_variation(retained.proof.confirm(), selected)?;
                self.accept_variation(edit, applied)
            }
            DesktopAnnotationCommandsEnumAction::Checkpoint
            | DesktopAnnotationCommandsEnumAction::RetrySave => {
                Err(DesktopAnnotationOwnerError::Protocol)
            }
        }
    }
}
