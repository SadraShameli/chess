use std::sync::Arc;

use crate::clock::duration::ClockInstant;
use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::game_supervisor::DesktopGameSupervisorError;
use crate::desktop::game_transition::{
    DesktopGameTransition, DesktopGameTransitionEnumEffect, DesktopGameTransitionOptions,
};
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::enums::{GameEnumMode, GameEnumPhase, GameMoveId, GameRevision};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::outcome::GameOutcome;
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault, GamePauseRequest};
use crate::game::save::GameSaveEnumStatus;
use crate::game::session::lifecycle::{
    GameSessionLifecycleEnumCommit, GameSessionLifecycleEnumContentChange,
    GameSessionLifecycleEnumRequest, GameSessionLifecyclePendingResult,
};
use crate::game::session::{GameSession, GameSessionError};
use crate::game::storage_repository::{
    GameStorageContentEdit, GameStorageEnumContentChange, GameStorageEnumEditCause,
    GameStorageReceipt, GameStorageWallTimeProvider,
};
use crate::game::takeback::{GameTakebackEnumConsent, GameTakebackEnumResponse};

pub(super) struct DesktopGameSupervisorContentObservation {
    outcome: Option<GameOutcome>,
    live: GameMoveId,
    revision: GameRevision,
    tree_size: usize,
    study_tip: GameMoveId,
}

pub(super) struct DesktopGameSupervisorPendingResult {
    pub(super) token: GameSessionLifecyclePendingResult,
    pub(super) failure: Arc<DesktopGameSupervisorError>,
}

pub(super) struct DesktopGameSupervisorContent {
    pub(super) session: GameSession,
    pub(super) accepted_edit: Option<GameStorageContentEdit>,
    pub(super) pending_result: Option<DesktopGameSupervisorPendingResult>,
    pub(super) storage_failure: Option<Arc<DesktopGameSupervisorError>>,
    wall_failure: Option<Arc<DesktopGameSupervisorError>>,
}

impl DesktopGameSupervisorContent {
    pub(super) fn new(session: GameSession) -> Self {
        Self {
            session,
            accepted_edit: None,
            pending_result: None,
            storage_failure: None,
            wall_failure: None,
        }
    }

    pub(super) fn record(
        &mut self,
        cause: GameStorageEnumEditCause,
        timestamp: GameLibraryOrganizationTimestamp,
    ) {
        let timestamp = self
            .accepted_edit
            .map_or(timestamp, |edit| timestamp.max(edit.timestamp()));
        self.accepted_edit = Some(GameStorageContentEdit::from_accepted_revision(
            self.session.revision(),
            cause,
            timestamp,
        ));
    }

    pub(super) fn observe(&self) -> DesktopGameSupervisorContentObservation {
        DesktopGameSupervisorContentObservation {
            outcome: self.session.outcome(),
            live: self.session.live(),
            revision: self.session.revision(),
            tree_size: self.session.tree().len(),
            study_tip: self.session.study_mainline_tip(),
        }
    }

    pub(super) fn record_effect(
        &mut self,
        before: DesktopGameSupervisorContentObservation,
        cause: GameStorageEnumEditCause,
        timestamp: GameLibraryOrganizationTimestamp,
    ) {
        if before.outcome != self.session.outcome() {
            self.record(GameStorageEnumEditCause::Result, timestamp);
        } else {
            let changed = match cause {
                GameStorageEnumEditCause::AcceptedMove | GameStorageEnumEditCause::Takeback => {
                    before.live != self.session.live()
                }
                GameStorageEnumEditCause::StudyTree => {
                    before.tree_size != self.session.tree().len()
                        || before.study_tip != self.session.study_mainline_tip()
                }
                GameStorageEnumEditCause::Annotation | GameStorageEnumEditCause::Metadata => {
                    before.revision != self.session.revision()
                }
                GameStorageEnumEditCause::Created | GameStorageEnumEditCause::Result => false,
            };
            if changed {
                self.record(cause, timestamp);
            }
        }
    }

    pub(super) fn change(&self) -> GameStorageEnumContentChange {
        self.accepted_edit.map_or(
            GameStorageEnumContentChange::Checkpoint,
            GameStorageEnumContentChange::Changed,
        )
    }

    pub(super) fn is_current(&self) -> bool {
        self.pending_result.is_none()
            && self.accepted_edit.is_none()
            && self.storage_failure.is_none()
            && self.session.save_status() == GameSaveEnumStatus::Current
    }

    pub(super) fn acknowledge(
        &mut self,
        receipt: GameStorageReceipt,
    ) -> Result<GameSaveEnumStatus, DesktopGameSupervisorError> {
        self.session.acknowledge_save(receipt)?;
        if self
            .accepted_edit
            .is_some_and(|edit| edit.revision() <= receipt.game_revision())
        {
            self.accepted_edit = None;
        }
        if receipt.game_revision() == self.session.revision() && self.pending_result.is_none() {
            self.storage_failure = None;
        }
        Ok(self.session.save_status())
    }

    pub(super) fn failure(&self) -> Option<Arc<DesktopGameSupervisorError>> {
        let wall_time = self
            .pending_result
            .as_ref()
            .map(|pending| Arc::clone(&pending.failure))
            .or_else(|| self.wall_failure.as_ref().map(Arc::clone));
        match (&self.storage_failure, wall_time) {
            (Some(storage), Some(wall_time)) => {
                Some(Arc::new(DesktopGameSupervisorError::ContentSave {
                    storage: Arc::clone(storage),
                    wall_time,
                }))
            }
            (Some(storage), None) => Some(Arc::clone(storage)),
            (None, wall_time) => wall_time,
        }
    }

    pub(super) fn lifecycle<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameSessionLifecycleEnumRequest,
        proof: &P,
        wall_time: &dyn GameStorageWallTimeProvider,
    ) -> Result<bool, DesktopGameSupervisorError> {
        let prepared = self.session.prepare_lifecycle(request, proof)?;
        let timestamp = match prepared.content_change() {
            GameSessionLifecycleEnumContentChange::Checkpoint => None,
            GameSessionLifecycleEnumContentChange::Result => match wall_time.now() {
                Ok(timestamp) => Some(timestamp),
                Err(source) => {
                    let failure = Arc::new(DesktopGameSupervisorError::WallTime(source));
                    let (_, _, pending) = self
                        .session
                        .commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::DeferResult)?
                        .into_parts();
                    self.pending_result = Some(DesktopGameSupervisorPendingResult {
                        token: pending.ok_or(DesktopGameSupervisorError::StateUnavailable)?,
                        failure: Arc::clone(&failure),
                    });
                    self.wall_failure = Some(failure);
                    return Ok(true);
                }
            },
        };
        let committed = self
            .session
            .commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
        if let Some(timestamp) = timestamp {
            self.record(GameStorageEnumEditCause::Result, timestamp);
        }
        Ok(committed.transition().is_some())
    }

    fn retry_pending(
        &mut self,
        received_at: ClockInstant,
        wall_time: &dyn GameStorageWallTimeProvider,
    ) -> Result<(), DesktopGameSupervisorError> {
        let pending = self
            .pending_result
            .as_ref()
            .ok_or(DesktopGameSupervisorError::StateUnavailable)?;
        let prepared = self
            .session
            .prepare_pending_result(&pending.token, received_at)?;
        let timestamp = wall_time.now()?;
        self.session
            .commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
        self.record(GameStorageEnumEditCause::Result, timestamp);
        self.pending_result = None;
        self.wall_failure = None;
        Ok(())
    }

    pub(super) fn apply<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        options: DesktopGameTransitionOptions,
        proof: &P,
        wall_time: &dyn GameStorageWallTimeProvider,
    ) -> Result<DesktopGameTransitionEnumEffect, DesktopGameSupervisorError> {
        self.session
            .revision()
            .check_expected(options.request.expected_revision)?;
        let received_at = options.request.ordered_received_at(self.session.clock());
        let action = options.request.action.clone();
        if matches!(action, DesktopGameCommandsEnumAction::RetrySave)
            && self.pending_result.is_some()
        {
            self.retry_pending(received_at, wall_time)?;
            return Ok(DesktopGameTransitionEnumEffect::Autosave);
        }
        let pause = match action {
            DesktopGameCommandsEnumAction::Pause { cause } => Some(cause),
            DesktopGameCommandsEnumAction::Stop => Some(GamePauseEnumCause::Manual),
            DesktopGameCommandsEnumAction::Quit => Some(GamePauseEnumCause::ApplicationQuit),
            DesktopGameCommandsEnumAction::Start
            | DesktopGameCommandsEnumAction::Play { .. }
            | DesktopGameCommandsEnumAction::Navigate { .. }
            | DesktopGameCommandsEnumAction::Branch { .. }
            | DesktopGameCommandsEnumAction::ReturnToLive
            | DesktopGameCommandsEnumAction::BeginAnalysis
            | DesktopGameCommandsEnumAction::Resume
            | DesktopGameCommandsEnumAction::RequestTakeback { .. }
            | DesktopGameCommandsEnumAction::RespondTakeback { .. }
            | DesktopGameCommandsEnumAction::ConfigureTakeback { .. }
            | DesktopGameCommandsEnumAction::Resign { .. }
            | DesktopGameCommandsEnumAction::FinishLocal { .. }
            | DesktopGameCommandsEnumAction::ClaimDraw { .. }
            | DesktopGameCommandsEnumAction::ConfigurePremove { .. }
            | DesktopGameCommandsEnumAction::QueuePremove { .. }
            | DesktopGameCommandsEnumAction::CancelPremove
            | DesktopGameCommandsEnumAction::RetryEngine
            | DesktopGameCommandsEnumAction::RetrySave
            | DesktopGameCommandsEnumAction::RecoverGraphics
            | DesktopGameCommandsEnumAction::QuiesceForPreparation { .. }
            | DesktopGameCommandsEnumAction::ResumeSearches { .. } => None,
        };
        if let Some(cause) = pause
            && (self.session.phase() == GameEnumPhase::Active || self.pending_result.is_some())
        {
            let request = if self.pending_result.is_some() {
                GameSessionLifecycleEnumRequest::PollDeadline {
                    expected_revision: self.session.revision(),
                    received_at,
                }
            } else {
                GameSessionLifecycleEnumRequest::Pause(GamePauseRequest {
                    expected_revision: self.session.revision(),
                    cause,
                    received_at,
                    engine: options.engine,
                })
            };
            self.lifecycle(request, proof, wall_time)?;
            if matches!(
                cause,
                GamePauseEnumCause::AutosaveFailure
                    | GamePauseEnumCause::Fault(GamePauseEnumFault::Save)
            ) && self.pending_result.is_none()
            {
                self.session.interrupt_for_save_failure(
                    crate::game::save::GameSaveFailureRequest {
                        expected_revision: self.session.revision(),
                        received_at,
                        engine: crate::game::pause::GamePauseEnumEngineBarrier::NoEngine,
                    },
                    proof,
                )?;
                return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
            }
            return Ok(if matches!(action, DesktopGameCommandsEnumAction::Quit) {
                DesktopGameTransitionEnumEffect::Quit
            } else {
                DesktopGameTransitionEnumEffect::Autosave
            });
        }
        if self.pending_result.is_some()
            && !matches!(
                action,
                DesktopGameCommandsEnumAction::Navigate { .. }
                    | DesktopGameCommandsEnumAction::ReturnToLive
            )
        {
            return Err(GameSessionError::PendingResult.into());
        }
        if matches!(
            action,
            DesktopGameCommandsEnumAction::RequestTakeback { .. }
                | DesktopGameCommandsEnumAction::RespondTakeback { .. }
        ) && self.session.phase() == GameEnumPhase::Active
        {
            self.lifecycle(
                GameSessionLifecycleEnumRequest::PollDeadline {
                    expected_revision: self.session.revision(),
                    received_at,
                },
                proof,
                wall_time,
            )?;
            if self.session.phase() != GameEnumPhase::Active {
                return Ok(DesktopGameTransitionEnumEffect::Autosave);
            }
        }
        let timestamp = if self.requires_preflight(&action) {
            Some(wall_time.now()?)
        } else {
            None
        };
        let before = self.observe();
        let cause = if matches!(
            action,
            DesktopGameCommandsEnumAction::RequestTakeback { .. }
                | DesktopGameCommandsEnumAction::RespondTakeback { .. }
        ) {
            GameStorageEnumEditCause::Takeback
        } else if matches!(action, DesktopGameCommandsEnumAction::Branch { .. }) {
            GameStorageEnumEditCause::StudyTree
        } else {
            GameStorageEnumEditCause::AcceptedMove
        };
        let effect = DesktopGameTransition::apply(&mut self.session, options, proof)?;
        if let Some(timestamp) = timestamp {
            self.record_effect(before, cause, timestamp);
        }
        Ok(effect)
    }

    fn requires_preflight(&self, action: &DesktopGameCommandsEnumAction) -> bool {
        match action {
            DesktopGameCommandsEnumAction::Start
            | DesktopGameCommandsEnumAction::Resume
            | DesktopGameCommandsEnumAction::Play { .. }
            | DesktopGameCommandsEnumAction::Branch { .. }
            | DesktopGameCommandsEnumAction::Resign { .. }
            | DesktopGameCommandsEnumAction::FinishLocal { .. }
            | DesktopGameCommandsEnumAction::ClaimDraw { .. } => true,
            DesktopGameCommandsEnumAction::RequestTakeback { .. } => {
                self.session.seats().mode() == GameEnumMode::HumanVersusEngine
                    || self.session.takeback().consent() == GameTakebackEnumConsent::NotRequired
            }
            DesktopGameCommandsEnumAction::RespondTakeback { response, .. } => {
                *response == GameTakebackEnumResponse::Accept
            }
            DesktopGameCommandsEnumAction::Navigate { .. }
            | DesktopGameCommandsEnumAction::ReturnToLive
            | DesktopGameCommandsEnumAction::BeginAnalysis
            | DesktopGameCommandsEnumAction::Pause { .. }
            | DesktopGameCommandsEnumAction::ConfigureTakeback { .. }
            | DesktopGameCommandsEnumAction::ConfigurePremove { .. }
            | DesktopGameCommandsEnumAction::QueuePremove { .. }
            | DesktopGameCommandsEnumAction::CancelPremove
            | DesktopGameCommandsEnumAction::RetryEngine
            | DesktopGameCommandsEnumAction::RetrySave
            | DesktopGameCommandsEnumAction::RecoverGraphics
            | DesktopGameCommandsEnumAction::Stop
            | DesktopGameCommandsEnumAction::Quit
            | DesktopGameCommandsEnumAction::QuiesceForPreparation { .. }
            | DesktopGameCommandsEnumAction::ResumeSearches { .. } => false,
        }
    }
}
