use bevy_egui::egui;
use chess::desktop::identity::DesktopRevision;
use egui_kittest::{Harness, kittest::Queryable};

use crate::presentation_font::PresentationFontFixture;

struct PresentationCompatibilityFixture {
    needs_fonts: bool,
    revision: DesktopRevision,
    submissions: Vec<DesktopRevision>,
}

impl PresentationCompatibilityFixture {
    const BUTTON_LABEL: &str = "Submit native revision";
    const BUTTON_SIZE: egui::Vec2 = egui::vec2(240.0, 32.0);
    const WINDOW_SIZE: egui::Vec2 = egui::vec2(1280.0, 800.0);
    const PRECISE_REVISION: &str = "9007199254740993";
    const SETTLED_PASSES: usize = 4;
    const RECT_TOLERANCE: f32 = 0.01;

    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            needs_fonts: true,
            revision: Self::PRECISE_REVISION.parse()?,
            submissions: Vec::new(),
        })
    }

    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_fonts {
            ui.ctx().set_fonts(PresentationFontFixture::definitions());
            ui.ctx().request_repaint();
            state.needs_fonts = false;
            return;
        }
        if ui
            .add_sized(Self::BUTTON_SIZE, egui::Button::new(Self::BUTTON_LABEL))
            .clicked()
        {
            state.submissions.push(state.revision);
        }
    }

    fn harness(
        pixels_per_point: f32,
    ) -> Result<Harness<'static, Self>, Box<dyn std::error::Error>> {
        let mut harness = Harness::builder()
            .with_size(Self::WINDOW_SIZE)
            .with_pixels_per_point(pixels_per_point)
            .build_ui_state(Self::show, Self::new()?);
        harness
            .run_ok()
            .ok_or("The initial egui frame did not settle")?;
        Ok(harness)
    }
}

#[test]
fn matching_egui_accesskit_and_keyboard_keep_native_revision_precision()
-> Result<(), Box<dyn std::error::Error>> {
    let mut harness = PresentationCompatibilityFixture::harness(1.0)?;
    let revision = harness.state().revision;
    assert_eq!(
        revision.to_string(),
        PresentationCompatibilityFixture::PRECISE_REVISION
    );

    harness
        .get_by_label(PresentationCompatibilityFixture::BUTTON_LABEL)
        .click_accesskit();
    harness
        .run_ok()
        .ok_or("The AccessKit action did not settle")?;
    assert_eq!(harness.state().submissions, [revision]);

    harness.run_steps(PresentationCompatibilityFixture::SETTLED_PASSES);
    assert_eq!(harness.state().submissions, [revision]);

    harness
        .get_by_label(PresentationCompatibilityFixture::BUTTON_LABEL)
        .focus();
    harness
        .run_ok()
        .ok_or("The keyboard focus request did not settle")?;
    assert!(
        harness
            .get_by_label(PresentationCompatibilityFixture::BUTTON_LABEL)
            .is_focused()
    );
    harness.key_press(egui::Key::Enter);
    harness
        .run_ok()
        .ok_or("The keyboard action did not settle")?;
    assert_eq!(harness.state().submissions, [revision, revision]);
    Ok(())
}

#[test]
fn matching_egui_semantic_hit_targets_use_logical_points_at_each_scale()
-> Result<(), Box<dyn std::error::Error>> {
    for scale in [1.0, 1.5, 2.0] {
        let mut harness = PresentationCompatibilityFixture::harness(scale)?;
        let rectangle = harness
            .get_by_label(PresentationCompatibilityFixture::BUTTON_LABEL)
            .rect();
        assert!(
            (rectangle.width() - PresentationCompatibilityFixture::BUTTON_SIZE.x).abs()
                <= PresentationCompatibilityFixture::RECT_TOLERANCE
        );
        assert!(
            (rectangle.height() - PresentationCompatibilityFixture::BUTTON_SIZE.y).abs()
                <= PresentationCompatibilityFixture::RECT_TOLERANCE
        );
        harness
            .get_by_label(PresentationCompatibilityFixture::BUTTON_LABEL)
            .click();
        harness
            .run_ok()
            .ok_or("The scaled pointer action did not settle")?;
        assert_eq!(harness.state().submissions, [harness.state().revision]);
    }
    Ok(())
}
