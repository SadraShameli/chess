use bevy_egui::egui::Ui;

use crate::presentation::cursor::PresentationStudyCursor;
use crate::presentation::opening::controller::{
    PresentationOpeningController, PresentationOpeningIntent,
};
use crate::presentation::opening::explorer::PresentationOpeningExplorer;
use crate::presentation::report::controller::{
    PresentationReportController, PresentationReportIntent,
};
use crate::presentation::report::view::PresentationReportView;

#[derive(Default)]
pub struct PresentationStudy {
    cursor: PresentationStudyCursor,
    report: PresentationReportController,
    opening: PresentationOpeningController,
}

impl PresentationStudy {
    pub const fn new() -> Self {
        Self {
            cursor: PresentationStudyCursor::new(),
            report: PresentationReportController::new(),
            opening: PresentationOpeningController::new(),
        }
    }

    pub const fn cursor(&self) -> PresentationStudyCursor {
        self.cursor
    }

    pub const fn report_mut(&mut self) -> &mut PresentationReportController {
        &mut self.report
    }

    pub const fn opening_mut(&mut self) -> &mut PresentationOpeningController {
        &mut self.opening
    }

    pub fn show_report(&mut self, ui: &mut Ui) -> Option<PresentationReportIntent> {
        PresentationReportView::show(ui, &self.report, &mut self.cursor)
    }

    pub fn show_opening(&mut self, ui: &mut Ui) -> Option<PresentationOpeningIntent> {
        PresentationOpeningExplorer::show_tabs(ui, &mut self.opening, self.cursor)
    }
}
