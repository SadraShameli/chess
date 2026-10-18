use std::fmt::{Debug, Formatter};
use std::sync::{Arc, Mutex};

use crate::desktop::file_handle::{DesktopFileEnumSelection, DesktopFileHandleId};
use crate::desktop::library_service::DesktopLibraryServiceError;

#[derive(Clone)]
pub struct DesktopLibraryServiceFile {
    handle: DesktopFileHandleId,
    slot: Arc<Mutex<Option<DesktopFileEnumSelection>>>,
}

impl DesktopLibraryServiceFile {
    pub fn new(handle: DesktopFileHandleId) -> Self {
        Self {
            handle,
            slot: Arc::new(Mutex::new(None)),
        }
    }

    pub const fn handle(&self) -> DesktopFileHandleId {
        self.handle
    }

    pub fn fill(
        &self,
        selection: DesktopFileEnumSelection,
    ) -> Result<(), DesktopLibraryServiceError> {
        let mut slot = self
            .slot
            .lock()
            .map_err(|_error| DesktopLibraryServiceError::Poisoned)?;
        if slot.is_some() {
            return Err(DesktopLibraryServiceError::FileAlreadySelected);
        }
        *slot = Some(selection);
        Ok(())
    }

    pub(super) fn take(&self) -> Result<DesktopFileEnumSelection, DesktopLibraryServiceError> {
        self.slot
            .lock()
            .map_err(|_error| DesktopLibraryServiceError::Poisoned)?
            .take()
            .ok_or(DesktopLibraryServiceError::FileNotSelected)
    }
}

impl PartialEq for DesktopLibraryServiceFile {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl Eq for DesktopLibraryServiceFile {}

impl Debug for DesktopLibraryServiceFile {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("DesktopLibraryServiceFile")
            .field(&self.handle)
            .finish()
    }
}
