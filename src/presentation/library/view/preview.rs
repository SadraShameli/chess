use bevy_egui::egui::Ui;

use crate::desktop::identity::DesktopRequestId;
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationGameTarget,
};
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::rows::PresentationLibraryRows;
use crate::presentation::library::view::{PresentationLibraryEnumEffect, PresentationLibraryPanel};
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::storage::trash::StorageTrashPreviewRequest;

impl PresentationLibraryPanel {
    pub(super) fn preview(
        &mut self,
        ui: &mut Ui,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        PresentationThemeComponents::eyebrow(ui, "Preview");
        let in_trash = self.in_trash();
        let can_open = !self.controller.is_open_pending();
        let can_trash = !self
            .controller
            .is_pending(PresentationLibraryEnumLane::Trash);
        let can_export = !self
            .controller
            .is_pending(PresentationLibraryEnumLane::Export);
        let can_organize = !self
            .controller
            .is_pending(PresentationLibraryEnumLane::Organize);
        let previewed = self.preview_row().map(|row| row.id());
        let Some(row) = previewed.and_then(|id| {
            self.controller
                .page()
                .and_then(|page| page.entries().iter().find(|row| row.id() == id))
        }) else {
            PresentationThemeComponents::caption(ui, "Select a game to preview it");
            return;
        };
        let mut open = None;
        let mut favorite = None;
        let mut restore = None;
        let mut trash = None;
        let mut export = None;
        let mut organization_error = None;
        PresentationThemeComponents::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            PresentationThemeComponents::subhead(ui, PresentationLibraryRows::players(row));
            PresentationThemeComponents::caption(ui, PresentationLibraryRows::summary(row));
            PresentationThemeComponents::caption(ui, PresentationLibraryRows::opening(row));
            PresentationThemeComponents::divider(ui);
            PresentationLibraryRows::show(ui, row, &self.controller);
            PresentationThemeComponents::divider(ui);
            if in_trash {
                let button = ui.add_enabled(
                    can_trash,
                    PresentationThemeEnumButton::Primary.widget(
                        ui,
                        "Restore to Library",
                        PresentationThemeEnumSize::Medium,
                    ),
                );
                if self.frame.clicked(ui, &button) {
                    restore = Some(Self::restore_target(row));
                }
                let delete = ui.add_enabled(
                    can_trash,
                    PresentationThemeEnumButton::Danger.widget(
                        ui,
                        "Preview permanent delete",
                        PresentationThemeEnumSize::Medium,
                    ),
                );
                if self.frame.clicked(ui, &delete) {
                    trash = Some(StorageTrashPreviewRequest::PermanentlyDelete(row.id()));
                }
                return;
            }
            ui.horizontal(|ui| {
                let study = ui.add_enabled(
                    can_open,
                    PresentationThemeEnumButton::Primary.widget(
                        ui,
                        "Open for study",
                        PresentationThemeEnumSize::Medium,
                    ),
                );
                if self.frame.clicked(ui, &study) {
                    open = Some(row.id());
                }
                PresentationThemeComponents::kbd(ui, "\u{21b5}");
            });
            ui.horizontal_wrapped(|ui| {
                let is_favorite = row.organization().is_favorite();
                let label = if is_favorite {
                    "Remove from Favorites"
                } else {
                    "Add to Favorites"
                };
                PresentationThemeIcon::show(
                    ui,
                    PresentationThemeEnumIcon::Star,
                    16.0,
                    ui.visuals().text_color(),
                );
                let toggle = ui.add_enabled(
                    can_organize,
                    PresentationThemeEnumButton::Secondary.widget(
                        ui,
                        label,
                        PresentationThemeEnumSize::Small,
                    ),
                );
                if self.frame.clicked(ui, &toggle) {
                    favorite = Some(GameLibraryOrganizationEnumMutation::SetFavorite(
                        GameLibraryOrganizationGameTarget::new(
                            row.id(),
                            row.organization().revision(),
                        ),
                        !is_favorite,
                    ));
                }
            });
            ui.horizontal_wrapped(|ui| {
                if let Some(request) = Self::export_buttons(ui, &mut self.frame, row, can_export) {
                    export = Some(request);
                }
            });
            PresentationThemeComponents::field_label(ui, "Folder and tags");
            match Self::row_organization(ui, &mut self.frame, &self.controller, row, can_organize) {
                Ok(Some(mutation)) => favorite = Some(mutation),
                Ok(None) => {}
                Err(error) => organization_error = Some(error),
            }
            PresentationThemeComponents::divider(ui);
            let move_to_trash = ui.add_enabled(
                can_trash,
                PresentationThemeEnumButton::Danger.widget(
                    ui,
                    "Preview Move to Trash",
                    PresentationThemeEnumSize::Small,
                ),
            );
            if self.frame.clicked(ui, &move_to_trash) {
                trash = Some(StorageTrashPreviewRequest::MoveToTrash(row.id()));
            }
        });
        if let Some(game) = open {
            let result = self.controller.open_saved(DesktopRequestId::new(), game);
            self.collect(result, effects);
        }
        if let Some(error) = organization_error {
            self.input_error = Some(error.into());
        }
        if let Some(request) = export {
            Self::export_effect(request, effects);
        }
        if let Some(request) = trash {
            self.request_game_trash(request, effects);
        }
        if let Some(target) = restore {
            let result = self.controller.restore(DesktopRequestId::new(), target);
            self.collect(result, effects);
        }
        if let Some(mutation) = favorite {
            let result = self.controller.organize(DesktopRequestId::new(), mutation);
            self.collect(result, effects);
        }
    }
}
