use std::sync::Arc;

use bevy_egui::egui::{FontData, FontDefinitions, FontFamily};

pub struct PresentationThemeFont;

impl PresentationThemeFont {
    const NAME: &str = "Inter";

    pub fn definitions() -> FontDefinitions {
        let mut fonts = FontDefinitions::empty();
        fonts.font_data.insert(
            Self::NAME.to_owned(),
            Arc::new(FontData::from_static(include_bytes!(
                "../../../resources/fonts/InterVariable.ttf"
            ))),
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.insert(family, vec![Self::NAME.to_owned()]);
        }
        fonts
    }
}
