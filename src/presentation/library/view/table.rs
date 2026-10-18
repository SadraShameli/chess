use bevy_egui::egui::{self, Align, Layout, Response, Sense, Ui, UiBuilder};

use crate::presentation::library::rows::PresentationLibraryRows;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;
use crate::storage::library_query::StorageLibraryQueryEntry;

pub(super) struct PresentationLibraryTableRow {
    pub(super) row: Response,
    pub(super) open: Option<Response>,
}

#[derive(Debug, Clone, Copy)]
struct PresentationLibraryTableColumns {
    result: f32,
    players: f32,
    opening: f32,
    date: f32,
}

impl PresentationLibraryTableColumns {
    const STAR: f32 = 20.0;
    const RESULT: f32 = 44.0;
    const DATE: f32 = 84.0;
    const ACTION: f32 = 120.0;

    fn of(width: f32) -> Self {
        let flexible = (width
            - Self::STAR
            - Self::RESULT
            - Self::DATE
            - Self::ACTION
            - 6.0 * PresentationThemeTokens::SPACE_2)
            .max(120.0);
        Self {
            result: Self::RESULT,
            players: flexible * 0.55,
            opening: flexible * 0.45,
            date: Self::DATE,
        }
    }
}

pub(super) struct PresentationLibraryTable;

impl PresentationLibraryTable {
    pub(super) const ROW_HEIGHT: f32 = 36.0;

    pub(super) fn header(ui: &mut Ui, colors: &PresentationThemeColors) {
        let columns = PresentationLibraryTableColumns::of(ui.available_width());
        ui.horizontal(|ui| {
            ui.add_space(PresentationThemeTokens::SPACE_2 + PresentationLibraryTableColumns::STAR);
            for (text, width) in [
                ("Result", columns.result),
                ("Players", columns.players),
                ("Opening", columns.opening),
                ("Date", columns.date),
            ] {
                Self::cell(
                    ui,
                    egui::RichText::new(text.to_uppercase())
                        .size(PresentationThemeTypography::MICRO_POINTS)
                        .strong()
                        .color(colors.text_3),
                    width,
                );
            }
        });
        PresentationThemeComponents::divider(ui);
    }

    pub(super) fn row(
        ui: &mut Ui,
        colors: &PresentationThemeColors,
        entry: &StorageLibraryQueryEntry,
        selected: bool,
        open: Option<bool>,
    ) -> PresentationLibraryTableRow {
        let width = ui.available_width();
        let columns = PresentationLibraryTableColumns::of(width);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, Self::ROW_HEIGHT), Sense::click());
        let fill = if selected {
            colors.accent_soft
        } else if response.hovered() {
            colors.surface_2
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, PresentationThemeTokens::RADIUS_MD, fill);
        let players = PresentationLibraryRows::players(entry);
        let accessible = format!("Preview {players}");
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &accessible)
        });
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(PresentationThemeTokens::SPACE_2, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let star = if entry.organization().is_favorite() {
            colors.accent
        } else {
            colors.text_3
        };
        PresentationThemeIcon::show(
            &mut child,
            PresentationThemeEnumIcon::Star,
            PresentationLibraryTableColumns::STAR - 4.0,
            star,
        );
        child.add_space(4.0);
        let open = open.map(|enabled| {
            child.add_enabled(
                enabled,
                PresentationThemeEnumButton::Ghost.widget(
                    &child,
                    "Open saved game",
                    PresentationThemeEnumSize::Small,
                ),
            )
        });
        Self::cell(
            &mut child,
            egui::RichText::new(entry.result().pgn_token())
                .monospace()
                .size(PresentationThemeTypography::CAPTION_POINTS)
                .color(colors.text_2),
            columns.result,
        );
        Self::cell(
            &mut child,
            egui::RichText::new(players)
                .size(PresentationThemeTypography::UI_POINTS)
                .color(colors.text),
            columns.players,
        );
        Self::cell(
            &mut child,
            egui::RichText::new(PresentationLibraryRows::opening(entry))
                .size(PresentationThemeTypography::CAPTION_POINTS)
                .color(colors.text_2),
            columns.opening,
        );
        Self::cell(
            &mut child,
            egui::RichText::new(PresentationLibraryRows::year(entry))
                .monospace()
                .size(PresentationThemeTypography::CAPTION_POINTS)
                .color(colors.text_3),
            columns.date,
        );
        PresentationLibraryTableRow {
            row: response,
            open,
        }
    }

    fn cell(ui: &mut Ui, text: egui::RichText, width: f32) {
        ui.add_sized(
            egui::vec2(width, PresentationThemeTokens::CONTROL_SM),
            egui::Label::new(text).truncate().selectable(false),
        );
    }
}
