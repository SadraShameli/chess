use bevy_egui::egui::{Color32, Context, CornerRadius, Stroke, Visuals};

use crate::desktop::preferences::DesktopPreferencesEnumTheme;
use crate::presentation::theme::font::PresentationThemeFont;

pub struct PresentationThemeTokens;

struct PresentationThemeTokensPalette {
    background: Color32,
    surface: Color32,
    raised: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    border: Color32,
    warning: Color32,
}

impl PresentationThemeTokens {
    pub const MIN_TARGET: f32 = 28.0;
    pub const PANEL_WIDTH: f32 = 272.0;

    pub fn apply(context: &Context, theme: DesktopPreferencesEnumTheme) {
        context.set_fonts(PresentationThemeFont::definitions());
        let (mut visuals, palette) = match theme {
            DesktopPreferencesEnumTheme::Dark => (
                Visuals::dark(),
                PresentationThemeTokensPalette {
                    background: Color32::from_rgb(18, 21, 25),
                    surface: Color32::from_rgb(27, 32, 38),
                    raised: Color32::from_rgb(38, 45, 53),
                    text: Color32::from_rgb(239, 241, 234),
                    muted: Color32::from_rgb(176, 186, 195),
                    accent: Color32::from_rgb(74, 211, 155),
                    border: Color32::from_rgb(75, 88, 98),
                    warning: Color32::from_rgb(255, 203, 115),
                },
            ),
            DesktopPreferencesEnumTheme::Light => (
                Visuals::light(),
                PresentationThemeTokensPalette {
                    background: Color32::from_rgb(246, 243, 234),
                    surface: Color32::from_rgb(255, 253, 247),
                    raised: Color32::from_rgb(232, 228, 216),
                    text: Color32::from_rgb(30, 39, 35),
                    muted: Color32::from_rgb(84, 99, 91),
                    accent: Color32::from_rgb(15, 112, 75),
                    border: Color32::from_rgb(144, 157, 146),
                    warning: Color32::from_rgb(137, 76, 0),
                },
            ),
        };
        visuals.panel_fill = palette.background;
        visuals.window_fill = palette.surface;
        visuals.override_text_color = Some(palette.text);
        visuals.weak_text_color = Some(palette.muted);
        visuals.warn_fg_color = palette.warning;
        visuals.selection.bg_fill = palette.accent.gamma_multiply(0.3);
        visuals.selection.stroke = Stroke::new(1.5, palette.accent);
        visuals.widgets.noninteractive.bg_fill = palette.surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
        visuals.widgets.inactive.weak_bg_fill = palette.raised;
        visuals.widgets.hovered.weak_bg_fill = palette.raised;
        visuals.widgets.active.weak_bg_fill = palette.accent.gamma_multiply(0.35);
        visuals.window_corner_radius = CornerRadius::same(12);
        context.set_theme(match theme {
            DesktopPreferencesEnumTheme::Dark => bevy_egui::egui::Theme::Dark,
            DesktopPreferencesEnumTheme::Light => bevy_egui::egui::Theme::Light,
        });
        context.set_visuals(visuals);
        context.all_styles_mut(|style| {
            style.spacing.item_spacing = bevy_egui::egui::vec2(10.0, 10.0);
            style.spacing.button_padding = bevy_egui::egui::vec2(12.0, 8.0);
            style.spacing.interact_size.y = Self::MIN_TARGET;
        });
    }
}
