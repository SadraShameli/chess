use std::sync::Arc;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationError, DesktopApplicationWakeProvider,
    DesktopApplicationWork,
};
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceError,
};
use crate::desktop::messages::{DesktopEnumCommand, DesktopMessagesEnumResult};

impl<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> DesktopApplication<C, W> {
    pub(crate) fn complete_library(
        &self,
        work: &DesktopApplicationWork,
        response: &mut Option<
            Result<DesktopLibraryServiceEnumResult, Arc<DesktopLibraryServiceError>>,
        >,
    ) -> Result<(), DesktopApplicationError> {
        if !matches!(
            work.request().command().operation(),
            DesktopEnumCommand::Library(_)
        ) {
            return Err(DesktopApplicationError::InvalidTransition);
        }
        let is_value = response
            .as_ref()
            .ok_or(DesktopApplicationError::InvalidTransition)?
            .is_ok();
        self.complete_effect_with(work, is_value, || {
            Ok(
                match response
                    .take()
                    .ok_or(DesktopApplicationError::InvalidTransition)?
                {
                    Ok(value) => {
                        let revision = value.revision();
                        DesktopMessagesEnumResult::Value {
                            value: DesktopEnumValue::Library(Box::new(value)),
                            revision,
                        }
                    }
                    Err(error) => DesktopMessagesEnumResult::LibraryFailure(error),
                },
            )
        })
    }
}
