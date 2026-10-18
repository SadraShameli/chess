use bevy_egui::egui::{self, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use crate::game::enums::GameId;
use crate::presentation::game::overview::{
    PresentationGameOverview, PresentationGameOverviewAction, PresentationGameOverviewEnumStatus,
    PresentationGameOverviewRow,
};
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;

pub struct PresentationGameTabs;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PresentationGameTabsAction {
    pub selected: Option<GameId>,
    pub close: Option<GameId>,
    pub overview: bool,
    pub new_game: bool,
}

impl PresentationGameTabs {
    pub const CLOSE_LABEL: &'static str = "Close tab";
    pub const OVERVIEW_LABEL: &'static str = "All games";
    pub const NEW_LABEL: &'static str = "Open a new game";
    const PADDING: f32 = 10.0;
    const GAP: f32 = 6.0;
    const DOT: f32 = 6.0;
    const GLYPH: f32 = 12.0;

    pub fn show(
        ui: &mut Ui,
        rows: &[PresentationGameOverviewRow],
        selected: Option<GameId>,
    ) -> PresentationGameTabsAction {
        let mut action = PresentationGameTabsAction::default();
        if Self::overview_tab(ui, rows.len(), selected.is_none()).clicked() {
            action.overview = true;
        }
        for row in rows {
            let is_selected = selected == Some(row.id());
            let response = Self::game_tab(ui, row, is_selected).on_hover_text(row.summary());
            if response.clicked() {
                action.selected = Some(row.id());
            }
            if is_selected
                && PresentationThemeIcon::button(
                    ui,
                    PresentationThemeEnumIcon::Close,
                    Self::CLOSE_LABEL,
                )
                .clicked()
            {
                action.close = Some(row.id());
            }
        }
        if PresentationThemeIcon::button(ui, PresentationThemeEnumIcon::Plus, Self::NEW_LABEL)
            .clicked()
        {
            action.new_game = true;
        }
        action
    }

    pub fn show_overview(ui: &mut Ui, rows: &[PresentationGameOverviewRow]) {
        Self::show_overview_page(ui, rows);
    }

    pub fn show_overview_page(
        ui: &mut Ui,
        rows: &[PresentationGameOverviewRow],
    ) -> PresentationGameOverviewAction {
        if rows.len() < 2 {
            return PresentationGameOverviewAction::default();
        }
        ui.collapsing(Self::OVERVIEW_LABEL, |ui| {
            PresentationGameOverview::show(ui, rows)
        })
        .body_returned
        .unwrap_or_default()
    }

    fn frame(ui: &Ui, rect: Rect, selected: bool, hovered: bool) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let fill = if selected {
            colors.surface_2
        } else if hovered {
            colors.surface_1
        } else {
            Color32::TRANSPARENT
        };
        let stroke = if selected {
            Stroke::new(1.0, colors.line)
        } else {
            Stroke::NONE
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(PresentationThemeTokens::RADIUS_MD),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
    }

    fn overview_tab(ui: &mut Ui, count: usize, selected: bool) -> egui::Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let text_color = if selected { colors.text } else { colors.text_2 };
        let label = ui.painter().layout_no_wrap(
            Self::OVERVIEW_LABEL.to_owned(),
            FontId::proportional(PresentationThemeTypography::UI_POINTS),
            text_color,
        );
        let pill = ui.painter().layout_no_wrap(
            count.to_string(),
            FontId::proportional(PresentationThemeTypography::MICRO_POINTS),
            colors.text_2,
        );
        let pill_size = Vec2::new(pill.size().x + 12.0, pill.size().y + 4.0);
        let width = Self::PADDING * 2.0 + label.size().x + Self::GAP + pill_size.x;
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(width, PresentationThemeTokens::CONTROL_MD),
            Sense::click(),
        );
        Self::frame(ui, rect, selected, response.hovered());
        let mut x = rect.min.x + Self::PADDING;
        let label_width = label.size().x;
        ui.painter().galley(
            egui::pos2(x, rect.center().y - label.size().y / 2.0),
            label,
            text_color,
        );
        x += label_width + Self::GAP;
        let pill_rect = Rect::from_min_size(
            egui::pos2(x, rect.center().y - pill_size.y / 2.0),
            pill_size,
        );
        ui.painter()
            .rect_filled(pill_rect, CornerRadius::same(u8::MAX), colors.surface_3);
        ui.painter()
            .galley(pill_rect.center() - pill.size() / 2.0, pill, colors.text_2);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                true,
                selected,
                Self::OVERVIEW_LABEL,
            )
        });
        response
    }

    fn game_tab(ui: &mut Ui, row: &PresentationGameOverviewRow, selected: bool) -> egui::Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let text_color = if selected { colors.text } else { colors.text_2 };
        let running = row.status() == PresentationGameOverviewEnumStatus::Running;
        let paused = row.status() == PresentationGameOverviewEnumStatus::Paused;
        let name = ui.painter().layout_no_wrap(
            row.name(),
            FontId::proportional(PresentationThemeTypography::UI_POINTS),
            text_color,
        );
        let control = ui.painter().layout_no_wrap(
            row.control().to_owned(),
            FontId::monospace(PresentationThemeTypography::MICRO_POINTS),
            colors.text_3,
        );
        let mut width = Self::PADDING * 2.0 + name.size().x + Self::GAP + control.size().x;
        if running {
            width += Self::DOT + Self::GAP;
        }
        if row.is_unsaved() {
            width += Self::GAP + Self::DOT;
        }
        if paused {
            width += Self::GAP + Self::GLYPH;
        }
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(width, PresentationThemeTokens::CONTROL_MD),
            Sense::click(),
        );
        Self::frame(ui, rect, selected, response.hovered());
        let center = rect.center().y;
        let mut x = rect.min.x + Self::PADDING;
        if running {
            ui.painter().circle_filled(
                egui::pos2(x + Self::DOT / 2.0, center),
                Self::DOT / 2.0,
                colors.run,
            );
            x += Self::DOT + Self::GAP;
        }
        let name_size = name.size();
        ui.painter()
            .galley(egui::pos2(x, center - name_size.y / 2.0), name, text_color);
        x += name_size.x + Self::GAP;
        let control_size = control.size();
        ui.painter().galley(
            egui::pos2(x, center - control_size.y / 2.0),
            control,
            colors.text_3,
        );
        x += control_size.x;
        if row.is_unsaved() {
            x += Self::GAP;
            ui.painter().circle_filled(
                egui::pos2(x + Self::DOT / 2.0, center),
                Self::DOT / 2.0,
                colors.unsaved,
            );
            x += Self::DOT;
        }
        if paused {
            x += Self::GAP;
            let glyph = Rect::from_center_size(
                egui::pos2(x + Self::GLYPH / 2.0, center),
                Vec2::splat(Self::GLYPH),
            );
            PresentationThemeIcon::paint(
                ui.painter(),
                glyph,
                PresentationThemeEnumIcon::Pause,
                colors.text_3,
            );
        }
        let label = row.tab_label();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label.clone())
        });
        response
    }
}
