use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, TryLockError};

use tokio::sync::watch;

use crate::clock::duration::ClockInstant;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumPending, DesktopAnnotationCommandsEnumResult,
    DesktopAnnotationCommandsEnumScope, DesktopAnnotationCommandsScopePage,
    DesktopAnnotationCommandsVariationPreview,
};
use crate::desktop::application::{DesktopApplicationCancellation, DesktopApplicationReceipt};
use crate::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorContext, DesktopGameEditorEpoch,
    DesktopGameEditorGeneration,
};
use crate::desktop::game_supervisor::content::DesktopGameSupervisorContent;
use crate::desktop::game_supervisor::{
    DesktopGameSupervisorEnumOrdinary, DesktopGameSupervisorError, DesktopGameSupervisorPort,
};
use crate::desktop::game_transition::DesktopGameTransitionOptions;
use crate::desktop::identity::DesktopRequestId;
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::annotations::{GameAnnotationsEnumChange, GameAnnotationsSessionPreview};
use crate::game::enums::{GameEnumEngineReadiness, GameMoveId, GameRevision};
use crate::game::pause::GamePauseEnumEngineBarrier;
use crate::game::session::GameSession;
use crate::game::storage_repository::{
    GameStorageEnumEditCause, GameStorageReceipt, GameStorageRevision, GameStorageWallTimeProvider,
};
use crate::storage::document_store::StorageDocumentClient;

type DesktopGameSupervisorEditorResponse =
    Result<DesktopAnnotationCommandsEnumResult, Arc<DesktopGameSupervisorError>>;

#[derive(Clone)]
pub struct DesktopGameSupervisorEditorReceipt {
    application: DesktopApplicationReceipt,
    response: watch::Receiver<Option<DesktopGameSupervisorEditorResponse>>,
    phase: watch::Receiver<Option<DesktopAnnotationCommandsEnumPending>>,
    storage_receipt: Arc<OnceLock<GameStorageReceipt>>,
}

impl DesktopGameSupervisorEditorReceipt {
    pub fn request_id(&self) -> DesktopRequestId {
        self.application.request_id()
    }

    pub const fn application_receipt(&self) -> &DesktopApplicationReceipt {
        &self.application
    }

    pub fn storage_receipt(&self) -> Option<GameStorageReceipt> {
        self.storage_receipt.get().copied()
    }

    pub fn try_result(&self) -> Option<DesktopGameSupervisorEditorResponse> {
        self.response.borrow().clone()
    }

    pub fn pending(&self) -> Option<DesktopAnnotationCommandsEnumPending> {
        *self.phase.borrow()
    }

    pub async fn wait(&mut self) -> DesktopGameSupervisorEditorResponse {
        loop {
            if let Some(result) = self.try_result()
                && !matches!(
                    &result,
                    Ok(DesktopAnnotationCommandsEnumResult::PendingProjection { .. })
                )
            {
                return result;
            }
            self.response
                .changed()
                .await
                .map_err(|_closed| Arc::new(DesktopGameSupervisorError::Closed))?;
        }
    }
}

#[derive(Default)]
pub(super) struct DesktopGameSupervisorEditorReplies(
    Mutex<VecDeque<DesktopGameSupervisorEditorRetainedReply>>,
);

struct DesktopGameSupervisorEditorRetainedReply {
    receipt: DesktopGameSupervisorEditorReceipt,
    response: watch::Sender<Option<DesktopGameSupervisorEditorResponse>>,
    retry_save: bool,
    phase: watch::Sender<Option<DesktopAnnotationCommandsEnumPending>>,
}

pub(super) struct DesktopGameSupervisorEditorEnvelope {
    pub(super) application: DesktopApplicationReceipt,
    pub(super) operation: DesktopGameSupervisorEditorEnumOperation,
    pub(super) response: watch::Sender<Option<DesktopGameSupervisorEditorResponse>>,
    pub(super) retry_save: bool,
    pub(super) phase: watch::Sender<Option<DesktopAnnotationCommandsEnumPending>>,
    pub(super) storage_receipt: Arc<OnceLock<GameStorageReceipt>>,
}

impl DesktopGameSupervisorEditorEnvelope {
    pub(super) fn complete(&self, response: DesktopGameSupervisorEditorResponse) {
        let pending = if let Err(error) = &response
            && let DesktopGameSupervisorError::EditorPublicationPending { context, .. } =
                error.as_ref()
        {
            Some(DesktopAnnotationCommandsEnumPending::Publishing { context: *context })
        } else {
            None
        };
        self.phase.send_replace(pending);
        self.response.send_replace(Some(response));
    }
}

pub(super) enum DesktopGameSupervisorEditorEnumOperation {
    Open,
    Publish,
    Action {
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
    },
    Project(DesktopAnnotationCommandsEnumResult),
}

impl DesktopGameSupervisorEditorEnumOperation {
    pub(super) fn is_retry_save(&self) -> bool {
        matches!(
            self,
            Self::Action {
                action: DesktopAnnotationCommandsEnumAction::RetrySave,
                ..
            }
        )
    }
}

impl DesktopGameSupervisorPort {
    pub fn try_open_editor(
        &self,
        application: DesktopApplicationReceipt,
    ) -> Result<DesktopGameSupervisorEditorReceipt, DesktopGameSupervisorError> {
        self.admit_editor(application, DesktopGameSupervisorEditorEnumOperation::Open)
    }

    pub fn try_submit_editor(
        &self,
        command: DesktopAnnotationCommands,
    ) -> Result<DesktopGameSupervisorEditorReceipt, DesktopGameSupervisorError> {
        let (application, expected, action) = command.into_parts();
        self.admit_editor(
            application,
            DesktopGameSupervisorEditorEnumOperation::Action { expected, action },
        )
    }

    fn admit_editor(
        &self,
        application: DesktopApplicationReceipt,
        operation: DesktopGameSupervisorEditorEnumOperation,
    ) -> Result<DesktopGameSupervisorEditorReceipt, DesktopGameSupervisorError> {
        let mut replies = self
            .editor_replies
            .0
            .try_lock()
            .map_err(|error| match error {
                TryLockError::WouldBlock => DesktopGameSupervisorError::Busy,
                TryLockError::Poisoned(_) => DesktopGameSupervisorError::Poisoned,
            })?;
        if let Some(retained) = replies
            .iter()
            .find(|reply| reply.receipt.request_id() == application.request_id())
        {
            if !retained.receipt.application.shares_outcome(&application) {
                return Err(DesktopGameSupervisorError::EditorReceipt);
            }
            if matches!(retained.receipt.try_result(), Some(Err(error)) if matches!(error.as_ref(), DesktopGameSupervisorError::EditorPublicationPending { .. }))
            {
                let permit = self.ordinary.try_reserve().map_err(Self::admission_error)?;
                retained
                    .phase
                    .send_replace(Some(DesktopAnnotationCommandsEnumPending::Computing));
                retained.response.send_replace(None);
                permit.send(DesktopGameSupervisorEnumOrdinary::Editor(
                    DesktopGameSupervisorEditorEnvelope {
                        application,
                        operation: DesktopGameSupervisorEditorEnumOperation::Publish,
                        response: retained.response.clone(),
                        retry_save: retained.retry_save,
                        phase: retained.phase.clone(),
                        storage_receipt: Arc::clone(&retained.receipt.storage_receipt),
                    },
                ));
            }
            return Ok(retained.receipt.clone());
        }
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(DesktopGameSupervisorError::Closed);
        }
        let evict = if replies.len() == Self::ORDINARY_CAPACITY {
            Some(
                replies
                    .iter()
                    .position(|reply| {
                        reply.receipt.try_result().is_some() && reply.receipt.pending().is_none()
                    })
                    .ok_or(DesktopGameSupervisorError::Busy)?,
            )
        } else {
            None
        };
        let (response, receiver) = watch::channel(None);
        let (phase, phase_receiver) =
            watch::channel(Some(DesktopAnnotationCommandsEnumPending::Computing));
        let receipt = DesktopGameSupervisorEditorReceipt {
            application: application.clone(),
            response: receiver,
            phase: phase_receiver,
            storage_receipt: Arc::new(OnceLock::new()),
        };
        let retry_save = operation.is_retry_save();
        self.ordinary
            .try_send(DesktopGameSupervisorEnumOrdinary::Editor(
                DesktopGameSupervisorEditorEnvelope {
                    application,
                    operation,
                    response: response.clone(),
                    retry_save,
                    phase: phase.clone(),
                    storage_receipt: Arc::clone(&receipt.storage_receipt),
                },
            ))
            .map_err(Self::admission_error)?;
        if let Some(index) = evict {
            replies.remove(index);
        }
        replies.push_back(DesktopGameSupervisorEditorRetainedReply {
            receipt: receipt.clone(),
            response,
            retry_save,
            phase,
        });
        Ok(receipt)
    }
}

struct DesktopGameSupervisorEditorProof {
    scope: DesktopAnnotationCommandsVariationPreview,
    proof: GameAnnotationsSessionPreview,
}

pub(super) struct DesktopGameSupervisorEditor {
    epoch: Option<DesktopGameEditorEpoch>,
    generation: DesktopGameEditorGeneration,
    proof: Option<DesktopGameSupervisorEditorProof>,
    publication: Option<DesktopGameSupervisorEditorPendingPublication>,
}

#[derive(Clone, Copy)]
enum DesktopGameSupervisorEditorEnumPublication {
    Snapshot,
    Edited(GameAnnotationsEnumChange),
}

struct DesktopGameSupervisorEditorPendingPublication {
    request_id: DesktopRequestId,
    kind: DesktopGameSupervisorEditorEnumPublication,
    save: bool,
}

impl DesktopGameSupervisorEditorPendingPublication {
    fn new(
        application: &DesktopGameSupervisorEditorApplication,
        kind: DesktopGameSupervisorEditorEnumPublication,
        save: bool,
    ) -> Self {
        Self {
            request_id: application.request_id(),
            kind,
            save,
        }
    }
}

pub(super) struct DesktopGameSupervisorEditorApplication {
    request_id: DesktopRequestId,
    received_at: ClockInstant,
    cancellation: DesktopApplicationCancellation,
}

impl DesktopGameSupervisorEditorApplication {
    pub(super) fn capture(receipt: &DesktopApplicationReceipt) -> Self {
        Self {
            request_id: receipt.request_id(),
            received_at: receipt.received_at(),
            cancellation: receipt.cancellation(),
        }
    }

    const fn request_id(&self) -> DesktopRequestId {
        self.request_id
    }

    const fn received_at(&self) -> ClockInstant {
        self.received_at
    }

    fn is_cancellation_requested(&self) -> bool {
        self.cancellation.is_requested()
    }
}

pub(super) struct DesktopGameSupervisorEditorComputeContext<'a, P: ?Sized> {
    pub(super) application: &'a DesktopGameSupervisorEditorApplication,
    pub(super) storage_revision: GameStorageRevision,
    pub(super) storage: &'a StorageDocumentClient,
    pub(super) wall_time: &'a dyn GameStorageWallTimeProvider,
    pub(super) proof: &'a P,
}

impl DesktopGameSupervisorEditor {
    pub(super) const fn new() -> Self {
        Self {
            epoch: None,
            generation: DesktopGameEditorGeneration::ZERO,
            proof: None,
            publication: None,
        }
    }

    pub(super) fn next_generation(
        &self,
    ) -> Result<DesktopGameEditorGeneration, DesktopGameSupervisorError> {
        Ok(self.generation.next()?)
    }

    pub(super) fn observe(
        &mut self,
        before: (GameRevision, GameMoveId),
        session: &GameSession,
        next: DesktopGameEditorGeneration,
    ) {
        if before != (session.revision(), session.view()) {
            self.generation = next;
        }
    }

    fn project<'a>(
        &self,
        session: &'a GameSession,
        storage_revision: GameStorageRevision,
    ) -> Result<DesktopGameEditor<'a>, DesktopGameSupervisorError> {
        Ok(DesktopGameEditor::played(
            session,
            DesktopGameEditorBinding::new(
                storage_revision,
                self.epoch
                    .ok_or(DesktopGameSupervisorError::EditorNotOpen)?,
                self.generation,
            ),
        )?)
    }

    pub(super) fn is_publishing(&self) -> bool {
        self.publication.is_some()
    }

    fn checked_proof(
        &self,
        context: DesktopGameEditorContext,
        token: DesktopRequestId,
    ) -> Result<&DesktopGameSupervisorEditorProof, DesktopGameSupervisorError> {
        let proof = self
            .proof
            .as_ref()
            .ok_or(DesktopGameSupervisorError::EditorVariationToken)?;
        context.check_expected(proof.scope.context())?;
        if proof.scope.token() != token {
            return Err(DesktopGameSupervisorError::EditorVariationToken);
        }
        Ok(proof)
    }
}

pub(super) struct DesktopGameSupervisorEditorComputed {
    pub(super) result: DesktopAnnotationCommandsEnumResult,
    pub(super) save: bool,
}

impl DesktopGameSupervisorContent {
    pub(super) fn editor_operation<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        operation: DesktopGameSupervisorEditorEnumOperation,
        context: DesktopGameSupervisorEditorComputeContext<'_, P>,
    ) -> Result<DesktopGameSupervisorEditorComputed, DesktopGameSupervisorError> {
        let DesktopGameSupervisorEditorComputeContext {
            application,
            storage_revision,
            storage,
            wall_time,
            proof,
        } = context;
        if matches!(operation, DesktopGameSupervisorEditorEnumOperation::Publish) {
            return self.editor_publish(application.request_id(), storage_revision, storage);
        }
        if let DesktopGameSupervisorEditorEnumOperation::Project(result) = operation {
            let kind = match result {
                DesktopAnnotationCommandsEnumResult::Snapshot(snapshot) => {
                    drop(snapshot);
                    DesktopGameSupervisorEditorEnumPublication::Snapshot
                }
                DesktopAnnotationCommandsEnumResult::Edited { change, snapshot } => {
                    drop(snapshot);
                    DesktopGameSupervisorEditorEnumPublication::Edited(change)
                }
                DesktopAnnotationCommandsEnumResult::Tree(_)
                | DesktopAnnotationCommandsEnumResult::Children(_)
                | DesktopAnnotationCommandsEnumResult::Variation(_)
                | DesktopAnnotationCommandsEnumResult::Scope(_)
                | DesktopAnnotationCommandsEnumResult::PendingProjection { .. } => {
                    return Err(DesktopGameSupervisorError::StateUnavailable);
                }
            };
            return self.editor_start_publication(
                DesktopGameSupervisorEditorPendingPublication::new(application, kind, false),
                storage_revision,
                storage,
            );
        }
        if application.is_cancellation_requested() {
            return Err(DesktopGameSupervisorError::EditorCancelled);
        }
        let (expected, action) = match operation {
            DesktopGameSupervisorEditorEnumOperation::Open => {
                if self.editor.epoch.is_none() {
                    self.editor.epoch = Some(DesktopGameEditorEpoch::from_request_id(
                        application.request_id(),
                    ));
                }
                return self.editor_start_publication(
                    DesktopGameSupervisorEditorPendingPublication::new(
                        application,
                        DesktopGameSupervisorEditorEnumPublication::Snapshot,
                        false,
                    ),
                    storage_revision,
                    storage,
                );
            }
            DesktopGameSupervisorEditorEnumOperation::Action { expected, action } => {
                (expected, action)
            }
            DesktopGameSupervisorEditorEnumOperation::Project(_)
            | DesktopGameSupervisorEditorEnumOperation::Publish => {
                return Err(DesktopGameSupervisorError::StateUnavailable);
            }
        };
        let editor = self.editor.project(&self.session, storage_revision)?;
        editor.context().check_expected(expected)?;
        if self.editor.is_publishing() {
            return Err(DesktopGameSupervisorError::Busy);
        }
        if action.edits_content()
            || matches!(
                action,
                DesktopAnnotationCommandsEnumAction::Checkpoint
                    | DesktopAnnotationCommandsEnumAction::RetrySave
            )
        {
            storage_revision
                .value()
                .checked_add(1)
                .ok_or(crate::storage::library_repository::StorageLibraryError::ContentExhausted)?;
        }
        match action {
            DesktopAnnotationCommandsEnumAction::Snapshot => self.editor_start_publication(
                DesktopGameSupervisorEditorPendingPublication::new(
                    application,
                    DesktopGameSupervisorEditorEnumPublication::Snapshot,
                    false,
                ),
                storage_revision,
                storage,
            ),
            DesktopAnnotationCommandsEnumAction::TreePage { cursor } => {
                Ok(DesktopGameSupervisorEditorComputed {
                    result: DesktopAnnotationCommandsEnumResult::Tree(
                        editor.tree_page(expected, cursor)?,
                    ),
                    save: false,
                })
            }
            DesktopAnnotationCommandsEnumAction::ChildrenPage { parent, cursor } => {
                Ok(DesktopGameSupervisorEditorComputed {
                    result: DesktopAnnotationCommandsEnumResult::Children(
                        editor.children_page(expected, parent, cursor)?,
                    ),
                    save: false,
                })
            }
            DesktopAnnotationCommandsEnumAction::PreviewVariation(request) => {
                let proof = self
                    .session
                    .preview_variation(self.session.revision(), request)?;
                let scope = DesktopAnnotationCommandsVariationPreview::capture(
                    expected,
                    application.request_id(),
                    proof.variation(),
                );
                self.editor.proof = Some(DesktopGameSupervisorEditorProof {
                    scope: scope.clone(),
                    proof,
                });
                Ok(DesktopGameSupervisorEditorComputed {
                    result: DesktopAnnotationCommandsEnumResult::Variation(scope),
                    save: false,
                })
            }
            DesktopAnnotationCommandsEnumAction::VariationScope {
                token,
                scope,
                offset,
            } => {
                let retained = self.editor.checked_proof(expected, token)?;
                let nodes = match scope {
                    DesktopAnnotationCommandsEnumScope::PreviousPath => {
                        retained.proof.variation().previous_path()
                    }
                    DesktopAnnotationCommandsEnumScope::SelectedPath => retained
                        .proof
                        .variation()
                        .selected_path()
                        .ok_or(DesktopGameSupervisorError::EditorScopeOffset)?,
                    DesktopAnnotationCommandsEnumScope::DeletedNodes => {
                        retained.proof.variation().deleted_nodes()
                    }
                };
                let page = DesktopAnnotationCommandsScopePage::capture(
                    expected, token, scope, nodes, offset,
                )
                .ok_or(DesktopGameSupervisorError::EditorScopeOffset)?;
                Ok(DesktopGameSupervisorEditorComputed {
                    result: DesktopAnnotationCommandsEnumResult::Scope(page),
                    save: false,
                })
            }
            DesktopAnnotationCommandsEnumAction::Navigate { selected } => {
                let generation = self.editor.next_generation()?;
                let before = (self.session.revision(), self.session.view());
                self.session.select_view(selected)?;
                self.editor.observe(before, &self.session, generation);
                self.editor_start_publication(
                    DesktopGameSupervisorEditorPendingPublication::new(
                        application,
                        DesktopGameSupervisorEditorEnumPublication::Snapshot,
                        false,
                    ),
                    storage_revision,
                    storage,
                )
            }
            DesktopAnnotationCommandsEnumAction::Checkpoint
            | DesktopAnnotationCommandsEnumAction::RetrySave => {
                if matches!(action, DesktopAnnotationCommandsEnumAction::RetrySave) {
                    self.apply(
                        DesktopGameTransitionOptions {
                            request: DesktopGameCommandsRequest {
                                request_id: application.request_id(),
                                expected_revision: self.session.revision(),
                                received_at: application.received_at(),
                                action: DesktopGameCommandsEnumAction::RetrySave,
                            },
                            engine: GamePauseEnumEngineBarrier::NoEngine,
                            readiness: GameEnumEngineReadiness::NotRequired,
                        },
                        proof,
                        wall_time,
                    )?;
                }
                self.editor_start_publication(
                    DesktopGameSupervisorEditorPendingPublication::new(
                        application,
                        DesktopGameSupervisorEditorEnumPublication::Snapshot,
                        true,
                    ),
                    storage_revision,
                    storage,
                )
            }
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { .. }
            | DesktopAnnotationCommandsEnumAction::SaveMarkings { .. }
            | DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
            | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. } => {
                let change = self.editor_edit(expected, action, wall_time)?;
                self.editor_start_publication(
                    DesktopGameSupervisorEditorPendingPublication::new(
                        application,
                        DesktopGameSupervisorEditorEnumPublication::Edited(change),
                        true,
                    ),
                    storage_revision,
                    storage,
                )
            }
        }
    }

    fn editor_edit(
        &mut self,
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
        wall_time: &dyn GameStorageWallTimeProvider,
    ) -> Result<GameAnnotationsEnumChange, DesktopGameSupervisorError> {
        if let DesktopAnnotationCommandsEnumAction::ConfirmVariation { token } = &action {
            self.editor.checked_proof(expected, *token)?;
        }
        let before = (self.session.revision(), self.session.view());
        let _next_revision = self.session.revision().next()?;
        let generation = self.editor.next_generation()?;
        let timestamp = wall_time.now()?;
        let (cause, change) = match action {
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { notes, nags } => {
                let revision =
                    self.session
                        .save_notes_and_nags(before.0, expected.selected(), notes, nags)?;
                (
                    GameStorageEnumEditCause::Annotation,
                    Self::editor_revision_change(before.0, revision),
                )
            }
            DesktopAnnotationCommandsEnumAction::SaveMarkings { markings } => {
                let revision =
                    self.session
                        .save_markings(before.0, expected.selected(), markings)?;
                (
                    GameStorageEnumEditCause::Annotation,
                    Self::editor_revision_change(before.0, revision),
                )
            }
            DesktopAnnotationCommandsEnumAction::AppendStudyBranch { node, movement } => {
                let applied = self
                    .session
                    .append_study_variation(before.0, node, movement)?;
                (GameStorageEnumEditCause::StudyTree, applied.change())
            }
            DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. } => {
                let retained = self
                    .editor
                    .proof
                    .take()
                    .ok_or(DesktopGameSupervisorError::EditorVariationToken)?;
                let revision = self.session.apply_variation(retained.proof.confirm())?;
                (
                    GameStorageEnumEditCause::StudyTree,
                    Self::editor_revision_change(before.0, revision),
                )
            }
            DesktopAnnotationCommandsEnumAction::Snapshot
            | DesktopAnnotationCommandsEnumAction::TreePage { .. }
            | DesktopAnnotationCommandsEnumAction::ChildrenPage { .. }
            | DesktopAnnotationCommandsEnumAction::Navigate { .. }
            | DesktopAnnotationCommandsEnumAction::PreviewVariation(_)
            | DesktopAnnotationCommandsEnumAction::VariationScope { .. }
            | DesktopAnnotationCommandsEnumAction::Checkpoint
            | DesktopAnnotationCommandsEnumAction::RetrySave => {
                return Err(DesktopGameSupervisorError::StateUnavailable);
            }
        };
        self.editor.observe(before, &self.session, generation);
        if change == GameAnnotationsEnumChange::Changed {
            self.record(cause, timestamp);
        }
        Ok(change)
    }

    fn editor_start_publication(
        &mut self,
        publication: DesktopGameSupervisorEditorPendingPublication,
        storage_revision: GameStorageRevision,
        storage: &StorageDocumentClient,
    ) -> Result<DesktopGameSupervisorEditorComputed, DesktopGameSupervisorError> {
        if self.editor.is_publishing() {
            return Err(DesktopGameSupervisorError::Busy);
        }
        let request_id = publication.request_id;
        self.editor.publication = Some(publication);
        self.editor_publish(request_id, storage_revision, storage)
    }

    fn editor_publish(
        &mut self,
        request_id: DesktopRequestId,
        storage_revision: GameStorageRevision,
        storage: &StorageDocumentClient,
    ) -> Result<DesktopGameSupervisorEditorComputed, DesktopGameSupervisorError> {
        let pending = self
            .editor
            .publication
            .as_ref()
            .ok_or(DesktopGameSupervisorError::EditorReceipt)?;
        if pending.request_id != request_id {
            return Err(DesktopGameSupervisorError::EditorReceipt);
        }
        let editor = self.editor.project(&self.session, storage_revision)?;
        let context = editor.context();
        let snapshot = match editor.publish(context, storage) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let source = Arc::new(error);
                return Err(DesktopGameSupervisorError::EditorPublicationPending {
                    context,
                    source,
                });
            }
        };
        let pending = self
            .editor
            .publication
            .take()
            .ok_or(DesktopGameSupervisorError::StateUnavailable)?;
        let result = match pending.kind {
            DesktopGameSupervisorEditorEnumPublication::Snapshot => {
                DesktopAnnotationCommandsEnumResult::Snapshot(snapshot)
            }
            DesktopGameSupervisorEditorEnumPublication::Edited(change) => {
                DesktopAnnotationCommandsEnumResult::Edited { change, snapshot }
            }
        };
        Ok(DesktopGameSupervisorEditorComputed {
            result,
            save: pending.save,
        })
    }

    fn editor_revision_change(
        before: GameRevision,
        after: GameRevision,
    ) -> GameAnnotationsEnumChange {
        if before == after {
            GameAnnotationsEnumChange::Unchanged
        } else {
            GameAnnotationsEnumChange::Changed
        }
    }
}

pub(super) struct DesktopGameSupervisorEditorProjection {
    pub(super) envelope: DesktopGameSupervisorEditorEnvelope,
    pub(super) result: DesktopAnnotationCommandsEnumResult,
}

pub(super) struct DesktopGameSupervisorEditorReply {
    pub(super) envelope: DesktopGameSupervisorEditorEnvelope,
    pub(super) result: DesktopAnnotationCommandsEnumResult,
}

pub(super) enum DesktopGameSupervisorEnumReply {
    Game(crate::desktop::game_supervisor::DesktopGameSupervisorEnvelope),
    Editor(Box<DesktopGameSupervisorEditorReply>),
}

impl DesktopGameSupervisorEnumReply {
    pub(super) fn is_retry_save(&self) -> bool {
        match self {
            Self::Game(envelope) => matches!(
                envelope.request.action,
                DesktopGameCommandsEnumAction::RetrySave | DesktopGameCommandsEnumAction::Quit
            ),
            Self::Editor(reply) => reply.envelope.retry_save,
        }
    }

    pub(super) fn fail(self, failure: Arc<DesktopGameSupervisorError>) {
        match self {
            Self::Game(envelope) => envelope.complete(Err(failure)),
            Self::Editor(reply) => reply.envelope.complete(Err(failure)),
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/desktop/game_editor_runtime.rs"]
mod tests;
