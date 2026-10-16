use crate::desktop::file_handle::DesktopFileEnumPurpose;
use crate::desktop::identity::DesktopRevision;
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationActionEnumId {
    Refresh,
    SelectFile,
    ConfirmQuit,
}

impl PresentationActionEnumId {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Refresh => "Refresh state",
            Self::SelectFile => "Select local file",
            Self::ConfirmQuit => "Confirm quit",
        }
    }

    pub fn command(self, file_owner: DesktopMessagesEntityId) -> DesktopCommand {
        let (operation, revision) = match self {
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
    const ACTIONS: [PresentationActionEnumId; 3] = [
        PresentationActionEnumId::Refresh,
        PresentationActionEnumId::SelectFile,
        PresentationActionEnumId::ConfirmQuit,
    ];

    pub fn actions() -> &'static [PresentationActionEnumId] {
        &Self::ACTIONS
    }
}
