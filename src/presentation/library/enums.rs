use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceOperation,
};
use crate::presentation::library::error::PresentationLibraryError;
use crate::storage::library_metadata::StorageLibraryMetadataEnumRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentationLibraryEnumLane {
    Query,
    Folders,
    Tags,
    Folder,
    Open,
    Organize,
    Trash,
    Import,
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
            | DesktopLibraryServiceEnumAction::ConfirmGameTrash(_) => Self::Trash,
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

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Query => "Games",
            Self::Folders => "Folders",
            Self::Tags => "Tags",
            Self::Folder => "Folder details",
            Self::Open => "Open saved game",
            Self::Organize => "Organize library",
            Self::Trash => "Trash",
            Self::Import => "Import games",
            Self::Export => "Export game",
        }
    }
}
