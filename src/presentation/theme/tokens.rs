use bevy_egui::egui::{Color32, Context, CornerRadius, Stroke, Visuals};

use crate::presentation::theme::font::PresentationThemeFont;

pub struct PresentationThemeTokens;

impl PresentationThemeTokens {
    pub const BACKGROUND: Color32 = Color32::from_rgb(18, 21, 25);
    pub const SURFACE: Color32 = Color32::from_rgb(27, 32, 38);
    pub const RAISED: Color32 = Color32::from_rgb(38, 45, 53);
    pub const TEXT: Color32 = Color32::from_rgb(239, 241, 234);
    pub const MUTED: Color32 = Color32::from_rgb(176, 186, 195);
    pub const ACCENT: Color32 = Color32::from_rgb(74, 211, 155);
    pub const BORDER: Color32 = Color32::from_rgb(75, 88, 98);
    pub const WARNING: Color32 = Color32::from_rgb(255, 203, 115);
    pub const MIN_TARGET: f32 = 28.0;
    pub const PANEL_WIDTH: f32 = 272.0;

    pub fn apply(context: &Context) {
        context.set_fonts(PresentationThemeFont::definitions());
        let mut visuals = Visuals::dark();
        visuals.panel_fill = Self::BACKGROUND;
        visuals.window_fill = Self::SURFACE;
        visuals.override_text_color = Some(Self::TEXT);
        visuals.selection.bg_fill = Self::ACCENT.gamma_multiply(0.3);
        visuals.selection.stroke = Stroke::new(1.5, Self::ACCENT);
        visuals.widgets.noninteractive.bg_fill = Self::SURFACE;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Self::BORDER);
        visuals.widgets.inactive.weak_bg_fill = Self::RAISED;
        visuals.widgets.hovered.weak_bg_fill = Self::RAISED;
        visuals.widgets.active.weak_bg_fill = Self::ACCENT.gamma_multiply(0.35);
        visuals.window_corner_radius = CornerRadius::same(12);
        context.set_visuals(visuals);
        context.all_styles_mut(|style| {
            style.spacing.item_spacing = bevy_egui::egui::vec2(10.0, 10.0);
            style.spacing.button_padding = bevy_egui::egui::vec2(12.0, 8.0);
            style.spacing.interact_size.y = Self::MIN_TARGET;
        });
    }
}
