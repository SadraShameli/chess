use std::sync::Arc;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeProvider,
};
use crate::desktop::editor_commands::DesktopEditorCommandsEnumOperation;
use crate::desktop::messages::{DesktopCommand, DesktopMessagesEntityId, DesktopRequest};
use crate::presentation::editor::controller::PresentationEditorCommandPort;

pub struct PresentationEditorApplicationPort<
    'a,
    C: ClockTimeProvider,
    W: DesktopApplicationWakeProvider,
> {
    owner: DesktopMessagesEntityId,
    application: &'a DesktopApplicationPort<C, W>,
}

impl<'a, C: ClockTimeProvider, W: DesktopApplicationWakeProvider>
    PresentationEditorApplicationPort<'a, C, W>
{
    pub const fn new(
        owner: DesktopMessagesEntityId,
        application: &'a DesktopApplicationPort<C, W>,
    ) -> Self {
        Self { owner, application }
    }
}

impl<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> PresentationEditorCommandPort
    for PresentationEditorApplicationPort<'_, C, W>
{
    fn prepare(
        &self,
        operation: DesktopEditorCommandsEnumOperation,
    ) -> Result<DesktopRequest, DesktopApplicationError> {
        operation
            .validate_payload()
            .map_err(|error| DesktopApplicationError::EditorPayload(Arc::new(error)))?;
        self.application
            .request(DesktopCommand::editor(self.owner, operation))
    }

    fn try_submit(
        &self,
        request: &DesktopRequest,
    ) -> Result<DesktopApplicationReceipt, DesktopApplicationError> {
        self.application.try_submit(request)
    }
}
