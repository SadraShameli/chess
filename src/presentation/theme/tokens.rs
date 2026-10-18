use bevy_egui::egui::{
    Color32, Context, CornerRadius, Id, Margin, Stroke, Theme, Visuals, epaint::Shadow,
};

use crate::desktop::preferences::DesktopPreferencesEnumTheme;
use crate::presentation::theme::font::PresentationThemeFont;
use crate::presentation::theme::typography::PresentationThemeTypography;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationThemeColors {
    pub bg: Color32,
    pub surface_1: Color32,
    pub surface_2: Color32,
    pub surface_3: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub text: Color32,
    pub text_2: Color32,
    pub text_3: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub accent_soft: Color32,
    pub run: Color32,
    pub run_soft: Color32,
    pub low: Color32,
    pub low_soft: Color32,
    pub err: Color32,
    pub err_soft: Color32,
    pub on_err: Color32,
    pub unsaved: Color32,
    pub unsaved_soft: Color32,
    pub eval_white: Color32,
    pub eval_black: Color32,
    pub eval_draw: Color32,
    pub scrim: Color32,
}

impl PresentationThemeColors {
    pub const DARK: Self = Self {
        bg: Color32::from_rgb(0x12, 0x11, 0x0f),
        surface_1: Color32::from_rgb(0x1a, 0x19, 0x16),
        surface_2: Color32::from_rgb(0x22, 0x20, 0x1c),
        surface_3: Color32::from_rgb(0x2c, 0x2a, 0x25),
        line: Color32::from_rgb(0x34, 0x31, 0x2b),
        line_strong: Color32::from_rgb(0x4a, 0x46, 0x3e),
        text: Color32::from_rgb(0xee, 0xea, 0xe2),
        text_2: Color32::from_rgb(0xba, 0xb3, 0xa7),
        text_3: Color32::from_rgb(0x93, 0x8c, 0x80),
        accent: Color32::from_rgb(0xa9, 0xbc, 0xf2),
        accent_hover: Color32::from_rgb(0xc2, 0xd0, 0xf7),
        on_accent: Color32::from_rgb(0x0f, 0x13, 0x20),
        accent_soft: Color32::from_rgba_unmultiplied_const(0xa9, 0xbc, 0xf2, 0x24),
        run: Color32::from_rgb(0x7f, 0xcb, 0x98),
        run_soft: Color32::from_rgba_unmultiplied_const(0x7f, 0xcb, 0x98, 0x24),
        low: Color32::from_rgb(0xf2, 0x92, 0x5f),
        low_soft: Color32::from_rgba_unmultiplied_const(0xf2, 0x92, 0x5f, 0x29),
        err: Color32::from_rgb(0xf0, 0x7b, 0x7b),
        err_soft: Color32::from_rgba_unmultiplied_const(0xf0, 0x7b, 0x7b, 0x26),
        on_err: Color32::from_rgb(0x17, 0x07, 0x07),
        unsaved: Color32::from_rgb(0xe3, 0xbc, 0x5b),
        unsaved_soft: Color32::from_rgba_unmultiplied_const(0xe3, 0xbc, 0x5b, 0x26),
        eval_white: Color32::from_rgb(0xee, 0xe8, 0xdc),
        eval_black: Color32::from_rgb(0x2a, 0x26, 0x22),
        eval_draw: Color32::from_rgb(0x8f, 0x88, 0x7d),
        scrim: Color32::from_rgba_unmultiplied_const(0x08, 0x07, 0x06, 0xa3),
    };

    pub const LIGHT: Self = Self {
        bg: Color32::from_rgb(0xf3, 0xf0, 0xe9),
        surface_1: Color32::from_rgb(0xfb, 0xfa, 0xf6),
        surface_2: Color32::from_rgb(0xee, 0xea, 0xe2),
        surface_3: Color32::from_rgb(0xe4, 0xdf, 0xd5),
        line: Color32::from_rgb(0xdc, 0xd6, 0xca),
        line_strong: Color32::from_rgb(0xc5, 0xbe, 0xb0),
        text: Color32::from_rgb(0x1c, 0x1a, 0x17),
        text_2: Color32::from_rgb(0x4f, 0x4a, 0x43),
        text_3: Color32::from_rgb(0x6b, 0x64, 0x5a),
        accent: Color32::from_rgb(0x36, 0x51, 0xb0),
        accent_hover: Color32::from_rgb(0x2a, 0x42, 0x96),
        on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
        accent_soft: Color32::from_rgba_unmultiplied_const(0x36, 0x51, 0xb0, 0x1a),
        run: Color32::from_rgb(0x27, 0x75, 0x44),
        run_soft: Color32::from_rgba_unmultiplied_const(0x27, 0x75, 0x44, 0x1f),
        low: Color32::from_rgb(0xa8, 0x4b, 0x19),
        low_soft: Color32::from_rgba_unmultiplied_const(0xa8, 0x4b, 0x19, 0x1f),
        err: Color32::from_rgb(0xb8, 0x3a, 0x3a),
        err_soft: Color32::from_rgba_unmultiplied_const(0xb8, 0x3a, 0x3a, 0x1a),
        on_err: Color32::from_rgb(0xff, 0xff, 0xff),
        unsaved: Color32::from_rgb(0x8a, 0x65, 0x10),
        unsaved_soft: Color32::from_rgba_unmultiplied_const(0x8a, 0x65, 0x10, 0x1f),
        eval_white: Color32::from_rgb(0xee, 0xe8, 0xdc),
        eval_black: Color32::from_rgb(0x2a, 0x26, 0x22),
        eval_draw: Color32::from_rgb(0x8f, 0x88, 0x7d),
        scrim: Color32::from_rgba_unmultiplied_const(0x1c, 0x1a, 0x17, 0x66),
    };

    pub const fn of_theme(theme: DesktopPreferencesEnumTheme) -> Self {
        match theme {
            DesktopPreferencesEnumTheme::Dark => Self::DARK,
            DesktopPreferencesEnumTheme::Light => Self::LIGHT,
        }
    }

    pub fn of(context: &Context) -> Self {
        context
            .data(|data| data.get_temp::<Self>(Id::NULL))
            .unwrap_or_else(|| {
                if context.global_style().visuals.dark_mode {
                    Self::DARK
                } else {
                    Self::LIGHT
                }
            })
    }
}

pub struct PresentationThemeTokens;

impl PresentationThemeTokens {
    pub const MIN_TARGET: f32 = 28.0;
    pub const CONTROL_SM: f32 = 28.0;
    pub const CONTROL_MD: f32 = 32.0;
    pub const CONTROL_LG: f32 = 40.0;
    pub const PANEL_WIDTH: f32 = 360.0;
    pub const MODAL_WIDTH: f32 = 440.0;
    pub const RAIL_WIDTH: f32 = 76.0;
    pub const HEADER_HEIGHT: f32 = 48.0;

    pub const SPACE_1: f32 = 4.0;
    pub const SPACE_2: f32 = 8.0;
    pub const SPACE_3: f32 = 12.0;
    pub const SPACE_4: f32 = 16.0;
    pub const SPACE_5: f32 = 20.0;
    pub const SPACE_6: f32 = 24.0;

    pub const RADIUS_SM: u8 = 4;
    pub const RADIUS_MD: u8 = 6;
    pub const RADIUS_LG: u8 = 10;
    pub const RADIUS_XL: u8 = 14;

    pub fn shadow(theme: DesktopPreferencesEnumTheme, blur: u8, offset: i8) -> Shadow {
        Shadow {
            offset: [0, offset],
            blur,
            spread: 0,
            color: match theme {
                DesktopPreferencesEnumTheme::Dark => Color32::from_black_alpha(0x99),
                DesktopPreferencesEnumTheme::Light => {
                    Color32::from_rgba_unmultiplied_const(0x28, 0x20, 0x14, 0x38)
                }
            },
        }
    }

    pub fn apply(context: &Context, theme: DesktopPreferencesEnumTheme) {
        context.set_fonts(PresentationThemeFont::definitions());
        let colors = PresentationThemeColors::of_theme(theme);
        context.data_mut(|data| data.insert_temp(Id::NULL, colors));
        let mut visuals = match theme {
            DesktopPreferencesEnumTheme::Dark => Visuals::dark(),
            DesktopPreferencesEnumTheme::Light => Visuals::light(),
        };
        let control = CornerRadius::same(Self::RADIUS_MD);
        visuals.panel_fill = colors.bg;
        visuals.window_fill = colors.surface_2;
        visuals.window_stroke = Stroke::new(1.0, colors.line);
        visuals.window_corner_radius = CornerRadius::same(Self::RADIUS_XL);
        visuals.window_shadow = Self::shadow(theme, 72, 24);
        visuals.popup_shadow = Self::shadow(theme, 24, 8);
        visuals.menu_corner_radius = CornerRadius::same(Self::RADIUS_LG);
        visuals.extreme_bg_color = colors.bg;
        visuals.faint_bg_color = colors.surface_1;
        visuals.code_bg_color = colors.surface_2;
        visuals.override_text_color = Some(colors.text);
        visuals.weak_text_color = Some(colors.text_3);
        visuals.hyperlink_color = colors.accent;
        visuals.warn_fg_color = colors.low;
        visuals.error_fg_color = colors.err;
        visuals.selection.bg_fill = colors.accent_soft;
        visuals.selection.stroke = Stroke::new(1.5, colors.accent);
        visuals.text_cursor.stroke = Stroke::new(2.0, colors.accent);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = colors.surface_1;
        widgets.noninteractive.weak_bg_fill = colors.surface_1;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, colors.line);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors.text_2);
        widgets.noninteractive.corner_radius = control;
        widgets.inactive.bg_fill = colors.surface_2;
        widgets.inactive.weak_bg_fill = colors.surface_2;
        widgets.inactive.bg_stroke = Stroke::new(1.0, colors.line);
        widgets.inactive.fg_stroke = Stroke::new(1.0, colors.text);
        widgets.inactive.corner_radius = control;
        widgets.hovered.bg_fill = colors.surface_3;
        widgets.hovered.weak_bg_fill = colors.surface_3;
        widgets.hovered.bg_stroke = Stroke::new(1.0, colors.line_strong);
        widgets.hovered.fg_stroke = Stroke::new(1.0, colors.text);
        widgets.hovered.corner_radius = control;
        widgets.hovered.expansion = 0.0;
        widgets.active.bg_fill = colors.surface_3;
        widgets.active.weak_bg_fill = colors.surface_3;
        widgets.active.bg_stroke = Stroke::new(1.0, colors.accent);
        widgets.active.fg_stroke = Stroke::new(1.0, colors.text);
        widgets.active.corner_radius = control;
        widgets.active.expansion = 0.0;
        widgets.open.bg_fill = colors.surface_3;
        widgets.open.weak_bg_fill = colors.surface_3;
        widgets.open.bg_stroke = Stroke::new(1.0, colors.line_strong);
        widgets.open.corner_radius = control;
        context.set_theme(match theme {
            DesktopPreferencesEnumTheme::Dark => Theme::Dark,
            DesktopPreferencesEnumTheme::Light => Theme::Light,
        });
        context.set_visuals(visuals);
        PresentationThemeTypography::apply(context);
        context.all_styles_mut(|style| {
            style.spacing.item_spacing = bevy_egui::egui::vec2(Self::SPACE_2, Self::SPACE_2);
            style.spacing.button_padding = bevy_egui::egui::vec2(Self::SPACE_3, 7.0);
            style.spacing.interact_size.y = Self::MIN_TARGET;
            style.spacing.window_margin = Margin::same(20);
            style.spacing.menu_margin = Margin::same(6);
            style.spacing.indent = Self::SPACE_4;
        });
    }
}
