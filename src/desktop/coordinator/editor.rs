use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationWakeProvider, DesktopApplicationWork,
};
use crate::desktop::coordinator::{
    DesktopCoordinator, DesktopCoordinatorDialogProvider, DesktopCoordinatorError,
    DesktopCoordinatorLifecycleProvider,
};
use crate::desktop::editor_commands::{
    DesktopEditorCommands, DesktopEditorCommandsEnumOperation, DesktopEditorCommandsEnumResult,
    DesktopEditorCommandsError,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};

type DesktopCoordinatorEditorResponse =
    Result<DesktopEditorCommandsEnumResult, Arc<DesktopEditorCommandsError>>;

type DesktopCoordinatorEditorFuture = Pin<
    Box<dyn Future<Output = (DesktopCoordinatorEditor, DesktopCoordinatorEditorResponse)> + Send>,
>;

pub(super) struct DesktopCoordinatorEditor {
    owner: DesktopMessagesEntityId,
    commands: DesktopEditorCommands,
}

pub(super) struct DesktopCoordinatorEditorPending {
    owner: DesktopMessagesEntityId,
    completion: DesktopCoordinatorEditorEnumCompletion,
    future: Option<DesktopCoordinatorEditorFuture>,
    response: Option<DesktopCoordinatorEditorResponse>,
}

enum DesktopCoordinatorEditorEnumCompletion {
    Application(DesktopApplicationWork),
    QuitRecovery,
}

impl DesktopCoordinatorEditorPending {
    pub(super) async fn ready(
        pending: &mut Option<Self>,
    ) -> (DesktopCoordinatorEditor, DesktopCoordinatorEditorResponse) {
        match pending.as_mut().and_then(|pending| pending.future.as_mut()) {
            Some(future) => future.as_mut().await,
            None => std::future::pending().await,
        }
    }
}

impl<C, W, P, L> DesktopCoordinator<C, W, P, L>
where
    C: ClockTimeProvider + 'static,
    W: DesktopApplicationWakeProvider + 'static,
    P: DesktopCoordinatorDialogProvider + 'static,
    L: DesktopCoordinatorLifecycleProvider,
{
    pub fn with_editor(
        mut self,
        owner: DesktopMessagesEntityId,
        commands: DesktopEditorCommands,
    ) -> Self {
        self.editor = Some(DesktopCoordinatorEditor { owner, commands });
        self
    }

    pub(super) async fn dispatch_editor(
        &mut self,
        work: DesktopApplicationWork,
        owner: DesktopMessagesEntityId,
        operation: DesktopEditorCommandsEnumOperation,
    ) -> Result<(), DesktopCoordinatorError> {
        let source_owner = self
            .editor
            .as_ref()
            .map(|editor| editor.owner)
            .or_else(|| self.pending_editor.as_ref().map(|pending| pending.owner));
        let failure = if work.request().command().expected_revision()
            != Some(DesktopCommand::editor_expected_revision(&operation))
        {
            Some(DesktopFailure::new(
                DesktopEnumFailureCode::StaleRevision,
                "The editor command has a mismatched expected revision".to_owned(),
                true,
            ))
        } else if source_owner.is_some_and(|source_owner| source_owner != owner) {
            Some(DesktopFailure::new(
                DesktopEnumFailureCode::InvalidInput,
                "The editor command belongs to another workspace".to_owned(),
                false,
            ))
        } else if self.pending_editor.is_some() {
            Some(DesktopFailure::new(
                DesktopEnumFailureCode::ResourceRefusal,
                "The original editor operation is still being observed".to_owned(),
                true,
            ))
        } else if self.editor.is_none() {
            Some(DesktopFailure::new(
                DesktopEnumFailureCode::ResourceRefusal,
                "The editor command owner is unavailable".to_owned(),
                true,
            ))
        } else {
            None
        };
        if let Some(failure) = failure {
            return self.fail(&work, failure).await;
        }
        match Self::retry(|| self.application.begin_commit(&work)).await {
            Ok(()) => {}
            Err(DesktopApplicationError::CancellationRequested) => {
                Self::retry(|| self.application.acknowledge_stopped(&work)).await?;
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
        let mut editor = self
            .editor
            .take()
            .ok_or(DesktopApplicationError::InvalidTransition)?;
        let receipt = work.receipt().clone();
        let future = Box::pin(async move {
            let response = editor.commands.handle(operation, &receipt).await;
            let response = if matches!(
                &response,
                Ok(DesktopEditorCommandsEnumResult::WritePending(_))
            ) {
                editor.commands.finish_write().await
            } else {
                response
            };
            (editor, response.map_err(Arc::new))
        });
        self.pending_editor = Some(DesktopCoordinatorEditorPending {
            owner,
            completion: DesktopCoordinatorEditorEnumCompletion::Application(work),
            future: Some(future),
            response: None,
        });
        Ok(())
    }

    pub(super) fn finish_editor(
        &mut self,
        editor: DesktopCoordinatorEditor,
        response: DesktopCoordinatorEditorResponse,
    ) -> Result<(), DesktopCoordinatorError> {
        let pending = self
            .pending_editor
            .as_mut()
            .ok_or(DesktopApplicationError::UnknownWork)?;
        pending.future = None;
        pending.response = Some(response);
        self.editor = Some(editor);
        Ok(())
    }

    pub(super) async fn complete_editor(&mut self) -> Result<(), DesktopCoordinatorError> {
        let Some(pending) = self
            .pending_editor
            .as_mut()
            .filter(|pending| pending.response.is_some())
        else {
            return Ok(());
        };
        let DesktopCoordinatorEditorEnumCompletion::Application(work) = &pending.completion else {
            let response = pending
                .response
                .take()
                .ok_or(DesktopApplicationError::InvalidTransition)?;
            self.pending_editor = None;
            if let Err(error) = response {
                let control = self
                    .controls
                    .iter()
                    .position(|work| {
                        matches!(
                            work.request().command().operation(),
                            DesktopEnumCommand::Quit
                        )
                    })
                    .ok_or(DesktopApplicationError::InvalidTransition)?;
                self.record_shutdown_failure(
                    control,
                    DesktopCoordinatorError::Shutdown {
                        source: Box::new(error),
                    },
                );
            }
            return Ok(());
        };
        let result = Self::retry(|| {
            self.application
                .complete_editor(work, &mut pending.response)
        })
        .await;
        if work.receipt().response().is_some() {
            self.pending_editor = None;
        }
        result?;
        Ok(())
    }

    pub(super) fn retry_editor_cleanup(&mut self) -> Result<(), DesktopCoordinatorError> {
        if self.pending_editor.is_some() {
            return Ok(());
        }
        let Some(editor) = self
            .editor
            .as_mut()
            .filter(|editor| editor.commands.has_pending_write())
        else {
            return Ok(());
        };
        editor.commands.retry_retained_write().map_err(|source| {
            DesktopCoordinatorError::Shutdown {
                source: Box::new(source),
            }
        })?;
        let mut editor = self
            .editor
            .take()
            .ok_or(DesktopApplicationError::InvalidTransition)?;
        self.pending_editor = Some(DesktopCoordinatorEditorPending {
            owner: editor.owner,
            completion: DesktopCoordinatorEditorEnumCompletion::QuitRecovery,
            future: Some(Box::pin(async move {
                let response = editor.commands.finish_write().await.map_err(Arc::new);
                (editor, response)
            })),
            response: None,
        });
        Ok(())
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.pending_editor.is_some()
            || self
                .editor
                .as_ref()
                .is_some_and(|editor| editor.commands.has_pending_write())
    }
}
