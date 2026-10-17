use std::sync::Arc;

use tokio::sync::watch;

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumAdmission, DesktopAnnotationCommandsEnumPending,
    DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::annotation_owner::{
    DesktopAnnotationOwner, DesktopAnnotationOwnerEnumSaveProgress, DesktopAnnotationOwnerError,
    DesktopAnnotationOwnerResources,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_editor::{
    DesktopGameEditorContext, DesktopGameEditorEnumSource, DesktopGameEditorPublication,
};
use crate::desktop::library_service::port::DesktopLibraryServiceReply;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceEnumSaveReceipt,
    DesktopLibraryServiceError,
};
use crate::game::annotations::GameAnnotationsEnumChange;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_record::StorageLibraryRecord;
use crate::storage::library_repository::{StorageLibraryRecovered, StorageLibrarySourceMemory};

#[derive(Clone, Default)]
pub(super) struct DesktopLibraryServiceNotationProjection {
    pub(super) snapshot: Option<DesktopGameEditorPublication>,
    pub(super) pending: Option<DesktopAnnotationCommandsEnumPending>,
}

enum DesktopLibraryServiceNotationEnumSource {
    Unopened {
        record: StorageLibraryRecord,
        revision: GameStorageRevision,
    },
    Open(Box<DesktopAnnotationOwner>),
}

enum DesktopLibraryServiceNotationEnumPhase {
    Computing { opening: bool, save_changed: bool },
    Checkpoint { expected: DesktopGameEditorContext },
    Saving,
}

struct DesktopLibraryServiceNotationContinuation {
    application: DesktopApplicationReceipt,
    completion: Arc<DesktopLibraryServiceReply>,
    phase: DesktopLibraryServiceNotationEnumPhase,
    retry: bool,
}

pub(super) struct DesktopLibraryServiceNotation {
    source: Option<DesktopLibraryServiceNotationEnumSource>,
    resources: DesktopAnnotationOwnerResources,
    continuation: Option<DesktopLibraryServiceNotationContinuation>,
    retained_replies: Vec<Arc<DesktopLibraryServiceReply>>,
    progress: watch::Sender<DesktopLibraryServiceNotationProjection>,
    _memory: StorageLibrarySourceMemory,
}

impl DesktopLibraryServiceNotation {
    pub(super) fn new(
        recovered: StorageLibraryRecovered,
        resources: DesktopAnnotationOwnerResources,
        progress: watch::Sender<DesktopLibraryServiceNotationProjection>,
    ) -> Self {
        let revision = recovered.content_revision();
        let (record, memory) = recovered.into_retained_record();
        Self {
            source: Some(DesktopLibraryServiceNotationEnumSource::Unopened { record, revision }),
            resources,
            continuation: None,
            retained_replies: Vec::new(),
            progress,
            _memory: memory,
        }
    }

    pub(super) fn has_pending(&self) -> bool {
        self.continuation.is_some()
    }

    pub(super) fn continuation_reply(&self) -> Option<Arc<DesktopLibraryServiceReply>> {
        self.continuation
            .as_ref()
            .map(|continuation| Arc::clone(&continuation.completion))
    }

    pub(super) fn is_dirty(&self) -> bool {
        match &self.source {
            Some(DesktopLibraryServiceNotationEnumSource::Open(owner)) => owner.status().is_dirty(),
            Some(DesktopLibraryServiceNotationEnumSource::Unopened { .. }) | None => false,
        }
    }

    pub(super) async fn open(
        &mut self,
        completion: Arc<DesktopLibraryServiceReply>,
    ) -> Result<DesktopLibraryServiceEnumResult, DesktopLibraryServiceError> {
        let application = completion.application.clone();
        if let Some(continuation) = &self.continuation {
            if continuation.application.shares_outcome(&application) {
                return self.resume(&application).await;
            }
            let read = self.owner()?.read_snapshot()?;
            return Ok(DesktopLibraryServiceEnumResult::PendingSnapshot {
                snapshot: read.snapshot().clone(),
                pending: read
                    .pending()
                    .unwrap_or(DesktopAnnotationCommandsEnumPending::Unresolved),
            });
        }
        let source = self
            .source
            .take()
            .ok_or(DesktopLibraryServiceError::Protocol)?;
        match source {
            DesktopLibraryServiceNotationEnumSource::Unopened { record, revision } => {
                match DesktopAnnotationOwner::open(
                    record,
                    revision,
                    application.clone(),
                    DesktopAnnotationOwnerResources {
                        executor: Arc::clone(&self.resources.executor),
                        client: self.resources.client.clone(),
                        wall_time: Arc::clone(&self.resources.wall_time),
                    },
                ) {
                    Ok(owner) => {
                        self.source = Some(DesktopLibraryServiceNotationEnumSource::Open(Box::new(
                            owner,
                        )))
                    }
                    Err(error) => {
                        let (record, error) = error.into_parts();
                        self.source = Some(DesktopLibraryServiceNotationEnumSource::Unopened {
                            record,
                            revision,
                        });
                        return Err(error.into());
                    }
                }
                self.continuation = Some(DesktopLibraryServiceNotationContinuation {
                    application: application.clone(),
                    completion,
                    retry: false,
                    phase: DesktopLibraryServiceNotationEnumPhase::Computing {
                        opening: true,
                        save_changed: false,
                    },
                });
                self.resume(&application).await
            }
            DesktopLibraryServiceNotationEnumSource::Open(owner) => {
                let snapshot = owner.snapshot().cloned();
                self.source = Some(DesktopLibraryServiceNotationEnumSource::Open(owner));
                Ok(DesktopLibraryServiceEnumResult::Opened(snapshot?))
            }
        }
    }

    pub(super) async fn execute(
        &mut self,
        completion: Arc<DesktopLibraryServiceReply>,
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
    ) -> Result<DesktopLibraryServiceEnumResult, DesktopLibraryServiceError> {
        let application = completion.application.clone();
        if matches!(action, DesktopAnnotationCommandsEnumAction::Snapshot) {
            let read = self.owner()?.read_snapshot()?;
            read.snapshot().context().check_expected(expected)?;
            return Ok(match read.pending() {
                Some(pending) => DesktopLibraryServiceEnumResult::PendingSnapshot {
                    snapshot: read.snapshot().clone(),
                    pending,
                },
                None => DesktopLibraryServiceEnumResult::Editor(
                    DesktopAnnotationCommandsEnumResult::Snapshot(read.snapshot().clone()),
                ),
            });
        }
        if let Some(continuation) = &self.continuation {
            if continuation.application.shares_outcome(&application) {
                return self.resume(&application).await;
            }
            if matches!(
                action,
                DesktopAnnotationCommandsEnumAction::RetrySave
                    | DesktopAnnotationCommandsEnumAction::Checkpoint
            ) {
                self.owner()?
                    .read_snapshot()?
                    .snapshot()
                    .context()
                    .check_expected(expected)?;
                let retained = continuation.application.clone();
                if matches!(action, DesktopAnnotationCommandsEnumAction::RetrySave) {
                    self.continuation
                        .as_mut()
                        .ok_or(DesktopLibraryServiceError::Protocol)?
                        .retry = true;
                }
                return self.resume(&retained).await;
            }
            return Err(DesktopLibraryServiceError::Busy);
        }
        if matches!(action, DesktopAnnotationCommandsEnumAction::RetrySave)
            && let Some(retained) = self
                .retained_replies
                .iter()
                .find(|reply| reply.editor_context() == Some(expected))
                .cloned()
        {
            let application = retained.application.clone();
            let expected = self.owner()?.read_snapshot()?.snapshot().context();
            self.continuation = Some(DesktopLibraryServiceNotationContinuation {
                application: application.clone(),
                completion: retained,
                phase: DesktopLibraryServiceNotationEnumPhase::Checkpoint { expected },
                retry: true,
            });
            return self.resume(&application).await;
        }
        let save_changed = action.edits_content();
        let admission = self.owner_mut()?.submit(DesktopAnnotationCommands::new(
            application.clone(),
            expected,
            action,
        ))?;
        match admission {
            DesktopAnnotationCommandsEnumAdmission::Ready(result) => {
                Ok(DesktopLibraryServiceEnumResult::Editor(*result))
            }
            DesktopAnnotationCommandsEnumAdmission::PendingSnapshot { snapshot, pending } => {
                Ok(DesktopLibraryServiceEnumResult::PendingSnapshot { snapshot, pending })
            }
            DesktopAnnotationCommandsEnumAdmission::ComputePending => {
                self.continuation = Some(DesktopLibraryServiceNotationContinuation {
                    application: application.clone(),
                    completion,
                    retry: false,
                    phase: DesktopLibraryServiceNotationEnumPhase::Computing {
                        opening: false,
                        save_changed,
                    },
                });
                self.resume(&application).await
            }
            DesktopAnnotationCommandsEnumAdmission::SavePending => {
                self.continuation = Some(DesktopLibraryServiceNotationContinuation {
                    application: application.clone(),
                    completion,
                    retry: false,
                    phase: DesktopLibraryServiceNotationEnumPhase::Saving,
                });
                self.resume(&application).await
            }
        }
    }

    async fn resume(
        &mut self,
        application: &DesktopApplicationReceipt,
    ) -> Result<DesktopLibraryServiceEnumResult, DesktopLibraryServiceError> {
        if !self
            .continuation
            .as_ref()
            .is_some_and(|continuation| continuation.application.shares_outcome(application))
        {
            return Err(DesktopLibraryServiceError::ForeignReceipt);
        }
        let completion = self
            .continuation_reply()
            .ok_or(DesktopLibraryServiceError::Protocol)?;
        let result = self.resume_continuation(application).await;
        result.map_err(|error| {
            if self.has_pending()
                || self
                    .retained_replies
                    .iter()
                    .any(|reply| reply.application.shares_outcome(application))
            {
                error.continuation(&completion)
            } else {
                error
            }
        })
    }

    async fn resume_continuation(
        &mut self,
        application: &DesktopApplicationReceipt,
    ) -> Result<DesktopLibraryServiceEnumResult, DesktopLibraryServiceError> {
        loop {
            self.publish_progress();
            let phase = &self
                .continuation
                .as_ref()
                .ok_or(DesktopLibraryServiceError::Protocol)?
                .phase;
            match phase {
                DesktopLibraryServiceNotationEnumPhase::Computing {
                    opening,
                    save_changed,
                } => {
                    let opening = *opening;
                    let save_changed = *save_changed;
                    let result = self.owner_mut()?.observe_compute(application).await;
                    self.publish_progress();
                    let result = match result {
                        Ok(result) => result,
                        Err(error) => {
                            let accepted = self.has_accepted_content()?;
                            if !self.owner()?.status().has_compute()
                                && (accepted || Self::is_preacceptance_refusal(&error))
                            {
                                self.clear_continuation(accepted)?;
                            }
                            return Err(error.into());
                        }
                    };
                    if opening {
                        let DesktopAnnotationCommandsEnumResult::Snapshot(snapshot) = result else {
                            return Err(DesktopLibraryServiceError::Protocol);
                        };
                        return self
                            .finish_success(DesktopLibraryServiceEnumResult::Opened(snapshot));
                    }
                    if let DesktopAnnotationCommandsEnumResult::Edited {
                        change: GameAnnotationsEnumChange::Changed,
                        snapshot,
                    } = &result
                        && save_changed
                    {
                        self.continuation
                            .as_mut()
                            .ok_or(DesktopLibraryServiceError::Protocol)?
                            .phase = DesktopLibraryServiceNotationEnumPhase::Checkpoint {
                            expected: snapshot.context(),
                        };
                    } else {
                        return self
                            .finish_success(DesktopLibraryServiceEnumResult::Editor(result));
                    }
                }
                DesktopLibraryServiceNotationEnumPhase::Checkpoint { expected } => {
                    let expected = *expected;
                    let retry = self
                        .continuation
                        .as_ref()
                        .ok_or(DesktopLibraryServiceError::Protocol)?
                        .retry;
                    let admission = self.owner_mut()?.submit(DesktopAnnotationCommands::new(
                        application.clone(),
                        expected,
                        if retry {
                            DesktopAnnotationCommandsEnumAction::RetrySave
                        } else {
                            DesktopAnnotationCommandsEnumAction::Checkpoint
                        },
                    ))?;
                    if !matches!(
                        admission,
                        DesktopAnnotationCommandsEnumAdmission::SavePending
                    ) {
                        return Err(DesktopLibraryServiceError::Protocol);
                    }
                    self.continuation
                        .as_mut()
                        .ok_or(DesktopLibraryServiceError::Protocol)?
                        .phase = DesktopLibraryServiceNotationEnumPhase::Saving;
                }
                DesktopLibraryServiceNotationEnumPhase::Saving => {
                    let result = self.owner_mut()?.observe_save(application).await;
                    self.publish_progress();
                    match result {
                        Ok(DesktopAnnotationOwnerEnumSaveProgress::Committing) => {}
                        Ok(DesktopAnnotationOwnerEnumSaveProgress::Saved { receipt, covered }) => {
                            let snapshot = self.owner()?.snapshot()?.clone();
                            return self.finish_success(DesktopLibraryServiceEnumResult::Saved {
                                snapshot,
                                receipt: DesktopLibraryServiceEnumSaveReceipt::Notation {
                                    receipt,
                                    covered,
                                },
                            });
                        }
                        Err(error) => {
                            if !self.owner()?.status().has_save() {
                                self.clear_continuation(self.is_dirty())?;
                            }
                            return Err(error.into());
                        }
                    }
                }
            }
        }
    }

    pub(super) async fn cleanup(&mut self) -> Result<(), DesktopLibraryServiceError> {
        if let Some(continuation) = &self.continuation {
            let application = continuation.application.clone();
            self.resume(&application).await?;
        }
        if self.is_dirty() {
            let reply = self
                .retained_replies
                .last()
                .ok_or(DesktopLibraryServiceError::Protocol)?;
            return Err(DesktopLibraryServiceError::DirtySource.continuation(reply));
        }
        Ok(())
    }

    fn finish_success(
        &mut self,
        result: DesktopLibraryServiceEnumResult,
    ) -> Result<DesktopLibraryServiceEnumResult, DesktopLibraryServiceError> {
        let continuation = self
            .continuation
            .take()
            .ok_or(DesktopLibraryServiceError::Protocol)?;
        self.publish_progress();
        if matches!(result, DesktopLibraryServiceEnumResult::Saved { .. }) && !self.is_dirty() {
            for reply in self.retained_replies.drain(..) {
                reply.complete(Ok(result.clone()), false);
            }
        }
        continuation.completion.complete(Ok(result.clone()), false);
        Ok(result)
    }

    fn has_accepted_content(&self) -> Result<bool, DesktopLibraryServiceError> {
        let expected = self
            .continuation
            .as_ref()
            .and_then(|continuation| continuation.completion.operation.editor_context());
        let Some(expected) = expected else {
            return Ok(false);
        };
        let revision = match expected.source() {
            DesktopGameEditorEnumSource::Played { revision }
            | DesktopGameEditorEnumSource::Notation { revision } => revision,
        };
        Ok(self
            .owner()?
            .status()
            .accepted_edit()
            .is_some_and(|edit| edit.revision() > revision))
    }

    fn is_preacceptance_refusal(error: &DesktopAnnotationOwnerError) -> bool {
        if let DesktopAnnotationOwnerError::Retained(source) = error {
            return Self::is_preacceptance_refusal(source);
        }
        matches!(
            error,
            DesktopAnnotationOwnerError::Cancelled
                | DesktopAnnotationOwnerError::WallTime(_)
                | DesktopAnnotationOwnerError::Editor(_)
                | DesktopAnnotationOwnerError::Notation(_)
                | DesktopAnnotationOwnerError::Revision(_)
                | DesktopAnnotationOwnerError::VariationToken
                | DesktopAnnotationOwnerError::ScopeOffset
        )
    }

    fn clear_continuation(&mut self, retain: bool) -> Result<(), DesktopLibraryServiceError> {
        if retain
            && self.retained_replies.len()
                >= crate::desktop::library_service::DesktopLibraryServicePort::CAPACITY
        {
            let continuation = self
                .continuation
                .as_ref()
                .ok_or(DesktopLibraryServiceError::Protocol)?;
            return Err(DesktopLibraryServiceError::Busy.continuation(&continuation.completion));
        }
        if let Some(continuation) = self.continuation.take() {
            continuation
                .completion
                .retryable
                .store(false, std::sync::atomic::Ordering::Release);
            if retain {
                self.retained_replies.push(continuation.completion);
            }
        }
        self.publish_progress();
        Ok(())
    }

    fn owner(&self) -> Result<&DesktopAnnotationOwner, DesktopLibraryServiceError> {
        match &self.source {
            Some(DesktopLibraryServiceNotationEnumSource::Open(owner)) => Ok(owner),
            Some(DesktopLibraryServiceNotationEnumSource::Unopened { .. }) | None => {
                Err(DesktopLibraryServiceError::Protocol)
            }
        }
    }

    fn owner_mut(&mut self) -> Result<&mut DesktopAnnotationOwner, DesktopLibraryServiceError> {
        match &mut self.source {
            Some(DesktopLibraryServiceNotationEnumSource::Open(owner)) => Ok(owner),
            Some(DesktopLibraryServiceNotationEnumSource::Unopened { .. }) | None => {
                Err(DesktopLibraryServiceError::Protocol)
            }
        }
    }

    fn publish_progress(&self) {
        if let Ok(owner) = self.owner()
            && let Ok(read) = owner.read_snapshot()
        {
            self.progress
                .send_replace(DesktopLibraryServiceNotationProjection {
                    snapshot: Some(read.snapshot().clone()),
                    pending: read.pending().or_else(|| {
                        self.continuation
                            .as_ref()
                            .map(|_continuation| DesktopAnnotationCommandsEnumPending::Unresolved)
                    }),
                });
        }
    }
}
