use std::collections::{HashMap, hash_map::Entry};

use bevy_egui::egui::{self, Id, Ui};
use thiserror::Error;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeProvider,
};
use crate::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot, DesktopEnumValue};
use crate::desktop::file_handle::DesktopFileEnumPurpose;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
    DesktopRequest,
};
use crate::presentation::board::enums::PresentationBoardEnumCameraAction;
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::board::view::{
    PresentationBoardAction, PresentationBoardPickingProvider, PresentationBoardView,
    PresentationBoardViewError, PresentationBoardViewInput,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;
use crate::scene::picking::ScenePickingViewport;

#[derive(Debug, Error)]
pub enum PresentationAppError {
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error("Action {identity:?} was not admitted: {source}")]
    Admission {
        identity: DesktopRequestId,
        source: DesktopApplicationError,
    },
    #[error(transparent)]
    Board(#[from] PresentationBoardViewError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationAppEnumAction {
    Refresh,
    SelectFile,
    ConfirmQuit,
}

impl PresentationAppEnumAction {
    fn command(self, file_owner: DesktopMessagesEntityId) -> DesktopCommand {
        let operation = match self {
            Self::Refresh => DesktopEnumCommand::Bootstrap,
            Self::SelectFile => DesktopEnumCommand::FileSelect {
                owner: file_owner,
                purpose: DesktopFileEnumPurpose::ReadFile,
            },
            Self::ConfirmQuit => DesktopEnumCommand::Quit,
        };
        let revision = match self {
            Self::SelectFile => Some(crate::desktop::identity::DesktopRevision::ZERO),
            Self::Refresh | Self::ConfirmQuit => None,
        };
        DesktopCommand::new(operation, revision)
    }
}

struct PresentationAppPending {
    action: PresentationAppEnumAction,
    receipt: DesktopApplicationReceipt,
}

struct PresentationAppRetained {
    request: DesktopRequest,
    admitted: bool,
}

pub struct PresentationAppFrame {
    viewport: Option<ScenePickingViewport>,
    board_actions: Vec<PresentationBoardAction>,
    camera_action: Option<PresentationBoardEnumCameraAction>,
}

impl PresentationAppFrame {
    pub const fn viewport(&self) -> Option<ScenePickingViewport> {
        self.viewport
    }

    pub fn board_actions(&self) -> &[PresentationBoardAction] {
        &self.board_actions
    }

    pub const fn camera_action(&self) -> Option<PresentationBoardEnumCameraAction> {
        self.camera_action
    }
}

pub struct PresentationApp {
    snapshot: Option<DesktopBootstrapSnapshot>,
    board: PresentationBoardView,
    file_owner: DesktopMessagesEntityId,
    action_frame: Option<u64>,
    retained: HashMap<PresentationAppEnumAction, PresentationAppRetained>,
    pending: Vec<PresentationAppPending>,
    status: String,
    failure: Option<String>,
    quit_confirmation: bool,
    quit_focus_pending: bool,
    quit_ready: bool,
    quit_retry_frame: Option<u64>,
    theme_initialized: bool,
}

impl Default for PresentationApp {
    fn default() -> Self {
        Self::new()
    }
}

impl PresentationApp {
    pub fn new() -> Self {
        Self {
            snapshot: None,
            board: PresentationBoardView::new(),
            file_owner: DesktopMessagesEntityId::new(),
            action_frame: None,
            retained: HashMap::new(),
            pending: Vec::new(),
            status: "Loading local application state".to_owned(),
            failure: None,
            quit_confirmation: false,
            quit_focus_pending: false,
            quit_ready: false,
            quit_retry_frame: None,
            theme_initialized: false,
        }
    }

    pub fn accept_snapshot(&mut self, snapshot: DesktopBootstrapSnapshot) {
        self.snapshot = Some(snapshot);
        self.status = "Local application state is ready".to_owned();
    }

    pub const fn snapshot(&self) -> Option<&DesktopBootstrapSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub const fn is_quit_confirmation_open(&self) -> bool {
        self.quit_confirmation
    }

    pub const fn is_quit_ready(&self) -> bool {
        self.quit_ready
    }

    pub fn has_pending_work(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn request_quit(&mut self) {
        if !self.quit_confirmation {
            self.quit_confirmation = true;
            self.quit_focus_pending = true;
            self.board.cancel_gesture();
        }
    }

    pub fn activate<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        action: PresentationAppEnumAction,
        frame: u64,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<DesktopRequestId, PresentationAppError> {
        if self.action_frame != Some(frame) {
            self.action_frame = Some(frame);
            self.retained.clear();
        }
        if let Some(retained) = self.retained.get(&action)
            && retained.admitted
        {
            return Ok(retained.request.request_id());
        }
        if let Some(pending) = self.pending.iter().find(|pending| pending.action == action) {
            return Ok(pending.receipt.request_id());
        }
        let retained = match self.retained.entry(action) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(PresentationAppRetained {
                request: port.request(action.command(self.file_owner))?,
                admitted: false,
            }),
        };
        let identity = retained.request.request_id();
        let receipt = port
            .try_submit(&retained.request)
            .map_err(|source| PresentationAppError::Admission { identity, source })?;
        retained.admitted = true;
        self.pending
            .push(PresentationAppPending { action, receipt });
        self.failure = None;
        Ok(identity)
    }

    pub fn poll(&mut self) {
        for pending in &self.pending {
            match pending.receipt.pending_notice() {
                Ok(Some(notice)) => self.failure = Some(notice.message().to_owned()),
                Ok(None) | Err(DesktopApplicationError::Busy) => {}
                Err(error) => self.failure = Some(error.to_string()),
            }
        }
        let mut completed = Vec::new();
        self.pending.retain(|pending| {
            if let Some(response) = pending.receipt.response() {
                completed.push((pending.action, response.clone()));
                false
            } else {
                true
            }
        });
        for (action, response) in completed {
            match response {
                DesktopMessagesEnumResult::Value { value, .. } => match value {
                    DesktopEnumValue::Bootstrap(snapshot)
                    | DesktopEnumValue::StreamSubscribe { snapshot, .. }
                    | DesktopEnumValue::StreamResync { snapshot, .. } => {
                        self.accept_snapshot(snapshot);
                    }
                    DesktopEnumValue::FileSelect(Some(_)) => {
                        self.status = "Local file selection is ready".to_owned();
                    }
                    DesktopEnumValue::FileSelect(None) => {
                        self.status = "No file selected".to_owned();
                    }
                    DesktopEnumValue::StreamAcknowledge => {}
                },
                DesktopMessagesEnumResult::Failure(failure) => {
                    self.failure = Some(failure.message().to_owned());
                }
                DesktopMessagesEnumResult::Cancelled => {
                    self.status = "Action canceled".to_owned();
                }
                DesktopMessagesEnumResult::Stopped => {
                    self.status = "Owned work stopped".to_owned();
                }
                DesktopMessagesEnumResult::Paused => {
                    self.status = "Owned work paused".to_owned();
                }
                DesktopMessagesEnumResult::QuitReady => {
                    if action == PresentationAppEnumAction::ConfirmQuit {
                        self.quit_ready = true;
                        self.status = "Application is ready to close".to_owned();
                    } else {
                        self.failure =
                            Some("The application returned an unexpected quit receipt".to_owned());
                    }
                }
                DesktopMessagesEnumResult::ReconciliationRequired => {
                    self.failure =
                        Some("Refresh local state before retrying this action".to_owned());
                }
            }
        }
    }

    pub fn show<C, W, P>(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
        scene: Option<&PresentationBoardScene>,
        picker: &mut P,
    ) -> Result<PresentationAppFrame, PresentationAppError>
    where
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
        P: PresentationBoardPickingProvider,
    {
        if !self.theme_initialized {
            PresentationThemeTokens::apply(ui.ctx());
            self.theme_initialized = true;
            ui.ctx()
                .request_discard("apply initial native theme before layout");
        }
        self.poll();
        let frame = ui.ctx().cumulative_frame_nr();
        let mut actions = Vec::new();
        let mut camera_action = None;
        egui::Panel::top("chess.header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("CHESS")
                        .strong()
                        .color(PresentationThemeTokens::ACCENT),
                );
                ui.label(
                    egui::RichText::new("Offline study and play")
                        .color(PresentationThemeTokens::MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Quit").clicked() {
                        self.request_quit();
                    }
                    if ui.button("Refresh state").clicked() {
                        actions.push(PresentationAppEnumAction::Refresh);
                    }
                    if ui.button("Select local file").clicked() {
                        actions.push(PresentationAppEnumAction::SelectFile);
                    }
                });
            });
        });
        egui::Panel::right("chess.inspector")
            .default_size(PresentationThemeTokens::PANEL_WIDTH)
            .min_size(PresentationThemeTokens::PANEL_WIDTH)
            .show(ui, |ui| {
                ui.heading("Position");
                if let Some(snapshot) = &self.snapshot {
                    ui.label(format!(
                        "{} to move{}",
                        PresentationBoardSemanticGrid::color_name(snapshot.board().turn()),
                        if snapshot.board().is_check() { ", in check" } else { "" }
                    ));
                    ui.label(format!("{} legal moves", snapshot.board().legal_moves().len()));
                    if let Some(square) = self.board.selected() {
                        ui.separator();
                        ui.label(PresentationBoardSemanticGrid::label(square, snapshot.board(), Some(square)));
                        let destinations = snapshot.board().legal_moves().iter()
                            .filter(|movement| movement.origin() == Some(square))
                            .map(|movement| movement.destination().to_string())
                            .collect::<Vec<_>>();
                        ui.label(if destinations.is_empty() {
                            "No legal destinations from this square".to_owned()
                        } else {
                            format!("Legal destinations: {}", destinations.join(", "))
                        });
                    }
                    ui.separator();
                    ui.heading("Reckless");
                    match snapshot.engine() {
                        DesktopEnumEngineSnapshot::Ready { identity } => {
                            ui.label(identity.name());
                            ui.label(egui::RichText::new(identity.author()).color(PresentationThemeTokens::MUTED));
                            ui.label(format!("{} supported engine options", identity.supported_options().len()));
                        }
                        DesktopEnumEngineSnapshot::Unavailable { message } => {
                            ui.colored_label(PresentationThemeTokens::WARNING, "Engine unavailable");
                            ui.label(message);
                        }
                    }
                    ui.label(format!("{} usable hardware threads", snapshot.cpu_threads()));
                } else {
                    ui.label("Waiting for the local application");
                }
                ui.separator();
                ui.heading("Camera");
                ui.horizontal_wrapped(|ui| {
                    for (label, action) in [
                        ("Rotate left", PresentationBoardEnumCameraAction::RotateLeft),
                        ("Rotate right", PresentationBoardEnumCameraAction::RotateRight),
                        ("Tilt up", PresentationBoardEnumCameraAction::TiltUp),
                        ("Tilt down", PresentationBoardEnumCameraAction::TiltDown),
                        ("Zoom in", PresentationBoardEnumCameraAction::ZoomIn),
                        ("Zoom out", PresentationBoardEnumCameraAction::ZoomOut),
                        ("Reset view", PresentationBoardEnumCameraAction::Reset),
                        ("Flip board", PresentationBoardEnumCameraAction::Flip),
                    ] {
                        if ui.add_enabled(scene.is_some(), egui::Button::new(label)).clicked() {
                            camera_action = Some(action);
                            self.board.cancel_gesture();
                        }
                    }
                });
                ui.separator();
                ui.label(egui::RichText::new("Use Tab to enter the board, arrow keys to move focus, and Enter or Space to select a square.").color(PresentationThemeTokens::MUTED));
            });
        let retry_quit = self.failure.is_some()
            && self
                .pending
                .iter()
                .any(|pending| pending.action == PresentationAppEnumAction::ConfirmQuit);
        let status = self.failure.as_deref().unwrap_or(&self.status);
        let mut retry = false;
        egui::Panel::bottom("chess.status").show(ui, |ui| {
            let response = ui.label(status);
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(egui::accesskit::Role::Status);
                node.set_live(egui::accesskit::Live::Polite);
            });
            if retry_quit {
                retry = ui.button("Retry quit").clicked();
            }
        });
        if retry && self.quit_retry_frame != Some(frame) {
            port.retry_quit()?;
            self.quit_retry_frame = Some(frame);
        }
        let board_frame = egui::CentralPanel::default()
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("White");
                    ui.label(
                        egui::RichText::new("No active game").color(PresentationThemeTokens::MUTED),
                    );
                });
                let result = match &self.snapshot {
                    Some(snapshot) => self
                        .board
                        .show(
                            ui,
                            PresentationBoardViewInput::new(snapshot.board(), scene, picker)
                                .with_enabled(!self.quit_confirmation),
                        )
                        .map(Some),
                    None => {
                        ui.label("Loading the saved position");
                        Ok(None)
                    }
                };
                ui.strong("Black");
                result
            })
            .inner?;
        if self.quit_confirmation {
            let mut cancel = false;
            let mut confirm = false;
            let modal = egui::Modal::new(Id::new("chess.quit.confirmation")).show(ui.ctx(), |ui| {
                ui.heading("Quit Chess?");
                ui.label("The application will finish its owned shutdown before closing.");
                ui.horizontal(|ui| {
                    let button = ui.button("Keep open");
                    if self.quit_focus_pending {
                        button.request_focus();
                        self.quit_focus_pending = false;
                    }
                    cancel = button.clicked();
                    confirm = ui.button("Confirm quit").clicked();
                });
            });
            if confirm {
                actions.push(PresentationAppEnumAction::ConfirmQuit);
            }
            if cancel || confirm || modal.should_close() {
                self.quit_confirmation = false;
            }
        }
        for action in actions {
            self.activate(action, frame, port)?;
        }
        Ok(PresentationAppFrame {
            viewport: board_frame.as_ref().map(|frame| frame.viewport()),
            board_actions: board_frame.map_or_else(Vec::new, |frame| frame.actions().to_vec()),
            camera_action,
        })
    }
}
