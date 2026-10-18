use bevy_egui::egui::{Context, FontFamily, FontId, TextStyle};

pub struct PresentationThemeTypography;

impl PresentationThemeTypography {
    pub const UI_POINTS: f32 = 13.0;
    pub const BODY_POINTS: f32 = 14.0;
    pub const CAPTION_POINTS: f32 = 12.0;
    pub const MICRO_POINTS: f32 = 11.0;
    pub const SUBHEAD_POINTS: f32 = 14.0;
    pub const HEADING_POINTS: f32 = 18.0;
    pub const TITLE_POINTS: f32 = 24.0;
    pub const CLOCK_POINTS: f32 = 24.0;
    pub const INPUT_POINTS: f32 = 16.0;
    pub const SECTION_POINTS: f32 = 20.0;
    pub const LOGO_POINTS: f32 = 24.0;

    pub fn apply(context: &Context) {
        context.all_styles_mut(|style| {
            for (text_style, points, family) in [
                (TextStyle::Body, Self::UI_POINTS, FontFamily::Proportional),
                (TextStyle::Button, Self::UI_POINTS, FontFamily::Proportional),
                (
                    TextStyle::Small,
                    Self::CAPTION_POINTS,
                    FontFamily::Proportional,
                ),
                (
                    TextStyle::Heading,
                    Self::HEADING_POINTS,
                    FontFamily::Proportional,
                ),
                (TextStyle::Monospace, Self::UI_POINTS, FontFamily::Monospace),
            ] {
                style
                    .text_styles
                    .insert(text_style, FontId::new(points, family));
            }
        });
    }
}
