use bevy_egui::egui::{Color32, Context, CornerRadius, Stroke, Visuals, epaint::Shadow};

use crate::desktop::preferences::DesktopPreferencesEnumTheme;
use crate::presentation::theme::font::PresentationThemeFont;
use crate::presentation::theme::typography::PresentationThemeTypography;

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
    pub const PANEL_WIDTH: f32 = 360.0;

    pub const WINDOW_ELEVATION: u8 = 18;
    pub const POPUP_ELEVATION: u8 = 10;

    pub fn elevation(theme: DesktopPreferencesEnumTheme, blur: u8) -> Shadow {
        Shadow {
            offset: [0, i8::try_from(blur / 3).unwrap_or(0)],
            blur,
            spread: 0,
            color: match theme {
                DesktopPreferencesEnumTheme::Dark => Color32::from_black_alpha(140),
                DesktopPreferencesEnumTheme::Light => Color32::from_black_alpha(48),
            },
        }
    }

    pub fn apply(context: &Context, theme: DesktopPreferencesEnumTheme) {
        context.set_fonts(PresentationThemeFont::definitions());
        let (mut visuals, palette) = match theme {
            DesktopPreferencesEnumTheme::Dark => (
                Visuals::dark(),
                PresentationThemeTokensPalette {
                    background: Color32::from_rgb(0x11, 0x15, 0x1a),
                    surface: Color32::from_rgb(0x1b, 0x22, 0x2b),
                    raised: Color32::from_rgb(0x25, 0x2f, 0x3b),
                    text: Color32::from_rgb(0xf3, 0xf5, 0xf7),
                    muted: Color32::from_rgb(0xb8, 0xc2, 0xce),
                    accent: Color32::from_rgb(0x34, 0xd3, 0x99),
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
                    text: Color32::from_rgb(0x17, 0x21, 0x2b),
                    muted: Color32::from_rgb(0x4b, 0x59, 0x68),
                    accent: Color32::from_rgb(0x00, 0x6b, 0x50),
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
        visuals.window_shadow = Self::elevation(theme, Self::WINDOW_ELEVATION);
        visuals.popup_shadow = Self::elevation(theme, Self::POPUP_ELEVATION);
        context.set_theme(match theme {
            DesktopPreferencesEnumTheme::Dark => bevy_egui::egui::Theme::Dark,
            DesktopPreferencesEnumTheme::Light => bevy_egui::egui::Theme::Light,
        });
        context.set_visuals(visuals);
        PresentationThemeTypography::apply(context);
        context.all_styles_mut(|style| {
            style.spacing.item_spacing = bevy_egui::egui::vec2(10.0, 10.0);
            style.spacing.button_padding = bevy_egui::egui::vec2(12.0, 8.0);
            style.spacing.interact_size.y = Self::MIN_TARGET;
        });
    }
}
