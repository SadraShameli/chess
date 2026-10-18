use bevy_egui::egui::{self, Ui};

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{DesktopApplicationPort, DesktopApplicationWakeProvider};
use crate::desktop::identity::DesktopRequestId;
use crate::presentation::analysis::annotations::{
    PresentationAnalysisAnnotationsEnumDestination, PresentationAnalysisAnnotationsEnumGuardChoice,
};
use crate::presentation::app::library::{
    PresentationAppLibrary, PresentationAppLibraryEnumNavigation, PresentationAppLibraryError,
};
use crate::presentation::board::figurines::PresentationBoardFigurines;
use crate::presentation::library::view::PresentationLibraryEnumEffect;

impl PresentationAppLibrary {
    pub(in crate::presentation::app) fn show_query<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        for effect in self.panel.show(ui) {
            match effect {
                PresentationLibraryEnumEffect::Dispatch(intent) => {
                    self.dispatch_panel(intent, port)?
                }
                PresentationLibraryEnumEffect::SelectFile(file) => {
                    self.select_file(file, port)?;
                }
                PresentationLibraryEnumEffect::OpenSaved(intent) => {
                    if let Some(PresentationAppLibraryEnumNavigation::OpenSaved(intent)) = self
                        .request_navigation(PresentationAppLibraryEnumNavigation::OpenSaved(
                            Box::new(intent),
                        ))?
                    {
                        self.dispatch_panel(*intent, port)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn clicked(&mut self, ui: &Ui, response: &egui::Response) -> bool {
        let frame = ui.ctx().cumulative_frame_nr();
        if !response.clicked() || ui.is_sizing_pass() || self.input_frame == Some(frame) {
            return false;
        }
        self.input_frame = Some(frame);
        true
    }

    pub(in crate::presentation::app) fn show_editor<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        self.frame = Some(ui.ctx().cumulative_frame_nr());
        ui.heading("Notes and Markings");
        if let Some(failure) = &self.failure {
            ui.colored_label(ui.visuals().error_fg_color, failure);
        }
        if let Some(error) = &self.service_failure {
            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
        }
        if self.source_changed || self.annotations.is_conflicted() {
            ui.label("The source changed. Your notes are retained for review.");
        }
        for pending in self.pending.iter().filter(|pending| pending.observed) {
            if let Some(error) = &pending.failure {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
            ui.label("This action remains retained. Recovery is unavailable in this build. Your draft is kept here.");
        }
        if self.publication.is_none() {
            ui.label("Open a saved game to review its board, notes and variations.");
            return Ok(());
        }
        let can_edit = !self.annotations.has_guard() && !self.opening_busy();
        let context = self
            .annotations
            .context()
            .ok_or(PresentationAppLibraryError::UnexpectedResult)?;
        if self.annotations.notes() != Some(self.notes.as_str()) {
            self.notes.clear();
            if let Some(notes) = self.annotations.notes() {
                self.notes.push_str(notes);
            }
        }
        let notes_label = ui.label("Plain text notes");
        if ui
            .add_enabled(
                can_edit,
                egui::TextEdit::multiline(&mut self.notes)
                    .id_salt("chess.library.notes")
                    .desired_rows(6),
            )
            .labelled_by(notes_label.id)
            .changed()
        {
            self.annotations.set_notes(context, self.notes.clone())?;
        }
        let can_act = !self.editor_busy()
            && !self.opening_busy()
            && !self.source_changed
            && !self.annotations.is_conflicted()
            && !self.annotations.has_guard();
        let save = ui.add_enabled(
            can_act && self.annotations.has_notes_changes(),
            egui::Button::new("Save Notes and NAGs"),
        );
        if self.clicked(ui, &save) {
            self.save_notes(false, port)?;
        }
        if let Some(nags) = self.annotations.nags() {
            for nag in nags {
                ui.label(format!("NAG ${}", nag.value()));
            }
        }
        if self.annotations.is_conflicted() && !self.editor_busy() {
            let discard = ui.button("Discard Draft and Use Current Position");
            if self.clicked(ui, &discard) {
                self.annotations.discard_draft()?;
                self.notes.clear();
                if let Some(notes) = self.annotations.notes() {
                    self.notes.push_str(notes);
                }
            }
        }
        ui.separator();
        ui.heading("Moves and Variations");
        let load = ui.add_enabled(can_act, egui::Button::new("Load Moves"));
        if self.clicked(ui, &load) {
            self.load_tree(false, port)?;
        }
        let next = ui.add_enabled(
            can_act
                && self
                    .tree
                    .tree_page()
                    .is_some_and(|page| page.next_cursor().is_some()),
            egui::Button::new("Next moves"),
        );
        if self.clicked(ui, &next) {
            self.load_tree(true, port)?;
        }
        let mut navigate = None;
        if let Some(page) = self.tree.tree_page() {
            for node in page.nodes() {
                let selected = self
                    .publication
                    .as_ref()
                    .is_some_and(|publication| publication.context().selected() == node.id());
                let response = ui
                    .push_id(node.id(), |ui| {
                        ui.add_enabled(
                            !self.editor_busy()
                                && !self.opening_busy()
                                && !self.annotations.has_guard()
                                && !self.source_changed,
                            egui::Button::selectable(
                                selected,
                                PresentationBoardFigurines::display(
                                    ui.ctx(),
                                    node.san().unwrap_or("Initial position"),
                                    None,
                                ),
                            ),
                        )
                    })
                    .inner;
                if response.clicked() && !ui.is_sizing_pass() && self.input_frame != self.frame {
                    navigate = Some(node.id());
                }
            }
        }
        if let Some(selected) = navigate {
            self.input_frame = self.frame;
            if let Some(effect) = self.annotations.request_switch(
                DesktopRequestId::new(),
                PresentationAnalysisAnnotationsEnumDestination::Navigate(selected),
            )? {
                self.annotation_effect(effect, port)?;
            }
        }
        let publication = self
            .publication
            .as_ref()
            .ok_or(PresentationAppLibraryError::UnexpectedResult)?;
        let board = publication.board();
        egui::ComboBox::from_id_salt("chess.library.study_move")
            .selected_text(self.selected_move.map_or_else(
                || "Choose a legal move".to_owned(),
                |movement| movement.uci(board.variant()),
            ))
            .show_ui(ui, |ui| {
                for legal in board.legal_moves() {
                    let movement = legal.movement();
                    ui.selectable_value(
                        &mut self.selected_move,
                        Some(movement),
                        movement.uci(board.variant()),
                    );
                }
            });
        let append = ui.add_enabled(
            can_act && self.selected_move.is_some(),
            egui::Button::new("Append Study Branch"),
        );
        if self.clicked(ui, &append)
            && let Some(movement) = self.selected_move
        {
            let node = crate::game::enums::GameMoveId::try_from(DesktopRequestId::new().as_uuid())?;
            self.append_move(node, movement, port)?;
        }
        self.show_guard(ui, port)
    }

    fn show_guard<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        ui: &Ui,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppLibraryError> {
        if !self.annotations.has_guard() {
            self.modal.close(ui.ctx());
            self.guard_visible = false;
            return Ok(());
        }
        if !self.guard_visible {
            self.modal.open(None);
            self.guard_visible = true;
        }
        self.modal.capture_invoker(ui.ctx());
        let mut choice = None;
        let modal =
            egui::Modal::new(egui::Id::new("chess.library.unsaved_notes")).show(ui.ctx(), |ui| {
                crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal::label(ui, "Unsaved Notes");
                ui.heading("Save your notes before leaving?");
                ui.label("Save these notes, discard your edits, or stay here.");
                let stay = ui.button("Stay");
                if self.clicked(ui, &stay) {
                    choice = Some(PresentationAnalysisAnnotationsEnumGuardChoice::Stay);
                }
                let enabled = !self.editor_busy()
                    && !self.source_changed
                    && !self.annotations.is_conflicted();
                let discard = ui.add_enabled(!self.editor_busy(), egui::Button::new("Discard"));
                if self.clicked(ui, &discard) {
                    choice = Some(PresentationAnalysisAnnotationsEnumGuardChoice::Discard);
                }
                let save = ui.add_enabled(enabled, egui::Button::new("Save"));
                if self.clicked(ui, &save) {
                    choice = Some(PresentationAnalysisAnnotationsEnumGuardChoice::Save);
                }
                if self.editor_busy() {
                    ui.label("Waiting for the durable save. Your draft is still here.");
                }
                let mut focusable = [stay.id; 3];
                let mut count = 0;
                for response in [&stay, &discard, &save] { if response.enabled() { focusable[count] = response.id; count += 1; } }
                self.modal.contain(ui, &focusable[..count]);
            });
        if modal.should_close() && !ui.is_sizing_pass() {
            choice = Some(PresentationAnalysisAnnotationsEnumGuardChoice::Stay);
        }
        match choice {
            Some(PresentationAnalysisAnnotationsEnumGuardChoice::Save) => {
                self.save_notes(true, port)?
            }
            Some(choice) => {
                if let Some(effect) = self
                    .annotations
                    .resolve_guard(choice, DesktopRequestId::new())?
                {
                    self.annotation_effect(effect, port)?;
                }
                if choice == PresentationAnalysisAnnotationsEnumGuardChoice::Stay {
                    self.abandon_navigation()?;
                }
            }
            None => {}
        }
        if !self.annotations.has_guard() {
            self.modal.close(ui.ctx());
            self.guard_visible = false;
        }
        Ok(())
    }
}
