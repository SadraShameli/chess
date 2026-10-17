pub mod editor;
mod library;

pub use crate::presentation::app::library::PresentationAppLibraryError;

use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;

use bevy_egui::egui::{self, Id, Ui};
use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::state::ClockStateError;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeProvider,
};
use crate::desktop::dto::{
    DesktopBoardSnapshot, DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot, DesktopEnumEvent,
    DesktopEnumValue,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopError, DesktopFailure};
use crate::desktop::game_bridge::DesktopGameBridgeEnumOperation;
use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::game_snapshot::{DesktopGameSnapshot, DesktopGameSnapshotEnumNotice};
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
    DesktopRequest,
};
use crate::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesEnumAid, DesktopPreferencesEnumAudioCategory,
    DesktopPreferencesEnumPatch, DesktopPreferencesEnumSection,
    DesktopPreferencesEnumShortcutAction, DesktopPreferencesPercent,
};
use crate::desktop::puzzle_service::DesktopPuzzleServiceError;
use crate::desktop::stream::DesktopStream;
use crate::game::enums::{GameEnumPhase, GameEnumSeat, GameId};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use crate::game::session::GameSessionEnumInterruption;
use crate::presentation::action::registry::{PresentationActionEnumId, PresentationActionRegistry};
use crate::presentation::analysis::annotations::PresentationAnalysisAnnotationsEnumMarkingInput;
use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal;
use crate::presentation::app::editor::{
    PresentationAppEditor, PresentationAppEditorBoardInput, PresentationAppEditorError,
};
use crate::presentation::app::library::{
    PresentationAppLibrary, PresentationAppLibraryEnumNavigation,
};
use crate::presentation::board::animation::PresentationBoardAnimationFrame;
use crate::presentation::board::enums::PresentationBoardEnumCameraAction;
use crate::presentation::board::markings::{PresentationBoardMark, PresentationBoardMarkRequest};
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::board::recovery::{
    PresentationBoardRecovery, PresentationBoardRecoveryEnumIntent,
};
use crate::presentation::board::toolbar::PresentationBoardToolbar;
use crate::desktop::layout::DesktopLayout;
use crate::desktop::renderer_commands::DesktopRendererCommands;
use crate::presentation::board::view::{
    PresentationBoardAction, PresentationBoardPickingProvider, PresentationBoardView,
    PresentationBoardViewError, PresentationBoardViewFrame, PresentationBoardViewInput,
};
use crate::presentation::engine_panels::{
    PresentationEnginePanels, PresentationEnginePanelsConnection, PresentationEnginePanelsError,
};
use crate::presentation::game::controller::{
    PresentationGameController, PresentationGameControllerError, PresentationGameControllerIntent,
};
use crate::presentation::game::new_game::PresentationGameNewGame;
use crate::presentation::game::play::{
    PresentationGamePlay, PresentationGamePlayClock, PresentationGamePlayEnumOperation,
    PresentationGamePlayError, PresentationGamePlayIntent,
};
use crate::presentation::puzzle::controller::PresentationPuzzleController;
use crate::presentation::study::PresentationStudy;
use crate::presentation::puzzle::panel::{
    PresentationPuzzlePanel, PresentationPuzzlePanelConnection, PresentationPuzzlePanelError,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;
use crate::puzzle::timer::PuzzleTimerEnumReadiness;
use crate::scene::camera::SceneCamera;
use crate::scene::picking::ScenePickingViewport;

#[derive(Debug, Error)]
pub enum PresentationAppError {
    #[error(transparent)]
    Time(#[from] ClockDurationError),
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error("Action {identity:?} was not admitted: {source}")]
    Admission {
        identity: DesktopRequestId,
        source: DesktopApplicationError,
    },
    #[error(transparent)]
    Board(#[from] PresentationBoardViewError),
    #[error(transparent)]
    GameInput(#[from] PresentationGameControllerError),
    #[error(transparent)]
    Protocol(#[from] DesktopError),
    #[error(transparent)]
    GamePanel(#[from] PresentationGamePlayError),
    #[error(transparent)]
    Clock(#[from] ClockStateError),
    #[error(transparent)]
    EnginePanels(#[from] PresentationEnginePanelsError),
    #[error(transparent)]
    PuzzlePanel(#[from] PresentationPuzzlePanelError),
    #[error(transparent)]
    Library(#[from] PresentationAppLibraryError),
    #[error(transparent)]
    Editor(#[from] PresentationAppEditorError),
    #[error("The puzzle interruption was refused: {0}")]
    PuzzleInterruption(#[from] Arc<DesktopPuzzleServiceError>),
    #[error("This game input identity was already used for another command")]
    GameRequestConflict,
    #[error("Wait for pending game actions before starting another action")]
    GameRequestCapacity,
    #[error("The selected game is unavailable")]
    MissingGame,
    #[error("This interruption needs its native recovery action")]
    UnsupportedRecovery,
    #[error("The native stream returned an unexpected synchronization result")]
    UnexpectedStreamResult,
}

#[cfg(test)]
#[path = "../../tests/presentation/app_puzzles.rs"]
mod puzzle_tests;

#[cfg(test)]
#[path = "../../tests/presentation/app_library.rs"]
mod library_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationAppEnumWorkspace {
    Games,
    Puzzles,
    Library,
    Editor,
}

struct PresentationAppPending {
    action: PresentationActionEnumId,
    receipt: DesktopApplicationReceipt,
}

struct PresentationAppGame {
    snapshot: DesktopGameSnapshot,
    controller: PresentationGameController,
    play: PresentationGamePlay,
    last_notice: Option<DesktopGameSnapshotEnumNotice>,
}

struct PresentationAppGameNotice {
    notice: DesktopGameSnapshotEnumNotice,
    visible_since: Option<ClockInstant>,
}

impl PresentationAppGameNotice {
    const DURATION: ClockDuration = ClockDuration::from_nanos(3_000_000_000);

    fn new(notice: DesktopGameSnapshotEnumNotice) -> Self {
        Self {
            notice,
            visible_since: None,
        }
    }

    fn remaining(&mut self, now: ClockInstant) -> Result<ClockDuration, ClockDurationError> {
        let elapsed = now.elapsed_since(*self.visible_since.get_or_insert(now))?;
        if elapsed >= Self::DURATION {
            Ok(ClockDuration::ZERO)
        } else {
            Self::DURATION.checked_sub(elapsed)
        }
    }
}

struct PresentationAppGameRetained {
    request: DesktopRequest,
    board_input: bool,
    admitted: bool,
    completed: bool,
}

struct PresentationAppGamePending {
    input: DesktopRequestId,
    game: Option<GameId>,
    board_input: bool,
    receipt: DesktopApplicationReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PresentationAppEnumInput {
    Create,
    Play(GameId),
    Cancel(GameId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationAppEnumStreamState {
    Ready,
    ResyncRequired,
    RetryRequired,
}

struct PresentationAppStream {
    subscription_id: DesktopRequestId,
    sequence: DesktopRevision,
    acknowledged: DesktopRevision,
    state: PresentationAppEnumStreamState,
}

struct PresentationAppStreamPending {
    request: DesktopRequest,
    receipt: Option<DesktopApplicationReceipt>,
}

struct PresentationAppRetained {
    request: DesktopRequest,
    admitted: bool,
}

pub struct PresentationAppFrame {
    viewport: Option<ScenePickingViewport>,
    board_actions: Vec<PresentationBoardAction>,
    camera_action: Option<PresentationBoardEnumCameraAction>,
    animation: Option<PresentationBoardAnimationFrame>,
    recovery_intent: Option<PresentationBoardRecoveryEnumIntent>,
}

impl PresentationAppFrame {
    pub const fn recovery_intent(&self) -> Option<PresentationBoardRecoveryEnumIntent> {
        self.recovery_intent
    }

    pub const fn animation(&self) -> Option<PresentationBoardAnimationFrame> {
        self.animation
    }

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
    games: Vec<PresentationAppGame>,
    selected_game: Option<GameId>,
    engine_panels: Option<PresentationEnginePanels>,
    puzzle_panel: Option<PresentationPuzzlePanel>,
    study: PresentationStudy,
    library: Option<PresentationAppLibrary>,
    editor: Option<PresentationAppEditor>,
    workspace: PresentationAppEnumWorkspace,
    new_game: Option<PresentationGameNewGame>,
    game_retained: HashMap<DesktopRequestId, PresentationAppGameRetained>,
    game_pending: Vec<PresentationAppGamePending>,
    input_frame: Option<u64>,
    promotion_frame: Option<u64>,
    input_identities: HashMap<PresentationAppEnumInput, DesktopRequestId>,
    new_game_toggle_frame: Option<u64>,
    stream: Option<PresentationAppStream>,
    stream_pending: Option<PresentationAppStreamPending>,
    file_owner: DesktopMessagesEntityId,
    new_game_owner: DesktopMessagesEntityId,
    action_frame: Option<u64>,
    retained: HashMap<PresentationActionEnumId, PresentationAppRetained>,
    pending: Vec<PresentationAppPending>,
    status: String,
    failure: Option<String>,
    game_notice: Option<PresentationAppGameNotice>,
    quit_confirmation: bool,
    quit_modal: PresentationAnalysisEditorModal,
    confirmed_quit_pending: bool,
    quit_ready: bool,
    quit_retry_frame: Option<u64>,
    theme_initialized: bool,
    preferences: DesktopPreferences,
    settings_open: bool,
    settings_toggle_frame: Option<u64>,
    markings_identity: Option<(PresentationAppEnumWorkspace, Option<GameId>, Option<GameId>)>,
    inspector_open: bool,
    inspector_toggle_frame: Option<u64>,
    layout: DesktopLayout,
    layout_reset_requested: bool,
    renderer_commands: DesktopRendererCommands,
    recovery_intent: Option<PresentationBoardRecoveryEnumIntent>,
    render_scale: f32,
    camera_queue: Vec<PresentationBoardEnumCameraAction>,
    navigation_frame: Option<u64>,
}

impl PresentationApp {
    const MAX_GAME_REQUESTS: usize = 128;

    pub fn new(clock: Arc<dyn ClockTimeProvider>) -> Self {
        let preferences = DesktopPreferences::default();
        let mut board =
            PresentationBoardView::with_animation(clock, preferences.motion().is_reduced());
        board.rest_panel(true);
        Self {
            snapshot: None,
            board,
            games: Vec::new(),
            selected_game: None,
            engine_panels: None,
            puzzle_panel: None,
            study: PresentationStudy::new(),
            library: None,
            editor: None,
            workspace: PresentationAppEnumWorkspace::Games,
            new_game: None,
            game_retained: HashMap::new(),
            game_pending: Vec::new(),
            input_frame: None,
            promotion_frame: None,
            input_identities: HashMap::new(),
            new_game_toggle_frame: None,
            stream: None,
            stream_pending: None,
            file_owner: DesktopMessagesEntityId::new(),
            new_game_owner: DesktopMessagesEntityId::new(),
            action_frame: None,
            retained: HashMap::new(),
            pending: Vec::new(),
            status: "Loading local application state".to_owned(),
            failure: None,
            game_notice: None,
            quit_confirmation: false,
            quit_modal: PresentationAnalysisEditorModal::default(),
            confirmed_quit_pending: false,
            quit_ready: false,
            quit_retry_frame: None,
            theme_initialized: false,
            preferences,
            settings_open: false,
            settings_toggle_frame: None,
            markings_identity: None,
            inspector_open: true,
            inspector_toggle_frame: None,
            layout: DesktopLayout::default(),
            layout_reset_requested: false,
            renderer_commands: DesktopRendererCommands::new(),
            recovery_intent: None,
            render_scale: 1.0,
            camera_queue: Vec::new(),
            navigation_frame: None,
        }
    }

    pub fn mount_engine(&mut self, connection: PresentationEnginePanelsConnection) {
        self.engine_panels = Some(PresentationEnginePanels::new(connection));
    }

    pub fn engine_panels(&self) -> Option<&PresentationEnginePanels> {
        self.engine_panels.as_ref()
    }

    pub fn mount_puzzles(&mut self, connection: PresentationPuzzlePanelConnection) {
        self.puzzle_panel = Some(PresentationPuzzlePanel::new(connection));
    }

    pub fn mount_library(&mut self, workspace: DesktopMessagesEntityId) {
        self.library = Some(PresentationAppLibrary::new(workspace));
    }

    pub fn synchronize_library<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let target = match &mut self.library {
            Some(library) => library.synchronize(port)?,
            None => None,
        };
        if let Some(target) = target {
            self.apply_library_navigation(target, port)?;
        }
        Ok(())
    }

    fn apply_library_navigation<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        target: PresentationAppLibraryEnumNavigation,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        match target {
            PresentationAppLibraryEnumNavigation::OpenSaved(intent) => {
                self.select_workspace_unguarded(PresentationAppEnumWorkspace::Library, port)?;
                if let Some(library) = &mut self.library {
                    library.dispatch_panel(*intent, port)?;
                }
            }
            PresentationAppLibraryEnumNavigation::Workspace(workspace) => {
                self.select_workspace_unguarded(workspace, port)?
            }
            PresentationAppLibraryEnumNavigation::NewGame => {
                self.select_workspace_unguarded(PresentationAppEnumWorkspace::Games, port)?;
                self.new_game = Some(PresentationGameNewGame::new());
                self.settings_open = false;
            }
            PresentationAppLibraryEnumNavigation::Game(game) => {
                self.select_workspace_unguarded(PresentationAppEnumWorkspace::Games, port)?;
                self.selected_game = Some(game);
                self.new_game = None;
                self.board.cancel_gesture();
            }
            PresentationAppLibraryEnumNavigation::Quit => self.request_quit_unguarded(),
        }
        Ok(())
    }

    fn navigate<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        target: PresentationAppLibraryEnumNavigation,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        if self.guard_editor_navigation(&target)? {
            return self.synchronize_editor(port);
        }
        self.navigate_library(target, port)
    }

    fn navigate_library<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        target: PresentationAppLibraryEnumNavigation,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let target = match &mut self.library {
            Some(library) => {
                let target = library.request_navigation(target)?;
                if library.has_guard() {
                    self.settings_open = false;
                }
                target
            }
            None => Some(target),
        };
        if let Some(target) = target {
            self.apply_library_navigation(target, port)?;
        }
        Ok(())
    }

    pub const fn study(&self) -> &PresentationStudy {
        &self.study
    }

    pub const fn study_mut(&mut self) -> &mut PresentationStudy {
        &mut self.study
    }

    pub const fn puzzle_panel(&self) -> Option<&PresentationPuzzlePanel> {
        self.puzzle_panel.as_ref()
    }

    pub const fn workspace(&self) -> PresentationAppEnumWorkspace {
        self.workspace
    }

    pub fn is_puzzle_input_ready(&self) -> bool {
        self.workspace == PresentationAppEnumWorkspace::Puzzles
            && !self.settings_open
            && !self.quit_confirmation
            && self.new_game.is_none()
            && self
                .puzzle_panel
                .as_ref()
                .and_then(PresentationPuzzlePanel::controller)
                .is_none_or(|controller| !controller.is_confirming_end())
    }

    pub fn report_failure(&mut self, error: PresentationAppError) {
        self.failure = Some(error.to_string());
    }

    pub fn select_workspace<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        workspace: PresentationAppEnumWorkspace,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        if workspace == self.workspace {
            return Ok(());
        }
        self.navigate(
            PresentationAppLibraryEnumNavigation::Workspace(workspace),
            port,
        )
    }

    fn select_workspace_unguarded<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        workspace: PresentationAppEnumWorkspace,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        if workspace == PresentationAppEnumWorkspace::Editor
            && self.workspace != PresentationAppEnumWorkspace::Editor
            && let Some(editor) = &mut self.editor
        {
            editor.source_workspace = self.workspace;
        }
        if self.workspace == PresentationAppEnumWorkspace::Puzzles
            && let Some(panel) = &mut self.puzzle_panel
        {
            panel.report_readiness(PuzzleTimerEnumReadiness::Hidden, port)?;
        }
        self.workspace = workspace;
        self.board.cancel_gesture();
        self.board.set_selected(None);
        if workspace != PresentationAppEnumWorkspace::Games {
            self.new_game = None;
            self.settings_open = false;
            if workspace == PresentationAppEnumWorkspace::Puzzles
                && let Some(panel) = &mut self.puzzle_panel
                && panel.inventory().is_none()
                && panel.controller().is_none()
                && !panel.has_pending_work()
            {
                panel.first_page(port)?;
            }
        }
        Ok(())
    }

    pub fn synchronize_puzzles<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        if let Some(panel) = &mut self.puzzle_panel {
            let key = |controller: &PresentationPuzzleController| {
                let context = controller.snapshot().context();
                (context.session_id(), context.revision())
            };
            let previous = panel.controller().map(key);
            panel.synchronize(port)?;
            if previous != panel.controller().map(key) {
                self.board.cancel_gesture();
            }
        }
        Ok(())
    }

    pub fn report_puzzle_readiness<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        readiness: PuzzleTimerEnumReadiness,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let input_ready = self.is_puzzle_input_ready();
        if let Some(panel) = &mut self.puzzle_panel {
            let readiness = if self.workspace != PresentationAppEnumWorkspace::Puzzles {
                PuzzleTimerEnumReadiness::Hidden
            } else if readiness == PuzzleTimerEnumReadiness::Ready && !input_ready {
                PuzzleTimerEnumReadiness::BoardNotReady
            } else {
                readiness
            };
            panel.report_readiness(readiness, port)?;
        }
        Ok(())
    }

    pub fn accept_snapshot(&mut self, snapshot: DesktopBootstrapSnapshot) {
        self.snapshot = Some(snapshot);
        self.status = "Local application state is ready".to_owned();
    }

    pub const fn snapshot(&self) -> Option<&DesktopBootstrapSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn status(&self) -> &str {
        self.game_notice
            .as_ref()
            .map_or(&self.status, |notice| notice.notice.message())
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

    pub fn take_confirmed_quit_request(&mut self) -> bool {
        std::mem::take(&mut self.confirmed_quit_pending)
    }

    pub fn has_pending_work(&self) -> bool {
        self.editor
            .as_ref()
            .is_some_and(PresentationAppEditor::has_pending_work)
            || self
                .library
                .as_ref()
                .is_some_and(PresentationAppLibrary::has_pending_work)
            || self
                .engine_panels
                .as_ref()
                .is_some_and(PresentationEnginePanels::has_pending_work)
            || !self.pending.is_empty()
            || !self.game_pending.is_empty()
            || self
                .puzzle_panel
                .as_ref()
                .is_some_and(PresentationPuzzlePanel::has_pending_work)
            || self.stream_pending.as_ref().is_some_and(|pending| {
                pending.receipt.is_some()
                    || self.stream.as_ref().is_some_and(|stream| {
                        stream.state != PresentationAppEnumStreamState::RetryRequired
                    })
            })
    }

    pub fn game_snapshot(&self) -> Option<&DesktopGameSnapshot> {
        if self.workspace != PresentationAppEnumWorkspace::Games {
            return None;
        }
        Self::selected_game_snapshot(&self.games, self.selected_game)
    }

    fn selected_game_snapshot(
        games: &[PresentationAppGame],
        selected: Option<GameId>,
    ) -> Option<&DesktopGameSnapshot> {
        games
            .iter()
            .find(|game| Some(game.snapshot.id()) == selected)
            .map(|game| &game.snapshot)
    }

    pub fn board_snapshot(&self) -> Option<&DesktopBoardSnapshot> {
        Self::board_for_workspace(
            self.workspace,
            Self::selected_game_snapshot(&self.games, self.selected_game),
            self.snapshot.as_ref(),
            self.puzzle_panel.as_ref(),
            self.library.as_ref(),
        )
    }

    fn board_for_workspace<'a>(
        workspace: PresentationAppEnumWorkspace,
        game: Option<&'a DesktopGameSnapshot>,
        bootstrap: Option<&'a DesktopBootstrapSnapshot>,
        puzzles: Option<&'a PresentationPuzzlePanel>,
        library: Option<&'a PresentationAppLibrary>,
    ) -> Option<&'a DesktopBoardSnapshot> {
        match workspace {
            PresentationAppEnumWorkspace::Editor => None,
            PresentationAppEnumWorkspace::Games => game
                .map(DesktopGameSnapshot::board)
                .or_else(|| bootstrap.map(DesktopBootstrapSnapshot::board)),
            PresentationAppEnumWorkspace::Puzzles => {
                puzzles.and_then(PresentationPuzzlePanel::board)
            }
            PresentationAppEnumWorkspace::Library => {
                library.and_then(PresentationAppLibrary::board)
            }
        }
    }

    pub fn accept_game_snapshot(&mut self, snapshot: DesktopGameSnapshot) -> bool {
        if let Some(library) = &mut self.library {
            library.observe_live_source(&snapshot);
        }
        let id = snapshot.id();
        let notice = snapshot.notice();
        if let Some(game) = self.games.iter_mut().find(|game| game.snapshot.id() == id) {
            let has_new_notice = notice.is_some_and(|notice| {
                game.last_notice
                    .is_none_or(|last| notice.revision() > last.revision())
            });
            if snapshot.revision() < game.snapshot.revision()
                || (snapshot.revision() == game.snapshot.revision()
                    && (snapshot.captured_at() < game.snapshot.captured_at()
                        || snapshot.captured_at() == game.snapshot.captured_at()
                            && !has_new_notice))
            {
                return false;
            }
            if game.controller.synchronize(&snapshot)
                && self.selected_game == Some(id)
                && self.workspace == PresentationAppEnumWorkspace::Games
            {
                self.board.cancel_gesture();
            }
            if let Some(notice) = notice.filter(|_| has_new_notice) {
                self.game_notice = Some(PresentationAppGameNotice::new(notice));
                game.last_notice = Some(notice);
            }
            game.snapshot = snapshot;
        } else {
            if let Some(notice) = notice {
                self.game_notice = Some(PresentationAppGameNotice::new(notice));
            }
            self.games.push(PresentationAppGame {
                snapshot,
                controller: PresentationGameController::new(),
                play: PresentationGamePlay::new(),
                last_notice: notice,
            });
            if self.selected_game.is_none() {
                self.selected_game = Some(id);
            }
        }
        self.status = "The native game state is ready".to_owned();
        true
    }

    pub fn activate_game<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        operation: DesktopGameBridgeEnumOperation,
        input: DesktopRequestId,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<DesktopRequestId, PresentationAppError> {
        self.activate_game_request(operation, input, false, port)
    }

    fn activate_game_request<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        operation: DesktopGameBridgeEnumOperation,
        input: DesktopRequestId,
        board_input: bool,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<DesktopRequestId, PresentationAppError> {
        let game = match &operation {
            DesktopGameBridgeEnumOperation::Create { .. } => None,
            DesktopGameBridgeEnumOperation::Session { game, .. } => Some(*game),
        };
        let revision = operation.revision();
        let command = DesktopCommand::new(DesktopEnumCommand::Game(operation), Some(revision));
        if let Some(retained) = self.game_retained.get(&input) {
            if retained.request.command() != &command {
                return Err(PresentationAppError::GameRequestConflict);
            }
            if retained.admitted {
                return Ok(retained.request.request_id());
            }
        } else {
            if self.game_retained.len() >= Self::MAX_GAME_REQUESTS {
                return Err(PresentationAppError::GameRequestCapacity);
            }
            self.game_retained.insert(
                input,
                PresentationAppGameRetained {
                    request: port.request(command)?,
                    board_input,
                    admitted: false,
                    completed: false,
                },
            );
        }
        let retained = self
            .game_retained
            .get_mut(&input)
            .ok_or(PresentationAppError::GameRequestConflict)?;
        let identity = retained.request.request_id();
        let receipt = port
            .try_submit(&retained.request)
            .map_err(|source| PresentationAppError::Admission { identity, source })?;
        retained.admitted = true;
        self.game_pending.push(PresentationAppGamePending {
            input,
            game,
            board_input: retained.board_input,
            receipt,
        });
        self.failure = None;
        Ok(identity)
    }

    pub const fn preferences(&self) -> &DesktopPreferences {
        &self.preferences
    }

    pub const fn is_settings_open(&self) -> bool {
        self.settings_open
    }

    pub fn request_quit(&mut self) {
        match self.guard_editor_navigation(&PresentationAppLibraryEnumNavigation::Quit) {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                self.failure = Some(error.to_string());
                return;
            }
        }
        if let Some(library) = &mut self.library {
            match library.request_navigation(PresentationAppLibraryEnumNavigation::Quit) {
                Ok(Some(PresentationAppLibraryEnumNavigation::Quit)) => {}
                Ok(_) => {
                    self.settings_open = false;
                    return;
                }
                Err(error) => {
                    self.failure = Some(error.to_string());
                    return;
                }
            }
        }
        self.request_quit_unguarded();
    }

    fn request_quit_unguarded(&mut self) {
        if !self.quit_confirmation {
            self.quit_confirmation = true;
            self.quit_modal.open(None);
            self.board.cancel_gesture();
        }
    }

    pub fn activate<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        action: PresentationActionEnumId,
        frame: u64,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<DesktopRequestId, PresentationAppError> {
        if self.action_frame != Some(frame) {
            self.action_frame = Some(frame);
            self.retained.retain(|_, request| !request.admitted);
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
            if matches!(response, DesktopMessagesEnumResult::QuitReady) {
                if action == PresentationActionEnumId::ConfirmQuit {
                    self.quit_ready = true;
                    self.status = "Application is ready to close".to_owned();
                } else {
                    self.failure =
                        Some("The application returned an unexpected quit receipt".to_owned());
                }
            } else {
                self.accept_response(response);
            }
        }
        let mut completed = Vec::new();
        self.game_pending.retain(|pending| {
            if let Some(response) = pending.receipt.response() {
                completed.push((
                    pending.input,
                    pending.game,
                    pending.board_input,
                    response.clone(),
                ));
                false
            } else {
                true
            }
        });
        for (input, game_id, board_input, response) in completed {
            if let Some(retained) = self.game_retained.get_mut(&input) {
                retained.completed = true;
            }
            if board_input
                && let Some(game) = self
                    .games
                    .iter_mut()
                    .find(|game| Some(game.snapshot.id()) == game_id)
            {
                let result = match &response {
                    DesktopMessagesEnumResult::Value {
                        value: DesktopEnumValue::Game(_),
                        ..
                    } => Ok(()),
                    DesktopMessagesEnumResult::Failure(failure) => Err(failure.clone()),
                    DesktopMessagesEnumResult::Cancelled => Err(DesktopFailure::new(
                        DesktopEnumFailureCode::Cancelled,
                        "The native game action was canceled".to_owned(),
                        false,
                    )),
                    DesktopMessagesEnumResult::Value { .. }
                    | DesktopMessagesEnumResult::LibraryFailure(_)
                    | DesktopMessagesEnumResult::EditorFailure(_)
                    | DesktopMessagesEnumResult::Stopped
                    | DesktopMessagesEnumResult::Paused
                    | DesktopMessagesEnumResult::QuitReady
                    | DesktopMessagesEnumResult::ReconciliationRequired => {
                        Err(DesktopFailure::new(
                            DesktopEnumFailureCode::InternalFailure,
                            "The native game returned an unexpected action result".to_owned(),
                            false,
                        ))
                    }
                };
                if let Err(error) = game.controller.observe_result(input, result) {
                    self.failure = Some(error.to_string());
                }
            }
            if game_id.is_none()
                && let DesktopMessagesEnumResult::Value {
                    value: DesktopEnumValue::Game(snapshot),
                    ..
                } = &response
            {
                self.selected_game = Some(snapshot.id());
                self.new_game = None;
                self.board.cancel_gesture();
            }
            self.accept_response(response);
        }
        self.game_retained.retain(|_, retained| {
            if !retained.completed {
                return true;
            }
            match retained.request.command().operation() {
                DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
                    game,
                    revision,
                    ..
                }) => self.games.iter().any(|current| {
                    current.snapshot.id() == *game && current.snapshot.revision() == *revision
                }),
                DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Create { .. }) => false,
                DesktopEnumCommand::Engine(_)
                | DesktopEnumCommand::Library(_)
                | DesktopEnumCommand::Editor { .. }
                | DesktopEnumCommand::Puzzle { .. }
                | DesktopEnumCommand::Bootstrap
                | DesktopEnumCommand::FileSelect { .. }
                | DesktopEnumCommand::StreamSubscribe
                | DesktopEnumCommand::StreamAcknowledge { .. }
                | DesktopEnumCommand::StreamResync { .. }
                | DesktopEnumCommand::Stop { .. }
                | DesktopEnumCommand::Pause { .. }
                | DesktopEnumCommand::Quit => true,
            }
        });
    }

    fn accept_response(&mut self, response: DesktopMessagesEnumResult) {
        match response {
            DesktopMessagesEnumResult::Value { value, .. } => self.accept_value(value),
            DesktopMessagesEnumResult::LibraryFailure(failure) => {
                self.failure = Some(failure.to_string());
            }
            DesktopMessagesEnumResult::EditorFailure(failure) => {
                self.failure = Some(failure.to_string());
            }
            DesktopMessagesEnumResult::Failure(failure) => {
                if matches!(
                    failure.code(),
                    DesktopEnumFailureCode::ResyncRequired
                        | DesktopEnumFailureCode::InvalidAcknowledgement
                ) && let Some(stream) = &mut self.stream
                    && stream.state == PresentationAppEnumStreamState::Ready
                {
                    stream.state = PresentationAppEnumStreamState::ResyncRequired;
                }
                self.failure = Some(failure.message().to_owned());
            }
            DesktopMessagesEnumResult::Cancelled => self.status = "Action canceled".to_owned(),
            DesktopMessagesEnumResult::Stopped => self.status = "Owned work stopped".to_owned(),
            DesktopMessagesEnumResult::Paused => self.status = "Owned work paused".to_owned(),
            DesktopMessagesEnumResult::QuitReady => {
                self.failure = Some("The native game returned an unexpected quit result".to_owned())
            }
            DesktopMessagesEnumResult::ReconciliationRequired => {
                self.failure = Some("Refresh local state before retrying this action".to_owned())
            }
        }
    }

    fn accept_value(&mut self, value: DesktopEnumValue) {
        match value {
            DesktopEnumValue::Library(_) => {
                self.failure =
                    Some("The Library response has no matching panel request".to_owned());
            }
            DesktopEnumValue::Editor(_) => {
                self.failure = Some("The editor response has no matching request".to_owned());
            }
            DesktopEnumValue::Puzzle(_) => {
                self.failure = Some(
                    "An unexpected puzzle response was refused. Refresh the puzzle view".to_owned(),
                );
            }
            DesktopEnumValue::Engine(result) => match self.engine_panels.as_mut() {
                Some(panels) => {
                    if let Err(error) = panels.observe_value(*result) {
                        self.failure = Some(error.to_string());
                    }
                }
                None => {
                    self.failure = Some("The native engine panel owner is unavailable".to_owned())
                }
            },
            DesktopEnumValue::Game(snapshot) => {
                self.accept_game_snapshot(*snapshot);
            }
            DesktopEnumValue::Bootstrap(snapshot) => self.accept_snapshot(snapshot),
            DesktopEnumValue::StreamSubscribe {
                subscription_id,
                sequence,
                snapshot,
            }
            | DesktopEnumValue::StreamResync {
                subscription_id,
                sequence,
                snapshot,
            } => {
                self.stream = Some(PresentationAppStream {
                    subscription_id,
                    sequence,
                    acknowledged: DesktopRevision::ZERO,
                    state: PresentationAppEnumStreamState::Ready,
                });
                self.accept_snapshot(snapshot);
            }
            DesktopEnumValue::FileSelect(Some(_)) => {
                self.status = "Local file selection is ready".to_owned()
            }
            DesktopEnumValue::FileSelect(None) => self.status = "No file selected".to_owned(),
            DesktopEnumValue::StreamAcknowledge => {}
        }
    }

    fn poll_events<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let response = self
            .stream_pending
            .as_ref()
            .and_then(|pending| pending.receipt.as_ref())
            .and_then(|receipt| receipt.response().cloned());
        if let Some(response) = response {
            let pending = self
                .stream_pending
                .take()
                .ok_or(PresentationAppError::UnexpectedStreamResult)?;
            let mut is_success = false;
            if let DesktopEnumCommand::StreamAcknowledge {
                subscription_id,
                sequence,
            } = pending.request.command().operation()
            {
                if matches!(
                    &response,
                    DesktopMessagesEnumResult::Value {
                        value: DesktopEnumValue::StreamAcknowledge,
                        ..
                    }
                ) && let Some(stream) = &mut self.stream
                    && stream.subscription_id == *subscription_id
                {
                    stream.acknowledged = *sequence;
                    is_success = true;
                } else if let Some(stream) = &mut self.stream {
                    stream.state = PresentationAppEnumStreamState::ResyncRequired;
                }
            } else if let DesktopEnumCommand::StreamResync { subscription_id } =
                pending.request.command().operation()
            {
                is_success = matches!(&response, DesktopMessagesEnumResult::Value { value: DesktopEnumValue::StreamResync { subscription_id: returned, .. }, .. } if returned == subscription_id);
                if !is_success && let Some(stream) = &mut self.stream {
                    stream.state = PresentationAppEnumStreamState::RetryRequired;
                }
            }
            if is_success || matches!(&response, DesktopMessagesEnumResult::Failure(_)) {
                self.accept_response(response);
            } else {
                if let Some(stream) = &mut self.stream {
                    stream.state = PresentationAppEnumStreamState::RetryRequired;
                }
                self.failure = Some(PresentationAppError::UnexpectedStreamResult.to_string());
            }
        }
        for _ in 0..DesktopStream::<DesktopEnumEvent>::MAX_UNACKNOWLEDGED {
            let Some(event) = port.try_event()? else {
                break;
            };
            let Some(stream) = &mut self.stream else {
                self.failure = Some("Received a native event before subscribing".to_owned());
                continue;
            };
            if stream.subscription_id != event.subscription_id() {
                self.failure = Some("Received an event for an inactive subscription".to_owned());
                continue;
            }
            if event.sequence() <= stream.sequence
                || stream.state != PresentationAppEnumStreamState::Ready
            {
                continue;
            }
            if stream.sequence.next()? != event.sequence() {
                stream.state = PresentationAppEnumStreamState::ResyncRequired;
                self.failure = Some("The native game stream needs resynchronization".to_owned());
                continue;
            }
            stream.sequence = event.sequence();
            match event.payload() {
                DesktopEnumEvent::Games(snapshots) => {
                    for snapshot in snapshots {
                        self.accept_game_snapshot(snapshot.clone());
                    }
                }
                DesktopEnumEvent::Snapshot(snapshot) => self.accept_snapshot(snapshot.clone()),
                DesktopEnumEvent::Failure(failure) => {
                    self.accept_response(DesktopMessagesEnumResult::Failure(failure.clone()))
                }
            }
        }
        if self.stream_pending.is_none()
            && let Some(stream) = &self.stream
        {
            let operation = match stream.state {
                PresentationAppEnumStreamState::Ready if stream.sequence > stream.acknowledged => {
                    Some(DesktopEnumCommand::StreamAcknowledge {
                        subscription_id: stream.subscription_id,
                        sequence: stream.sequence,
                    })
                }
                PresentationAppEnumStreamState::Ready
                | PresentationAppEnumStreamState::RetryRequired => None,
                PresentationAppEnumStreamState::ResyncRequired => {
                    Some(DesktopEnumCommand::StreamResync {
                        subscription_id: stream.subscription_id,
                    })
                }
            };
            if let Some(operation) = operation {
                match port.request(DesktopCommand::new(operation, None)) {
                    Ok(request) => {
                        self.stream_pending = Some(PresentationAppStreamPending {
                            request,
                            receipt: None,
                        })
                    }
                    Err(error) => {
                        if let Some(stream) = &mut self.stream {
                            stream.state = PresentationAppEnumStreamState::RetryRequired;
                        }
                        return Err(error.into());
                    }
                }
            }
        }
        if self
            .stream
            .as_ref()
            .is_some_and(|stream| stream.state != PresentationAppEnumStreamState::RetryRequired)
            && let Some(pending) = &mut self.stream_pending
            && pending.receipt.is_none()
        {
            match port.try_submit(&pending.request) {
                Ok(receipt) => pending.receipt = Some(receipt),
                Err(DesktopApplicationError::Busy) => {}
                Err(error) => {
                    if let Some(stream) = &mut self.stream {
                        stream.state = PresentationAppEnumStreamState::RetryRequired;
                    }
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    fn retry_stream(&mut self) {
        if let Some(stream) = &mut self.stream {
            stream.state = if self.stream_pending.as_ref().is_some_and(|pending| {
                matches!(
                    pending.request.command().operation(),
                    DesktopEnumCommand::StreamAcknowledge { .. }
                )
            }) {
                PresentationAppEnumStreamState::Ready
            } else {
                PresentationAppEnumStreamState::ResyncRequired
            };
            self.failure = None;
            self.status = "Retrying native stream synchronization".to_owned();
        }
    }

    fn input_identity(&mut self, frame: u64, key: PresentationAppEnumInput) -> DesktopRequestId {
        if self.input_frame != Some(frame) {
            self.input_frame = Some(frame);
            self.input_identities.clear();
        }
        *self.input_identities.entry(key).or_default()
    }

    fn is_game_busy(&self, id: GameId) -> bool {
        self.game_pending.iter().any(|pending| pending.game == Some(id))
            || self.game_retained.values().any(|retained| !retained.admitted && matches!(retained.request.command().operation(), DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session { game, .. }) if *game == id))
    }

    fn dispatch_controller<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        intent: PresentationGameControllerIntent,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let input = intent.request_id();
        let operation = DesktopGameBridgeEnumOperation::Session {
            game: intent.game(),
            revision: intent.revision(),
            action: intent.into_action(),
        };
        self.activate_game_request(operation, input, true, port)?;
        Ok(())
    }

    fn play_action(
        intent: PresentationGamePlayIntent,
        snapshot: &DesktopGameSnapshot,
    ) -> Result<DesktopGameCommandsEnumAction, PresentationAppError> {
        Ok(match intent.operation() {
            PresentationGamePlayEnumOperation::Navigate(node) => {
                DesktopGameCommandsEnumAction::Navigate { node }
            }
            PresentationGamePlayEnumOperation::ReturnToLive => {
                DesktopGameCommandsEnumAction::ReturnToLive
            }
            PresentationGamePlayEnumOperation::Pause => DesktopGameCommandsEnumAction::Pause {
                cause: GamePauseEnumCause::Manual,
            },
            PresentationGamePlayEnumOperation::Resume => DesktopGameCommandsEnumAction::Resume,
            PresentationGamePlayEnumOperation::Retry => {
                if snapshot.is_save_required() {
                    DesktopGameCommandsEnumAction::RetrySave
                } else {
                    match snapshot.interruption() {
                        Some(GameSessionEnumInterruption::RecoveryRequired(
                            GamePauseEnumFault::Engine,
                        )) => DesktopGameCommandsEnumAction::RetryEngine,
                        Some(GameSessionEnumInterruption::RecoveryRequired(
                            GamePauseEnumFault::Save,
                        ))
                        | Some(GameSessionEnumInterruption::PendingResult) => {
                            DesktopGameCommandsEnumAction::RetrySave
                        }
                        Some(GameSessionEnumInterruption::Adjudication { .. }) => {
                            DesktopGameCommandsEnumAction::Resume
                        }
                        Some(GameSessionEnumInterruption::RecoveryRequired(
                            GamePauseEnumFault::Graphics,
                        ))
                        | Some(GameSessionEnumInterruption::Recovered(_))
                        | Some(GameSessionEnumInterruption::Paused(_))
                        | None => return Err(PresentationAppError::UnsupportedRecovery),
                    }
                }
            }
            PresentationGamePlayEnumOperation::Takeback { requester } => {
                DesktopGameCommandsEnumAction::RequestTakeback { requester }
            }
            PresentationGamePlayEnumOperation::RespondTakeback {
                responder,
                response,
            } => DesktopGameCommandsEnumAction::RespondTakeback {
                responder,
                response,
            },
            PresentationGamePlayEnumOperation::ClaimDraw {
                claimant,
                kind,
                intent,
            } => DesktopGameCommandsEnumAction::ClaimDraw {
                color: claimant,
                kind,
                intent,
            },
            PresentationGamePlayEnumOperation::Resign { resigning } => {
                DesktopGameCommandsEnumAction::Resign { side: resigning }
            }
            PresentationGamePlayEnumOperation::CancelPremove => {
                DesktopGameCommandsEnumAction::CancelPremove
            }
        })
    }

    fn show_settings(&mut self, ui: &mut Ui) {
        ui.heading("Settings");
        ui.label("Appearance applies immediately for this session. Saving preferences across launches is not available yet.");
        if ui.button("Reset Layout").clicked() {
            self.layout.reset();
            self.layout_reset_requested = true;
        }
        for &section in DesktopPreferences::sections() {
            ui.separator();
            ui.heading(section.label());
            match section {
                DesktopPreferencesEnumSection::Appearance => {
                    let mut theme = self.preferences.theme();
                    for &choice in DesktopPreferences::themes() {
                        ui.radio_value(&mut theme, choice, choice.label());
                    }
                    if self.preferences.set_theme(theme) {
                        PresentationThemeTokens::apply(ui.ctx(), theme);
                        ui.ctx().request_discard("apply appearance before layout");
                    }
                    let mut motion = self.preferences.motion();
                    for &choice in DesktopPreferences::motions() {
                        ui.radio_value(&mut motion, choice, choice.label());
                    }
                    if self.preferences.set_motion(motion) {
                        self.board.set_reduced_motion(motion.is_reduced());
                    }
                }
                DesktopPreferencesEnumSection::BoardAids => {
                    for aid in DesktopPreferencesEnumAid::ALL {
                        let mut enabled = self.preferences.aid(aid);
                        if ui.checkbox(&mut enabled, aid.label()).changed() {
                            self.preferences
                                .apply(DesktopPreferencesEnumPatch::Aid(aid, enabled));
                        }
                    }
                }
                DesktopPreferencesEnumSection::Audio => {
                    let mut master = self.preferences.master_volume().value();
                    if ui
                        .add(
                            bevy_egui::egui::Slider::new(
                                &mut master,
                                0..=DesktopPreferencesPercent::MAX,
                            )
                            .text("Master volume"),
                        )
                        .changed()
                        && let Ok(volume) = DesktopPreferencesPercent::new(master)
                    {
                        self.preferences
                            .apply(DesktopPreferencesEnumPatch::MasterVolume(volume));
                    }
                    for category in DesktopPreferencesEnumAudioCategory::ALL {
                        let entry = self.preferences.category(category);
                        let mut enabled = entry.enabled();
                        if ui.checkbox(&mut enabled, category.label()).changed() {
                            self.preferences
                                .apply(DesktopPreferencesEnumPatch::CategoryEnabled(
                                    category, enabled,
                                ));
                        }
                        let mut level = entry.volume().value();
                        if ui
                            .add(
                                bevy_egui::egui::Slider::new(
                                    &mut level,
                                    0..=DesktopPreferencesPercent::MAX,
                                )
                                .text(format!("{} volume", category.label())),
                            )
                            .changed()
                            && let Ok(volume) = DesktopPreferencesPercent::new(level)
                        {
                            self.preferences
                                .apply(DesktopPreferencesEnumPatch::CategoryVolume(
                                    category, volume,
                                ));
                        }
                    }
                }
                DesktopPreferencesEnumSection::Shortcuts => {
                    for action in DesktopPreferencesEnumShortcutAction::ALL {
                        let text = self
                            .preferences
                            .shortcut(action)
                            .map_or_else(|| "Default".to_owned(), |chord| chord.key().to_string());
                        ui.label(format!("{}: {text}", action.label()));
                    }
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
            PresentationThemeTokens::apply(ui.ctx(), self.preferences.theme());
            self.theme_initialized = true;
            ui.ctx()
                .request_discard("apply initial native theme before layout");
        }
        self.poll();
        if let Err(error) = self.synchronize_library(port) {
            self.report_failure(error);
        }
        if let Err(error) = self.synchronize_editor(port) {
            self.report_failure(error);
        }
        if let Err(error) = self.synchronize_puzzles(port) {
            self.report_failure(error);
        }
        if let Err(error) = self.poll_events(port) {
            self.failure = Some(error.to_string());
        }
        let now = port.now()?;
        if self.failure.is_none()
            && let Some(notice) = &mut self.game_notice
        {
            let remaining = notice.remaining(now)?;
            if remaining == ClockDuration::ZERO {
                self.game_notice = None;
            } else {
                ui.ctx().request_repaint_after(remaining.as_duration());
            }
        }
        let frame = ui.ctx().cumulative_frame_nr();
        let promotion_owns_input = self.promotion_frame == Some(frame)
            || self.workspace == PresentationAppEnumWorkspace::Games
                && self.games.iter().any(|game| {
                    Some(game.snapshot.id()) == self.selected_game
                        && game.controller.is_promotion_pending()
                })
            || self.workspace == PresentationAppEnumWorkspace::Puzzles
                && self
                    .puzzle_panel
                    .as_ref()
                    .is_some_and(PresentationPuzzlePanel::is_promotion_pending);
        if promotion_owns_input {
            self.promotion_frame = Some(frame);
            ui.disable();
        }
        let mut configuration = None;
        let mut game_intent = None;
        let mut controller_intent = None;
        let mut actions = Vec::new();
        let mut camera_action = None;
        let mut inspector_toggle = false;
        let mut workspace = None;
        let mut navigation = None;
        egui::Panel::top("chess.header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("CHESS")
                        .strong()
                        .color(ui.visuals().selection.stroke.color),
                );
                ui.label(
                    egui::RichText::new("Offline study and play")
                        .color(ui.visuals().weak_text_color()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Quit").clicked() {
                        navigation = Some(PresentationAppLibraryEnumNavigation::Quit);
                    }
                    if ui.button("New Game").clicked() && self.new_game_toggle_frame != Some(frame)
                    {
                        self.new_game_toggle_frame = Some(frame);
                        navigation = Some(PresentationAppLibraryEnumNavigation::NewGame);
                    }
                    if ui
                        .button(if self.inspector_open {
                            "Hide inspector"
                        } else {
                            "Show inspector"
                        })
                        .clicked()
                    {
                        inspector_toggle = true;
                    }
                    if ui.button("Settings").clicked() && self.settings_toggle_frame != Some(frame)
                    {
                        self.settings_open = !self.settings_open;
                        self.settings_toggle_frame = Some(frame);
                        self.board.cancel_gesture();
                    }
                    for &action in PresentationActionRegistry::actions() {
                        match action {
                            PresentationActionEnumId::Refresh
                            | PresentationActionEnumId::SelectFile => {
                                if ui.button(action.label()).clicked() {
                                    actions.push(action);
                                }
                            }
                            PresentationActionEnumId::ConfirmQuit
                            | PresentationActionEnumId::Subscribe => {}
                        }
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_label(
                        self.workspace == PresentationAppEnumWorkspace::Games,
                        "Play",
                    )
                    .clicked()
                {
                    workspace = Some(PresentationAppEnumWorkspace::Games);
                }
                if ui
                    .add_enabled(
                        self.library.is_some(),
                        egui::Button::selectable(
                            self.workspace == PresentationAppEnumWorkspace::Library,
                            "Library",
                        ),
                    )
                    .clicked()
                {
                    workspace = Some(PresentationAppEnumWorkspace::Library);
                }
                if ui
                    .add_enabled(
                        self.puzzle_panel.is_some(),
                        egui::Button::selectable(
                            self.workspace == PresentationAppEnumWorkspace::Puzzles,
                            "Puzzles",
                        ),
                    )
                    .clicked()
                {
                    workspace = Some(PresentationAppEnumWorkspace::Puzzles);
                }
                if ui
                    .add_enabled(
                        self.editor.is_some(),
                        egui::Button::selectable(
                            self.workspace == PresentationAppEnumWorkspace::Editor,
                            "Editor",
                        ),
                    )
                    .clicked()
                {
                    workspace = Some(PresentationAppEnumWorkspace::Editor);
                }
                let mut selected = self.selected_game;
                for (index, game) in self.games.iter().enumerate() {
                    let label = format!("Game {}", index.saturating_add(1));
                    if ui
                        .selectable_label(selected == Some(game.snapshot.id()), label)
                        .clicked()
                    {
                        selected = Some(game.snapshot.id());
                        navigation = Some(PresentationAppLibraryEnumNavigation::Game(
                            game.snapshot.id(),
                        ));
                    }
                }
                if selected != self.selected_game {
                    navigation = selected.map(PresentationAppLibraryEnumNavigation::Game);
                }
            });
        });
        if !ui.is_sizing_pass() && self.navigation_frame != Some(frame) {
            if let Some(target) = navigation {
                self.navigation_frame = Some(frame);
                if let Err(error) = self.navigate(target, port) {
                    self.report_failure(error);
                }
            } else if let Some(workspace) = workspace {
                self.navigation_frame = Some(frame);
                if let Err(error) = self.select_workspace(workspace, port) {
                    self.report_failure(error);
                }
            }
        }
        if self.workspace == PresentationAppEnumWorkspace::Games
            && let Some(panels) = &mut self.engine_panels
        {
            let selected = self
                .games
                .iter()
                .find(|game| Some(game.snapshot.id()) == self.selected_game)
                .map(|game| &game.snapshot);
            if let Err(error) = panels.synchronize(selected, port) {
                self.failure = Some(error.to_string());
            }
        }
        let is_busy = self.workspace == PresentationAppEnumWorkspace::Games
            && self
                .selected_game
                .is_some_and(|game| self.is_game_busy(game));
        let is_create_busy = self
            .game_pending
            .iter()
            .any(|pending| pending.game.is_none())
            || self.game_retained.values().any(|retained| {
                !retained.admitted
                    && matches!(
                        retained.request.command().operation(),
                        DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Create { .. })
                    )
            });
        if self.workspace == PresentationAppEnumWorkspace::Library {
            egui::Panel::left("chess.library.query")
                .default_size(PresentationThemeTokens::PANEL_WIDTH)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let positions = if let Some(editor) = &mut self.editor {
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut editor.positions_visible, false, "Games");
                                ui.selectable_value(
                                    &mut editor.positions_visible,
                                    true,
                                    "Positions",
                                );
                            });
                            editor.positions_visible
                        } else {
                            false
                        };
                        if positions {
                            if let Err(error) = self.show_positions(ui, port) {
                                self.report_failure(error);
                            }
                            return;
                        }
                        if let Some(library) = &mut self.library
                            && let Err(error) = library.show_query(ui, port)
                        {
                            self.failure = Some(error.to_string());
                        }
                    });
                });
        }
        if inspector_toggle && self.toggle_frame_is_new(frame) {
            self.inspector_open = !self.inspector_open;
            if let Err(error) = self.board.set_panel_visible(self.inspector_open) {
                self.failure = Some(error.to_string());
            }
        }
        let inspector_progress = match self.board.panel_progress() {
            Ok(progress) => progress,
            Err(error) => {
                self.failure = Some(error.to_string());
                f32::from(u8::from(self.inspector_open))
            }
        };
        self.board.request_repaint(ui.ctx());
        if inspector_progress > 0.0 {
            egui::Panel::right("chess.inspector")
            .exact_size(self.layout.panel_width() * inspector_progress)
            .show(ui, |ui| {
                if self.settings_open {
                    self.show_settings(ui);
                    return;
                }
                if self.workspace == PresentationAppEnumWorkspace::Editor {
                    ui.add_enabled_ui(!self.quit_confirmation, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            if let Err(error) = self.show_editor(ui, port) { self.report_failure(error); }
                        });
                    });
                    return;
                }
                if self.workspace == PresentationAppEnumWorkspace::Library {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(library) = &mut self.library && let Err(error) = library.show_editor(ui, port) { self.failure = Some(error.to_string()); }
                    });
                    return;
                }
                if self.workspace == PresentationAppEnumWorkspace::Puzzles {
                    if let Some(panel) = &mut self.puzzle_panel
                        && let Err(error) = panel.show(ui, port) {
                        self.failure = Some(error.to_string());
                    }
                    return;
                }
                if let Some(new_game) = &mut self.new_game {
                    configuration = ui.add_enabled_ui(!is_create_busy, |ui| new_game.show(ui)).inner;
                    if is_create_busy {
                        ui.weak("Waiting for the native game to start.");
                    }
                    return;
                }
                if let Some(game) = self.games.iter_mut().find(|game| Some(game.snapshot.id()) == self.selected_game) {
                    match game.play.show(ui, &game.snapshot, now, is_busy) {
                        Ok(intent) => game_intent = intent,
                        Err(error) => self.failure = Some(error.to_string()),
                    }
                    match game.controller.show(ui, &game.snapshot, is_busy || game_intent.is_some()) {
                        Ok(intent) => controller_intent = intent,
                        Err(error) => self.failure = Some(error.to_string()),
                    }
                    if self.games.len() > 1 {
                        ui.separator();
                        ui.heading("Live games");
                        for (index, game) in self.games.iter().enumerate() {
                            match game.snapshot.clock().view(now) {
                                Ok(clock) => {
                                    ui.label(format!("Game {}: {} to move", index.saturating_add(1), PresentationBoardSemanticGrid::color_name(game.snapshot.live_turn())));
                                    ui.label(format!("White {} | Black {}", PresentationGamePlayClock::text(clock.side(BoardEnumColor::White).displayed()), PresentationGamePlayClock::text(clock.side(BoardEnumColor::Black).displayed())));
                                }
                                Err(error) => self.failure = Some(error.to_string()),
                            }
                        }
                    }
                } else {
                    ui.heading("Position");
                    if let Some(snapshot) = self.board_snapshot() {
                        ui.label(format!("{} to move{}", PresentationBoardSemanticGrid::color_name(snapshot.turn()), if snapshot.is_check() { ", in check" } else { "" }));
                        ui.label(format!("{} legal moves", snapshot.legal_moves().len()));
                    }
                }
                if let Some(snapshot) = &self.snapshot {
                    ui.separator();
                    ui.heading("Reckless");
                    match snapshot.engine() {
                        DesktopEnumEngineSnapshot::Ready { identity } => {
                            ui.label(identity.name());
                            ui.label(egui::RichText::new(identity.author()).color(ui.visuals().weak_text_color()));
                            ui.label(format!("{} supported engine options", identity.supported_options().len()));
                        }
                        DesktopEnumEngineSnapshot::Unavailable { message } => {
                            ui.colored_label(ui.visuals().warn_fg_color, "Engine unavailable");
                            ui.label(message);
                        }
                    }
                    ui.label(format!("{} usable hardware threads", snapshot.cpu_threads()));
                } else {
                    ui.label("Waiting for the local application");
                }
                if let Some(panels) = &mut self.engine_panels
                    && let Err(error) = panels.show(ui, port) {
                    self.failure = Some(error.to_string());
                }
                ui.separator();
                ui.label(egui::RichText::new("Use Tab to enter the board, arrow keys to move focus, and Enter or Space to select a square.").color(ui.visuals().weak_text_color()));
            });
        }
        let retry_quit = self.failure.is_some()
            && self
                .pending
                .iter()
                .any(|pending| pending.action == PresentationActionEnumId::ConfirmQuit);
        let status = self.failure.as_deref().unwrap_or_else(|| self.status());
        let has_refused_game = self
            .game_retained
            .values()
            .any(|retained| !retained.admitted);
        let mut retry = false;
        let mut retry_game = false;
        let mut retry_stream = false;
        let needs_stream_retry = self
            .stream
            .as_ref()
            .is_some_and(|stream| stream.state == PresentationAppEnumStreamState::RetryRequired);
        egui::Panel::bottom("chess.status").show(ui, |ui| {
            let response = ui.label(status);
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(egui::accesskit::Role::Status);
                node.set_live(egui::accesskit::Live::Polite);
            });
            if retry_quit {
                retry = ui.button("Retry quit").clicked();
            }
            if has_refused_game {
                retry_game = ui.button("Retry action").clicked();
            }
            if needs_stream_retry {
                retry_stream = ui.button("Retry sync").clicked();
            }
        });
        if retry && self.quit_retry_frame != Some(frame) {
            if let Err(error) = port.retry_quit() {
                self.failure = Some(error.to_string());
            }
            self.quit_retry_frame = Some(frame);
        }
        if self.workspace == PresentationAppEnumWorkspace::Games
            && let Some(game) = self
                .games
                .iter()
                .find(|game| Some(game.snapshot.id()) == self.selected_game)
        {
            self.board.set_selected(game.controller.selected());
        }
        let can_enter_moves = match self.workspace {
            PresentationAppEnumWorkspace::Editor => false,
            PresentationAppEnumWorkspace::Library => false,
            PresentationAppEnumWorkspace::Puzzles => self
                .puzzle_panel
                .as_ref()
                .and_then(PresentationPuzzlePanel::controller)
                .is_some_and(|controller| controller.can_input()),
            PresentationAppEnumWorkspace::Games => self.game_snapshot().is_none_or(|snapshot| {
                snapshot.phase() == GameEnumPhase::Active
                    && snapshot.is_viewing_live()
                    && !snapshot.is_save_required()
                    && (snapshot.seats().seat(snapshot.live_turn()) == GameEnumSeat::Human
                        || snapshot.are_premoves_enabled())
            }),
        };
        if self.workspace == PresentationAppEnumWorkspace::Puzzles {
            self.board.set_selected(
                self.puzzle_panel
                    .as_ref()
                    .and_then(PresentationPuzzlePanel::controller)
                    .and_then(|controller| controller.selected()),
            );
        }
        let pending_move = self
            .game_snapshot()
            .filter(|game| {
                game.is_viewing_live()
                    && game.phase() == GameEnumPhase::Active
                    && !game.is_save_required()
            })
            .and_then(DesktopGameSnapshot::premove);
        let mut editor_viewport = None;
        let board_frame = egui::CentralPanel::default()
            .show(ui, |ui| {
                if let Some(intent) =
                    PresentationBoardRecovery::show(ui, &self.renderer_commands)
                {
                    self.recovery_intent = Some(intent);
                }
                if let Some(action) =
                    PresentationBoardToolbar::show(ui, scene.is_some(), self.board.markings_mut())
                {
                    camera_action = Some(action);
                    self.board.cancel_gesture();
                }
                if self.workspace == PresentationAppEnumWorkspace::Editor {
                    if let Some(editor) = &mut self.editor {
                        editor_viewport = Some(
                            editor
                                .show_board(
                                    ui,
                                    port,
                                    PresentationAppEditorBoardInput::new(
                                        scene,
                                        picker,
                                        self.board.markings_mut(),
                                    )
                                    .with_enabled(!self.quit_confirmation && !self.settings_open),
                                )
                                .map_err(PresentationAppError::from)?,
                        );
                    }
                    return Ok::<_, PresentationAppError>(None);
                }
                ui.horizontal(|ui| {
                    ui.strong("White");
                    if self.selected_game.is_none()
                        && self.workspace == PresentationAppEnumWorkspace::Games
                    {
                        ui.label(
                            egui::RichText::new("No active game")
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
                });
                let board_snapshot = Self::board_for_workspace(
                    self.workspace,
                    Self::selected_game_snapshot(&self.games, self.selected_game),
                    self.snapshot.as_ref(),
                    self.puzzle_panel.as_ref(),
                    self.library.as_ref(),
                );
                let result = match board_snapshot {
                    Some(snapshot) => self
                        .board
                        .show(
                            ui,
                            PresentationBoardViewInput::new(snapshot, scene, picker)
                                .with_pending_move(pending_move)
                                .with_render_scale(self.render_scale)
                                .with_marks_enabled(
                                    !self.quit_confirmation
                                        && !promotion_owns_input
                                        && self.new_game.is_none()
                                        && !self.settings_open,
                                )
                                .with_enabled(
                                    !self.quit_confirmation
                                        && !promotion_owns_input
                                        && can_enter_moves
                                        && !is_busy
                                        && self.new_game.is_none()
                                        && !self.settings_open,
                                ),
                        )
                        .map(Some),
                    None => {
                        ui.label(match self.workspace {
                            PresentationAppEnumWorkspace::Games => "Loading the saved position",
                            PresentationAppEnumWorkspace::Library => {
                                "Open a saved game to display its 3D board"
                            }
                            PresentationAppEnumWorkspace::Puzzles => {
                                "The puzzle board is hidden until its run is ready and resumed"
                            }
                            PresentationAppEnumWorkspace::Editor => "Open a position draft",
                        });
                        Ok(None)
                    }
                };
                ui.strong("Black");
                result.map_err(PresentationAppError::from)
            })
            .inner?;
        if self.quit_confirmation {
            let mut cancel = false;
            let mut confirm = false;
            self.quit_modal.capture_invoker(ui.ctx());
            let modal = egui::Modal::new(Id::new("chess.quit.confirmation")).show(ui.ctx(), |ui| {
                PresentationAnalysisEditorModal::label(ui, "Quit Chess?");
                ui.heading("Quit Chess?");
                ui.label("The application will finish its owned shutdown before closing.");
                ui.horizontal(|ui| {
                    let keep = ui.button("Keep open");
                    let quit = ui.button(PresentationActionEnumId::ConfirmQuit.label());
                    cancel = keep.clicked();
                    confirm = quit.clicked();
                    self.quit_modal.contain(ui, &[keep.id, quit.id]);
                });
            });
            if confirm {
                self.confirmed_quit_pending = true;
            }
            if cancel || confirm || modal.should_close() {
                self.quit_confirmation = false;
                self.quit_modal.close(ui.ctx());
            }
        }
        for action in actions {
            if let Err(error) = self.activate(action, frame, port) {
                self.failure = Some(error.to_string());
            }
        }
        if retry_stream {
            self.retry_stream();
            if let Err(error) = self.poll_events(port) {
                self.failure = Some(error.to_string());
            }
        }
        if retry_game {
            let retries: Vec<_> = self
                .game_retained
                .iter()
                .filter(|(_, retained)| !retained.admitted)
                .filter_map(|(input, retained)| {
                    if let DesktopEnumCommand::Game(operation) =
                        retained.request.command().operation()
                    {
                        Some((*input, operation.clone(), retained.board_input))
                    } else {
                        None
                    }
                })
                .collect();
            for (input, operation, board_input) in retries {
                if let Err(error) = self.activate_game_request(operation, input, board_input, port)
                {
                    self.failure = Some(error.to_string());
                }
            }
        }
        if let Some(configuration) = configuration {
            let input = self.input_identity(frame, PresentationAppEnumInput::Create);
            let operation = DesktopGameBridgeEnumOperation::Create {
                owner: self.new_game_owner,
                configuration,
            };
            if let Err(error) = self.activate_game(operation, input, port) {
                self.failure = Some(error.to_string());
            }
        }
        if let Some(intent) = game_intent {
            let result = self
                .game_snapshot()
                .ok_or(PresentationAppError::MissingGame)
                .and_then(|snapshot| Self::play_action(intent, snapshot));
            match result {
                Ok(action) => {
                    let input =
                        self.input_identity(frame, PresentationAppEnumInput::Play(intent.game()));
                    let operation = DesktopGameBridgeEnumOperation::Session {
                        game: intent.game(),
                        revision: intent.revision(),
                        action,
                    };
                    if let Err(error) = self.activate_game(operation, input, port) {
                        self.failure = Some(error.to_string());
                    }
                }
                Err(error) => self.failure = Some(error.to_string()),
            }
        }
        if let Some(intent) = controller_intent
            && let Err(error) = self.dispatch_controller(intent, port)
        {
            self.failure = Some(error.to_string());
        }
        if !promotion_owns_input
            && !self.quit_confirmation
            && !self.settings_open
            && self.new_game.is_none()
            && self.workspace == PresentationAppEnumWorkspace::Games
        {
            let is_escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
            if let Some(id) = self.selected_game
                && !self.is_game_busy(id)
            {
                let cancellation = is_escape
                    .then(|| self.input_identity(frame, PresentationAppEnumInput::Cancel(id)));
                let mut intents = Vec::new();
                if let Some(game) = self.games.iter_mut().find(|game| game.snapshot.id() == id) {
                    if let Some(identity) = cancellation {
                        match game.controller.cancel(&game.snapshot, identity) {
                            Ok(Some(intent)) => intents.push(intent),
                            Ok(None) => {}
                            Err(error) => self.failure = Some(error.to_string()),
                        }
                    } else if let Some(board_frame) = &board_frame {
                        for action in board_frame.actions() {
                            let result = match action.origin() {
                                Some(origin) => game.controller.drag(
                                    &game.snapshot,
                                    origin,
                                    action.square(),
                                    action.identity(),
                                ),
                                None => game.controller.select(
                                    &game.snapshot,
                                    action.square(),
                                    action.identity(),
                                ),
                            };
                            match result {
                                Ok(Some(intent)) => {
                                    intents.push(intent);
                                    break;
                                }
                                Ok(None) => {}
                                Err(error) => self.failure = Some(error.to_string()),
                            }
                        }
                    }
                }
                for intent in intents {
                    if let Err(error) = self.dispatch_controller(intent, port) {
                        self.failure = Some(error.to_string());
                    }
                }
            }
        }
        if self.workspace == PresentationAppEnumWorkspace::Puzzles
            && !promotion_owns_input
            && !self.quit_confirmation
            && !self.settings_open
            && let (Some(panel), Some(board_frame)) = (&mut self.puzzle_panel, &board_frame)
        {
            for action in board_frame.actions() {
                if let Err(error) = panel.board_action(*action, port) {
                    self.failure = Some(error.to_string());
                    break;
                }
            }
        }
        if self.workspace == PresentationAppEnumWorkspace::Games
            && let Some(game) = self
                .games
                .iter()
                .find(|game| Some(game.snapshot.id()) == self.selected_game)
        {
            self.board.set_selected(game.controller.selected());
        }
        if let (Some(board_frame), Some(scene)) = (&board_frame, scene)
            && self.workspace == PresentationAppEnumWorkspace::Games
            && board_frame.has_semantic_grid()
            && !promotion_owns_input
            && !self.settings_open
            && self.new_game.is_none()
            && !self.quit_confirmation
            && let Some(game) = self
                .games
                .iter_mut()
                .find(|game| Some(game.snapshot.id()) == self.selected_game)
        {
            game.controller.restore_board_focus(ui.ctx(), scene);
        }
        self.synchronize_markings(ui.ctx());
        let animation = if self.workspace == PresentationAppEnumWorkspace::Editor {
            match self.board.camera_animation(ui.ctx()) {
                Ok(frame) => frame,
                Err(error) => {
                    self.failure = Some(error.to_string());
                    None
                }
            }
        } else {
            board_frame
                .as_ref()
                .and_then(PresentationBoardViewFrame::animation)
        };
        Ok(PresentationAppFrame {
            viewport: editor_viewport
                .or_else(|| board_frame.as_ref().map(|frame| frame.viewport())),
            board_actions: board_frame.map_or_else(Vec::new, |frame| frame.actions().to_vec()),
            camera_action,
            animation,
            recovery_intent: self.recovery_intent.take(),
        })
    }

    fn synchronize_markings(&mut self, ctx: &egui::Context) {
        let library_game = self
            .library
            .as_ref()
            .and_then(|library| library.annotations.context())
            .map(|context| context.game());
        let identity = (self.workspace, self.selected_game, library_game);
        if self.markings_identity != Some(identity) {
            self.markings_identity = Some(identity);
            self.board.markings_mut().reset_baseline();
        }
        let requests = self.board.markings_mut().take_requests();
        let library = match (&mut self.library, self.workspace) {
            (Some(library), PresentationAppEnumWorkspace::Library) => library,
            _ => {
                self.board.markings_mut().set_external(None);
                return;
            }
        };
        if !requests.is_empty() {
            ctx.request_repaint();
        }
        for request in requests {
            let Some(context) = library
                .annotations
                .markings()
                .map(|markings| markings.context())
            else {
                break;
            };
            let input = match request {
                PresentationBoardMarkRequest::Toggle(mark) => match mark.to_game() {
                    Some(marking) => {
                        PresentationAnalysisAnnotationsEnumMarkingInput::Toggle(marking)
                    }
                    None => continue,
                },
                PresentationBoardMarkRequest::ClearTemporary => {
                    PresentationAnalysisAnnotationsEnumMarkingInput::ClearTemporary
                }
            };
            if let Err(error) = library.annotations.marking_input(context, input) {
                self.failure = Some(error.to_string());
            }
        }
        let external = library.annotations.markings().map(|markings| {
            (
                markings
                    .saved()
                    .iter()
                    .copied()
                    .map(PresentationBoardMark::from_game)
                    .collect(),
                markings
                    .temporary()
                    .iter()
                    .copied()
                    .map(PresentationBoardMark::from_game)
                    .collect(),
            )
        });
        self.board.markings_mut().set_external(external);
    }

    pub const fn layout(&self) -> &DesktopLayout {
        &self.layout
    }

    pub fn layout_mut(&mut self) -> &mut DesktopLayout {
        &mut self.layout
    }

    pub fn set_layout(&mut self, layout: DesktopLayout) {
        self.layout = layout;
    }

    pub fn take_layout_reset(&mut self) -> bool {
        std::mem::take(&mut self.layout_reset_requested)
    }

    pub fn set_renderer_commands(&mut self, commands: DesktopRendererCommands) {
        self.renderer_commands = commands;
    }

    pub fn set_render_scale(&mut self, render_scale: f32) {
        if render_scale.is_finite() {
            self.render_scale = render_scale.clamp(0.75, 1.0);
        }
    }

    pub fn report_notice(&mut self, message: String) {
        self.failure = Some(message);
    }

    fn toggle_frame_is_new(&mut self, frame: u64) -> bool {
        let is_new = self.inspector_toggle_frame != Some(frame);
        self.inspector_toggle_frame = Some(frame);
        is_new
    }

    pub fn queue_camera_action(&mut self, action: PresentationBoardEnumCameraAction) {
        self.camera_queue.push(action);
    }

    pub fn has_queued_camera_actions(&self) -> bool {
        !self.camera_queue.is_empty()
    }

    pub fn drain_camera_actions(
        &mut self,
        camera: SceneCamera,
    ) -> Result<SceneCamera, PresentationAppError> {
        let queued = std::mem::take(&mut self.camera_queue);
        queued.into_iter().try_fold(camera, |camera, action| {
            self.apply_camera_action(camera, action)
        })
    }

    pub fn apply_camera_action(
        &mut self,
        camera: SceneCamera,
        action: PresentationBoardEnumCameraAction,
    ) -> Result<SceneCamera, PresentationAppError> {
        Ok(self.board.camera_action(camera, action)?)
    }
}
