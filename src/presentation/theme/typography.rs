use bevy_egui::egui::{Context, FontFamily, FontId, TextStyle};

pub struct PresentationThemeTypography;

impl PresentationThemeTypography {
    pub const BODY_POINTS: f32 = 15.0;
    pub const SMALL_POINTS: f32 = 13.0;
    pub const HEADING_POINTS: f32 = 22.0;

    pub fn apply(context: &Context) {
        context.all_styles_mut(|style| {
            for (text_style, points) in [
                (TextStyle::Body, Self::BODY_POINTS),
                (TextStyle::Button, Self::BODY_POINTS),
                (TextStyle::Small, Self::SMALL_POINTS),
                (TextStyle::Heading, Self::HEADING_POINTS),
            ] {
                style
                    .text_styles
                    .insert(text_style, FontId::new(points, FontFamily::Proportional));
            }
        });
    }
}
