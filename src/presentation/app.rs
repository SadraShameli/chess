mod backup;
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
use crate::desktop::backup_commands::DesktopBackupCommandsPrerequisites;
use crate::desktop::dto::{
    DesktopBoardSnapshot, DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot, DesktopEnumEvent,
    DesktopEnumValue,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopError, DesktopFailure};
use crate::desktop::game_bridge::DesktopGameBridgeEnumOperation;
use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::game_snapshot::{DesktopGameSnapshot, DesktopGameSnapshotEnumNotice};
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::layout::DesktopLayout;
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
    DesktopRequest,
};
use crate::desktop::preferences::DesktopPreferences;
use crate::desktop::puzzle_service::DesktopPuzzleServiceError;
use crate::desktop::renderer_commands::DesktopRendererCommands;
use crate::desktop::stream::DesktopStream;
use crate::game::enums::{GameEnumPhase, GameEnumSeat, GameId};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use crate::game::save_coordinator::{
    GameSaveCoordinator, GameSaveCoordinatorEnumBarrier, GameSaveCoordinatorError,
};
use crate::game::session::GameSessionEnumInterruption;
use crate::presentation::action::palette_view::{
    PresentationActionPaletteEnumChoice, PresentationActionPaletteView,
};
use crate::presentation::action::registry::{
    PresentationActionContext, PresentationActionEnumId, PresentationActionEnumWorkspace,
};
use crate::presentation::action::shortcuts::{
    PresentationShortcutEnumPlatform, PresentationShortcuts,
};
use crate::presentation::analysis::annotations::PresentationAnalysisAnnotationsEnumMarkingInput;
use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal;
use crate::presentation::app::editor::{
    PresentationAppEditor, PresentationAppEditorBoardInput, PresentationAppEditorError,
};
use crate::presentation::app::library::{
    PresentationAppLibrary, PresentationAppLibraryEnumNavigation,
};
use crate::presentation::audio::observer::PresentationAudioObserver;
use crate::presentation::audio::router::PresentationAudioEvent;
use crate::presentation::board::animation::PresentationBoardAnimationFrame;
use crate::presentation::board::enums::PresentationBoardEnumCameraAction;
use crate::presentation::board::figurines::PresentationBoardFigurines;
use crate::presentation::board::markings::{PresentationBoardMark, PresentationBoardMarkRequest};
use crate::presentation::board::recovery::{
    PresentationBoardRecovery, PresentationBoardRecoveryEnumIntent,
};
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::board::toolbar::PresentationBoardToolbar;
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
use crate::presentation::game::edge::PresentationGameEdge;
use crate::presentation::game::new_game::PresentationGameNewGame;
use crate::presentation::game::overview::{
    PresentationGameOverview, PresentationGameOverviewAction,
};
use crate::presentation::game::play::{
    PresentationGamePlay, PresentationGamePlayClock, PresentationGamePlayEnumOperation,
    PresentationGamePlayError, PresentationGamePlayIntent,
};
use crate::presentation::game::tabs::PresentationGameTabs;
use crate::presentation::puzzle::controller::PresentationPuzzleController;
use crate::presentation::puzzle::panel::{
    PresentationPuzzlePanel, PresentationPuzzlePanelConnection, PresentationPuzzlePanelError,
};
use crate::presentation::settings::controller::{
    PresentationSettingsConnection, PresentationSettingsController, PresentationSettingsOutcome,
};
use crate::presentation::settings::view::{
    PresentationSettingsEnumPage, PresentationSettingsNavigation, PresentationSettingsView,
};
use crate::presentation::study::PresentationStudy;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
    PresentationThemeEnumTone,
};
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;
use crate::puzzle::timer::PuzzleTimerEnumReadiness;
use crate::scene::camera::SceneCamera;
use crate::scene::picking::ScenePickingViewport;
use crate::scene::quality::{SceneQualityEnumMode, SceneQualityEnumTier};

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

impl PresentationAppEnumWorkspace {
    pub const fn inspector_title(self) -> &'static str {
        match self {
            Self::Games => "Game inspector",
            Self::Puzzles => "Puzzle inspector",
            Self::Library => "Library inspector",
            Self::Editor => "Editor inspector",
        }
    }
}

struct PresentationAppStageInput<'a> {
    scene: Option<&'a PresentationBoardScene>,
    now: ClockInstant,
    pending_move: Option<crate::game::premove::GamePremoveIntent>,
    promotion_owns_input: bool,
    can_enter_moves: bool,
    is_busy: bool,
    editor_viewport: &'a mut Option<ScenePickingViewport>,
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
    save: GameSaveCoordinator,
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
    Close(GameId),
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
    overview_open: bool,
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
    close_confirmation: Option<GameId>,
    closing_games: std::collections::HashSet<GameId>,
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
    settings: PresentationSettingsController,
    audio_observer: PresentationAudioObserver,
    update_pending: Vec<DesktopApplicationReceipt>,
    settings_open: bool,
    backup: backup::PresentationAppBackup,
    settings_toggle_frame: Option<u64>,
    markings_identity: Option<(PresentationAppEnumWorkspace, Option<GameId>, Option<GameId>)>,
    inspector_open: bool,
    inspector_toggle_frame: Option<u64>,
    layout: DesktopLayout,
    layout_reset_requested: bool,
    renderer_commands: DesktopRendererCommands,
    recovery_intent: Option<PresentationBoardRecoveryEnumIntent>,
    render_scale: f32,
    quality_mode: SceneQualityEnumMode,
    quality_mode_changed: bool,
    camera_queue: Vec<PresentationBoardEnumCameraAction>,
    navigation_frame: Option<u64>,
    palette: PresentationActionPaletteView,
    settings_navigation: PresentationSettingsNavigation,
}

impl PresentationApp {
    const MAX_GAME_REQUESTS: usize = 128;
    const PALETTE_FIELD_WIDTH: f32 = 240.0;

    pub fn new(clock: Arc<dyn ClockTimeProvider>) -> Self {
        let settings = PresentationSettingsController::new(None);
        let mut board = PresentationBoardView::with_animation(
            clock,
            settings.effective().motion().is_reduced(),
        );
        board.rest_panel(true);
        Self {
            overview_open: false,
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
            close_confirmation: None,
            closing_games: std::collections::HashSet::new(),
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
            settings,
            audio_observer: PresentationAudioObserver::default(),
            update_pending: Vec::new(),
            settings_open: false,
            backup: backup::PresentationAppBackup::new(),
            settings_toggle_frame: None,
            markings_identity: None,
            inspector_open: true,
            inspector_toggle_frame: None,
            layout: DesktopLayout::default(),
            layout_reset_requested: false,
            renderer_commands: DesktopRendererCommands::new(),
            recovery_intent: None,
            render_scale: 1.0,
            quality_mode: SceneQualityEnumMode::Automatic,
            quality_mode_changed: false,
            camera_queue: Vec::new(),
            navigation_frame: None,
            palette: PresentationActionPaletteView::default(),
            settings_navigation: PresentationSettingsNavigation::default(),
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

    fn has_unsaved_editor_state(&self) -> bool {
        self.editor
            .as_ref()
            .is_some_and(PresentationAppEditor::has_pending_work)
            || self
                .library
                .as_ref()
                .is_some_and(PresentationAppLibrary::has_pending_work)
    }

    pub fn has_pending_work(&self) -> bool {
        self.backup.is_busy()
            || self
                .editor
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

    fn dispatch_close<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        game: GameId,
        frame: u64,
        port: &DesktopApplicationPort<C, W>,
    ) {
        let Some(revision) = self
            .games
            .iter()
            .find(|current| current.snapshot.id() == game)
            .map(|current| current.snapshot.revision())
        else {
            return;
        };
        let input = self.input_identity(frame, PresentationAppEnumInput::Close(game));
        let operation = DesktopGameBridgeEnumOperation::Close { game, revision };
        if let Err(error) = self.activate_game(operation, input, port) {
            self.failure = Some(error.to_string());
        }
    }

    fn remove_game(&mut self, id: GameId) {
        self.games.retain(|game| game.snapshot.id() != id);
        self.closing_games.insert(id);
        if self.selected_game == Some(id) {
            self.selected_game = self.games.first().map(|game| game.snapshot.id());
            self.board.cancel_gesture();
        }
        self.status = "The game was saved and closed".to_owned();
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
                            && !has_new_notice
                            && snapshot.admission() == game.snapshot.admission()
                            && snapshot.allocation() == game.snapshot.allocation()))
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
            let tracked = Self::track_save(&mut game.save, &snapshot);
            game.snapshot = snapshot;
            if let Err(error) = tracked {
                self.failure = Some(error.to_string());
            }
        } else {
            if let Some(notice) = notice {
                self.game_notice = Some(PresentationAppGameNotice::new(notice));
            }
            let mut save = GameSaveCoordinator::new(id, snapshot.revision());
            if let Err(error) = Self::track_save(&mut save, &snapshot) {
                self.failure = Some(error.to_string());
            }
            self.games.push(PresentationAppGame {
                snapshot,
                controller: PresentationGameController::new(),
                play: PresentationGamePlay::new(),
                last_notice: notice,
                save,
            });
            if self.selected_game.is_none() {
                self.selected_game = Some(id);
            }
        }
        let unsaved: Vec<GameId> = self
            .games
            .iter()
            .filter(|game| {
                game.save
                    .require_saved(GameSaveCoordinatorEnumBarrier::MoveToTrash)
                    .is_err()
            })
            .map(|game| game.snapshot.id())
            .collect();
        if let Some(library) = &mut self.library {
            library.observe_unsaved_games(unsaved);
        }
        self.status = "The native game state is ready".to_owned();
        true
    }

    pub fn interrupt_for_sleep<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        self.pause_active_games(GamePauseEnumCause::ComputerSleep, port)
    }

    fn pause_active_games<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        cause: GamePauseEnumCause,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let targets: Vec<_> = self
            .games
            .iter()
            .filter(|game| game.snapshot.phase() == GameEnumPhase::Active)
            .map(|game| (game.snapshot.id(), game.snapshot.revision()))
            .collect();
        let mut first_failure = None;
        for (game, revision) in targets {
            let operation = DesktopGameBridgeEnumOperation::Session {
                game,
                revision,
                action: DesktopGameCommandsEnumAction::Pause { cause },
            };
            if let Err(error) = self.activate_game(operation, DesktopRequestId::new(), port) {
                first_failure.get_or_insert(error);
            }
        }
        first_failure.map_or(Ok(()), Err)
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
            DesktopGameBridgeEnumOperation::Session { game, .. }
            | DesktopGameBridgeEnumOperation::Close { game, .. } => Some(*game),
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
        self.settings.effective()
    }

    fn submit_update<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) {
        const MAX_PENDING: usize = 8;
        if self.update_pending.len() >= MAX_PENDING {
            return;
        }
        let Some(operation) = self.settings.take_update_intent() else {
            return;
        };
        let submitted = port
            .request(DesktopCommand::new(
                DesktopEnumCommand::Update(operation),
                None,
            ))
            .and_then(|request| port.try_submit(&request));
        match submitted {
            Ok(receipt) => self.update_pending.push(receipt),
            Err(error) => self.failure = Some(error.to_string()),
        }
    }

    fn poll_updates(&mut self) {
        let mut completed = Vec::new();
        self.update_pending.retain(|receipt| {
            if let Some(response) = receipt.response() {
                completed.push(response.clone());
                false
            } else {
                true
            }
        });
        for response in completed {
            self.accept_response(response);
        }
    }

    pub fn audio_events(&mut self) -> Vec<PresentationAudioEvent> {
        self.audio_observer
            .observe(self.games.iter().map(|game| &game.snapshot))
    }

    pub const fn selected_game_id(&self) -> Option<GameId> {
        self.selected_game
    }

    pub fn mount_settings(&mut self, connection: PresentationSettingsConnection) {
        self.settings = PresentationSettingsController::new(Some(connection));
        self.board
            .set_reduced_motion(self.settings.effective().motion().is_reduced());
        self.theme_initialized = false;
    }

    fn apply_settings_outcome(
        &mut self,
        ctx: &egui::Context,
        outcome: PresentationSettingsOutcome,
    ) {
        if outcome.appearance_changed {
            PresentationThemeTokens::apply(ctx, self.settings.effective().theme());
            self.board
                .set_reduced_motion(self.settings.effective().motion().is_reduced());
            ctx.request_discard("apply appearance before layout");
        }
        if outcome.close {
            self.settings_open = false;
        }
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

    fn track_save(
        coordinator: &mut GameSaveCoordinator,
        snapshot: &DesktopGameSnapshot,
    ) -> Result<(), GameSaveCoordinatorError> {
        coordinator.observe_memory(snapshot.revision());
        if snapshot.is_save_required() {
            coordinator.record_failure("the latest moves are not saved".to_owned());
        } else {
            coordinator.acknowledge(snapshot.id(), snapshot.revision())?;
        }
        Ok(())
    }

    pub fn require_game_saved(
        &self,
        game: GameId,
        barrier: GameSaveCoordinatorEnumBarrier,
    ) -> Result<(), GameSaveCoordinatorError> {
        self.games
            .iter()
            .find(|candidate| candidate.snapshot.id() == game)
            .map_or(Ok(()), |candidate| candidate.save.require_saved(barrier))
    }

    fn request_quit_unguarded(&mut self) {
        if let Some(error) = self.games.iter().find_map(|game| {
            game.save
                .require_saved(GameSaveCoordinatorEnumBarrier::QuitApplication)
                .err()
        }) {
            self.failure = Some(format!(
                "{error}. Use Retry Save on the unsaved game, then quit again."
            ));
            return;
        }
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
        self.poll_updates();
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
            let is_close = self.game_retained.get(&input).is_some_and(|retained| {
                matches!(
                    retained.request.command().operation(),
                    DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Close { .. })
                )
            });
            if let Some(retained) = self.game_retained.get_mut(&input) {
                retained.completed = true;
            }
            if is_close
                && let Some(closed) = game_id
                && matches!(
                    &response,
                    DesktopMessagesEnumResult::Value {
                        value: DesktopEnumValue::Game(_),
                        ..
                    }
                )
            {
                self.remove_game(closed);
                continue;
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
                DesktopEnumCommand::Game(
                    DesktopGameBridgeEnumOperation::Create { .. }
                    | DesktopGameBridgeEnumOperation::Close { .. },
                ) => false,
                DesktopEnumCommand::Engine(_)
                | DesktopEnumCommand::Backup(_)
                | DesktopEnumCommand::Library(_)
                | DesktopEnumCommand::Editor { .. }
                | DesktopEnumCommand::Puzzle { .. }
                | DesktopEnumCommand::Bootstrap
                | DesktopEnumCommand::Update(_)
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
            DesktopEnumValue::Backup(_) => {
                self.failure = Some("The backup response has no matching request".to_owned());
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
            DesktopEnumValue::Update(snapshot) => self.settings.accept_update(*snapshot),
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
                        if self.closing_games.contains(&snapshot.id()) {
                            continue;
                        }
                        self.accept_game_snapshot(snapshot.clone());
                    }
                    self.closing_games
                        .retain(|id| snapshots.iter().any(|snapshot| snapshot.id() == *id));
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

    fn show_settings<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
    ) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let update_badge = self
            .settings
            .update()
            .and_then(|snapshot| snapshot.state().release())
            .map(|release| release.version.to_string());
        egui::Panel::left("chess.settings.navigation")
            .exact_size(PresentationSettingsNavigation::NAVIGATION_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(colors.surface_1)
                    .stroke(egui::Stroke::new(1.0, colors.line))
                    .inner_margin(egui::Margin::symmetric(10, 16)),
            )
            .show(ui, |ui| {
                self.settings_navigation.show(ui, update_badge.as_deref());
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.bg))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("chess.settings.page")
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: 40,
                                right: 40,
                                top: 32,
                                bottom: 64,
                            })
                            .show(ui, |ui| {
                                ui.set_max_width(PresentationSettingsNavigation::CONTENT_WIDTH);
                                self.show_settings_content(ui, port);
                            });
                    });
            });
        self.submit_update(port);
    }

    fn show_settings_content<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
    ) {
        let layout = &mut self.layout;
        let layout_reset_requested = &mut self.layout_reset_requested;
        let quality_mode = &mut self.quality_mode;
        let quality_mode_changed = &mut self.quality_mode_changed;
        let outcome = PresentationSettingsView::show(
            ui,
            &mut self.settings,
            &mut self.settings_navigation,
            |ui| {
                PresentationSettingsView::row(
                    ui,
                    "Board quality",
                    Some("Automatic adapts rendering to this machine."),
                    |ui| {
                        let mut quality = *quality_mode;
                        PresentationThemeComponents::segmented(
                            ui,
                            &mut quality,
                            &[
                                (SceneQualityEnumMode::Automatic, "Automatic"),
                                (
                                    SceneQualityEnumMode::Manual(SceneQualityEnumTier::Low),
                                    "Low",
                                ),
                                (
                                    SceneQualityEnumMode::Manual(SceneQualityEnumTier::Medium),
                                    "Medium",
                                ),
                                (
                                    SceneQualityEnumMode::Manual(SceneQualityEnumTier::High),
                                    "High",
                                ),
                            ],
                            false,
                        );
                        if quality != *quality_mode {
                            *quality_mode = quality;
                            *quality_mode_changed = true;
                        }
                    },
                );
                PresentationThemeComponents::divider(ui);
                PresentationSettingsView::row(
                    ui,
                    "Workspace layout",
                    Some("Restores the rail, inspector width and panels."),
                    |ui| {
                        if PresentationThemeEnumButton::Secondary
                            .show_sized(ui, "Reset Layout", PresentationThemeEnumSize::Small)
                            .clicked()
                        {
                            layout.reset();
                            *layout_reset_requested = true;
                        }
                    },
                );
            },
        );
        self.apply_settings_outcome(&ui.ctx().clone(), outcome);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        let prerequisites = DesktopBackupCommandsPrerequisites {
            games_paused_and_saved: self.games.iter().all(|game| {
                game.snapshot.phase() != GameEnumPhase::Active && !game.snapshot.is_save_required()
            }),
            editors_have_no_unsaved_edits: !self.has_unsaved_editor_state(),
        };
        if self.settings.take_install_request() {
            self.settings.request_update(
                crate::desktop::update_commands::DesktopUpdateOperation::Install { prerequisites },
            );
        }
        let backup = &mut self.backup;
        self.settings_navigation
            .page(ui, PresentationSettingsEnumPage::Backups, |ui| {
                PresentationSettingsView::card(ui, |ui| {
                    PresentationSettingsView::block(ui, |ui| {
                        backup.show(ui, port, now_ms, prerequisites);
                    });
                });
            });
        let study = &mut self.study;
        self.settings_navigation
            .page(ui, PresentationSettingsEnumPage::Study, |ui| {
                PresentationSettingsView::card(ui, |ui| {
                    PresentationSettingsView::block(ui, |ui| study.show_panels(ui));
                });
            });
    }

    fn open_settings(&mut self, frame: u64) {
        if self.settings_toggle_frame != Some(frame) {
            if !self.settings_open {
                self.settings_open = true;
            } else if self.settings.request_close() {
                self.settings_open = false;
            }
            self.settings_toggle_frame = Some(frame);
            self.board.cancel_gesture();
        }
    }

    fn paint_stage(painter: &egui::Painter, rect: egui::Rect, colors: &PresentationThemeColors) {
        const SEGMENTS: u32 = 64;
        const STOPS: [(f32, f32); 4] = [(0.0, 0.0), (0.3, 0.25), (0.55, 0.65), (0.72, 1.0)];
        let center = egui::pos2(rect.center().x, rect.top() + rect.height() * 0.42);
        let radius = egui::vec2(rect.width() * 0.75, rect.height() * 0.95);
        painter.rect_filled(rect, 0.0, colors.bg);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(center, colors.surface_2);
        for (ring, (extent, blend)) in STOPS.iter().enumerate().skip(1) {
            let color = colors.surface_2.lerp_to_gamma(colors.bg, *blend);
            for step in 0..SEGMENTS {
                let angle = std::f32::consts::TAU * f32::from(u16::try_from(step).unwrap_or(0))
                    / f32::from(u16::try_from(SEGMENTS).unwrap_or(1));
                let offset = egui::vec2(angle.cos() * radius.x, angle.sin() * radius.y) * *extent;
                mesh.colored_vertex(center + offset, color);
                let current = u32::try_from(ring - 1).unwrap_or(0) * SEGMENTS + 1 + step;
                let next =
                    u32::try_from(ring - 1).unwrap_or(0) * SEGMENTS + 1 + (step + 1) % SEGMENTS;
                if ring == 1 {
                    mesh.add_triangle(0, current, next);
                } else {
                    let inner = current - SEGMENTS;
                    let inner_next = next - SEGMENTS;
                    mesh.add_triangle(inner, current, next);
                    mesh.add_triangle(inner, next, inner_next);
                }
            }
        }
        painter.with_clip_rect(rect).add(egui::Shape::mesh(mesh));
    }

    fn show_logo(ui: &mut Ui, colors: &PresentationThemeColors) {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(36.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 9.0, colors.text);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "\u{265E}",
            egui::FontId::proportional(PresentationThemeTypography::LOGO_POINTS),
            colors.bg,
        );
    }

    fn show_palette_shortcut(ui: &mut Ui, label: &str, keys: &[String]) -> egui::Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(egui::vec2(60.0, 40.0), egui::Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, PresentationThemeTokens::RADIUS_LG, colors.surface_2);
        }
        let font = egui::FontId::monospace(PresentationThemeTypography::MICRO_POINTS);
        let galleys: Vec<_> = keys
            .iter()
            .map(|key| {
                ui.painter()
                    .layout_no_wrap(key.clone(), font.clone(), colors.text_2)
            })
            .collect();
        let widths: Vec<f32> = galleys
            .iter()
            .map(|galley| galley.size().x.max(8.0) + 10.0)
            .collect();
        let gaps = u8::try_from(galleys.len().saturating_sub(1)).unwrap_or(0);
        let total = widths.iter().sum::<f32>() + 2.0 * f32::from(gaps);
        let mut x = rect.center().x - total / 2.0;
        for (galley, width) in galleys.into_iter().zip(widths) {
            let key_rect = egui::Rect::from_min_size(
                egui::pos2(x, rect.center().y - 9.0),
                egui::vec2(width, 18.0),
            );
            ui.painter().rect(
                key_rect,
                PresentationThemeTokens::RADIUS_SM,
                colors.surface_1,
                egui::Stroke::new(1.0, colors.line),
                egui::StrokeKind::Inside,
            );
            ui.painter().galley(
                key_rect.center() - galley.size() / 2.0,
                galley,
                colors.text_2,
            );
            x += width + 2.0;
        }
        let label = label.to_owned();
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
        });
        response.on_hover_text(label)
    }

    fn palette_chord_keys(preferences: DesktopPreferences) -> Vec<String> {
        let chord = PresentationShortcuts::chord(
            preferences,
            crate::desktop::preferences::DesktopPreferencesEnumShortcutAction::Palette,
        );
        let mut keys = Vec::new();
        if chord.alt() {
            keys.push("Alt".to_owned());
        }
        if chord.shift() {
            keys.push("Shift".to_owned());
        }
        if chord.command() {
            keys.push(match PresentationShortcutEnumPlatform::current() {
                PresentationShortcutEnumPlatform::Mac => "\u{2318}".to_owned(),
                PresentationShortcutEnumPlatform::Other => "Ctrl".to_owned(),
            });
        }
        keys.push(chord.key().to_ascii_uppercase().to_string());
        keys
    }

    fn show_palette_field(ui: &mut Ui, width: f32, shortcut: &str) -> egui::Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let size = egui::vec2(width, PresentationThemeTokens::CONTROL_SM);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let stroke = if response.hovered() {
            colors.line_strong
        } else {
            colors.line
        };
        ui.painter().rect(
            rect,
            PresentationThemeTokens::RADIUS_MD,
            colors.bg,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        let icon = egui::Rect::from_center_size(
            rect.left_center() + egui::vec2(18.0, 0.0),
            egui::Vec2::splat(16.0),
        );
        PresentationThemeIcon::paint(
            ui.painter(),
            icon,
            PresentationThemeEnumIcon::Search,
            colors.text_3,
        );
        let label = "Search or run a command";
        ui.painter().text(
            rect.left_center() + egui::vec2(34.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(PresentationThemeTypography::UI_POINTS),
            colors.text_3,
        );
        let font = egui::FontId::monospace(PresentationThemeTypography::MICRO_POINTS);
        let galley = ui
            .painter()
            .layout_no_wrap(shortcut.to_owned(), font, colors.text_2);
        let key_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - galley.size().x - 18.0, rect.center().y - 9.0),
            egui::vec2(galley.size().x + 10.0, 18.0),
        );
        ui.painter().rect(
            key_rect,
            PresentationThemeTokens::RADIUS_SM,
            colors.surface_1,
            egui::Stroke::new(1.0, colors.line),
            egui::StrokeKind::Inside,
        );
        ui.painter().galley(
            key_rect.center() - galley.size() / 2.0,
            galley,
            colors.text_2,
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        response
    }

    fn palette_shortcut_pressed(&self, ctx: &egui::Context) -> bool {
        let chord = PresentationShortcuts::chord(
            *self.settings.effective(),
            crate::desktop::preferences::DesktopPreferencesEnumShortcutAction::Palette,
        );
        let Some(key) = egui::Key::from_name(&chord.key().to_ascii_uppercase().to_string()) else {
            return false;
        };
        let mut modifiers = egui::Modifiers::NONE;
        if chord.command() {
            modifiers |= egui::Modifiers::COMMAND;
        }
        if chord.shift() {
            modifiers |= egui::Modifiers::SHIFT;
        }
        if chord.alt() {
            modifiers |= egui::Modifiers::ALT;
        }
        ctx.input_mut(|input| input.consume_shortcut(&egui::KeyboardShortcut::new(modifiers, key)))
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
            PresentationThemeTokens::apply(ui.ctx(), self.settings.effective().theme());
            self.theme_initialized = true;
            ui.ctx()
                .request_discard("apply initial native theme before layout");
        }
        PresentationBoardFigurines::publish(ui.ctx(), self.settings.effective().notation());
        let settings_outcome = self.settings.poll();
        self.apply_settings_outcome(&ui.ctx().clone(), settings_outcome);
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
        let mut overview_rows = Vec::new();
        let mut close_click = None;
        let mut palette_camera = None;
        let colors = PresentationThemeColors::of(ui.ctx());
        let palette_keys = Self::palette_chord_keys(*self.settings.effective());
        let palette_text = palette_keys.concat();
        let mut open_palette = false;
        let mut open_settings = false;
        egui::Panel::left("chess.rail")
            .exact_size(PresentationThemeTokens::RAIL_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(colors.surface_1)
                    .stroke(egui::Stroke::new(1.0, colors.line))
                    .inner_margin(egui::Margin::symmetric(8, 12)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = PresentationThemeTokens::SPACE_1;
                ui.vertical_centered(|ui| {
                    Self::show_logo(ui, &colors);
                    ui.add_space(14.0);
                    for (target, icon, label, enabled) in [
                        (
                            PresentationAppEnumWorkspace::Games,
                            PresentationThemeEnumIcon::Games,
                            "Play",
                            true,
                        ),
                        (
                            PresentationAppEnumWorkspace::Puzzles,
                            PresentationThemeEnumIcon::Puzzles,
                            "Puzzles",
                            self.puzzle_panel.is_some(),
                        ),
                        (
                            PresentationAppEnumWorkspace::Library,
                            PresentationThemeEnumIcon::Library,
                            "Library",
                            self.library.is_some(),
                        ),
                        (
                            PresentationAppEnumWorkspace::Editor,
                            PresentationThemeEnumIcon::Study,
                            "Editor",
                            self.editor.is_some(),
                        ),
                    ] {
                        if PresentationThemeComponents::rail_item(
                            ui,
                            icon,
                            label,
                            self.workspace == target && !self.settings_open,
                            enabled,
                        )
                        .clicked()
                        {
                            workspace = Some(target);
                        }
                    }
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        if PresentationThemeIcon::button(
                            ui,
                            PresentationThemeEnumIcon::Close,
                            "Quit",
                        )
                        .clicked()
                        {
                            navigation = Some(PresentationAppLibraryEnumNavigation::Quit);
                        }
                        if PresentationThemeComponents::rail_item(
                            ui,
                            PresentationThemeEnumIcon::Settings,
                            "Settings",
                            self.settings_open,
                            true,
                        )
                        .clicked()
                        {
                            open_settings = true;
                        }
                        if Self::show_palette_shortcut(ui, "Command palette", &palette_keys)
                            .clicked()
                        {
                            open_palette = true;
                        }
                    });
                });
            });
        egui::Panel::top("chess.header")
            .exact_size(PresentationThemeTokens::HEADER_HEIGHT)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(colors.surface_1)
                    .stroke(egui::Stroke::new(1.0, colors.line))
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_1;
                    match PresentationGameOverview::rows(
                        self.games.iter().map(|game| &game.snapshot),
                        now,
                    ) {
                        Ok(rows) => {
                            let action = PresentationGameTabs::show(ui, &rows, self.selected_game);
                            if let Some(chosen) = action.selected {
                                navigation =
                                    Some(PresentationAppLibraryEnumNavigation::Game(chosen));
                            }
                            close_click = action.close;
                            if action.overview {
                                self.overview_open = true;
                            }
                            if action.selected.is_some() {
                                self.overview_open = false;
                            }
                            if action.new_game && self.new_game_toggle_frame != Some(frame) {
                                self.new_game_toggle_frame = Some(frame);
                                navigation = Some(PresentationAppLibraryEnumNavigation::NewGame);
                            }
                            overview_rows = rows;
                        }
                        Err(error) => self.failure = Some(error.to_string()),
                    }
                    if PresentationThemeIcon::button(
                        ui,
                        PresentationThemeEnumIcon::Plus,
                        "New Game",
                    )
                    .clicked()
                        && self.new_game_toggle_frame != Some(frame)
                    {
                        self.new_game_toggle_frame = Some(frame);
                        navigation = Some(PresentationAppLibraryEnumNavigation::NewGame);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_2;
                        if PresentationThemeIcon::button(
                            ui,
                            PresentationThemeEnumIcon::Panel,
                            if self.inspector_open {
                                "Hide inspector"
                            } else {
                                "Show inspector"
                            },
                        )
                        .clicked()
                        {
                            inspector_toggle = true;
                        }
                        if Self::show_palette_field(ui, Self::PALETTE_FIELD_WIDTH, &palette_text)
                            .clicked()
                        {
                            open_palette = true;
                        }
                    });
                });
            });
        if self.palette_shortcut_pressed(ui.ctx()) && !self.quit_confirmation {
            open_palette = true;
        }
        if open_palette && !promotion_owns_input {
            self.palette.open();
        }
        let palette_context = PresentationActionContext {
            board_visible: !self.settings_open && self.board_snapshot().is_some(),
            settings_open: self.settings_open,
        };
        match self
            .palette
            .show(ui.ctx(), palette_context, *self.settings.effective())
        {
            Some(PresentationActionPaletteEnumChoice::Workspace(action)) => match action {
                PresentationActionEnumWorkspace::OpenPalette => {}
                PresentationActionEnumWorkspace::NewGame => {
                    if self.new_game_toggle_frame != Some(frame) {
                        self.new_game_toggle_frame = Some(frame);
                        navigation = Some(PresentationAppLibraryEnumNavigation::NewGame);
                    }
                }
                PresentationActionEnumWorkspace::OpenSettings => open_settings = true,
                PresentationActionEnumWorkspace::FlipBoard => {
                    palette_camera = Some(PresentationBoardEnumCameraAction::Flip);
                }
                PresentationActionEnumWorkspace::Quit => {
                    navigation = Some(PresentationAppLibraryEnumNavigation::Quit);
                }
            },
            Some(PresentationActionPaletteEnumChoice::System(action)) => actions.push(action),
            None => {}
        }
        if open_settings {
            self.open_settings(frame);
        }
        if overview_rows.is_empty() {
            self.overview_open = false;
        }
        let mut new_game_cancelled = false;
        let mut overview_action = PresentationGameOverviewAction::default();
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
        if self.workspace == PresentationAppEnumWorkspace::Library && !self.settings_open {
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
        if self.settings_open {
            self.show_settings(ui, port);
        } else if inspector_progress > 0.0 {
            egui::Panel::right("chess.inspector")
            .exact_size(self.layout.panel_width() * inspector_progress)
            .frame(
                egui::Frame::new()
                    .fill(colors.surface_1)
                    .stroke(egui::Stroke::new(1.0, colors.line))
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| {
                self.show_inspector_header(ui, &colors);
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
                    new_game_cancelled = new_game.take_cancel();
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
                        PresentationThemeComponents::divider(ui);
                        PresentationThemeComponents::subhead(ui, "Live games");
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
                    PresentationThemeComponents::subhead(ui, "Position");
                    if let Some(snapshot) = self.board_snapshot() {
                        ui.label(format!("{} to move{}", PresentationBoardSemanticGrid::color_name(snapshot.turn()), if snapshot.is_check() { ", in check" } else { "" }));
                        ui.label(format!("{} legal moves", snapshot.legal_moves().len()));
                    }
                }
                if let Some(snapshot) = &self.snapshot {
                    ui.add_space(PresentationThemeTokens::SPACE_3);
                    PresentationThemeComponents::card(ui).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            PresentationThemeComponents::subhead(ui, "Reckless");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                match snapshot.engine() {
                                    DesktopEnumEngineSnapshot::Ready { .. } => {
                                        PresentationThemeComponents::pill(ui, "Ready", PresentationThemeEnumTone::Run);
                                    }
                                    DesktopEnumEngineSnapshot::Unavailable { .. } => {
                                        PresentationThemeComponents::pill(ui, "Engine unavailable", PresentationThemeEnumTone::Error);
                                    }
                                }
                            });
                        });
                        match snapshot.engine() {
                            DesktopEnumEngineSnapshot::Ready { identity } => {
                                ui.label(identity.name());
                                PresentationThemeComponents::caption(ui, identity.author());
                                PresentationThemeComponents::caption(ui, format!("{} supported engine options", identity.supported_options().len()));
                            }
                            DesktopEnumEngineSnapshot::Unavailable { message } => {
                                ui.label(message);
                            }
                        }
                        PresentationThemeComponents::caption(ui, format!("{} usable hardware threads", snapshot.cpu_threads()));
                    });
                } else {
                    PresentationThemeComponents::caption(ui, "Waiting for the local application");
                }
                if let Some(panels) = &mut self.engine_panels
                    && let Err(error) = panels.show(ui, port) {
                    self.failure = Some(error.to_string());
                }
                PresentationThemeComponents::divider(ui);
                PresentationThemeComponents::caption(ui, "Use Tab to enter the board, arrow keys to move focus, and Enter or Space to select a square.");
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
        egui::Panel::bottom("chess.status")
            .frame(
                egui::Frame::new()
                    .fill(colors.surface_1)
                    .stroke(egui::Stroke::new(1.0, colors.line))
                    .inner_margin(egui::Margin::symmetric(16, 6)),
            )
            .show(ui, |ui| {
                let response = ui.label(
                    egui::RichText::new(status)
                        .size(PresentationThemeTypography::CAPTION_POINTS)
                        .color(colors.text_2),
                );
                ui.ctx().accesskit_node_builder(response.id, |node| {
                    node.set_role(egui::accesskit::Role::Status);
                    node.set_live(egui::accesskit::Live::Polite);
                });
                if retry_quit {
                    retry = PresentationThemeEnumButton::Secondary
                        .show_sized(ui, "Retry quit", PresentationThemeEnumSize::Small)
                        .clicked();
                }
                if has_refused_game {
                    retry_game = PresentationThemeEnumButton::Secondary
                        .show_sized(ui, "Retry action", PresentationThemeEnumSize::Small)
                        .clicked();
                }
                if needs_stream_retry {
                    retry_stream = PresentationThemeEnumButton::Secondary
                        .show_sized(ui, "Retry sync", PresentationThemeEnumSize::Small)
                        .clicked();
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
        if let Some(action) = palette_camera {
            camera_action = Some(action);
            self.board.cancel_gesture();
        }
        let board_frame = if self.settings_open {
            None
        } else {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 12)))
                .show(ui, |ui| {
                    let stage = ui.max_rect().expand2(egui::vec2(24.0, 12.0));
                    Self::paint_stage(ui.painter(), stage, &colors);
                    if let Some(intent) =
                        PresentationBoardRecovery::show(ui, &self.renderer_commands)
                    {
                        self.recovery_intent = Some(intent);
                    }
                    if self.overview_open && self.workspace == PresentationAppEnumWorkspace::Games {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            overview_action = PresentationGameOverview::show(ui, &overview_rows);
                        });
                        return Ok::<_, PresentationAppError>(None);
                    }
                    let result = self.show_stage(
                        ui,
                        port,
                        picker,
                        PresentationAppStageInput {
                            scene,
                            now,
                            pending_move,
                            promotion_owns_input,
                            can_enter_moves,
                            is_busy,
                            editor_viewport: &mut editor_viewport,
                        },
                    );
                    if let Some(action) = PresentationBoardToolbar::show(
                        ui,
                        stage,
                        scene.is_some(),
                        self.board.markings_mut(),
                    ) {
                        camera_action = Some(action);
                        self.board.cancel_gesture();
                    }
                    result
                })
                .inner?
        };
        if let Some(chosen) = overview_action.open
            && self.navigation_frame != Some(frame)
        {
            self.overview_open = false;
            self.navigation_frame = Some(frame);
            if let Err(error) =
                self.navigate(PresentationAppLibraryEnumNavigation::Game(chosen), port)
            {
                self.report_failure(error);
            }
        }
        if overview_action.new_game && self.new_game_toggle_frame != Some(frame) {
            self.new_game_toggle_frame = Some(frame);
            if let Err(error) = self.navigate(PresentationAppLibraryEnumNavigation::NewGame, port) {
                self.report_failure(error);
            }
        }
        if overview_action.pause_all
            && let Err(error) = self.pause_active_games(GamePauseEnumCause::Manual, port)
        {
            self.report_failure(error);
        }
        if let Some(game) = close_click
            && self.close_confirmation.is_none()
            && !self.is_game_busy(game)
        {
            let unsaved = self
                .games
                .iter()
                .find(|current| current.snapshot.id() == game)
                .map(|current| current.snapshot.has_unsaved_changes());
            match unsaved {
                Some(true) => self.close_confirmation = Some(game),
                Some(false) => self.dispatch_close(game, frame, port),
                None => {}
            }
        }
        if let Some(game) = self.close_confirmation {
            let mut keep = false;
            let mut confirm = false;
            let modal = crate::presentation::theme::components::PresentationThemeComponents::modal(ui.ctx(), Id::new("chess.game.close")).show(ui.ctx(), |ui| {
                ui.heading("Close this game?");
                ui.label("Its unsaved moves are saved before the game closes.");
                ui.horizontal(|ui| {
                    keep = crate::presentation::theme::components::PresentationThemeEnumButton::Ghost.show(ui, "Keep game open").clicked();
                    confirm = crate::presentation::theme::components::PresentationThemeEnumButton::Primary.show(ui, "Save and close").clicked();
                });
            });
            if confirm {
                self.dispatch_close(game, frame, port);
            }
            if keep || confirm || modal.should_close() {
                self.close_confirmation = None;
            }
        }
        if self.quit_confirmation {
            let mut cancel = false;
            let mut confirm = false;
            self.quit_modal.capture_invoker(ui.ctx());
            let modal = crate::presentation::theme::components::PresentationThemeComponents::modal(
                ui.ctx(),
                Id::new("chess.quit.confirmation"),
            )
            .show(ui.ctx(), |ui| {
                PresentationAnalysisEditorModal::label(ui, "Quit Chess?");
                ui.heading("Quit Chess?");
                ui.label("The application will finish its owned shutdown before closing.");
                ui.horizontal(|ui| {
                    let keep =
                        crate::presentation::theme::components::PresentationThemeEnumButton::Ghost
                            .show(ui, "Keep open");
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
        if new_game_cancelled {
            self.new_game = None;
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

    fn show_stage<C, W, P>(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
        picker: &mut P,
        input: PresentationAppStageInput<'_>,
    ) -> Result<Option<PresentationBoardViewFrame>, PresentationAppError>
    where
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
        P: PresentationBoardPickingProvider,
    {
        let PresentationAppStageInput {
            scene,
            now,
            pending_move,
            promotion_owns_input,
            can_enter_moves,
            is_busy,
            editor_viewport,
        } = input;
        if self.workspace == PresentationAppEnumWorkspace::Editor {
            if let Some(editor) = &mut self.editor {
                *editor_viewport = Some(
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
            return Ok(None);
        }
        let edge_game = if self.workspace == PresentationAppEnumWorkspace::Games {
            Self::selected_game_snapshot(&self.games, self.selected_game)
        } else {
            None
        };
        PresentationGameEdge::show(ui, BoardEnumColor::White, edge_game, now, |ui| {
            if self.selected_game.is_none() && self.workspace == PresentationAppEnumWorkspace::Games
            {
                PresentationThemeComponents::caption(ui, "No active game");
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
        PresentationGameEdge::show(ui, BoardEnumColor::Black, edge_game, now, |_ui| {});
        result.map_err(PresentationAppError::from)
    }

    fn show_inspector_header(&self, ui: &mut Ui, colors: &PresentationThemeColors) {
        ui.horizontal(|ui| {
            PresentationThemeComponents::eyebrow(ui, self.workspace.inspector_title());
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match self.workspace {
                    PresentationAppEnumWorkspace::Games => {
                        if let Some(game) =
                            Self::selected_game_snapshot(&self.games, self.selected_game)
                        {
                            if game.has_unsaved_changes() {
                                PresentationThemeComponents::pill(
                                    ui,
                                    "Unsaved",
                                    PresentationThemeEnumTone::Unsaved,
                                );
                            } else if game.phase() == GameEnumPhase::Active {
                                PresentationThemeComponents::pill(
                                    ui,
                                    "Live",
                                    PresentationThemeEnumTone::Run,
                                );
                            }
                        }
                    }
                    PresentationAppEnumWorkspace::Puzzles
                    | PresentationAppEnumWorkspace::Library
                    | PresentationAppEnumWorkspace::Editor => {}
                },
            );
        });
        ui.add_space(PresentationThemeTokens::SPACE_2);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().hline(
            rect.x_range().expand(16.0),
            rect.center().y,
            egui::Stroke::new(1.0, colors.line),
        );
        ui.add_space(PresentationThemeTokens::SPACE_2);
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

    pub const fn quality_mode(&self) -> SceneQualityEnumMode {
        self.quality_mode
    }

    pub fn take_quality_mode(&mut self) -> Option<SceneQualityEnumMode> {
        std::mem::take(&mut self.quality_mode_changed).then_some(self.quality_mode)
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
