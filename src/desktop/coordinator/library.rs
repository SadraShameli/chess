use std::sync::Arc;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationWakeProvider, DesktopApplicationWork,
};
use crate::desktop::coordinator::{
    DesktopCoordinator, DesktopCoordinatorDialogProvider, DesktopCoordinatorError,
    DesktopCoordinatorLifecycleProvider, DesktopCoordinatorPending,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::file_handle::DesktopFileHandleError;
use crate::desktop::library_service::{
    DesktopLibraryServiceOperation, DesktopLibraryServiceProvider, DesktopLibraryServiceRequest,
};
use crate::desktop::messages::DesktopMessagesEntityId;

pub(super) struct DesktopCoordinatorLibrary {
    workspace: DesktopMessagesEntityId,
    provider: Arc<dyn DesktopLibraryServiceProvider>,
}

impl<C, W, P, L> DesktopCoordinator<C, W, P, L>
where
    C: ClockTimeProvider + 'static,
    W: DesktopApplicationWakeProvider + 'static,
    P: DesktopCoordinatorDialogProvider + 'static,
    L: DesktopCoordinatorLifecycleProvider,
{
    pub fn with_library(
        mut self,
        workspace: DesktopMessagesEntityId,
        provider: Arc<dyn DesktopLibraryServiceProvider>,
    ) -> Self {
        self.library = Some(DesktopCoordinatorLibrary {
            workspace,
            provider,
        });
        self
    }

    pub(super) async fn dispatch_library(
        &mut self,
        work: DesktopApplicationWork,
        operation: DesktopLibraryServiceOperation,
    ) -> Result<(), DesktopCoordinatorError> {
        let failure =
            if work.request().command().expected_revision() != operation.expected_revision() {
                Some(DesktopFailure::new(
                    DesktopEnumFailureCode::StaleRevision,
                    "The library command has a mismatched expected revision".into(),
                    true,
                ))
            } else if self
                .library
                .as_ref()
                .is_some_and(|library| library.workspace != operation.workspace())
            {
                Some(DesktopFailure::new(
                    DesktopEnumFailureCode::InvalidInput,
                    "The library command belongs to another workspace".into(),
                    false,
                ))
            } else if self.library.is_none() {
                Some(DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    "The library service is unavailable".into(),
                    true,
                ))
            } else {
                None
            };
        if let Some(failure) = failure {
            return self.fail(&work, failure).await;
        }
        if let Some((file, purpose)) = operation.file_selection() {
            let now = self.clock.now()?;
            let owner = operation.workspace().file_handle_owner();
            let handle = file.handle();
            let filled = self
                .files
                .as_mut()
                .ok_or(DesktopFileHandleError::Saturated)
                .and_then(|files| files.registry.consume(handle, owner, purpose, now))
                .map_err(|error| error.to_string())
                .and_then(|selection| {
                    file.fill(selection).map_err(|error| error.to_string())
                });
            if let Err(message) = filled {
                return self
                    .fail(
                        &work,
                        DesktopFailure::new(DesktopEnumFailureCode::FileUnavailable, message, true),
                    )
                    .await;
            }
        }
        match Self::retry(|| self.application.begin_commit(&work)).await {
            Ok(()) => {}
            Err(DesktopApplicationError::CancellationRequested) => {
                Self::retry(|| self.application.acknowledge_stopped(&work)).await?;
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
        let library = self
            .library
            .as_ref()
            .ok_or(DesktopApplicationError::InvalidTransition)?;
        let future = library.provider.execute(DesktopLibraryServiceRequest::new(
            work.receipt().clone(),
            operation,
        ));
        self.pending_library.push(DesktopCoordinatorPending {
            work,
            future: Some(future),
            response: None,
        });
        Ok(())
    }

    pub(super) async fn complete_library(&mut self) -> Result<(), DesktopCoordinatorError> {
        let mut index = 0;
        while index < self.pending_library.len() {
            let pending = &mut self.pending_library[index];
            if pending.response.is_none() {
                index += 1;
                continue;
            }
            let result = Self::retry(|| {
                self.application
                    .complete_library(&pending.work, &mut pending.response)
            })
            .await;
            if pending.work.receipt().response().is_some() {
                self.pending_library.swap_remove(index);
            }
            result?;
        }
        Ok(())
    }
}
