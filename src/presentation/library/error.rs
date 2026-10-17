use thiserror::Error;

use crate::game::library_organization::GameLibraryOrganizationError;
use crate::game::session::GameMetadataError;
use crate::storage::import_session::StorageImportSessionError;
use crate::storage::library_repository::StorageLibraryError;

#[derive(Debug, Error)]
pub enum PresentationLibraryError {
    #[error("Wait for the current library action")]
    Busy,
    #[error("This input belongs to another library action")]
    UnexpectedReply,
    #[error("This application request belongs to another library action")]
    ForeignRequest,
    #[error("This receipt belongs to another application request")]
    ForeignReceipt,
    #[error("Attach the original application request before admitting this action")]
    MissingRequest,
    #[error("Attach the accepted application receipt before observing this action")]
    MissingReceipt,
    #[error("An admitted action must retain its original request and receipt")]
    AlreadyBound,
    #[error("The library reply does not match the requested information")]
    WrongResult,
    #[error("The retained failure does not identify the original library request")]
    ForeignContinuation,
    #[error("There is no next page")]
    EndOfPages,
    #[error("Load a current library page before opening this game")]
    MissingGame,
    #[error("Preview the Trash action and confirm the exact plan first")]
    MissingPlan,
    #[error("Load the browsed folder before creating a folder inside it")]
    ParentNotLoaded,
    #[error("Review and freeze the import preview before committing")]
    MissingConfirmation,
    #[error("Reload the library from the first page")]
    ResyncRequired,
    #[error("Library editor actions belong to the annotation panel")]
    EditorAction,
    #[error("Enter both date boundaries as YYYY-MM-DD")]
    DateRange,
    #[error("Enter both game length boundaries in half-moves")]
    PlyRange,
    #[error("A numeric filter is invalid: {0}")]
    Number(#[from] std::num::ParseIntError),
    #[error(transparent)]
    ImportRequest(#[from] StorageImportSessionError),
    #[error(transparent)]
    Metadata(#[from] GameMetadataError),
    #[error(transparent)]
    Organization(#[from] GameLibraryOrganizationError),
    #[error(transparent)]
    Query(#[from] StorageLibraryError),
}
