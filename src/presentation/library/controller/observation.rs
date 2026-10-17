use std::sync::Arc;

use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceError, DesktopLibraryServiceOperation,
};
use crate::presentation::library::controller::PresentationLibraryController;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::error::PresentationLibraryError;
use crate::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataEnumResponse,
};
use crate::storage::library_query::StorageLibraryQuery;
use crate::storage::library_repository::StorageLibraryError;

impl PresentationLibraryController {
    pub fn observe_result(
        &mut self,
        receipt: &DesktopApplicationReceipt,
        expected: &DesktopLibraryServiceOperation,
        result: &DesktopLibraryServiceEnumResult,
    ) -> Result<(), PresentationLibraryError> {
        let index = self.bound(receipt, expected)?;
        let lane = self.pending[index].lane;
        match (expected.action(), result) {
            (
                DesktopLibraryServiceEnumAction::Query { .. },
                DesktopLibraryServiceEnumResult::Query(page),
            ) => {
                if page.entries().len() > StorageLibraryQuery::PAGE_SIZE {
                    return Err(PresentationLibraryError::WrongResult);
                }
                self.page = Some(Arc::clone(page));
                self.needs_resync = false;
            }
            (
                DesktopLibraryServiceEnumAction::Metadata(request),
                DesktopLibraryServiceEnumResult::Metadata(response),
            ) => {
                self.observe_metadata(*request, response)?;
            }
            (
                DesktopLibraryServiceEnumAction::Open { .. },
                DesktopLibraryServiceEnumResult::Opened(publication),
            ) => {
                self.pending[index].check_opened(publication)?;
            }
            (DesktopLibraryServiceEnumAction::Editor { action: crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction::RetrySave, .. }, DesktopLibraryServiceEnumResult::Opened(publication)) if lane == PresentationLibraryEnumLane::Open => self.pending[index].check_opened(publication)?,
            (
                DesktopLibraryServiceEnumAction::Query { .. }
                | DesktopLibraryServiceEnumAction::Metadata(_)
                | DesktopLibraryServiceEnumAction::Open { .. }
                | DesktopLibraryServiceEnumAction::Editor { .. },
                DesktopLibraryServiceEnumResult::Query(_)
                | DesktopLibraryServiceEnumResult::Metadata(_)
                | DesktopLibraryServiceEnumResult::Opened(_)
                | DesktopLibraryServiceEnumResult::Editor(_)
                | DesktopLibraryServiceEnumResult::PendingSnapshot { .. }
                | DesktopLibraryServiceEnumResult::Saved { .. },
            ) => {
                return Err(PresentationLibraryError::WrongResult);
            }
        }
        self.failures[lane.index()] = None;
        self.finish(index);
        Ok(())
    }

    pub fn observe_error(
        &mut self,
        receipt: &DesktopApplicationReceipt,
        expected: &DesktopLibraryServiceOperation,
        error: Arc<DesktopLibraryServiceError>,
    ) -> Result<(), PresentationLibraryError> {
        let index = self.bound(receipt, expected)?;
        let lane = self.pending[index].lane;
        match error.disposition() {
            DesktopLibraryServiceEnumDisposition::Refused => {
                if self.pending[index].certified_open().is_none() {
                    self.finish(index);
                }
            }
            DesktopLibraryServiceEnumDisposition::Retained { request, expected } => {
                self.pending[index].observe_retained(request, expected)?;
            }
        }
        if lane == PresentationLibraryEnumLane::Query && Self::is_cursor_error(&error) {
            self.page = None;
            self.needs_resync = true;
        }
        self.failures[lane.index()] = Some(error);
        Ok(())
    }

    fn observe_metadata(
        &mut self,
        request: StorageLibraryMetadataEnumRequest,
        response: &StorageLibraryMetadataEnumResponse,
    ) -> Result<(), PresentationLibraryError> {
        match (request, response) {
            (
                StorageLibraryMetadataEnumRequest::Folders { parent, .. },
                StorageLibraryMetadataEnumResponse::Folders(page),
            ) => {
                if page.entries().len() > StorageLibraryMetadataEnumRequest::PAGE_SIZE
                    || page.entries().iter().any(|entry| entry.parent() != parent)
                {
                    return Err(PresentationLibraryError::WrongResult);
                }
                self.parent = parent;
                self.folders = Some(page.clone());
            }
            (
                StorageLibraryMetadataEnumRequest::Tags { .. },
                StorageLibraryMetadataEnumResponse::Tags(page),
            ) => {
                if page.entries().len() > StorageLibraryMetadataEnumRequest::PAGE_SIZE {
                    return Err(PresentationLibraryError::WrongResult);
                }
                self.tags = Some(page.clone());
            }
            (
                StorageLibraryMetadataEnumRequest::Folder { id, catalog },
                StorageLibraryMetadataEnumResponse::Folder(read),
            ) => {
                if read.folder().id() != id || read.catalog_revision() != catalog {
                    return Err(PresentationLibraryError::WrongResult);
                }
                self.folder = Some(read.clone());
            }
            (
                StorageLibraryMetadataEnumRequest::Folders { .. }
                | StorageLibraryMetadataEnumRequest::Tags { .. }
                | StorageLibraryMetadataEnumRequest::Folder { .. },
                StorageLibraryMetadataEnumResponse::Folders(_)
                | StorageLibraryMetadataEnumResponse::Tags(_)
                | StorageLibraryMetadataEnumResponse::Folder(_),
            ) => {
                return Err(PresentationLibraryError::WrongResult);
            }
        }
        Ok(())
    }

    fn is_cursor_error(error: &DesktopLibraryServiceError) -> bool {
        match error {
            DesktopLibraryServiceError::Library(error) => matches!(
                error.as_ref(),
                StorageLibraryError::StaleCursor | StorageLibraryError::CursorQuery
            ),
            DesktopLibraryServiceError::Continuation { source, .. } => {
                Self::is_cursor_error(source)
            }
            DesktopLibraryServiceError::Retained(source) => Self::is_cursor_error(source),
            DesktopLibraryServiceError::Busy
            | DesktopLibraryServiceError::Closing
            | DesktopLibraryServiceError::Closed
            | DesktopLibraryServiceError::ForeignWorkspace
            | DesktopLibraryServiceError::ForeignReceipt
            | DesktopLibraryServiceError::Cancelled
            | DesktopLibraryServiceError::NotOpen
            | DesktopLibraryServiceError::DirtySource
            | DesktopLibraryServiceError::Protocol
            | DesktopLibraryServiceError::Poisoned
            | DesktopLibraryServiceError::Deadline(_)
            | DesktopLibraryServiceError::Registry(_)
            | DesktopLibraryServiceError::Annotation(_)
            | DesktopLibraryServiceError::Context(_)
            | DesktopLibraryServiceError::Executor(_) => false,
        }
    }
}
