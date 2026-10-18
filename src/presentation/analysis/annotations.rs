mod service;

use bevy_egui::egui::{self, Ui};

use crate::board::r#move::BoardMove;
use crate::board::square::BoardSquare;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumPending,
    DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::annotation_owner::DesktopAnnotationOwnerEnumSaveProgress;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::error::DesktopFailure;
use crate::desktop::game_editor::{DesktopGameEditorContext, DesktopGameEditorPublication};
use crate::desktop::identity::DesktopRequestId;
use crate::game::enums::{GameEnumMarkingColor, GameMoveId};
use crate::game::move_node::{GameMoveNodeMarking, GameMoveNodeNag};
use crate::presentation::analysis::editor_intent::{
    PresentationAnalysisEditorError, PresentationAnalysisEditorFrame,
    PresentationAnalysisEditorHistory, PresentationAnalysisEditorIntent,
    PresentationAnalysisEditorModal, PresentationAnalysisEditorPending,
    PresentationAnalysisEditorServiceCompletion,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationAnalysisAnnotationsEnumDestination {
    Navigate(GameMoveId),
    AppendStudyBranch {
        node: GameMoveId,
        movement: BoardMove,
    },
    Leave(DesktopRequestId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationAnalysisAnnotationsEnumGuardChoice {
    Save,
    Discard,
    Stay,
}

#[derive(Debug, Clone)]
pub enum PresentationAnalysisAnnotationsEnumEffect {
    Command(PresentationAnalysisEditorIntent),
    Leave {
        request_id: DesktopRequestId,
        expected: DesktopGameEditorContext,
        destination: DesktopRequestId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationAnalysisAnnotationsEnumMarkingInput {
    Toggle(GameMoveNodeMarking),
    ClearTemporary,
    ClearSaved,
}

pub struct PresentationAnalysisAnnotationsMarkings<'a> {
    context: DesktopGameEditorContext,
    saved: &'a [GameMoveNodeMarking],
    temporary: &'a [GameMoveNodeMarking],
}

impl<'a> PresentationAnalysisAnnotationsMarkings<'a> {
    pub const fn context(&self) -> DesktopGameEditorContext {
        self.context
    }

    pub const fn saved(&self) -> &'a [GameMoveNodeMarking] {
        self.saved
    }

    pub const fn temporary(&self) -> &'a [GameMoveNodeMarking] {
        self.temporary
    }
}

struct PresentationAnalysisAnnotationsDraft {
    baseline: DesktopGameEditorPublication,
    notes: String,
    nags: Vec<GameMoveNodeNag>,
    version: PresentationAnalysisAnnotationsDraftVersion,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct PresentationAnalysisAnnotationsDraftVersion(u64);

impl PresentationAnalysisAnnotationsDraftVersion {
    fn advance(&mut self) -> Result<(), PresentationAnalysisEditorError> {
        self.0 = self
            .0
            .checked_add(1)
            .ok_or(PresentationAnalysisEditorError::DraftVersionExhausted)?;
        Ok(())
    }
}

impl PresentationAnalysisAnnotationsDraft {
    fn capture(publication: &DesktopGameEditorPublication) -> Self {
        Self {
            baseline: publication.clone(),
            notes: publication.annotations().notes().to_owned(),
            nags: publication.annotations().nags().to_vec(),
            version: PresentationAnalysisAnnotationsDraftVersion::default(),
        }
    }

    fn is_changed(&self) -> bool {
        self.notes != self.baseline.annotations().notes()
            || self.nags != self.baseline.annotations().nags()
    }
}

struct PresentationAnalysisAnnotationsSave {
    request: PresentationAnalysisEditorPending,
    accepted: Option<DesktopGameEditorPublication>,
    checkpoint: Option<DesktopApplicationReceipt>,
    checkpoint_intent: Option<PresentationAnalysisEditorIntent>,
    draft_version: PresentationAnalysisAnnotationsDraftVersion,
}

impl PresentationAnalysisAnnotationsSave {
    fn matches_publication(&self, snapshot: &DesktopGameEditorPublication) -> bool {
        match self.request.intent().action() {
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { notes, nags } => {
                snapshot.annotations().notes() == notes && snapshot.annotations().nags() == nags
            }
            DesktopAnnotationCommandsEnumAction::SaveMarkings { markings } => {
                snapshot.annotations().markings() == markings
            }
            DesktopAnnotationCommandsEnumAction::Snapshot
            | DesktopAnnotationCommandsEnumAction::TreePage { .. }
            | DesktopAnnotationCommandsEnumAction::ChildrenPage { .. }
            | DesktopAnnotationCommandsEnumAction::Navigate { .. }
            | DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
            | DesktopAnnotationCommandsEnumAction::PreviewVariation(_)
            | DesktopAnnotationCommandsEnumAction::VariationScope { .. }
            | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. }
            | DesktopAnnotationCommandsEnumAction::Checkpoint
            | DesktopAnnotationCommandsEnumAction::RetrySave => false,
        }
    }

    fn matches_draft(&self, draft: &PresentationAnalysisAnnotationsDraft) -> bool {
        draft.version == self.draft_version
            && match self.request.intent().action() {
                DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { notes, nags } => {
                    draft.notes == *notes && draft.nags == *nags
                }
                DesktopAnnotationCommandsEnumAction::SaveMarkings { .. } => !draft.is_changed(),
                DesktopAnnotationCommandsEnumAction::Snapshot
                | DesktopAnnotationCommandsEnumAction::TreePage { .. }
                | DesktopAnnotationCommandsEnumAction::ChildrenPage { .. }
                | DesktopAnnotationCommandsEnumAction::Navigate { .. }
                | DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
                | DesktopAnnotationCommandsEnumAction::PreviewVariation(_)
                | DesktopAnnotationCommandsEnumAction::VariationScope { .. }
                | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. }
                | DesktopAnnotationCommandsEnumAction::Checkpoint
                | DesktopAnnotationCommandsEnumAction::RetrySave => false,
            }
    }
}

#[derive(Clone, Copy)]
struct PresentationAnalysisAnnotationsSwitch {
    request_id: DesktopRequestId,
    expected: DesktopGameEditorContext,
    destination: PresentationAnalysisAnnotationsEnumDestination,
}

#[derive(Default)]
pub struct PresentationAnalysisAnnotations {
    publication: Option<DesktopGameEditorPublication>,
    draft: Option<PresentationAnalysisAnnotationsDraft>,
    source_pending: Option<DesktopAnnotationCommandsEnumPending>,
    save: Option<PresentationAnalysisAnnotationsSave>,
    guard: Option<PresentationAnalysisAnnotationsSwitch>,
    history: PresentationAnalysisEditorHistory,
    temporary: Vec<GameMoveNodeMarking>,
    hide_saved: bool,
    is_conflicted: bool,
    modal: PresentationAnalysisEditorModal,
    frame: PresentationAnalysisEditorFrame,
    nag_value: u8,
    marking_origin: String,
    marking_destination: String,
    marking_color: Option<GameEnumMarkingColor>,
    feedback: Option<String>,
}

impl PresentationAnalysisAnnotations {
    const DEFAULT_MARKING_COLOR: GameEnumMarkingColor = GameEnumMarkingColor::Green;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn context(&self) -> Option<DesktopGameEditorContext> {
        self.draft.as_ref().map(|draft| draft.baseline.context())
    }

    pub fn notes(&self) -> Option<&str> {
        self.draft.as_ref().map(|draft| draft.notes.as_str())
    }

    pub fn nags(&self) -> Option<&[GameMoveNodeNag]> {
        self.draft.as_ref().map(|draft| draft.nags.as_slice())
    }

    pub fn has_notes_changes(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(PresentationAnalysisAnnotationsDraft::is_changed)
    }

    pub const fn has_guard(&self) -> bool {
        self.guard.is_some()
    }

    pub const fn is_conflicted(&self) -> bool {
        self.is_conflicted
    }

    pub fn pending_intent(&self) -> Option<&PresentationAnalysisEditorIntent> {
        self.save.as_ref().map(|save| save.request.active_intent())
    }

    pub fn failure(&self) -> Option<&DesktopFailure> {
        self.save.as_ref().and_then(|save| save.request.failure())
    }

    pub fn synchronize(
        &mut self,
        publication: &DesktopGameEditorPublication,
        pending: Option<DesktopAnnotationCommandsEnumPending>,
    ) -> bool {
        if let Some(save) = &mut self.save {
            save.request.set_service_pending(None);
        }
        self.source_pending = pending;
        let changed = self
            .publication
            .as_ref()
            .is_none_or(|previous| previous.context() != publication.context());
        if !changed {
            return false;
        }
        self.clear_temporary();
        let is_own_edit = self
            .save
            .as_ref()
            .and_then(|save| save.accepted.as_ref())
            .is_some_and(|accepted| accepted.context() == publication.context());
        self.publication = Some(publication.clone());
        if self.draft.is_none()
            || (!self.has_notes_changes() && self.save.is_none() && self.guard.is_none())
        {
            self.draft = Some(PresentationAnalysisAnnotationsDraft::capture(publication));
            self.is_conflicted = false;
        } else if self.context() != Some(publication.context()) && !is_own_edit {
            self.is_conflicted = true;
            self.feedback = Some("The game changed. Your notes are still here. Discard them to use the current position.".to_owned());
        }
        true
    }

    pub fn set_notes(
        &mut self,
        expected: DesktopGameEditorContext,
        notes: String,
    ) -> Result<(), PresentationAnalysisEditorError> {
        self.check_draft(expected)?;
        let draft = self
            .draft
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::NoSelection)?;
        if draft.notes != notes {
            draft.version.advance()?;
            draft.notes = notes;
        }
        Ok(())
    }

    pub fn set_nags(
        &mut self,
        expected: DesktopGameEditorContext,
        nags: Vec<GameMoveNodeNag>,
    ) -> Result<(), PresentationAnalysisEditorError> {
        self.check_draft(expected)?;
        let draft = self
            .draft
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::NoSelection)?;
        if draft.nags != nags {
            draft.version.advance()?;
            draft.nags = nags;
        }
        Ok(())
    }

    pub fn markings(&self) -> Option<PresentationAnalysisAnnotationsMarkings<'_>> {
        let publication = self.publication.as_ref()?;
        Some(PresentationAnalysisAnnotationsMarkings {
            context: publication.context(),
            saved: if self.hide_saved {
                &[]
            } else {
                publication.annotations().markings()
            },
            temporary: &self.temporary,
        })
    }

    pub fn marking_input(
        &mut self,
        expected: DesktopGameEditorContext,
        input: PresentationAnalysisAnnotationsEnumMarkingInput,
    ) -> Result<(), PresentationAnalysisEditorError> {
        if self.save.is_some() {
            return Err(PresentationAnalysisEditorError::Busy);
        }
        if self.source_pending.is_some() {
            return Err(PresentationAnalysisEditorError::SourcePending);
        }
        if self
            .publication
            .as_ref()
            .map(DesktopGameEditorPublication::context)
            != Some(expected)
        {
            return Err(PresentationAnalysisEditorError::Conflict);
        }
        match input {
            PresentationAnalysisAnnotationsEnumMarkingInput::Toggle(marking) => {
                if let Some(index) = self
                    .temporary
                    .iter()
                    .position(|existing| *existing == marking)
                {
                    self.temporary.remove(index);
                } else {
                    self.temporary.push(marking);
                }
            }
            PresentationAnalysisAnnotationsEnumMarkingInput::ClearTemporary => {
                self.temporary.clear()
            }
            PresentationAnalysisAnnotationsEnumMarkingInput::ClearSaved => self.hide_saved = true,
        }
        Ok(())
    }

    pub fn save_notes(
        &mut self,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        let expected = self.ready()?;
        if !self.has_notes_changes() {
            return Err(PresentationAnalysisEditorError::NoChanges);
        }
        let draft = self
            .draft
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::NoSelection)?;
        let action = DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: draft.notes.clone(),
            nags: draft.nags.clone(),
        };
        self.begin_save(request_id, expected, action)
    }

    pub fn save_markings(
        &mut self,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        let expected = self.ready()?;
        let marking_view = self
            .markings()
            .ok_or(PresentationAnalysisEditorError::NoSelection)?;
        if marking_view.temporary().is_empty() && !self.hide_saved {
            return Err(PresentationAnalysisEditorError::NoChanges);
        }
        let mut markings = marking_view.saved().to_vec();
        for marking in marking_view.temporary() {
            if !markings.contains(marking) {
                markings.push(*marking);
            }
        }
        self.begin_save(
            request_id,
            expected,
            DesktopAnnotationCommandsEnumAction::SaveMarkings { markings },
        )
    }

    pub fn request_switch(
        &mut self,
        request_id: DesktopRequestId,
        destination: PresentationAnalysisAnnotationsEnumDestination,
    ) -> Result<Option<PresentationAnalysisAnnotationsEnumEffect>, PresentationAnalysisEditorError>
    {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        if let Some(guard) = self.guard {
            return if guard.request_id == request_id && guard.destination == destination {
                Ok(None)
            } else {
                Err(PresentationAnalysisEditorError::Busy)
            };
        }
        if self.has_notes_changes() || self.is_conflicted {
            self.guard = Some(PresentationAnalysisAnnotationsSwitch {
                request_id,
                expected: self
                    .context()
                    .ok_or(PresentationAnalysisEditorError::NoSelection)?,
                destination,
            });
            self.modal.open(None);
            return Ok(None);
        }
        let expected = self.ready()?;
        self.release_switch(
            PresentationAnalysisAnnotationsSwitch {
                request_id,
                expected,
                destination,
            },
            expected,
        )
        .map(Some)
    }

    pub fn resolve_guard(
        &mut self,
        choice: PresentationAnalysisAnnotationsEnumGuardChoice,
        save_request: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisAnnotationsEnumEffect>, PresentationAnalysisEditorError>
    {
        let switch = self
            .guard
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        if self.save.is_some() && choice != PresentationAnalysisAnnotationsEnumGuardChoice::Stay {
            return Err(PresentationAnalysisEditorError::Busy);
        }
        match choice {
            PresentationAnalysisAnnotationsEnumGuardChoice::Stay => {
                self.guard = None;
                Ok(None)
            }
            PresentationAnalysisAnnotationsEnumGuardChoice::Discard => {
                if self.source_pending.is_some() {
                    return Err(PresentationAnalysisEditorError::SourcePending);
                }
                let publication = self
                    .publication
                    .as_ref()
                    .ok_or(PresentationAnalysisEditorError::NoSelection)?;
                let expected = publication.context();
                self.draft = Some(PresentationAnalysisAnnotationsDraft::capture(publication));
                self.is_conflicted = false;
                self.guard = None;
                if switch.expected != expected {
                    self.history.complete(switch.request_id);
                    self.feedback = Some(
                        "The game changed. Choose the action again for this position.".to_owned(),
                    );
                    return Ok(None);
                }
                self.release_switch(switch, expected).map(Some)
            }
            PresentationAnalysisAnnotationsEnumGuardChoice::Save => self
                .save_notes(save_request)
                .map(|intent| intent.map(PresentationAnalysisAnnotationsEnumEffect::Command)),
        }
    }

    pub fn attach_receipt(
        &mut self,
        intent: &PresentationAnalysisEditorIntent,
        receipt: DesktopApplicationReceipt,
    ) -> Result<(), PresentationAnalysisEditorError> {
        self.save
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?
            .request
            .attach(intent, receipt)
    }

    pub fn observe_result(
        &mut self,
        receipt: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
        result: &DesktopAnnotationCommandsEnumResult,
    ) -> Result<(), PresentationAnalysisEditorError> {
        let save = self
            .save
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        save.request.check(receipt, expected)?;
        if matches!(
            result,
            DesktopAnnotationCommandsEnumResult::PendingProjection { .. }
        ) {
            self.feedback = Some("Finishing the save. Your draft is still here.".to_owned());
            return Ok(());
        }
        let DesktopAnnotationCommandsEnumResult::Edited { snapshot, .. } = result else {
            return Err(PresentationAnalysisEditorError::WrongResult);
        };
        if !Self::same_position_owner(expected, snapshot.context()) {
            return Err(PresentationAnalysisEditorError::UnexpectedReply);
        }
        if !save.matches_publication(snapshot) {
            return Err(PresentationAnalysisEditorError::WrongResult);
        }
        if save
            .accepted
            .as_ref()
            .is_some_and(|previous| previous.context() != snapshot.context())
        {
            return Err(PresentationAnalysisEditorError::UnexpectedReply);
        }
        let is_current = self.publication.as_ref().is_some_and(|current| {
            current.context() == expected || current.context() == snapshot.context()
        });
        save.accepted = Some(snapshot.clone());
        save.request.recover();
        if !is_current || self.is_conflicted {
            self.is_conflicted = true;
            self.feedback = Some("The game changed. Your draft is still here. Finish the earlier save before reconciling it.".to_owned());
            return Err(PresentationAnalysisEditorError::Conflict);
        }
        self.synchronize(snapshot, None);
        self.feedback = Some("Saving your changes. Keep this page open.".to_owned());
        Ok(())
    }

    pub fn prepare_checkpoint(
        &mut self,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        let save = self
            .save
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        let accepted = save
            .accepted
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::NotSaved)?;
        if save.checkpoint_intent.is_some() {
            return Err(PresentationAnalysisEditorError::Busy);
        }
        if !self.history.begin(request_id)? {
            return Ok(None);
        }
        let intent = PresentationAnalysisEditorIntent::new(
            request_id,
            accepted.context(),
            DesktopAnnotationCommandsEnumAction::Checkpoint,
        );
        save.checkpoint_intent = Some(intent.clone());
        Ok(Some(intent))
    }

    pub fn attach_checkpoint(
        &mut self,
        request_id: DesktopRequestId,
        expected: DesktopGameEditorContext,
        receipt: DesktopApplicationReceipt,
    ) -> Result<(), PresentationAnalysisEditorError> {
        let save = self
            .save
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        if save.request.intent().request_id() != request_id
            || save.request.intent().expected() != expected
        {
            return Err(PresentationAnalysisEditorError::UnexpectedReply);
        }
        if save.accepted.is_none() {
            return Err(PresentationAnalysisEditorError::NotSaved);
        }
        let checkpoint_intent = save
            .checkpoint_intent
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::MissingReceipt)?;
        if checkpoint_intent.request_id() != receipt.request_id() {
            return Err(PresentationAnalysisEditorError::ForeignReceipt);
        }
        if save
            .checkpoint
            .as_ref()
            .is_some_and(|previous| !previous.shares_outcome(&receipt))
            && save.request.failure().is_none()
        {
            return Err(PresentationAnalysisEditorError::ForeignReceipt);
        }
        save.checkpoint = Some(receipt);
        save.request.recover();
        Ok(())
    }

    pub fn observe_save(
        &mut self,
        application_receipt: &DesktopApplicationReceipt,
        progress: &DesktopAnnotationOwnerEnumSaveProgress,
        publication: &DesktopGameEditorPublication,
    ) -> Result<Option<PresentationAnalysisAnnotationsEnumEffect>, PresentationAnalysisEditorError>
    {
        let save = self
            .save
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        let checkpoint = save
            .checkpoint
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::MissingReceipt)?;
        if !checkpoint.shares_outcome(application_receipt) {
            return Err(PresentationAnalysisEditorError::ForeignReceipt);
        }
        let DesktopAnnotationOwnerEnumSaveProgress::Saved { receipt, covered } = progress else {
            return Ok(None);
        };
        let accepted = save
            .accepted
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::NotSaved)?;
        let current = publication.context();
        if receipt.id() != current.game()
            || receipt.content_revision() != current.storage_revision()
            || !Self::same_position_owner(accepted.context(), current)
            || current.source() != accepted.context().source()
            || current.generation() != accepted.context().generation()
            || *covered != PresentationAnalysisEditorServiceCompletion::revision(accepted.context())
        {
            return Err(PresentationAnalysisEditorError::NotSaved);
        }
        let draft = self
            .draft
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::NoSelection)?;
        let draft_matches = save.matches_draft(draft);
        self.finish_save();
        if self.is_conflicted {
            self.feedback = Some("Your earlier changes were saved. Your draft is still here. Review it before leaving.".to_owned());
            return Ok(None);
        }
        self.publication = Some(publication.clone());
        self.source_pending = None;
        self.clear_temporary();
        if !draft_matches {
            self.is_conflicted = true;
            self.feedback = Some("Your earlier changes were saved. Your newer draft is still here. Review it before leaving.".to_owned());
            return Ok(None);
        }
        self.draft = Some(PresentationAnalysisAnnotationsDraft::capture(publication));
        self.feedback = Some("Changes saved.".to_owned());
        let switch = self.guard.take();
        switch
            .map(|switch| self.release_switch(switch, current))
            .transpose()
    }

    pub fn observe_failure(
        &mut self,
        receipt: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
        failure: &DesktopFailure,
    ) -> Result<(), PresentationAnalysisEditorError> {
        let save = self
            .save
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        save.request.check(receipt, expected)?;
        save.request.fail(failure);
        self.feedback = Some(failure.message().to_owned());
        Ok(())
    }

    pub fn observe_checkpoint_failure(
        &mut self,
        receipt: &DesktopApplicationReceipt,
        failure: &DesktopFailure,
    ) -> Result<(), PresentationAnalysisEditorError> {
        let save = self
            .save
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        if !save
            .checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.shares_outcome(receipt))
        {
            return Err(PresentationAnalysisEditorError::ForeignReceipt);
        }
        save.request.fail(failure);
        self.feedback = Some(failure.message().to_owned());
        Ok(())
    }

    pub fn retry_save(
        &mut self,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        let save = self
            .save
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        if self.is_conflicted && save.request.service_failure().is_none() {
            return Err(PresentationAnalysisEditorError::Conflict);
        }
        if !save.request.has_failure() {
            return Ok(None);
        }
        let intent = if save.request.service_failure().is_some() {
            if !self.history.begin(request_id)? {
                return Ok(None);
            }
            if let Some(previous) = save.request.retry_request_id() {
                self.history.complete(previous);
            }
            save.request.retry_service(request_id)
        } else if save.accepted.is_some() {
            if !self.history.begin(request_id)? {
                return Ok(None);
            }
            let expected = self
                .publication
                .as_ref()
                .ok_or(PresentationAnalysisEditorError::NoSelection)?
                .context();
            save.checkpoint = None;
            if let Some(previous) = save.checkpoint_intent.take() {
                self.history.complete(previous.request_id());
            }
            let retry = PresentationAnalysisEditorIntent::new(
                request_id,
                expected,
                DesktopAnnotationCommandsEnumAction::RetrySave,
            );
            save.checkpoint_intent = Some(retry.clone());
            retry
        } else {
            save.request.intent().clone()
        };
        save.request.recover();
        self.feedback = Some("Retrying the save.".to_owned());
        Ok(Some(intent))
    }

    pub fn discard_draft(&mut self) -> Result<(), PresentationAnalysisEditorError> {
        if self.save.is_some() {
            return Err(PresentationAnalysisEditorError::Busy);
        }
        let publication = self
            .publication
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::NoSelection)?;
        self.draft = Some(PresentationAnalysisAnnotationsDraft::capture(publication));
        self.is_conflicted = false;
        Ok(())
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisAnnotationsEnumEffect>, PresentationAnalysisEditorError>
    {
        ui.set_min_width(PresentationThemeTokens::PANEL_WIDTH);
        ui.heading("Notes and Markings");
        if self.publication.is_none() {
            ui.label("Open a game to add notes and drawings.");
            return Ok(None);
        }
        if let Some(feedback) = &self.feedback {
            ui.label(feedback);
        }
        if self.source_pending.is_some() {
            ui.label("Updating this game. Your draft is still here.");
        }
        let can_save = self.source_pending.is_none() && !self.is_conflicted && self.save.is_none();
        let can_edit = self.guard.is_none();
        if let Some(draft) = &mut self.draft {
            ui.label("Plain text notes");
            let notes = ui.add_enabled(
                can_edit,
                egui::TextEdit::multiline(&mut draft.notes)
                    .id_salt("chess.analysis.notes")
                    .desired_rows(6),
            );
            if notes.changed() {
                draft.version.advance()?;
            }
            ui.label("Notation marks (NAGs)");
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (index, nag) in draft.nags.iter().enumerate() {
                    let response = ui.add_enabled(
                        can_edit,
                        egui::Button::new(format!("Remove ${}", nag.value())),
                    );
                    if self.frame.clicked(ui, &response) {
                        remove = Some(index);
                    }
                }
            });
            if let Some(index) = remove {
                draft.version.advance()?;
                draft.nags.remove(index);
            }
            let mut added_nag = false;
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut self.nag_value)
                        .range(u8::MIN..=u8::MAX)
                        .prefix("NAG "),
                );
                let response = ui.add_enabled(can_edit, egui::Button::new("Add NAG"));
                if self.frame.clicked(ui, &response) {
                    let nag = GameMoveNodeNag::new(self.nag_value);
                    if !draft.nags.contains(&nag) {
                        draft.nags.push(nag);
                        added_nag = true;
                    }
                }
            });
            if added_nag {
                draft.version.advance()?;
            }
        }
        let mut effect = None;
        if ui
            .add_enabled(
                can_save && can_edit && self.has_notes_changes(),
                egui::Button::new("Save Notes and NAGs"),
            )
            .clicked()
            && !ui.is_sizing_pass()
        {
            effect = self
                .save_notes(request_id)?
                .map(PresentationAnalysisAnnotationsEnumEffect::Command);
        }
        if self.is_conflicted
            && ui
                .add_enabled(
                    self.save.is_none(),
                    egui::Button::new("Discard Draft and Use Current Position"),
                )
                .clicked()
            && !ui.is_sizing_pass()
        {
            self.discard_draft()?;
        }
        ui.separator();
        ui.add_enabled_ui(
            can_edit && self.save.is_none() && self.source_pending.is_none(),
            |ui| self.show_marking_input(ui),
        )
        .inner?;
        if ui
            .add_enabled(
                can_save && can_edit && (!self.temporary.is_empty() || self.hide_saved),
                egui::Button::new("Save Markings"),
            )
            .clicked()
            && !ui.is_sizing_pass()
            && effect.is_none()
        {
            effect = self
                .save_markings(request_id)?
                .map(PresentationAnalysisAnnotationsEnumEffect::Command);
        }
        if self
            .save
            .as_ref()
            .is_some_and(|save| save.request.has_failure())
            && crate::presentation::theme::components::PresentationThemeEnumButton::Secondary
                .show(ui, "Retry Save")
                .clicked()
            && !ui.is_sizing_pass()
            && effect.is_none()
        {
            effect = self
                .retry_save(request_id)?
                .map(PresentationAnalysisAnnotationsEnumEffect::Command);
        }
        if self.guard.is_some() {
            self.modal.capture_invoker(ui.ctx());
            let mut choice = None;
            let modal = crate::presentation::theme::components::PresentationThemeComponents::modal(
                ui.ctx(),
                egui::Id::new("chess.analysis.unsaved_notes"),
            )
            .show(ui.ctx(), |ui| {
                PresentationAnalysisEditorModal::label(ui, "Unsaved Notes");
                ui.heading("Save your notes before leaving?");
                ui.label("Save these notes, discard your edits, or stay here.");
                if self.save.is_some() {
                    ui.label("Saving your changes. Stay here to cancel leaving.");
                }
                let mut focusable = Vec::new();
                for (label, candidate, enabled) in [
                    (
                        "Stay",
                        PresentationAnalysisAnnotationsEnumGuardChoice::Stay,
                        true,
                    ),
                    (
                        "Discard",
                        PresentationAnalysisAnnotationsEnumGuardChoice::Discard,
                        self.source_pending.is_none() && self.save.is_none(),
                    ),
                    (
                        "Save",
                        PresentationAnalysisAnnotationsEnumGuardChoice::Save,
                        can_save && self.save.is_none(),
                    ),
                ] {
                    let response = ui.add_enabled(enabled, egui::Button::new(label));
                    if response.enabled() {
                        focusable.push(response.id);
                    }
                    if response.clicked() && !ui.is_sizing_pass() {
                        choice = Some(candidate);
                    }
                }
                self.modal.contain(ui, &focusable);
            });
            if modal.should_close() {
                choice = Some(PresentationAnalysisAnnotationsEnumGuardChoice::Stay);
            }
            if let Some(choice) = choice {
                effect = self.resolve_guard(choice, request_id)?;
                if self.guard.is_none() {
                    self.modal.close(ui.ctx());
                }
            }
        }
        if self.guard.is_none() {
            self.modal.close(ui.ctx());
        }
        Ok(effect)
    }

    fn ready(&self) -> Result<DesktopGameEditorContext, PresentationAnalysisEditorError> {
        if self.source_pending.is_some() {
            return Err(PresentationAnalysisEditorError::SourcePending);
        }
        if self.is_conflicted {
            return Err(PresentationAnalysisEditorError::Conflict);
        }
        if self.save.is_some() {
            return Err(PresentationAnalysisEditorError::Busy);
        }
        self.context()
            .ok_or(PresentationAnalysisEditorError::NoSelection)
    }

    fn check_draft(
        &self,
        expected: DesktopGameEditorContext,
    ) -> Result<(), PresentationAnalysisEditorError> {
        if self.context() != Some(expected) {
            return Err(PresentationAnalysisEditorError::Conflict);
        }
        Ok(())
    }

    fn begin_save(
        &mut self,
        request_id: DesktopRequestId,
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if !self.history.begin(request_id)? {
            return Ok(None);
        }
        let intent = PresentationAnalysisEditorIntent::new(request_id, expected, action);
        let draft_version = self
            .draft
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::NoSelection)?
            .version;
        self.save = Some(PresentationAnalysisAnnotationsSave {
            request: PresentationAnalysisEditorPending::new(intent.clone()),
            accepted: None,
            checkpoint: None,
            checkpoint_intent: None,
            draft_version,
        });
        self.feedback = Some("Waiting to save your changes.".to_owned());
        Ok(Some(intent))
    }

    fn release_switch(
        &mut self,
        switch: PresentationAnalysisAnnotationsSwitch,
        expected: DesktopGameEditorContext,
    ) -> Result<PresentationAnalysisAnnotationsEnumEffect, PresentationAnalysisEditorError> {
        if !self.history.record(switch.request_id) {
            return Err(PresentationAnalysisEditorError::UnexpectedReply);
        }
        Ok(match switch.destination {
            PresentationAnalysisAnnotationsEnumDestination::Navigate(selected) => {
                PresentationAnalysisAnnotationsEnumEffect::Command(
                    PresentationAnalysisEditorIntent::new(
                        switch.request_id,
                        expected,
                        DesktopAnnotationCommandsEnumAction::Navigate { selected },
                    ),
                )
            }
            PresentationAnalysisAnnotationsEnumDestination::Leave(destination) => {
                PresentationAnalysisAnnotationsEnumEffect::Leave {
                    request_id: switch.request_id,
                    expected,
                    destination,
                }
            }
            PresentationAnalysisAnnotationsEnumDestination::AppendStudyBranch {
                node,
                movement,
            } => PresentationAnalysisAnnotationsEnumEffect::Command(
                PresentationAnalysisEditorIntent::new(
                    switch.request_id,
                    expected,
                    DesktopAnnotationCommandsEnumAction::AppendStudyBranch { node, movement },
                ),
            ),
        })
    }

    fn finish_save(&mut self) {
        if let Some(save) = self.save.take() {
            self.history.complete(save.request.intent().request_id());
            if let Some(retry) = save.request.retry_request_id() {
                self.history.complete(retry);
            }
            if let Some(checkpoint) = save.checkpoint_intent {
                self.history.complete(checkpoint.request_id());
            }
        }
    }

    fn clear_temporary(&mut self) {
        self.temporary.clear();
        self.hide_saved = false;
        self.marking_origin.clear();
        self.marking_destination.clear();
    }

    fn same_position_owner(
        expected: DesktopGameEditorContext,
        actual: DesktopGameEditorContext,
    ) -> bool {
        PresentationAnalysisEditorServiceCompletion::same_owner(expected, actual)
            && expected.selected() == actual.selected()
    }

    fn show_marking_input(&mut self, ui: &mut Ui) -> Result<(), PresentationAnalysisEditorError> {
        ui.label("Board drawings");
        let color = self
            .marking_color
            .get_or_insert(Self::DEFAULT_MARKING_COLOR);
        ui.horizontal_wrapped(|ui| {
            for (candidate, label) in [
                (GameEnumMarkingColor::Green, "Green"),
                (GameEnumMarkingColor::Red, "Red"),
                (GameEnumMarkingColor::Blue, "Blue"),
                (GameEnumMarkingColor::Yellow, "Yellow"),
            ] {
                ui.selectable_value(color, candidate, label);
            }
        });
        ui.label("From square");
        ui.add(
            egui::TextEdit::singleline(&mut self.marking_origin)
                .id_salt("chess.analysis.marking_origin")
                .hint_text("e2"),
        );
        ui.label("To square (arrows)");
        ui.add(
            egui::TextEdit::singleline(&mut self.marking_destination)
                .id_salt("chess.analysis.marking_destination")
                .hint_text("e4"),
        );
        let mut is_circle = false;
        let mut is_arrow = false;
        let mut clear_temporary = false;
        let mut clear_saved = false;
        ui.horizontal_wrapped(|ui| {
            let circle =
                crate::presentation::theme::components::PresentationThemeEnumButton::Secondary
                    .show(ui, "Toggle Circle");
            is_circle = self.frame.clicked(ui, &circle);
            let arrow =
                crate::presentation::theme::components::PresentationThemeEnumButton::Secondary
                    .show(ui, "Toggle Arrow");
            is_arrow = self.frame.clicked(ui, &arrow);
            let temporary =
                crate::presentation::theme::components::PresentationThemeEnumButton::Danger
                    .show(ui, "Clear Temporary Markings");
            clear_temporary = self.frame.clicked(ui, &temporary);
            let saved = crate::presentation::theme::components::PresentationThemeEnumButton::Danger
                .show(ui, "Remove Saved Markings");
            clear_saved = self.frame.clicked(ui, &saved);
        });
        if ui.is_sizing_pass() {
            return Ok(());
        }
        let expected = self
            .publication
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::NoSelection)?
            .context();
        let input = if is_circle || is_arrow {
            let origin: BoardSquare = self
                .marking_origin
                .parse()
                .map_err(|_error| PresentationAnalysisEditorError::InvalidSquare)?;
            let color = *self
                .marking_color
                .get_or_insert(Self::DEFAULT_MARKING_COLOR);
            let marking = if is_circle {
                GameMoveNodeMarking::circle(color, origin)
            } else {
                let destination = self
                    .marking_destination
                    .parse()
                    .map_err(|_error| PresentationAnalysisEditorError::InvalidSquare)?;
                GameMoveNodeMarking::arrow(color, origin, destination)
                    .map_err(|_error| PresentationAnalysisEditorError::EmptyArrow)?
            };
            Some(PresentationAnalysisAnnotationsEnumMarkingInput::Toggle(
                marking,
            ))
        } else if clear_temporary {
            Some(PresentationAnalysisAnnotationsEnumMarkingInput::ClearTemporary)
        } else if clear_saved {
            Some(PresentationAnalysisAnnotationsEnumMarkingInput::ClearSaved)
        } else {
            None
        };
        if let Some(input) = input {
            self.marking_input(expected, input)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/presentation/analysis/annotations.rs"]
mod tests;
