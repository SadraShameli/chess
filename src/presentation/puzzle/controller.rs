use std::collections::HashMap;

use thiserror::Error;

use crate::board::enums::BoardEnumPieceKind;
use crate::board::r#move::BoardMove;
use crate::board::square::BoardSquare;
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::desktop::error::DesktopFailure;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::presentation::board::view::PresentationBoardAction;
use crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus;
use crate::puzzle::enums::{PuzzleEnumHintStage, PuzzleEnumPhase};
use crate::puzzle::persistence::PuzzlePersistenceEnumCommand;
use crate::puzzle::run::{PuzzleRunEnumConfirmation, PuzzleRunEnumStatus};
use crate::puzzle::session::PuzzleSessionContext;
use crate::puzzle::summary::PuzzleSummary;
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};

#[cfg(test)]
#[path = "../../../tests/presentation/puzzle.rs"]
mod tests;

#[derive(Debug, Error)]
pub enum PresentationPuzzleControllerError {
    #[error("Wait for the current puzzle action")]
    Busy,
    #[error("The puzzle action is unavailable in the current run or save state")]
    Unavailable,
    #[error("The move is illegal in this position. The countdown continues")]
    IllegalMove,
    #[error("Select the castling rook to distinguish castling from a king move")]
    AmbiguousMove,
    #[error("Choose one of the available promotion pieces or cancel")]
    PromotionRequired,
    #[error("Confirm End Run for the current puzzle state")]
    ConfirmationRequired,
    #[error("This request identity already belongs to a different puzzle action")]
    RequestConflict,
    #[error("The result does not match a pending puzzle action")]
    StaleResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationPuzzleControllerEnumAction {
    Move(BoardMove),
    OpponentReply,
    Hint,
    RetryPractice,
    Skip,
    Next,
    Pause(PuzzleTimerEnumPauseReason),
    Hidden,
    Resume,
    End(PuzzleRunEnumConfirmation),
    Reshuffle,
    Ready,
    RetrySave,
}

impl PresentationPuzzleControllerEnumAction {
    pub const fn command(self) -> Option<PuzzlePersistenceEnumCommand> {
        match self {
            Self::Move(movement) => Some(PuzzlePersistenceEnumCommand::Move(movement)),
            Self::OpponentReply => Some(PuzzlePersistenceEnumCommand::OpponentReply),
            Self::Hint => Some(PuzzlePersistenceEnumCommand::Hint),
            Self::RetryPractice => Some(PuzzlePersistenceEnumCommand::RetryPractice),
            Self::Skip => Some(PuzzlePersistenceEnumCommand::Skip),
            Self::Next => Some(PuzzlePersistenceEnumCommand::Next),
            Self::Pause(reason) => Some(PuzzlePersistenceEnumCommand::Pause(reason)),
            Self::Hidden => Some(PuzzlePersistenceEnumCommand::Readiness(
                PuzzleTimerEnumReadiness::Hidden,
            )),
            Self::Resume => Some(PuzzlePersistenceEnumCommand::Resume(
                PuzzleTimerEnumReadiness::Ready,
            )),
            Self::End(confirmation) => Some(PuzzlePersistenceEnumCommand::End(confirmation)),
            Self::Reshuffle => Some(PuzzlePersistenceEnumCommand::Reshuffle),
            Self::Ready => Some(PuzzlePersistenceEnumCommand::Readiness(
                PuzzleTimerEnumReadiness::Ready,
            )),
            Self::RetrySave => None,
        }
    }

    const fn is_urgent(self) -> bool {
        matches!(self, Self::Pause(_) | Self::Hidden)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationPuzzleControllerIntent {
    request_id: DesktopRequestId,
    context: PuzzleSessionContext,
    action: PresentationPuzzleControllerEnumAction,
}

impl PresentationPuzzleControllerIntent {
    pub const fn request_id(&self) -> DesktopRequestId {
        self.request_id
    }

    pub const fn context(&self) -> &PuzzleSessionContext {
        &self.context
    }

    pub const fn action(&self) -> PresentationPuzzleControllerEnumAction {
        self.action
    }
}

struct PresentationPuzzleControllerPromotion {
    context: PuzzleSessionContext,
    choices: Vec<BoardMove>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationPuzzleControllerEnumInput {
    Select(BoardSquare),
    Drag {
        origin: BoardSquare,
        destination: BoardSquare,
    },
}

pub struct PresentationPuzzleController {
    snapshot: DesktopPuzzleSnapshot,
    summary: Option<PuzzleSummary>,
    pending: Option<PresentationPuzzleControllerIntent>,
    urgent: Option<PresentationPuzzleControllerIntent>,
    submitted: HashMap<DesktopRequestId, PresentationPuzzleControllerIntent>,
    inputs: HashMap<DesktopRequestId, PresentationPuzzleControllerEnumInput>,
    failure: Option<DesktopFailure>,
    readiness: PuzzleTimerEnumReadiness,
    concealed: bool,
    selected: Option<BoardSquare>,
    promotion: Option<PresentationPuzzleControllerPromotion>,
    end_confirmation: Option<PuzzleSessionContext>,
}

impl PresentationPuzzleController {
    pub fn new(snapshot: DesktopPuzzleSnapshot) -> Self {
        Self {
            snapshot,
            summary: None,
            pending: None,
            urgent: None,
            submitted: HashMap::new(),
            inputs: HashMap::new(),
            failure: None,
            readiness: PuzzleTimerEnumReadiness::BoardNotReady,
            concealed: false,
            selected: None,
            promotion: None,
            end_confirmation: None,
        }
    }

    pub const fn snapshot(&self) -> &DesktopPuzzleSnapshot {
        &self.snapshot
    }

    pub const fn summary(&self) -> Option<&PuzzleSummary> {
        self.summary.as_ref()
    }

    pub const fn failure(&self) -> Option<&DesktopFailure> {
        self.failure.as_ref()
    }

    pub fn board(&self) -> Option<&DesktopBoardSnapshot> {
        if self.concealed || self.snapshot.checkpoint_status() != PuzzleCheckpointEnumStatus::Open {
            return None;
        }
        match self.snapshot.status() {
            PuzzleRunEnumStatus::AwaitingReadiness
            | PuzzleRunEnumStatus::Active
            | PuzzleRunEnumStatus::Review => self.snapshot.attempt().map(|attempt| attempt.board()),
            PuzzleRunEnumStatus::Paused { .. }
            | PuzzleRunEnumStatus::Exhausted
            | PuzzleRunEnumStatus::Expired(_)
            | PuzzleRunEnumStatus::Ended(_) => None,
        }
    }

    pub const fn selected(&self) -> Option<BoardSquare> {
        self.selected
    }

    pub fn promotion_choices(&self) -> &[BoardMove] {
        self.promotion
            .as_ref()
            .filter(|promotion| {
                promotion.context == *self.snapshot.context() && self.board().is_some()
            })
            .map_or(&[], |promotion| promotion.choices.as_slice())
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some() || self.urgent.is_some()
    }

    pub fn is_confirming_end(&self) -> bool {
        self.end_confirmation.as_ref() == Some(self.snapshot.context())
    }

    pub fn can_input(&self) -> bool {
        self.can_advance()
            && !self.is_confirming_end()
            && self.readiness == PuzzleTimerEnumReadiness::Ready
            && self.snapshot.status() == PuzzleRunEnumStatus::Active
            && self
                .snapshot
                .attempt()
                .is_some_and(|attempt| attempt.phase() == PuzzleEnumPhase::SolverMove)
    }

    pub fn can_request(&self, action: PresentationPuzzleControllerEnumAction) -> bool {
        match action {
            PresentationPuzzleControllerEnumAction::Pause(_)
            | PresentationPuzzleControllerEnumAction::Hidden => {
                self.urgent.is_none()
                    && !matches!(self.snapshot.status(), PuzzleRunEnumStatus::Ended(_))
            }
            PresentationPuzzleControllerEnumAction::RetrySave => {
                !self.is_pending()
                    && matches!(
                        self.snapshot.checkpoint_status(),
                        PuzzleCheckpointEnumStatus::RetryRequired { .. }
                    )
            }
            PresentationPuzzleControllerEnumAction::Resume => {
                !self.is_pending()
                    && self.readiness == PuzzleTimerEnumReadiness::Ready
                    && matches!(self.snapshot.status(), PuzzleRunEnumStatus::Paused { .. })
                    && matches!(
                        self.snapshot.checkpoint_status(),
                        PuzzleCheckpointEnumStatus::Open
                            | PuzzleCheckpointEnumStatus::AwaitingResume(_)
                    )
                    && !self.snapshot.has_unsaved_state()
                    && self.snapshot.durable_context() == Some(self.snapshot.context())
            }
            PresentationPuzzleControllerEnumAction::Ready => {
                self.can_advance()
                    && self.readiness == PuzzleTimerEnumReadiness::Ready
                    && self.snapshot.status() == PuzzleRunEnumStatus::AwaitingReadiness
            }
            PresentationPuzzleControllerEnumAction::Move(movement) => {
                self.can_input()
                    && self.board().is_some_and(|board| {
                        board
                            .legal_moves()
                            .iter()
                            .any(|legal| legal.movement() == movement)
                    })
            }
            PresentationPuzzleControllerEnumAction::OpponentReply => {
                self.can_advance()
                    && self.readiness == PuzzleTimerEnumReadiness::Ready
                    && self.snapshot.status() == PuzzleRunEnumStatus::Active
                    && self
                        .snapshot
                        .attempt()
                        .is_some_and(|attempt| attempt.phase() == PuzzleEnumPhase::OpponentReply)
            }
            PresentationPuzzleControllerEnumAction::Hint => {
                self.can_input()
                    && self.snapshot.attempt().is_some_and(|attempt| {
                        attempt.hint_stage() != PuzzleEnumHintStage::NextMove
                    })
            }
            PresentationPuzzleControllerEnumAction::RetryPractice => {
                self.can_advance()
                    && self.snapshot.status() == PuzzleRunEnumStatus::Active
                    && self
                        .snapshot
                        .attempt()
                        .is_some_and(|attempt| attempt.phase() == PuzzleEnumPhase::PracticeReady)
            }
            PresentationPuzzleControllerEnumAction::Skip => {
                self.can_advance() && self.snapshot.status() == PuzzleRunEnumStatus::Active
            }
            PresentationPuzzleControllerEnumAction::Next => {
                self.can_advance() && self.snapshot.status() == PuzzleRunEnumStatus::Review
            }
            PresentationPuzzleControllerEnumAction::Reshuffle => {
                self.can_advance() && self.snapshot.status() == PuzzleRunEnumStatus::Exhausted
            }
            PresentationPuzzleControllerEnumAction::End(confirmation) => {
                self.can_advance()
                    && !matches!(self.snapshot.status(), PuzzleRunEnumStatus::Ended(_))
                    && (confirmation == PuzzleRunEnumConfirmation::Unconfirmed
                        || self.is_confirming_end())
            }
        }
    }

    pub fn request(
        &mut self,
        action: PresentationPuzzleControllerEnumAction,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationPuzzleControllerIntent>, PresentationPuzzleControllerError> {
        if let Some(previous) = self.submitted.get(&request_id) {
            return if previous.action == action {
                Ok(None)
            } else {
                Err(PresentationPuzzleControllerError::RequestConflict)
            };
        }
        if action
            == PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Unconfirmed)
        {
            self.begin_end_confirmation()?;
            self.submitted.insert(
                request_id,
                PresentationPuzzleControllerIntent {
                    request_id,
                    context: self.snapshot.context().clone(),
                    action,
                },
            );
            return Ok(None);
        }
        if action
            == PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Confirmed)
            && !self.is_confirming_end()
        {
            return Err(PresentationPuzzleControllerError::ConfirmationRequired);
        }
        if !self.can_request(action) {
            return Err(if self.is_pending() && !action.is_urgent() {
                PresentationPuzzleControllerError::Busy
            } else {
                PresentationPuzzleControllerError::Unavailable
            });
        }
        let intent = PresentationPuzzleControllerIntent {
            request_id,
            context: self.snapshot.context().clone(),
            action,
        };
        if action.is_urgent() {
            self.urgent = Some(intent.clone());
            self.concealed = true;
        } else {
            self.pending = Some(intent.clone());
        }
        self.submitted.insert(request_id, intent.clone());
        self.failure = None;
        self.cancel_selection();
        self.end_confirmation = None;
        Ok(Some(intent))
    }

    pub fn report_readiness(
        &mut self,
        readiness: PuzzleTimerEnumReadiness,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationPuzzleControllerIntent>, PresentationPuzzleControllerError> {
        self.readiness = readiness;
        if readiness == PuzzleTimerEnumReadiness::Hidden {
            self.concealed = true;
            self.cancel_selection();
            if self.can_request(PresentationPuzzleControllerEnumAction::Hidden) {
                return self.request(PresentationPuzzleControllerEnumAction::Hidden, request_id);
            }
        } else if self.can_request(PresentationPuzzleControllerEnumAction::Ready) {
            return self.request(PresentationPuzzleControllerEnumAction::Ready, request_id);
        }
        Ok(None)
    }

    pub fn select_square(
        &mut self,
        square: BoardSquare,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationPuzzleControllerIntent>, PresentationPuzzleControllerError> {
        self.input(
            PresentationPuzzleControllerEnumInput::Select(square),
            request_id,
        )
    }

    pub fn board_action(
        &mut self,
        action: PresentationBoardAction,
    ) -> Result<Option<PresentationPuzzleControllerIntent>, PresentationPuzzleControllerError> {
        let input = match action.origin() {
            Some(origin) => PresentationPuzzleControllerEnumInput::Drag {
                origin,
                destination: action.square(),
            },
            None => PresentationPuzzleControllerEnumInput::Select(action.square()),
        };
        self.input(input, action.identity())
    }

    fn input(
        &mut self,
        input: PresentationPuzzleControllerEnumInput,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationPuzzleControllerIntent>, PresentationPuzzleControllerError> {
        if let Some(previous) = self.inputs.get(&request_id) {
            return if *previous == input {
                Ok(None)
            } else {
                Err(PresentationPuzzleControllerError::RequestConflict)
            };
        }
        self.inputs.insert(request_id, input);
        if !self.can_input() {
            return Err(PresentationPuzzleControllerError::Unavailable);
        }
        let square = match input {
            PresentationPuzzleControllerEnumInput::Select(square) => square,
            PresentationPuzzleControllerEnumInput::Drag {
                origin,
                destination,
            } => {
                self.selected = Some(origin);
                destination
            }
        };
        if self.promotion.is_some() {
            return Err(PresentationPuzzleControllerError::PromotionRequired);
        }
        let board = self
            .board()
            .ok_or(PresentationPuzzleControllerError::Unavailable)?;
        if let Some(origin) = self.selected {
            let rook_selected = board.legal_moves().iter().any(|movement| {
                movement.origin() == Some(origin) && movement.castling_rook() == Some(square)
            });
            let choices = board
                .legal_moves()
                .iter()
                .filter(|movement| {
                    movement.origin() == Some(origin)
                        && if rook_selected {
                            movement.castling_rook() == Some(square)
                        } else {
                            movement.destination() == square
                        }
                })
                .map(|movement| movement.movement())
                .collect::<Vec<_>>();
            if !choices.is_empty() {
                if choices
                    .iter()
                    .all(|movement| movement.promotion().is_some())
                {
                    self.promotion = Some(PresentationPuzzleControllerPromotion {
                        context: self.snapshot.context().clone(),
                        choices,
                    });
                    return Ok(None);
                }
                if choices.len() != 1 {
                    return Err(PresentationPuzzleControllerError::AmbiguousMove);
                }
                return self.request(
                    PresentationPuzzleControllerEnumAction::Move(choices[0]),
                    request_id,
                );
            }
        }
        if board
            .legal_moves()
            .iter()
            .any(|movement| movement.origin() == Some(square))
        {
            self.selected = Some(square);
            Ok(None)
        } else {
            self.cancel_selection();
            Err(PresentationPuzzleControllerError::IllegalMove)
        }
    }

    pub fn choose_promotion(
        &mut self,
        kind: BoardEnumPieceKind,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationPuzzleControllerIntent>, PresentationPuzzleControllerError> {
        let movement = self
            .promotion_choices()
            .iter()
            .copied()
            .find(|movement| movement.promotion() == Some(kind))
            .ok_or(PresentationPuzzleControllerError::PromotionRequired)?;
        self.request(
            PresentationPuzzleControllerEnumAction::Move(movement),
            request_id,
        )
    }

    pub fn cancel_selection(&mut self) {
        self.selected = None;
        self.promotion = None;
    }

    pub fn begin_end_confirmation(&mut self) -> Result<(), PresentationPuzzleControllerError> {
        if !self.can_request(PresentationPuzzleControllerEnumAction::End(
            PuzzleRunEnumConfirmation::Unconfirmed,
        )) {
            return Err(PresentationPuzzleControllerError::Unavailable);
        }
        self.cancel_selection();
        self.end_confirmation = Some(self.snapshot.context().clone());
        Ok(())
    }

    pub fn cancel_end_confirmation(&mut self) {
        self.end_confirmation = None;
    }

    pub fn observe_snapshot(&mut self, snapshot: DesktopPuzzleSnapshot) -> bool {
        if snapshot.context().session_id() != self.snapshot.context().session_id()
            || snapshot.context().revision() < self.snapshot.context().revision()
            || snapshot.captured_at() < self.snapshot.captured_at()
            || (snapshot.context().revision() == self.snapshot.context().revision()
                && snapshot.context() != self.snapshot.context())
        {
            return false;
        }
        if snapshot.context() != self.snapshot.context() {
            self.cancel_selection();
            self.end_confirmation = None;
            self.readiness = PuzzleTimerEnumReadiness::BoardNotReady;
        }
        if snapshot.context() != self.snapshot.context()
            || snapshot.timer().elapsed() != self.snapshot.timer().elapsed()
        {
            self.summary = None;
        }
        if matches!(snapshot.status(), PuzzleRunEnumStatus::Paused { .. }) {
            self.concealed = false;
        }
        self.snapshot = snapshot;
        if self.board().is_none() {
            self.cancel_selection();
        }
        true
    }

    pub fn observe_state(
        &mut self,
        snapshot: DesktopPuzzleSnapshot,
        summary: Option<PuzzleSummary>,
    ) -> bool {
        if !self.observe_snapshot(snapshot) {
            return false;
        }
        if let Some(summary) = summary {
            self.summary = Some(summary);
        }
        true
    }

    pub fn observe_success(
        &mut self,
        intent: &PresentationPuzzleControllerIntent,
        snapshot: DesktopPuzzleSnapshot,
        summary: Option<PuzzleSummary>,
    ) -> Result<bool, PresentationPuzzleControllerError> {
        if snapshot.context().session_id() != self.snapshot.context().session_id() {
            return Err(PresentationPuzzleControllerError::StaleResult);
        }
        self.finish_request(intent)?;
        if snapshot.context() == self.snapshot.context()
            && snapshot.captured_at() <= self.snapshot.captured_at()
        {
            if let Some(summary) = summary
                && snapshot.captured_at() == self.snapshot.captured_at()
            {
                self.summary = Some(summary);
            }
            return Ok(false);
        }
        let accepted = self.observe_state(snapshot, summary);
        if accepted {
            self.failure = None;
        }
        Ok(accepted)
    }

    pub fn observe_failure(
        &mut self,
        intent: &PresentationPuzzleControllerIntent,
        failure: DesktopFailure,
    ) -> Result<bool, PresentationPuzzleControllerError> {
        self.finish_request(intent)?;
        let current = intent.context == *self.snapshot.context();
        if current {
            self.failure = Some(failure);
        }
        Ok(current)
    }

    fn can_advance(&self) -> bool {
        !self.is_pending()
            && !self.concealed
            && self.snapshot.checkpoint_status() == PuzzleCheckpointEnumStatus::Open
    }

    fn finish_request(
        &mut self,
        intent: &PresentationPuzzleControllerIntent,
    ) -> Result<(), PresentationPuzzleControllerError> {
        let slot = if intent.action.is_urgent() {
            &mut self.urgent
        } else {
            &mut self.pending
        };
        if slot.as_ref() != Some(intent) {
            return Err(PresentationPuzzleControllerError::StaleResult);
        }
        *slot = None;
        Ok(())
    }
}
