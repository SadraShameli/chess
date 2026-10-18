use crate::board::r#move::BoardMove;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::application::{
    DesktopApplicationPort, DesktopApplicationReceipt, DesktopApplicationWakeProvider,
};
use crate::desktop::game_editor::DesktopGameEditorContext;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceError, DesktopLibraryServiceOperation,
};
use crate::desktop::messages::{DesktopCommand, DesktopRequest};
use crate::presentation::analysis::annotations::{
    PresentationAnalysisAnnotationsEnumDestination, PresentationAnalysisAnnotationsEnumEffect,
    PresentationAnalysisAnnotationsEnumGuardChoice,
};
use crate::presentation::analysis::editor_intent::{
    PresentationAnalysisEditorIntent, PresentationAnalysisEditorServiceCompletion,
};
use crate::presentation::app::library::{
    PresentationAppLibrary, PresentationAppLibraryEnumEditor, PresentationAppLibraryEnumOwner,
    PresentationAppLibraryError, PresentationAppLibraryPending,
};

impl PresentationAppLibrary {
    fn prepare_editor<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &self,
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<DesktopRequest, PresentationAppLibraryError> {
        if self.editor_busy() || self.opening_busy() {
            return Err(PresentationAppLibraryError::Busy);
        }
        if self.source_changed {
            return Err(PresentationAppLibraryError::SourceChanged);
        }
        Ok(port.request(DesktopCommand::library(
            DesktopLibraryServiceOperation::new(
                self.workspace,
                DesktopLibraryServiceEnumAction::Editor { expected, action },
            ),
        ))?)
    }

    fn retain_editor<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        owner: PresentationAppLibraryEnumEditor,
        request: DesktopRequest,
        intent: PresentationAnalysisEditorIntent,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        if request.request_id() != intent.request_id() {
            return Err(PresentationAppLibraryError::UnexpectedResult);
        }
        let operation = DesktopLibraryServiceOperation::new(
            self.workspace,
            DesktopLibraryServiceEnumAction::Editor {
                expected: intent.expected(),
                action: intent.action().clone(),
            },
        );
        if !matches!(request.command().operation(), crate::desktop::messages::DesktopEnumCommand::Library(actual) if actual == &operation)
        {
            return Err(PresentationAppLibraryError::UnexpectedResult);
        }
        self.pending.push(PresentationAppLibraryPending {
            owner: PresentationAppLibraryEnumOwner::Editor { owner, intent },
            operation,
            request,
            receipt: None,
            observed: false,
            failure: None,
        });
        self.admit(port)
    }

    pub(in crate::presentation::app) fn save_notes<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        guarded: bool,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        let expected = self.annotations.context().ok_or(crate::presentation::analysis::editor_intent::PresentationAnalysisEditorError::NoSelection)?;
        let action = DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: self
                .annotations
                .notes()
                .ok_or(PresentationAppLibraryError::UnexpectedResult)?
                .to_owned(),
            nags: self
                .annotations
                .nags()
                .ok_or(PresentationAppLibraryError::UnexpectedResult)?
                .to_vec(),
        };
        let request = self.prepare_editor(expected, action, port)?;
        let effect = if guarded {
            self.annotations.resolve_guard(
                PresentationAnalysisAnnotationsEnumGuardChoice::Save,
                request.request_id(),
            )?
        } else {
            self.annotations
                .save_notes(request.request_id())?
                .map(PresentationAnalysisAnnotationsEnumEffect::Command)
        };
        if let Some(PresentationAnalysisAnnotationsEnumEffect::Command(intent)) = effect {
            self.retain_editor(
                PresentationAppLibraryEnumEditor::Notes,
                request,
                intent,
                port,
            )?;
        }
        Ok(())
    }

    pub(in crate::presentation::app) fn load_tree<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        next: bool,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        let expected = self.tree.context().ok_or(crate::presentation::analysis::editor_intent::PresentationAnalysisEditorError::NoSelection)?;
        let cursor = if next {
            Some(self.tree.tree_page().and_then(crate::desktop::game_editor::DesktopGameEditorTreePage::next_cursor).ok_or(crate::presentation::analysis::editor_intent::PresentationAnalysisEditorError::EndOfPages)?)
        } else {
            None
        };
        let request = self.prepare_editor(
            expected,
            DesktopAnnotationCommandsEnumAction::TreePage { cursor },
            port,
        )?;
        if let Some(intent) = self.tree.request_tree(request.request_id(), next)? {
            self.retain_editor(
                PresentationAppLibraryEnumEditor::Tree,
                request,
                intent,
                port,
            )?;
        }
        Ok(())
    }

    pub(in crate::presentation::app) fn append_move<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        node: crate::game::enums::GameMoveId,
        movement: BoardMove,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        let publication = self
            .publication
            .as_ref()
            .ok_or(PresentationAppLibraryError::UnexpectedResult)?;
        if !publication
            .board()
            .legal_moves()
            .iter()
            .any(|legal| legal.movement() == movement)
        {
            return Err(PresentationAppLibraryError::UnexpectedResult);
        }
        if self.editor_busy() || self.navigation.is_some() {
            return Err(PresentationAppLibraryError::Busy);
        }
        let effect = self.annotations.request_switch(
            DesktopRequestId::new(),
            PresentationAnalysisAnnotationsEnumDestination::AppendStudyBranch { node, movement },
        )?;
        if let Some(effect) = effect {
            self.annotation_effect(effect, port)?;
        }
        Ok(())
    }

    pub(in crate::presentation::app) fn annotation_effect<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        effect: PresentationAnalysisAnnotationsEnumEffect,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        match effect {
            PresentationAnalysisAnnotationsEnumEffect::Leave { .. } => {
                self.release_navigation(effect)
            }
            PresentationAnalysisAnnotationsEnumEffect::Command(intent) => {
                let expected = intent.expected();
                let request = self.prepare_editor(expected, intent.action().clone(), port)?;
                match intent.action() {
                    DesktopAnnotationCommandsEnumAction::Navigate { .. } => {
                        let intent = PresentationAnalysisEditorIntent::new(
                            request.request_id(),
                            expected,
                            intent.into_action(),
                        );
                        self.retain_editor(
                            PresentationAppLibraryEnumEditor::Navigate,
                            request,
                            intent,
                            port,
                        )
                    }
                    DesktopAnnotationCommandsEnumAction::AppendStudyBranch { node, movement } => {
                        let Some(intent) = self.tree.append_study_move(
                            request.request_id(),
                            expected,
                            *node,
                            *movement,
                        )?
                        else {
                            return Ok(());
                        };
                        self.retain_editor(
                            PresentationAppLibraryEnumEditor::Tree,
                            request,
                            intent,
                            port,
                        )
                    }
                    DesktopAnnotationCommandsEnumAction::Snapshot
                    | DesktopAnnotationCommandsEnumAction::TreePage { .. }
                    | DesktopAnnotationCommandsEnumAction::ChildrenPage { .. }
                    | DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { .. }
                    | DesktopAnnotationCommandsEnumAction::SaveMarkings { .. }
                    | DesktopAnnotationCommandsEnumAction::PreviewVariation(_)
                    | DesktopAnnotationCommandsEnumAction::VariationScope { .. }
                    | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. }
                    | DesktopAnnotationCommandsEnumAction::Checkpoint
                    | DesktopAnnotationCommandsEnumAction::RetrySave => {
                        Err(PresentationAppLibraryError::UnexpectedResult)
                    }
                }
            }
        }
    }

    pub(super) fn check_navigation(
        intent: &PresentationAnalysisEditorIntent,
        result: &DesktopLibraryServiceEnumResult,
    ) -> Result<(), PresentationAppLibraryError> {
        let (
            DesktopAnnotationCommandsEnumAction::Navigate { selected },
            DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Snapshot(
                snapshot,
            )),
        ) = (intent.action(), result)
        else {
            return Err(PresentationAppLibraryError::UnexpectedResult);
        };
        PresentationAnalysisEditorServiceCompletion::check_snapshot(
            intent.expected(),
            snapshot.context(),
        )?;
        if snapshot.context().selected() != *selected {
            return Err(PresentationAppLibraryError::UnexpectedResult);
        }
        Ok(())
    }

    pub(super) fn check_disposition(
        receipt: &DesktopApplicationReceipt,
        context: DesktopGameEditorContext,
        error: &DesktopLibraryServiceError,
    ) -> Result<(), PresentationAppLibraryError> {
        if let DesktopLibraryServiceEnumDisposition::Retained { request, expected } =
            error.disposition()
            && (request != receipt.request_id() || expected != Some(context))
        {
            return Err(PresentationAppLibraryError::UnexpectedResult);
        }
        Ok(())
    }
}
