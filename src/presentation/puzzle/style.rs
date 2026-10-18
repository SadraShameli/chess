use bevy_egui::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Ui, Vec2,
};

use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumClock,
    PresentationThemeEnumSize,
};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationPuzzleStyleEnumMark {
    Archived,
    Current,
}

pub struct PresentationPuzzleStyle;

impl PresentationPuzzleStyle {
    pub const COUNTDOWN_POINTS: f32 = 40.0;
    pub const STAT_POINTS: f32 = 20.0;
    pub const DIALOG_STAT_POINTS: f32 = 18.0;
    pub const PROGRESS_HEIGHT: f32 = 4.0;
    pub const MARK_SIZE: f32 = 28.0;
    pub const STAT_COLUMNS: usize = 3;
    pub const STAT_COLUMNS_F32: f32 = 3.0;

    pub fn countdown(ui: &mut Ui, text: &str, state: PresentationThemeEnumClock) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let color = match state {
            PresentationThemeEnumClock::Running => colors.text,
            PresentationThemeEnumClock::Low => colors.low,
            PresentationThemeEnumClock::Idle => colors.text_2,
        };
        ui.label(
            RichText::new(text)
                .size(Self::COUNTDOWN_POINTS)
                .strong()
                .extra_letter_spacing(-0.8)
                .color(color),
        );
    }

    pub fn progress(ui: &mut Ui, share: f32, state: PresentationThemeEnumClock) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let fill = match state {
            PresentationThemeEnumClock::Running | PresentationThemeEnumClock::Idle => colors.text_2,
            PresentationThemeEnumClock::Low => colors.low,
        };
        let width = ui.available_width();
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(width, Self::PROGRESS_HEIGHT), Sense::hover());
        let radius = Self::PROGRESS_HEIGHT / 2.0;
        ui.painter().rect_filled(rect, radius, colors.surface_3);
        let filled = rect.width() * share.clamp(0.0, 1.0);
        let bar = egui::Rect::from_min_size(rect.min, Vec2::new(filled, rect.height()));
        ui.painter().rect_filled(bar, radius, fill);
    }

    pub fn stats(ui: &mut Ui, tiles: &[(&str, String)], fill: Color32, points: f32) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let gap = PresentationThemeTokens::SPACE_2;
        let columns = Self::STAT_COLUMNS;
        let count = Self::STAT_COLUMNS_F32;
        let width = ((ui.available_width() - gap * (count - 1.0)) / count).max(0.0);
        for row in tiles.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (label, value) in row {
                    Frame::new()
                        .fill(fill)
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_width((width - 24.0).max(0.0));
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(
                                RichText::new(*label)
                                    .size(PresentationThemeTypography::CAPTION_POINTS)
                                    .color(colors.text_3),
                            );
                            ui.label(
                                RichText::new(value)
                                    .size(points)
                                    .strong()
                                    .color(colors.text),
                            );
                        });
                }
            });
        }
    }

    pub fn key_value(ui: &mut Ui, key: &str, value: &str, strong: bool) {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(RichText::new(key).color(colors.text_2));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut text = RichText::new(value).color(colors.text);
                if strong {
                    text = text.strong();
                }
                ui.label(text);
            });
        });
    }

    pub fn note(ui: &mut Ui, text: impl Into<String>) {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.label(
            RichText::new(text)
                .size(PresentationThemeTypography::UI_POINTS)
                .color(colors.text_2),
        );
    }

    pub fn error(ui: &mut Ui, text: impl Into<String>) {
        let colors = PresentationThemeColors::of(ui.ctx());
        Frame::new()
            .fill(colors.err_soft)
            .corner_radius(CornerRadius::same(PresentationThemeTokens::RADIUS_MD))
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(text).color(colors.err));
            });
    }

    pub fn section(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = PresentationThemeTokens::SPACE_3;
            add(ui);
        });
        PresentationThemeComponents::divider(ui);
    }

    pub fn mark(ui: &mut Ui, number: usize, mark: PresentationPuzzleStyleEnumMark) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let (fill, text) = match mark {
            PresentationPuzzleStyleEnumMark::Archived => (colors.surface_3, colors.text_2),
            PresentationPuzzleStyleEnumMark::Current => (colors.accent_soft, colors.accent),
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(Self::MARK_SIZE), Sense::hover());
        ui.painter().rect_filled(rect, 6.0, fill);
        if mark == PresentationPuzzleStyleEnumMark::Current {
            ui.painter().rect_stroke(
                rect.shrink(0.75),
                6.0,
                egui::Stroke::new(1.5, colors.accent),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            number.to_string(),
            egui::FontId::proportional(PresentationThemeTypography::CAPTION_POINTS),
            text,
        );
        let label = match mark {
            PresentationPuzzleStyleEnumMark::Archived => format!("Puzzle {number}, archived"),
            PresentationPuzzleStyleEnumMark::Current => format!("Puzzle {number}, current"),
        };
        response.on_hover_text(label);
    }

    pub fn action(
        ui: &mut Ui,
        kind: PresentationThemeEnumButton,
        label: &str,
        key: Option<&str>,
        enabled: bool,
    ) -> bool {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_1;
            let button = kind.widget(ui, label, PresentationThemeEnumSize::Medium);
            let clicked = ui.add_enabled(enabled, button).clicked();
            if let Some(key) = key {
                ui.add_enabled_ui(enabled, |ui| {
                    PresentationThemeComponents::kbd(ui, key);
                });
            }
            clicked
        })
        .inner
    }

    pub fn alert_dialog(ui: &Ui, label: &str) {
        PresentationAnalysisEditorModal::label(ui, label);
        ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
            node.set_role(egui::accesskit::Role::AlertDialog);
        });
    }

    pub fn dialog_heading(ui: &mut Ui, title: &str, description: &str) {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(
                RichText::new(title)
                    .size(PresentationThemeTypography::HEADING_POINTS)
                    .strong()
                    .color(colors.text),
            );
            ui.label(
                RichText::new(description)
                    .size(PresentationThemeTypography::BODY_POINTS)
                    .color(colors.text_2),
            );
        });
    }

    pub fn dialog_footer(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_2;
            add(ui);
        });
    }

    pub fn small(
        ui: &mut Ui,
        kind: PresentationThemeEnumButton,
        label: &str,
        enabled: bool,
    ) -> bool {
        let button = kind.widget(ui, label, PresentationThemeEnumSize::Small);
        ui.add_enabled(enabled, button).clicked()
    }

    pub fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        let colors = PresentationThemeColors::of(ui.ctx());
        Frame::new()
            .corner_radius(CornerRadius::same(PresentationThemeTokens::RADIUS_MD))
            .inner_margin(Margin::symmetric(10, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(28.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).color(colors.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), add)
                        .inner
                })
                .inner
            })
            .inner
    }
}
