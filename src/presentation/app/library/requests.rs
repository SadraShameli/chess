use std::sync::Arc;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationWakeProvider,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::file_handle::DesktopFileEnumPurpose;
use crate::desktop::game_editor::DesktopGameEditorEnumSource;
use crate::desktop::identity::DesktopRevision;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumDisposition, DesktopLibraryServiceEnumResult,
};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEnumResult};
use crate::presentation::app::library::{
    PresentationAppLibrary, PresentationAppLibraryEnumEditor, PresentationAppLibraryEnumOwner,
    PresentationAppLibraryError, PresentationAppLibraryPending,
};
use crate::presentation::library::intent::PresentationLibraryIntent;
use crate::presentation::library::view::PresentationLibraryEnumEffect;

impl PresentationAppLibrary {
    pub(in crate::presentation::app) fn dispatch_panel<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        intent: PresentationLibraryIntent,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        if self.pending.iter().any(|pending| matches!(&pending.owner, PresentationAppLibraryEnumOwner::Panel(original) if original == &intent)) { return Ok(()); }
        let replacement = if matches!(intent.operation().action(), crate::desktop::library_service::DesktopLibraryServiceEnumAction::Editor { action: crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction::RetrySave, .. }) {
            self.pending.iter().position(|pending| pending.observed && matches!(&pending.owner, PresentationAppLibraryEnumOwner::Panel(original) if original.is_open_action()))
        } else { None };
        if self
            .pending
            .iter()
            .filter(|pending| matches!(pending.owner, PresentationAppLibraryEnumOwner::Panel(_)))
            .count()
            >= Self::MAX_PANEL_LANES
            && replacement.is_none()
        {
            return Err(PresentationAppLibraryError::Busy);
        }
        let operation = intent.operation().clone();
        let request = port.request(DesktopCommand::library(operation.clone()))?;
        self.panel
            .controller_mut()
            .attach_request(&intent, request.clone())?;
        if let Some(index) = replacement {
            self.pending.remove(index);
        }
        self.pending.push(PresentationAppLibraryPending {
            owner: PresentationAppLibraryEnumOwner::Panel(intent),
            operation,
            request,
            receipt: None,
            observed: false,
            failure: None,
        });
        self.admit(port)
    }

    pub(in crate::presentation::app) fn select_file<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        file: PresentationLibraryEnumFileRequest,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        if self.file_selection.is_some() {
            return Err(PresentationAppLibraryError::Busy);
        }
        let purpose = match file {
            PresentationLibraryEnumFileRequest::ImportSource => DesktopFileEnumPurpose::ReadFile,
            PresentationLibraryEnumFileRequest::ExportDestination { .. } => {
                DesktopFileEnumPurpose::WriteFile
            }
        };
        let request = port.request(DesktopCommand::new(
            DesktopEnumCommand::FileSelect {
                owner: self.workspace,
                purpose,
            },
            Some(DesktopRevision::ZERO),
        ))?;
        self.file_selection = Some(PresentationAppLibraryFileSelection {
            request,
            receipt: None,
            file,
        });
        self.admit(port)
    }

    fn poll_file_selection<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        let Some(selection) = &self.file_selection else {
            return Ok(());
        };
        let Some(response) = selection
            .receipt
            .as_ref()
            .and_then(DesktopApplicationReceipt::response)
            .cloned()
        else {
            return Ok(());
        };
        let file = selection.file;
        self.file_selection = None;
        match response {
            DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::FileSelect(Some(handle)),
                ..
            } => {
                for effect in self.panel.file_selected(file, handle) {
                    match effect {
                        PresentationLibraryEnumEffect::Dispatch(intent) => {
                            self.dispatch_panel(intent, port)?
                        }
                        PresentationLibraryEnumEffect::OpenSaved(_)
                        | PresentationLibraryEnumEffect::SelectFile(_) => {
                            return Err(PresentationAppLibraryError::UnexpectedResult);
                        }
                    }
                }
            }
            DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::FileSelect(None),
                ..
            }
            | DesktopMessagesEnumResult::Cancelled => {}
            DesktopMessagesEnumResult::Failure(failure) => {
                self.failure = Some(failure.to_string());
            }
            DesktopMessagesEnumResult::Value { .. }
            | DesktopMessagesEnumResult::LibraryFailure(_)
            | DesktopMessagesEnumResult::EditorFailure(_)
            | DesktopMessagesEnumResult::Stopped
            | DesktopMessagesEnumResult::Paused
            | DesktopMessagesEnumResult::QuitReady
            | DesktopMessagesEnumResult::ReconciliationRequired => {
                self.failure = Some(PresentationAppLibraryError::UnexpectedResult.to_string());
            }
        }
        Ok(())
    }

    pub(super) fn admit<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        if let Some(selection) = &mut self.file_selection
            && selection.receipt.is_none()
        {
            match port.try_submit(&selection.request) {
                Ok(receipt) => selection.receipt = Some(receipt),
                Err(DesktopApplicationError::Busy | DesktopApplicationError::EntityBusy) => {}
                Err(error) => {
                    self.file_selection = None;
                    return Err(error.into());
                }
            }
        }
        for pending in &mut self.pending {
            if pending.receipt.is_some() || pending.observed {
                continue;
            }
            let receipt = match port.try_submit(&pending.request) {
                Ok(receipt) => receipt,
                Err(DesktopApplicationError::Busy | DesktopApplicationError::EntityBusy) => {
                    continue;
                }
                Err(error) => {
                    pending.observed = true;
                    return Err(error.into());
                }
            };
            pending.receipt = Some(receipt.clone());
            match &pending.owner {
                PresentationAppLibraryEnumOwner::Panel(intent) => self
                    .panel
                    .controller_mut()
                    .attach_receipt(intent, receipt.clone())?,
                PresentationAppLibraryEnumOwner::Editor { owner, intent } => match owner {
                    PresentationAppLibraryEnumEditor::Notes => {
                        self.annotations.attach_receipt(intent, receipt.clone())?
                    }
                    PresentationAppLibraryEnumEditor::Tree => {
                        self.tree.attach_receipt(intent, receipt.clone())?
                    }
                    PresentationAppLibraryEnumEditor::Navigate => {}
                },
            }
        }
        Ok(())
    }

    pub(super) fn poll<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        let mut index = 0;
        while index < self.pending.len() {
            if self.pending[index].observed {
                index += 1;
                continue;
            }
            let Some(receipt) = self.pending[index].receipt.as_ref() else {
                index += 1;
                continue;
            };
            let Some(response) = receipt.response() else {
                index += 1;
                continue;
            };
            let response = response.clone();
            let receipt = receipt.clone();
            let operation = self.pending[index].operation.clone();
            self.pending[index].observed = true;
            let mut effect = None;
            let terminal = match response {
                DesktopMessagesEnumResult::Value {
                    value: DesktopEnumValue::Library(result),
                    ..
                } => {
                    match &self.pending[index].owner {
                        PresentationAppLibraryEnumOwner::Panel(_) => self
                            .panel
                            .controller_mut()
                            .observe_result(&receipt, &operation, &result)?,
                        PresentationAppLibraryEnumOwner::Editor { owner, intent } => match owner {
                            PresentationAppLibraryEnumEditor::Notes => {
                                effect = self.annotations.observe_service_result(
                                    &receipt,
                                    intent.expected(),
                                    &result,
                                )?
                            }
                            PresentationAppLibraryEnumEditor::Tree => self
                                .tree
                                .observe_service_result(&receipt, intent.expected(), &result)?,
                            PresentationAppLibraryEnumEditor::Navigate => {
                                Self::check_navigation(intent, &result)?
                            }
                        },
                    }
                    self.accept_publication(&result);
                    !matches!(result.as_ref(), DesktopLibraryServiceEnumResult::PendingSnapshot { .. } | DesktopLibraryServiceEnumResult::Editor(crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumResult::PendingProjection { .. } | crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumResult::Edited { change: crate::game::annotations::GameAnnotationsEnumChange::Changed, .. }))
                }
                DesktopMessagesEnumResult::LibraryFailure(error) => {
                    match &self.pending[index].owner {
                        PresentationAppLibraryEnumOwner::Panel(_) => self
                            .panel
                            .controller_mut()
                            .observe_error(&receipt, &operation, Arc::clone(&error))?,
                        PresentationAppLibraryEnumOwner::Editor { owner, intent } => match owner {
                            PresentationAppLibraryEnumEditor::Notes => self
                                .annotations
                                .observe_service_error(&receipt, intent.expected(), &error)?,
                            PresentationAppLibraryEnumEditor::Tree => self
                                .tree
                                .observe_service_error(&receipt, intent.expected(), &error)?,
                            PresentationAppLibraryEnumEditor::Navigate => {
                                Self::check_disposition(&receipt, intent.expected(), &error)?
                            }
                        },
                    }
                    let retained_open = matches!(&self.pending[index].owner, PresentationAppLibraryEnumOwner::Panel(intent) if intent.is_open_action() && self.panel.controller().is_open_pending());
                    let terminal = error.disposition()
                        == DesktopLibraryServiceEnumDisposition::Refused
                        && !retained_open;
                    self.service_failure = Some(Arc::clone(&error));
                    self.pending[index].failure = Some(error);
                    terminal
                }
                DesktopMessagesEnumResult::Failure(failure) => {
                    self.failure = Some(failure.to_string());
                    false
                }
                DesktopMessagesEnumResult::Value { .. }
                | DesktopMessagesEnumResult::EditorFailure(_)
                | DesktopMessagesEnumResult::Cancelled
                | DesktopMessagesEnumResult::Stopped
                | DesktopMessagesEnumResult::Paused
                | DesktopMessagesEnumResult::QuitReady
                | DesktopMessagesEnumResult::ReconciliationRequired => {
                    self.failure = Some(PresentationAppLibraryError::UnexpectedResult.to_string());
                    false
                }
            };
            if terminal {
                self.pending.remove(index);
            } else {
                self.pending[index].observed = true;
                index += 1;
            }
            if let Some(effect) = effect {
                self.annotation_effect(effect, port)?;
            }
        }
        self.poll_file_selection(port)?;
        Ok(())
    }

    fn accept_publication(&mut self, result: &DesktopLibraryServiceEnumResult) {
        use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumResult;
        let (snapshot, pending) = match result {
            DesktopLibraryServiceEnumResult::Opened(snapshot)
            | DesktopLibraryServiceEnumResult::Saved { snapshot, .. }
            | DesktopLibraryServiceEnumResult::Editor(
                DesktopAnnotationCommandsEnumResult::Snapshot(snapshot)
                | DesktopAnnotationCommandsEnumResult::Edited { snapshot, .. },
            ) => (snapshot, None),
            DesktopLibraryServiceEnumResult::PendingSnapshot { snapshot, pending } => {
                (snapshot, Some(*pending))
            }
            DesktopLibraryServiceEnumResult::Query(_)
            | DesktopLibraryServiceEnumResult::Metadata(_)
            | DesktopLibraryServiceEnumResult::Organized(_)
            | DesktopLibraryServiceEnumResult::Import(_)
            | DesktopLibraryServiceEnumResult::Exported(_)
            | DesktopLibraryServiceEnumResult::TrashPlan(_)
            | DesktopLibraryServiceEnumResult::Trashed(_)
            | DesktopLibraryServiceEnumResult::Restored(_)
            | DesktopLibraryServiceEnumResult::Editor(_) => return,
        };
        let changed = self
            .publication
            .as_ref()
            .is_none_or(|previous| previous.context() != snapshot.context());
        if matches!(result, DesktopLibraryServiceEnumResult::Opened(_)) {
            self.source_changed = false;
        }
        let open_game = match snapshot.context().source() {
            DesktopGameEditorEnumSource::Played { revision } => {
                Some((snapshot.context().game(), revision))
            }
            DesktopGameEditorEnumSource::Notation { .. } => None,
        };
        self.panel.controller_mut().observe_open_game(open_game);
        self.annotations.synchronize(snapshot, pending);
        self.tree.synchronize(snapshot, pending);
        self.publication = Some(snapshot.clone());
        if self.annotations.notes() != Some(self.notes.as_str()) {
            self.notes.clear();
            if let Some(notes) = self.annotations.notes() {
                self.notes.push_str(notes);
            }
        }
        if changed {
            self.selected_move = None;
        }
    }
}
