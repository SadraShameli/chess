use crate::desktop::file_handle::DesktopFileEnumPurpose;
use crate::desktop::identity::DesktopRevision;
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};
use crate::desktop::preferences::DesktopPreferencesEnumShortcutAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationActionEnumId {
    Subscribe,
    Refresh,
    SelectFile,
    ConfirmQuit,
}

impl PresentationActionEnumId {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Subscribe => "Subscribe to state",
            Self::Refresh => "Refresh state",
            Self::SelectFile => "Select local file",
            Self::ConfirmQuit => "Confirm quit",
        }
    }

    pub fn command(self, file_owner: DesktopMessagesEntityId) -> DesktopCommand {
        let (operation, revision) = match self {
            Self::Subscribe => (DesktopEnumCommand::StreamSubscribe, None),
            Self::Refresh => (DesktopEnumCommand::Bootstrap, None),
            Self::SelectFile => (
                DesktopEnumCommand::FileSelect {
                    owner: file_owner,
                    purpose: DesktopFileEnumPurpose::ReadFile,
                },
                Some(DesktopRevision::ZERO),
            ),
            Self::ConfirmQuit => (DesktopEnumCommand::Quit, None),
        };
        DesktopCommand::new(operation, revision)
    }
}

pub struct PresentationActionRegistry;

impl PresentationActionRegistry {
    const ACTIONS: [PresentationActionEnumId; 4] = [
        PresentationActionEnumId::Subscribe,
        PresentationActionEnumId::Refresh,
        PresentationActionEnumId::SelectFile,
        PresentationActionEnumId::ConfirmQuit,
    ];

    pub fn actions() -> &'static [PresentationActionEnumId] {
        &Self::ACTIONS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationActionEnumWorkspace {
    OpenPalette,
    NewGame,
    OpenSettings,
    FlipBoard,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresentationActionContext {
    pub board_visible: bool,
    pub settings_open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationActionEnumAvailability {
    Available,
    Unavailable(&'static str),
}

impl PresentationActionEnumAvailability {
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

impl PresentationActionEnumWorkspace {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenPalette => "Open command palette",
            Self::NewGame => "New game",
            Self::OpenSettings => "Open settings",
            Self::FlipBoard => "Flip board",
            Self::Quit => "Quit",
        }
    }

    pub const fn shortcut_action(self) -> Option<DesktopPreferencesEnumShortcutAction> {
        match self {
            Self::OpenPalette => Some(DesktopPreferencesEnumShortcutAction::Palette),
            Self::NewGame => Some(DesktopPreferencesEnumShortcutAction::NewGame),
            Self::OpenSettings => Some(DesktopPreferencesEnumShortcutAction::Settings),
            Self::FlipBoard => Some(DesktopPreferencesEnumShortcutAction::FlipBoard),
            Self::Quit => None,
        }
    }

    pub const fn requires_confirmation(self) -> bool {
        matches!(self, Self::Quit)
    }

    pub const fn availability(
        self,
        context: PresentationActionContext,
    ) -> PresentationActionEnumAvailability {
        match self {
            Self::FlipBoard if !context.board_visible => {
                PresentationActionEnumAvailability::Unavailable("No board is visible")
            }
            Self::OpenSettings if context.settings_open => {
                PresentationActionEnumAvailability::Unavailable("Settings are already open")
            }
            Self::OpenPalette
            | Self::NewGame
            | Self::OpenSettings
            | Self::FlipBoard
            | Self::Quit => PresentationActionEnumAvailability::Available,
        }
    }
}

impl PresentationActionRegistry {
    const WORKSPACE: [PresentationActionEnumWorkspace; 5] = [
        PresentationActionEnumWorkspace::OpenPalette,
        PresentationActionEnumWorkspace::NewGame,
        PresentationActionEnumWorkspace::OpenSettings,
        PresentationActionEnumWorkspace::FlipBoard,
        PresentationActionEnumWorkspace::Quit,
    ];

    pub fn workspace_actions() -> &'static [PresentationActionEnumWorkspace] {
        &Self::WORKSPACE
    }

    pub fn workspace_for(
        action: DesktopPreferencesEnumShortcutAction,
    ) -> Option<PresentationActionEnumWorkspace> {
        Self::WORKSPACE
            .into_iter()
            .find(|candidate| candidate.shortcut_action() == Some(action))
    }
}
