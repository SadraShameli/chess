mod navigation;

use bevy_egui::egui::{self, Id, Ui};

use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
};
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::presentation::library::controller::PresentationLibraryController;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::library::filters::PresentationLibraryFilters;
use crate::presentation::library::intent::PresentationLibraryIntent;
use crate::presentation::library::rows::PresentationLibraryRows;
use crate::storage::library_metadata::StorageLibraryMetadataEnumRequest;
use crate::storage::library_query::StorageLibraryQueryOptions;

#[derive(Debug, Clone)]
pub enum PresentationLibraryEnumEffect {
    Dispatch(PresentationLibraryIntent),
    OpenSaved(PresentationLibraryIntent),
}

pub struct PresentationLibraryPanel {
    controller: PresentationLibraryController,
    filters: PresentationLibraryFilters,
    frame: PresentationLibraryFrame,
    input_error: Option<PresentationLibraryError>,
}

impl PresentationLibraryPanel {
    pub fn new(workspace: DesktopMessagesEntityId) -> Self {
        Self {
            controller: PresentationLibraryController::new(workspace),
            filters: PresentationLibraryFilters::default(),
            frame: PresentationLibraryFrame::default(),
            input_error: None,
        }
    }

    pub const fn controller(&self) -> &PresentationLibraryController {
        &self.controller
    }

    pub fn controller_mut(&mut self) -> &mut PresentationLibraryController {
        &mut self.controller
    }

    pub fn show(&mut self, ui: &mut Ui) -> Vec<PresentationLibraryEnumEffect> {
        let mut effects = Vec::new();
        ui.push_id("library", |ui| {
            if !ui.is_sizing_pass() {
                self.initialize(&mut effects);
            }
            ui.heading("Library");
            ui.label("Search saved games. Unfinished games reopen paused.");
            self.filters.show(ui);
            self.organizations(ui, &mut effects);
            ui.horizontal(|ui| {
                let apply = ui.add_enabled(
                    !self.controller.is_query_pending(),
                    egui::Button::new("Apply filters"),
                );
                if self.frame.clicked(ui, &apply) {
                    let result = self.filters.options().and_then(|options| {
                        self.controller.query(DesktopRequestId::new(), options)
                    });
                    self.collect(result, &mut effects);
                }
                let reset = ui.add_enabled(
                    !self.controller.is_query_pending(),
                    egui::Button::new("Reset filters"),
                );
                if self.frame.clicked(ui, &reset) {
                    self.filters = PresentationLibraryFilters::default();
                    let result = self.controller.query(
                        DesktopRequestId::new(),
                        StorageLibraryQueryOptions::default(),
                    );
                    self.collect(result, &mut effects);
                }
                let label = if self.controller.needs_resync() {
                    "Reload from first page"
                } else {
                    "Refresh games"
                };
                let refresh = ui.add_enabled(
                    !self.controller.is_query_pending(),
                    egui::Button::new(label),
                );
                if self.frame.clicked(ui, &refresh) {
                    let result = self.controller.refresh(DesktopRequestId::new());
                    self.collect(result, &mut effects);
                }
            });
            self.errors(ui);
            if self.controller.can_retry_open() {
                let retry = ui.button("Retry opening");
                if self.frame.clicked(ui, &retry) {
                    let result = self.controller.retry_open(DesktopRequestId::new());
                    self.collect(result, &mut effects);
                }
            }
            if let Some(error) = &self.input_error {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
            self.games(ui, &mut effects);
        });
        effects
    }

    fn initialize(&mut self, effects: &mut Vec<PresentationLibraryEnumEffect>) {
        if self.controller.page().is_none()
            && !self.controller.needs_resync()
            && !self.controller.is_query_pending()
            && self
                .controller
                .failure(PresentationLibraryEnumLane::Query)
                .is_none()
        {
            let result = self.controller.refresh(DesktopRequestId::new());
            self.collect(result, effects);
        }
        for (lane, missing, request) in [
            (
                PresentationLibraryEnumLane::Folders,
                self.controller.folders().is_none(),
                StorageLibraryMetadataEnumRequest::Folders {
                    parent: self.controller.folder_parent(),
                    cursor: None,
                },
            ),
            (
                PresentationLibraryEnumLane::Tags,
                self.controller.tags().is_none(),
                StorageLibraryMetadataEnumRequest::Tags { cursor: None },
            ),
        ] {
            if missing
                && !self.controller.is_pending(lane)
                && self.controller.failure(lane).is_none()
            {
                let result = self.controller.metadata(DesktopRequestId::new(), request);
                self.collect(result, effects);
            }
        }
    }

    fn collect(
        &mut self,
        result: Result<Option<PresentationLibraryIntent>, PresentationLibraryError>,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        match result {
            Ok(Some(intent)) => {
                self.input_error = None;
                effects.push(Self::effect(intent));
            }
            Ok(None) => {}
            Err(error) => self.input_error = Some(error),
        }
    }

    fn effect(intent: PresentationLibraryIntent) -> PresentationLibraryEnumEffect {
        match intent.operation().action() {
            DesktopLibraryServiceEnumAction::Open { .. } => {
                PresentationLibraryEnumEffect::OpenSaved(intent)
            }
            DesktopLibraryServiceEnumAction::Query { .. }
            | DesktopLibraryServiceEnumAction::Metadata(_)
            | DesktopLibraryServiceEnumAction::Editor { .. } => {
                PresentationLibraryEnumEffect::Dispatch(intent)
            }
        }
    }

    fn errors(&mut self, ui: &mut Ui) {
        for lane in [
            PresentationLibraryEnumLane::Query,
            PresentationLibraryEnumLane::Folders,
            PresentationLibraryEnumLane::Tags,
            PresentationLibraryEnumLane::Folder,
            PresentationLibraryEnumLane::Open,
        ] {
            let Some(error) = self.controller.failure(lane) else {
                continue;
            };
            let message = error.to_string();
            let disposition = error.disposition();
            ui.push_id(lane.index(), |ui| {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("{}: {message}", lane.label()),
                );
                match disposition {
                    DesktopLibraryServiceEnumDisposition::Refused => {
                        ui.label("The action was refused. Refresh or choose the action again.");
                    }
                    DesktopLibraryServiceEnumDisposition::Retained { .. } => {
                        ui.label("The original action is retained. Recovery must finish before this action can be replaced.");
                    }
                }
            });
        }
        if self.controller.needs_resync() {
            ui.label(
                "The library changed or this continuation is invalid. Reload from the first page.",
            );
        }
    }

    fn games(&mut self, ui: &mut Ui, effects: &mut Vec<PresentationLibraryEnumEffect>) {
        if self.controller.is_query_pending() {
            ui.spinner();
            ui.label("Loading games");
        }
        if self.controller.is_open_pending() {
            ui.label("Open request pending. Any unsaved edits must be resolved before opening.");
        }
        let Some(page) = self.controller.page() else {
            if !self.controller.is_query_pending() {
                ui.label("Games are unavailable until the library is reloaded");
            }
            return;
        };
        if page.entries().is_empty() {
            ui.label("No games match these filters");
        }
        ui.label(format!("{} games on this page", page.entries().len()));
        let has_next = page.next_cursor().is_some();
        let can_open = !self.controller.is_open_pending();
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("library-games")
            .show(ui, |ui| {
                for row in page.entries() {
                    ui.push_id(row.id().as_uuid(), |ui| {
                        ui.group(|ui| {
                            PresentationLibraryRows::show(ui, row, &self.controller);
                            let open =
                                ui.add_enabled(can_open, egui::Button::new("Open saved game"));
                            if self.frame.clicked(ui, &open) {
                                selected = Some(row.id());
                            }
                        });
                    });
                }
            });
        if let Some(game) = selected {
            let result = self.controller.open_saved(DesktopRequestId::new(), game);
            self.collect(result, effects);
        }
        let next = ui.add_enabled(
            has_next && !self.controller.is_query_pending(),
            egui::Button::new("Next 100 games"),
        );
        if self.frame.clicked(ui, &next) {
            let result = self.controller.next_page(DesktopRequestId::new());
            self.collect(result, effects);
        }
    }
}

#[derive(Default)]
struct PresentationLibraryFrame {
    frame: Option<u64>,
    inputs: Vec<Id>,
}

impl PresentationLibraryFrame {
    fn clicked(&mut self, ui: &Ui, response: &egui::Response) -> bool {
        if ui.is_sizing_pass() || !response.clicked() {
            return false;
        }
        let frame = ui.ctx().cumulative_frame_nr();
        if self.frame != Some(frame) {
            self.frame = Some(frame);
            self.inputs.clear();
        }
        if self.inputs.contains(&response.id) {
            return false;
        }
        self.inputs.push(response.id);
        true
    }
}
