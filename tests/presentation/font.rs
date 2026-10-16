use std::sync::Arc;

use bevy_egui::egui::{FontData, FontDefinitions, FontFamily};

pub(crate) struct PresentationFontFixture;

impl PresentationFontFixture {
    const NAME: &str = "Inter compatibility fixture";

    pub(crate) fn definitions() -> FontDefinitions {
        let mut fonts = FontDefinitions::empty();
        fonts.font_data.insert(
            Self::NAME.to_owned(),
            Arc::new(FontData::from_static(include_bytes!(
                "fixtures/InterVariable.ttf"
            ))),
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.insert(family, vec![Self::NAME.to_owned()]);
        }
        fonts
    }
}
