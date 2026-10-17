use crate::desktop::report_commands::DesktopReportView;
use crate::report::job::{ReportJobEnumFreshness, ReportJobEnumState, ReportJobId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationReportIntent {
    Cancel(ReportJobId),
    PreviewReanalysis(ReportJobId),
    ConfirmReplacement(ReportJobId),
    DiscardReplacement(ReportJobId),
}

#[derive(Default)]
pub struct PresentationReportController {
    view: Option<DesktopReportView>,
    replacement_preview: bool,
    failures: Vec<String>,
}

impl PresentationReportController {
    pub const fn new() -> Self {
        Self {
            view: None,
            replacement_preview: false,
            failures: Vec::new(),
        }
    }

    pub const MAX_FAILURES: usize = 8;

    pub fn observe_failures(&mut self, mut failures: Vec<String>) {
        failures.truncate(Self::MAX_FAILURES);
        self.failures = failures;
    }

    pub fn failures(&self) -> &[String] {
        &self.failures
    }

    pub fn observe(&mut self, view: DesktopReportView) {
        if self
            .view
            .as_ref()
            .is_none_or(|current| current.id != view.id)
        {
            self.replacement_preview = false;
        }
        self.view = Some(view);
    }

    pub const fn view(&self) -> Option<&DesktopReportView> {
        self.view.as_ref()
    }

    pub const fn has_replacement_preview(&self) -> bool {
        self.replacement_preview
    }

    pub const fn set_replacement_preview(&mut self, pending: bool) {
        self.replacement_preview = pending;
    }

    pub fn can_cancel(&self) -> bool {
        self.view.as_ref().is_some_and(|view| {
            matches!(
                view.state,
                ReportJobEnumState::Queued
                    | ReportJobEnumState::Running
                    | ReportJobEnumState::Pausing
                    | ReportJobEnumState::Paused
            )
        })
    }

    pub fn can_reanalyze(&self) -> bool {
        self.view.as_ref().is_some_and(|view| {
            !self.replacement_preview
                && (matches!(
                    view.state,
                    ReportJobEnumState::Completed | ReportJobEnumState::Cancelled
                ) || view.freshness == ReportJobEnumFreshness::Outdated)
        })
    }
}
