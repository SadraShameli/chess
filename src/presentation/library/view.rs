mod export;
mod folders;
mod import;
mod navigation;
mod organization;
mod preview;
mod table;
mod trash;

use bevy_egui::egui::{self, Id, Ui};

use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
};
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::game::enums::GameId;
use crate::game::storage_repository::GameStorageRevision;
use crate::notation::pgn_export::NotationPgnExportEnumSelection;
use crate::presentation::library::controller::PresentationLibraryController;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::library::filters::PresentationLibraryFilters;
use crate::presentation::library::intent::PresentationLibraryIntent;
use crate::presentation::library::view::table::PresentationLibraryTable;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::storage::library_metadata::StorageLibraryMetadataEnumRequest;
use crate::storage::library_query::{StorageLibraryQueryEntry, StorageLibraryQueryOptions};
use crate::storage::trash::StorageTrashEnumLocation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationLibraryEnumFileRequest {
    ImportSource,
    ExportDestination {
        game: GameId,
        storage_revision: GameStorageRevision,
        selection: NotationPgnExportEnumSelection,
    },
}

#[derive(Debug, Clone)]
pub enum PresentationLibraryEnumEffect {
    Dispatch(PresentationLibraryIntent),
    OpenSaved(PresentationLibraryIntent),
    SelectFile(PresentationLibraryEnumFileRequest),
}

pub struct PresentationLibraryPanel {
    controller: PresentationLibraryController,
    filters: PresentationLibraryFilters,
    frame: PresentationLibraryFrame,
    input_error: Option<PresentationLibraryError>,
    new_folder_name: String,
    new_tag_name: String,
    preview: Option<GameId>,
}

impl PresentationLibraryPanel {
    const NAVIGATION_WIDTH: f32 = 248.0;
    const PREVIEW_WIDTH: f32 = 300.0;
    const WIDE_WIDTH: f32 = 980.0;
    const NAVIGATION_MIN_HEIGHT: f32 = 240.0;

    pub fn new(workspace: DesktopMessagesEntityId) -> Self {
        Self {
            controller: PresentationLibraryController::new(workspace),
            filters: PresentationLibraryFilters::default(),
            frame: PresentationLibraryFrame::default(),
            input_error: None,
            new_folder_name: String::new(),
            new_tag_name: String::new(),
            preview: None,
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
            if ui.available_width() >= Self::WIDE_WIDTH {
                let center = (ui.available_width()
                    - Self::NAVIGATION_WIDTH
                    - Self::PREVIEW_WIDTH
                    - 4.0 * PresentationThemeTokens::SPACE_4)
                    .max(Self::NAVIGATION_WIDTH);
                ui.horizontal_top(|ui| {
                    Self::column(ui, Self::NAVIGATION_WIDTH, |ui| {
                        self.navigation(ui, &mut effects);
                    });
                    ui.separator();
                    Self::column(ui, center, |ui| self.main(ui, &mut effects));
                    ui.separator();
                    Self::column(ui, Self::PREVIEW_WIDTH, |ui| {
                        self.preview(ui, &mut effects);
                    });
                });
            } else {
                self.main(ui, &mut effects);
                PresentationThemeComponents::divider(ui);
                self.navigation(ui, &mut effects);
                PresentationThemeComponents::divider(ui);
                self.preview(ui, &mut effects);
            }
        });
        effects
    }

    fn column(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                add(ui);
            },
        );
    }

    fn navigation(&mut self, ui: &mut Ui, effects: &mut Vec<PresentationLibraryEnumEffect>) {
        ui.horizontal(|ui| {
            let colors = PresentationThemeColors::of(ui.ctx());
            PresentationThemeIcon::show(
                ui,
                PresentationThemeEnumIcon::Library,
                20.0,
                colors.text_2,
            );
            ui.heading("Library");
        });
        PresentationThemeComponents::caption(
            ui,
            "Search saved games. Unfinished games reopen paused.",
        );
        egui::ScrollArea::vertical()
            .id_salt("library-navigation")
            .max_height(ui.available_height().max(Self::NAVIGATION_MIN_HEIGHT))
            .show(ui, |ui| {
                self.organizations(ui, effects);
                PresentationThemeComponents::divider(ui);
                self.organization_creation(ui, effects);
                self.trash_controls(ui, effects);
            });
    }

    fn main(&mut self, ui: &mut Ui, effects: &mut Vec<PresentationLibraryEnumEffect>) {
        self.errors(ui);
        if self.controller.can_retry_open() {
            let retry = PresentationThemeEnumButton::Secondary.show(ui, "Retry opening");
            if self.frame.clicked(ui, &retry) {
                let result = self.controller.retry_open(DesktopRequestId::new());
                self.collect(result, effects);
            }
        }
        let mut requery = false;
        ui.horizontal_wrapped(|ui| {
            requery |= self.filters.search(ui);
            requery |= self.filters.quick(ui);
            let import_busy = self
                .controller
                .is_pending(PresentationLibraryEnumLane::Import)
                || self.controller.import().summary().is_some();
            let import = ui.add_enabled(
                !import_busy,
                PresentationThemeEnumButton::Secondary.widget(
                    ui,
                    "Import PGN",
                    PresentationThemeEnumSize::Small,
                ),
            );
            if self.frame.clicked(ui, &import) {
                effects.push(PresentationLibraryEnumEffect::SelectFile(
                    PresentationLibraryEnumFileRequest::ImportSource,
                ));
            }
            let target = self.preview_row().map(|row| {
                PresentationLibraryEnumFileRequest::ExportDestination {
                    game: row.id(),
                    storage_revision: row.content_revision(),
                    selection: NotationPgnExportEnumSelection::Full,
                }
            });
            let export = ui.add_enabled(
                target.is_some()
                    && !self.in_trash()
                    && !self
                        .controller
                        .is_pending(PresentationLibraryEnumLane::Export),
                PresentationThemeEnumButton::Secondary.widget(
                    ui,
                    "Export",
                    PresentationThemeEnumSize::Small,
                ),
            );
            if self.frame.clicked(ui, &export)
                && let Some(request) = target
            {
                Self::export_effect(request, effects);
            }
        });
        requery |= self.filters.chips(ui);
        if requery && !self.controller.is_query_pending() {
            self.query_filters(self.controller.options().location, effects);
        }
        self.filters.show(ui);
        ui.horizontal(|ui| {
            let apply = ui.add_enabled(
                !self.controller.is_query_pending(),
                PresentationThemeEnumButton::Primary.widget(
                    ui,
                    "Apply filters",
                    PresentationThemeEnumSize::Small,
                ),
            );
            if self.frame.clicked(ui, &apply) {
                let result = self
                    .filters
                    .options()
                    .and_then(|options| self.controller.query(DesktopRequestId::new(), options));
                self.collect(result, effects);
            }
            let reset = ui.add_enabled(
                !self.controller.is_query_pending(),
                PresentationThemeEnumButton::Ghost.widget(
                    ui,
                    "Reset filters",
                    PresentationThemeEnumSize::Small,
                ),
            );
            if self.frame.clicked(ui, &reset) {
                self.filters = PresentationLibraryFilters::default();
                let result = self.controller.query(
                    DesktopRequestId::new(),
                    StorageLibraryQueryOptions::default(),
                );
                self.collect(result, effects);
            }
            let label = if self.controller.needs_resync() {
                "Reload from first page"
            } else {
                "Refresh games"
            };
            let refresh = ui.add_enabled(
                !self.controller.is_query_pending(),
                PresentationThemeEnumButton::Ghost.widget(
                    ui,
                    label,
                    PresentationThemeEnumSize::Small,
                ),
            );
            if self.frame.clicked(ui, &refresh) {
                let result = self.controller.refresh(DesktopRequestId::new());
                self.collect(result, effects);
            }
        });
        ui.collapsing("Import games", |ui| self.import_controls(ui, effects));
        self.export_status(ui);
        self.folder_removal(ui, effects);
        self.game_trash_confirmation(ui, effects);
        if let Some(error) = &self.input_error {
            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
        }
        self.games(ui, effects);
    }

    pub(super) fn query_filters(
        &mut self,
        location: StorageTrashEnumLocation,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        let result = self.filters.options().and_then(|mut options| {
            options.location = location;
            self.controller.query(DesktopRequestId::new(), options)
        });
        self.collect(result, effects);
    }

    pub(super) fn preview_row(&self) -> Option<&StorageLibraryQueryEntry> {
        let entries = self.controller.page()?.entries();
        self.preview
            .and_then(|id| entries.iter().find(|row| row.id() == id))
            .or_else(|| entries.first())
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
            | DesktopLibraryServiceEnumAction::Organize(_)
            | DesktopLibraryServiceEnumAction::PreviewTrash(_)
            | DesktopLibraryServiceEnumAction::ApplyTrash(_)
            | DesktopLibraryServiceEnumAction::Restore(_)
            | DesktopLibraryServiceEnumAction::RemoveFolder(_)
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash { .. }
            | DesktopLibraryServiceEnumAction::Import(_)
            | DesktopLibraryServiceEnumAction::Export(_)
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
            PresentationLibraryEnumLane::Organize,
            PresentationLibraryEnumLane::Trash,
            PresentationLibraryEnumLane::Import,
            PresentationLibraryEnumLane::Export,
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
        let in_trash = self.in_trash();
        let previewed = self.preview_row().map(StorageLibraryQueryEntry::id);
        let Some(page) = self.controller.page() else {
            if !self.controller.is_query_pending() {
                ui.label("Games are unavailable until the library is reloaded");
            }
            return;
        };
        if page.entries().is_empty() {
            ui.label("No games match these filters");
        }
        let has_next = page.next_cursor().is_some();
        let can_open = !self.controller.is_open_pending();
        let colors = PresentationThemeColors::of(ui.ctx());
        let mut selected = None;
        let mut preview = None;
        PresentationLibraryTable::header(ui, &colors);
        egui::ScrollArea::vertical()
            .id_salt("library-games")
            .show(ui, |ui| {
                for row in page.entries() {
                    ui.push_id(row.id().as_uuid(), |ui| {
                        let outcome = PresentationLibraryTable::row(
                            ui,
                            &colors,
                            row,
                            previewed == Some(row.id()),
                            (!in_trash).then_some(can_open),
                        );
                        if let Some(open) = &outcome.open
                            && self.frame.clicked(ui, open)
                        {
                            selected = Some(row.id());
                        } else if self.frame.clicked(ui, &outcome.row) {
                            preview = Some(row.id());
                        }
                    });
                }
            });
        PresentationThemeComponents::caption(
            ui,
            format!("{} games on this page", page.entries().len()),
        );
        if let Some(game) = preview {
            self.preview = Some(game);
        }
        if let Some(game) = selected {
            let result = self.controller.open_saved(DesktopRequestId::new(), game);
            self.collect(result, effects);
        }
        let next = ui.add_enabled(
            has_next && !self.controller.is_query_pending(),
            PresentationThemeEnumButton::Ghost.widget(
                ui,
                "Next 100 games",
                PresentationThemeEnumSize::Small,
            ),
        );
        if self.frame.clicked(ui, &next) {
            let result = self.controller.next_page(DesktopRequestId::new());
            self.collect(result, effects);
        }
    }
}

#[derive(Default)]
pub(super) struct PresentationLibraryFrame {
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
