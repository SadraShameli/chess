use bevy_egui::egui::{self, Ui};

use crate::engine::package::manifest::{EnginePackageEnumProfile, EnginePackageManifest};
use crate::engine::package::selector::EnginePackageSelected;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationWorkbenchEngineInformationEnumState {
    Ready,
    Diagnostic(String),
    SetupRequired(String),
    Retrying,
    RecoveredPaused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationWorkbenchEngineInformationEnumIntent {
    Retry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationWorkbenchEngineInformation {
    profile: &'static str,
    target: &'static str,
    executable: String,
    digest: String,
    fallback: Option<String>,
    state: PresentationWorkbenchEngineInformationEnumState,
}

impl PresentationWorkbenchEngineInformation {
    pub fn from_package(
        package: &EnginePackageSelected,
        state: PresentationWorkbenchEngineInformationEnumState,
    ) -> Self {
        Self {
            profile: package.profile().label(),
            target: package.target().triple(),
            executable: package
                .executable()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            digest: package.digest().to_string(),
            fallback: package.fallback().map(|fallback| {
                format!(
                    "Requested {} was not used: {}",
                    fallback.requested().label(),
                    fallback.reasons().join("; ")
                )
            }),
            state,
        }
    }

    pub fn unavailable(state: PresentationWorkbenchEngineInformationEnumState) -> Self {
        Self {
            profile: EnginePackageEnumProfile::Portable.label(),
            target: "unavailable",
            executable: String::new(),
            digest: String::new(),
            fallback: None,
            state,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Engine: Reckless {}", EnginePackageManifest::VERSION),
            format!("Source commit: {}", EnginePackageManifest::COMMIT),
            format!("Source: {}", EnginePackageManifest::REPOSITORY),
            format!("Network: {}", EnginePackageManifest::NETWORK_NAME),
            format!("Network SHA-256: {}", EnginePackageManifest::NETWORK_SHA256),
            format!("Profile: {}", self.profile),
            format!("Target: {}", self.target),
        ];
        if !self.executable.is_empty() {
            lines.push(format!("Executable: {}", self.executable));
            lines.push(format!("Binary SHA-256: {}", self.digest));
        }
        if let Some(fallback) = &self.fallback {
            lines.push(fallback.clone());
        }
        lines.push(self.status_line());
        lines
    }

    pub fn status_line(&self) -> String {
        match &self.state {
            PresentationWorkbenchEngineInformationEnumState::Ready => "Status: ready".to_owned(),
            PresentationWorkbenchEngineInformationEnumState::Diagnostic(message) => {
                format!("Diagnostic: {message}")
            }
            PresentationWorkbenchEngineInformationEnumState::SetupRequired(message) => {
                format!("Setup required: {message}")
            }
            PresentationWorkbenchEngineInformationEnumState::Retrying => {
                "Status: retrying the engine".to_owned()
            }
            PresentationWorkbenchEngineInformationEnumState::RecoveredPaused => {
                "Status: recovered; live games stay paused until you resume them".to_owned()
            }
        }
    }

    pub fn show(&self, ui: &mut Ui) -> Option<PresentationWorkbenchEngineInformationEnumIntent> {
        let mut intent = None;
        egui::CollapsingHeader::new("Engine Information")
            .default_open(true)
            .show(ui, |ui| {
                for line in self.lines() {
                    ui.label(line);
                }
                let can_retry = matches!(
                    self.state,
                    PresentationWorkbenchEngineInformationEnumState::Diagnostic(_)
                        | PresentationWorkbenchEngineInformationEnumState::SetupRequired(_)
                );
                if can_retry && ui.button("Retry Engine").clicked() {
                    intent = Some(PresentationWorkbenchEngineInformationEnumIntent::Retry);
                }
            });
        intent
    }
}
