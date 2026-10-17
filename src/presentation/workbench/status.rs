use bevy_egui::egui::{self, Ui};

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationWorkbenchStatusEnumState {
    Loading(String),
    Empty(String),
    Error(String),
    Result(String),
}

impl PresentationWorkbenchStatusEnumState {
    pub fn text(&self) -> &str {
        match self {
            Self::Loading(text) | Self::Empty(text) | Self::Error(text) | Self::Result(text) => {
                text
            }
        }
    }

    pub const fn prefix(&self) -> &'static str {
        match self {
            Self::Loading(_) => "Loading",
            Self::Empty(_) => "Empty",
            Self::Error(_) => "Error",
            Self::Result(_) => "Result",
        }
    }

    pub fn announcement(&self) -> String {
        format!("{}: {}", self.prefix(), self.text())
    }

    const fn live(&self) -> egui::accesskit::Live {
        match self {
            Self::Error(_) => egui::accesskit::Live::Assertive,
            Self::Loading(_) | Self::Empty(_) | Self::Result(_) => egui::accesskit::Live::Polite,
        }
    }

    pub fn show(&self, ui: &mut Ui) -> egui::Response {
        let response = ui.label(self.text());
        let live = self.live();
        let prefix = self.prefix();
        let text = self.text().to_owned();
        ui.ctx().accesskit_node_builder(response.id, |node| {
            node.set_role(egui::accesskit::Role::Status);
            node.set_label(text);
            node.set_live(live);
            node.set_description(prefix);
        });
        response
    }
}

pub struct PresentationWorkbenchStatusAnnouncer {
    interval: ClockDuration,
    last: Option<ClockInstant>,
    last_text: Option<String>,
}

impl PresentationWorkbenchStatusAnnouncer {
    pub const ANALYSIS_INTERVAL: ClockDuration = ClockDuration::from_nanos(1_000_000_000);

    pub const fn new(interval: ClockDuration) -> Self {
        Self {
            interval,
            last: None,
            last_text: None,
        }
    }

    pub fn offer(
        &mut self,
        now: ClockInstant,
        state: &PresentationWorkbenchStatusEnumState,
    ) -> Result<Option<String>, ClockDurationError> {
        let text = state.announcement();
        if self.last_text.as_deref() == Some(text.as_str()) {
            return Ok(None);
        }
        let urgent = matches!(state, PresentationWorkbenchStatusEnumState::Error(_));
        if !urgent
            && let Some(last) = self.last
            && now.elapsed_since(last)?.as_nanos() < self.interval.as_nanos()
        {
            return Ok(None);
        }
        self.last = Some(now);
        self.last_text = Some(text.clone());
        Ok(Some(text))
    }
}
