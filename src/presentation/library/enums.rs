use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceOperation,
};
use crate::presentation::library::error::PresentationLibraryError;
use crate::storage::library_metadata::StorageLibraryMetadataEnumRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::Display)]
pub(crate) enum PresentationLibraryEnumLane {
    #[strum(serialize = "Games")]
    Query,
    #[strum(serialize = "Folders")]
    Folders,
    #[strum(serialize = "Tags")]
    Tags,
    #[strum(serialize = "Folder details")]
    Folder,
    #[strum(serialize = "Open saved game")]
    Open,
    #[strum(serialize = "Organize library")]
    Organize,
    #[strum(serialize = "Trash")]
    Trash,
    #[strum(serialize = "Import games")]
    Import,
    #[strum(serialize = "Export game")]
    Export,
}

impl PresentationLibraryEnumLane {
    pub(crate) const COUNT: usize = 9;

    pub(crate) fn operation(
        operation: &DesktopLibraryServiceOperation,
    ) -> Result<Self, PresentationLibraryError> {
        Ok(match operation.action() {
            DesktopLibraryServiceEnumAction::Query { .. } => Self::Query,
            DesktopLibraryServiceEnumAction::Metadata(request) => match request {
                StorageLibraryMetadataEnumRequest::Folders { .. } => Self::Folders,
                StorageLibraryMetadataEnumRequest::Tags { .. } => Self::Tags,
                StorageLibraryMetadataEnumRequest::Folder { .. } => Self::Folder,
            },
            DesktopLibraryServiceEnumAction::Open { .. } => Self::Open,
            DesktopLibraryServiceEnumAction::Organize(_) => Self::Organize,
            DesktopLibraryServiceEnumAction::PreviewTrash(_)
            | DesktopLibraryServiceEnumAction::ApplyTrash(_)
            | DesktopLibraryServiceEnumAction::Restore(_)
            | DesktopLibraryServiceEnumAction::RemoveFolder(_)
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash { .. } => Self::Trash,
            DesktopLibraryServiceEnumAction::Import(_) => Self::Import,
            DesktopLibraryServiceEnumAction::Export(_) => Self::Export,
            DesktopLibraryServiceEnumAction::Editor { .. } => {
                return Err(PresentationLibraryError::EditorAction);
            }
        })
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Query => 0,
            Self::Folders => 1,
            Self::Tags => 2,
            Self::Folder => 3,
            Self::Open => 4,
            Self::Organize => 5,
            Self::Trash => 6,
            Self::Import => 7,
            Self::Export => 8,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        self.into()
    }
}
