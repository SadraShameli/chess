use std::collections::HashMap;

use bevy_egui::egui::{self, Ui};
use thiserror::Error;

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::square::BoardSquare;
use crate::desktop::dto::DesktopBoardMove;
use crate::desktop::error::DesktopFailure;
use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::identity::DesktopRequestId;
use crate::game::enums::{
    GameEnumPhase, GameEnumSeat, GameId, GameIdentityError, GameMoveId, GameRevision,
};
use crate::game::premove::{GamePremoveError, GamePremoveIntent};
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;

#[derive(Debug, Error)]
pub enum PresentationGameControllerError {
    #[error("Return to Live before entering a move")]
    ViewingHistory,
    #[error("Resume an active game before entering a move")]
    InactiveGame,
    #[error("Save this game successfully before entering a move")]
    UnsavedGame,
    #[error("Enable premoves before selecting a move during Reckless's turn")]
    PremovesDisabled,
    #[error("Select one of the human player's pieces")]
    Origin,
    #[error("This move is not available in the native position")]
    UnavailableMove,
    #[error("Select the castling rook to distinguish castling from the king move")]
    AmbiguousCastling,
    #[error("The native move choices are ambiguous")]
    AmbiguousMove,
    #[error("Choose a promotion piece or cancel the pending promotion first")]
    PromotionPending,
    #[error("There is no pending promotion to complete")]
    NoPromotion,
    #[error("The game changed while choosing a promotion. Select the move again")]
    StalePromotion,
    #[error("Choose one of the available promotion pieces")]
    InvalidPromotion,
    #[error("This input identity was already used for another board action")]
    RequestConflict,
    #[error("The board input capacity is exhausted. Wait for a native game transition")]
    InputCapacity,
    #[error("The native result does not match the pending board request")]
    UnexpectedResult,
    #[error(transparent)]
    Identity(#[from] GameIdentityError),
    #[error(transparent)]
    Premove(#[from] GamePremoveError),
}

#[derive(Debug, Clone)]
pub struct PresentationGameControllerIntent {
    game: GameId,
    revision: GameRevision,
    request_id: DesktopRequestId,
    action: DesktopGameCommandsEnumAction,
}

impl PresentationGameControllerIntent {
    pub const fn game(&self) -> GameId {
        self.game
    }

    pub const fn revision(&self) -> GameRevision {
        self.revision
    }

    pub const fn request_id(&self) -> DesktopRequestId {
        self.request_id
    }

    pub const fn action(&self) -> &DesktopGameCommandsEnumAction {
        &self.action
    }

    pub fn into_action(self) -> DesktopGameCommandsEnumAction {
        self.action
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PresentationGameControllerContext {
    game: GameId,
    revision: GameRevision,
    live: GameMoveId,
    view: GameMoveId,
    phase: GameEnumPhase,
    are_premoves_enabled: bool,
    is_save_required: bool,
}

impl PresentationGameControllerContext {
    fn capture(snapshot: &DesktopGameSnapshot) -> Self {
        Self {
            game: snapshot.id(),
            revision: snapshot.revision(),
            live: snapshot.live(),
            view: snapshot.view(),
            phase: snapshot.phase(),
            are_premoves_enabled: snapshot.are_premoves_enabled(),
            is_save_required: snapshot.is_save_required(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationGameControllerEnumInput {
    Select(BoardSquare),
    Drag {
        origin: BoardSquare,
        destination: BoardSquare,
    },
    Cancel,
}

#[derive(Debug, Clone, Copy)]
enum PresentationGameControllerEnumMode {
    Play(BoardEnumColor),
    Premove(BoardEnumColor),
}

impl PresentationGameControllerEnumMode {
    const fn color(self) -> BoardEnumColor {
        match self {
            Self::Play(color) | Self::Premove(color) => color,
        }
    }
}

#[derive(Debug)]
struct PresentationGameControllerPromotion {
    context: PresentationGameControllerContext,
    request_id: DesktopRequestId,
    movement: PresentationGameControllerMove,
    choices: Vec<BoardEnumPieceKind>,
    is_focus_pending: bool,
}

#[derive(Debug, Clone, Copy)]
struct PresentationGameControllerMove {
    origin: BoardSquare,
    destination: BoardSquare,
    promotion: Option<BoardEnumPieceKind>,
    mode: PresentationGameControllerEnumMode,
}

impl PresentationGameControllerMove {
    const fn new(
        origin: BoardSquare,
        destination: BoardSquare,
        mode: PresentationGameControllerEnumMode,
    ) -> Self {
        Self {
            origin,
            destination,
            promotion: None,
            mode,
        }
    }
}

#[derive(Debug, Default)]
pub struct PresentationGameController {
    context: Option<PresentationGameControllerContext>,
    selected: Option<BoardSquare>,
    promotion: Option<PresentationGameControllerPromotion>,
    focus_return: Option<BoardSquare>,
    seen: HashMap<DesktopRequestId, PresentationGameControllerEnumInput>,
    submitted: Option<(DesktopRequestId, PresentationGameControllerEnumInput)>,
    pending_result: Option<DesktopRequestId>,
    feedback: Option<String>,
}

impl PresentationGameController {
    const INPUT_CAPACITY: usize = 1024;
    const PROMOTION_CHOICES: [BoardEnumPieceKind; 4] = [
        BoardEnumPieceKind::Queen,
        BoardEnumPieceKind::Rook,
        BoardEnumPieceKind::Bishop,
        BoardEnumPieceKind::Knight,
    ];

    pub fn new() -> Self {
        Self::default()
    }

    pub const fn selected(&self) -> Option<BoardSquare> {
        self.selected
    }

    pub const fn is_promotion_pending(&self) -> bool {
        self.promotion.is_some()
    }

    pub fn promotion_choices(&self) -> &[BoardEnumPieceKind] {
        match &self.promotion {
            Some(promotion) => &promotion.choices,
            None => &[],
        }
    }

    pub fn feedback(&self) -> Option<&str> {
        self.feedback.as_deref()
    }

    pub(crate) fn restore_board_focus(
        &mut self,
        context: &egui::Context,
        scene: &PresentationBoardScene,
    ) {
        if let Some(origin) = self.focus_return {
            let id = PresentationBoardSemanticGrid::square_id(origin);
            if PresentationBoardSemanticGrid::square_rect(origin, scene).is_some()
                && context
                    .read_response(id)
                    .is_some_and(|response| response.enabled())
            {
                context.memory_mut(|memory| memory.request_focus(id));
                self.focus_return = None;
            }
        }
    }

    pub fn select(
        &mut self,
        snapshot: &DesktopGameSnapshot,
        square: BoardSquare,
        identity: DesktopRequestId,
    ) -> Result<Option<PresentationGameControllerIntent>, PresentationGameControllerError> {
        self.synchronize(snapshot);
        let mode = Self::mode(snapshot)?;
        if !self.record(
            identity,
            PresentationGameControllerEnumInput::Select(square),
        )? {
            return Ok(None);
        }
        if self.promotion.is_some() {
            return Err(PresentationGameControllerError::PromotionPending);
        }
        let Some(origin) = self.selected else {
            Self::owned_piece(snapshot, square, mode.color())?;
            self.selected = Some(square);
            return Ok(None);
        };
        if origin == square {
            self.selected = None;
            return Ok(None);
        }
        let is_castling_rook = snapshot.board().legal_moves().iter().any(|movement| {
            movement.origin() == Some(origin) && movement.castling_rook() == Some(square)
        });
        if matches!(mode, PresentationGameControllerEnumMode::Play(_))
            && !is_castling_rook
            && Self::owned_piece(snapshot, square, mode.color()).is_ok()
        {
            self.selected = Some(square);
            return Ok(None);
        }
        self.prepare_move(
            snapshot,
            identity,
            PresentationGameControllerMove::new(origin, square, mode),
        )
    }

    pub fn drag(
        &mut self,
        snapshot: &DesktopGameSnapshot,
        origin: BoardSquare,
        destination: BoardSquare,
        identity: DesktopRequestId,
    ) -> Result<Option<PresentationGameControllerIntent>, PresentationGameControllerError> {
        self.synchronize(snapshot);
        if !self.record(
            identity,
            PresentationGameControllerEnumInput::Drag {
                origin,
                destination,
            },
        )? {
            return Ok(None);
        }
        let mode = Self::mode(snapshot)?;
        if self.promotion.is_some() {
            return Err(PresentationGameControllerError::PromotionPending);
        }
        Self::owned_piece(snapshot, origin, mode.color())?;
        self.selected = Some(origin);
        if origin == destination {
            return Ok(None);
        }
        self.prepare_move(
            snapshot,
            identity,
            PresentationGameControllerMove::new(origin, destination, mode),
        )
    }

    pub fn cancel(
        &mut self,
        snapshot: &DesktopGameSnapshot,
        identity: DesktopRequestId,
    ) -> Result<Option<PresentationGameControllerIntent>, PresentationGameControllerError> {
        self.synchronize(snapshot);
        if !self.record(identity, PresentationGameControllerEnumInput::Cancel)? {
            return Ok(None);
        }
        self.clear_selection();
        if snapshot.premove().is_some() {
            self.submit(
                snapshot,
                identity,
                DesktopGameCommandsEnumAction::CancelPremove,
            )
            .map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn choose_promotion(
        &mut self,
        snapshot: &DesktopGameSnapshot,
        kind: BoardEnumPieceKind,
    ) -> Result<Option<PresentationGameControllerIntent>, PresentationGameControllerError> {
        let promotion = self
            .promotion
            .as_ref()
            .ok_or(PresentationGameControllerError::NoPromotion)?;
        if promotion.context != PresentationGameControllerContext::capture(snapshot) {
            self.synchronize(snapshot);
            self.clear_selection();
            return Err(PresentationGameControllerError::StalePromotion);
        }
        Self::mode(snapshot)?;
        if !promotion.choices.contains(&kind) {
            return Err(PresentationGameControllerError::InvalidPromotion);
        }
        let mut movement = promotion.movement;
        movement.promotion = Some(kind);
        let action = Self::move_action(promotion.request_id, movement)?;
        self.submit(snapshot, promotion.request_id, action)
            .map(Some)
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        is_busy: bool,
    ) -> Result<Option<PresentationGameControllerIntent>, PresentationGameControllerError> {
        self.synchronize(snapshot);
        if let Some(feedback) = self.feedback() {
            ui.label(feedback);
        }
        let mut selected = None;
        let mut is_cancelled = false;
        if let Some(promotion) = &mut self.promotion {
            let modal = egui::Modal::new(egui::Id::new(("chess.game.promotion", snapshot.id())))
                .show(ui.ctx(), |ui| {
                    let can_focus = ui.memory(|memory| memory.is_above_modal_layer(ui.layer_id()));
                    ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
                        node.set_role(egui::accesskit::Role::Dialog);
                        node.set_label("Choose Promotion");
                        node.set_modal();
                    });
                    let mut focusable = Vec::with_capacity(promotion.choices.len() + 1);
                    if can_focus && ui.input(|input| input.key_pressed(egui::Key::Tab)) {
                        ui.ctx()
                            .memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
                    }
                    ui.heading("Choose Promotion");
                    ui.horizontal_wrapped(|ui| {
                        for kind in &promotion.choices {
                            let button = ui.add_enabled(
                                !is_busy,
                                egui::Button::new(Self::promotion_label(*kind)),
                            );
                            if button.enabled() {
                                focusable.push(button.id);
                            }
                            if promotion.is_focus_pending
                                && can_focus
                                && button.enabled()
                                && !ui.is_sizing_pass()
                            {
                                button.request_focus();
                                promotion.is_focus_pending = false;
                            }
                            if button.clicked() {
                                selected = Some(*kind);
                            }
                        }
                    });
                    let cancel = ui.button("Cancel Promotion");
                    focusable.push(cancel.id);
                    is_cancelled = cancel.clicked();
                    if can_focus && !ui.is_sizing_pass() {
                        Self::contain_promotion_focus(ui.ctx(), &focusable);
                    }
                });
            if modal.is_top_modal && !is_busy {
                for kind in &promotion.choices {
                    if let Some(key) = Self::promotion_key(*kind)
                        && ui
                            .ctx()
                            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, key))
                    {
                        selected = Some(*kind);
                        break;
                    }
                }
            }
            is_cancelled |= modal.should_close();
        }
        if is_cancelled {
            self.clear_selection();
            return Ok(None);
        }
        match selected {
            Some(kind) => self.choose_promotion(snapshot, kind),
            None => Ok(None),
        }
    }

    pub fn observe_result(
        &mut self,
        request_id: DesktopRequestId,
        result: Result<(), DesktopFailure>,
    ) -> Result<(), PresentationGameControllerError> {
        if self.pending_result != Some(request_id) {
            return Err(PresentationGameControllerError::UnexpectedResult);
        }
        self.pending_result = None;
        self.feedback = Some(match result {
            Ok(()) => "Move accepted by the native game.".to_owned(),
            Err(error) => error.message().to_owned(),
        });
        Ok(())
    }

    pub fn synchronize(&mut self, snapshot: &DesktopGameSnapshot) -> bool {
        let context = PresentationGameControllerContext::capture(snapshot);
        if self.context != Some(context) {
            if self.selected.is_some() || self.promotion.is_some() {
                self.feedback = Some("The game changed. Select your move again.".to_owned());
            }
            self.clear_selection();
            self.seen.clear();
            self.context = Some(context);
            true
        } else {
            false
        }
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        if let Some(promotion) = self.promotion.take() {
            self.focus_return = Some(promotion.movement.origin);
        }
    }

    fn record(
        &mut self,
        identity: DesktopRequestId,
        input: PresentationGameControllerEnumInput,
    ) -> Result<bool, PresentationGameControllerError> {
        let previous = self.seen.get(&identity).copied().or_else(|| {
            self.submitted
                .filter(|(submitted, _)| *submitted == identity)
                .map(|(_, previous)| previous)
        });
        if let Some(previous) = previous {
            return if previous == input {
                Ok(false)
            } else {
                Err(PresentationGameControllerError::RequestConflict)
            };
        }
        if self.seen.len() >= Self::INPUT_CAPACITY {
            return Err(PresentationGameControllerError::InputCapacity);
        }
        self.seen.insert(identity, input);
        Ok(true)
    }

    fn mode(
        snapshot: &DesktopGameSnapshot,
    ) -> Result<PresentationGameControllerEnumMode, PresentationGameControllerError> {
        if !snapshot.is_viewing_live() {
            return Err(PresentationGameControllerError::ViewingHistory);
        }
        if snapshot.is_save_required() {
            return Err(PresentationGameControllerError::UnsavedGame);
        }
        if snapshot.phase() != GameEnumPhase::Active {
            return Err(PresentationGameControllerError::InactiveGame);
        }
        let turn = snapshot.live_turn();
        match snapshot.seats().seat(turn) {
            GameEnumSeat::Human => Ok(PresentationGameControllerEnumMode::Play(turn)),
            GameEnumSeat::Reckless => {
                if snapshot.are_premoves_enabled() {
                    Ok(PresentationGameControllerEnumMode::Premove(turn.opposite()))
                } else {
                    Err(PresentationGameControllerError::PremovesDisabled)
                }
            }
        }
    }

    fn owned_piece(
        snapshot: &DesktopGameSnapshot,
        square: BoardSquare,
        color: BoardEnumColor,
    ) -> Result<BoardEnumPieceKind, PresentationGameControllerError> {
        snapshot
            .board()
            .pieces()
            .iter()
            .find(|piece| piece.square() == square && piece.color() == color)
            .map(|piece| piece.kind())
            .ok_or(PresentationGameControllerError::Origin)
    }

    fn prepare_move(
        &mut self,
        snapshot: &DesktopGameSnapshot,
        identity: DesktopRequestId,
        movement: PresentationGameControllerMove,
    ) -> Result<Option<PresentationGameControllerIntent>, PresentationGameControllerError> {
        let PresentationGameControllerMove {
            origin,
            destination,
            mode,
            ..
        } = movement;
        let choices = match mode {
            PresentationGameControllerEnumMode::Play(_) => {
                Self::legal_promotion_choices(snapshot, origin, destination)?
            }
            PresentationGameControllerEnumMode::Premove(color) => {
                let kind = Self::owned_piece(snapshot, origin, color)?;
                if GamePremoveIntent::requires_promotion(kind, color, destination) {
                    Self::PROMOTION_CHOICES.to_vec()
                } else {
                    Vec::new()
                }
            }
        };
        if !choices.is_empty() {
            self.promotion = Some(PresentationGameControllerPromotion {
                context: PresentationGameControllerContext::capture(snapshot),
                request_id: identity,
                movement,
                choices,
                is_focus_pending: true,
            });
            return Ok(None);
        }
        let action = Self::move_action(identity, movement)?;
        self.submit(snapshot, identity, action).map(Some)
    }

    fn legal_promotion_choices(
        snapshot: &DesktopGameSnapshot,
        origin: BoardSquare,
        destination: BoardSquare,
    ) -> Result<Vec<BoardEnumPieceKind>, PresentationGameControllerError> {
        let from_origin = |movement: &&DesktopBoardMove| movement.origin() == Some(origin);
        let mut candidates: Vec<&DesktopBoardMove> = snapshot
            .board()
            .legal_moves()
            .iter()
            .filter(from_origin)
            .filter(|movement| movement.castling_rook() == Some(destination))
            .collect();
        if candidates.is_empty() {
            candidates.extend(
                snapshot
                    .board()
                    .legal_moves()
                    .iter()
                    .filter(from_origin)
                    .filter(|movement| movement.destination() == destination),
            );
            let has_castle = candidates
                .iter()
                .any(|movement| movement.castling_rook().is_some());
            let has_normal = candidates
                .iter()
                .any(|movement| movement.castling_rook().is_none());
            if has_castle && has_normal {
                return Err(PresentationGameControllerError::AmbiguousCastling);
            }
        }
        if candidates.is_empty() {
            return Err(PresentationGameControllerError::UnavailableMove);
        }
        let choices: Vec<BoardEnumPieceKind> = Self::PROMOTION_CHOICES
            .into_iter()
            .filter(|kind| {
                candidates
                    .iter()
                    .any(|movement| movement.promotion() == Some(*kind))
            })
            .collect();
        if candidates.len() > 1 && choices.is_empty() {
            return Err(PresentationGameControllerError::AmbiguousMove);
        }
        Ok(choices)
    }

    fn move_action(
        identity: DesktopRequestId,
        movement: PresentationGameControllerMove,
    ) -> Result<DesktopGameCommandsEnumAction, PresentationGameControllerError> {
        let PresentationGameControllerMove {
            origin,
            destination,
            promotion,
            mode,
        } = movement;
        match mode {
            PresentationGameControllerEnumMode::Play(color) => {
                let node = String::from(identity).parse()?;
                Ok(DesktopGameCommandsEnumAction::Play {
                    node,
                    origin,
                    destination,
                    promotion,
                    color,
                })
            }
            PresentationGameControllerEnumMode::Premove(_) => {
                let intent = GamePremoveIntent::new(origin, destination, promotion)?;
                Ok(DesktopGameCommandsEnumAction::QueuePremove { intent })
            }
        }
    }

    fn submit(
        &mut self,
        snapshot: &DesktopGameSnapshot,
        request_id: DesktopRequestId,
        action: DesktopGameCommandsEnumAction,
    ) -> Result<PresentationGameControllerIntent, PresentationGameControllerError> {
        let input = self
            .seen
            .get(&request_id)
            .copied()
            .ok_or(PresentationGameControllerError::UnexpectedResult)?;
        self.clear_selection();
        self.submitted = Some((request_id, input));
        self.pending_result = Some(request_id);
        self.feedback = Some("Waiting for the native game to accept the move.".to_owned());
        Ok(PresentationGameControllerIntent {
            game: snapshot.id(),
            revision: snapshot.revision(),
            request_id,
            action,
        })
    }

    const fn promotion_label(kind: BoardEnumPieceKind) -> &'static str {
        match kind {
            BoardEnumPieceKind::Queen => "Promote to Queen",
            BoardEnumPieceKind::Rook => "Promote to Rook",
            BoardEnumPieceKind::Bishop => "Promote to Bishop",
            BoardEnumPieceKind::Knight => "Promote to Knight",
            BoardEnumPieceKind::Pawn | BoardEnumPieceKind::King => "Unavailable Promotion",
        }
    }

    fn contain_promotion_focus(context: &egui::Context, focusable: &[egui::Id]) {
        let Some(first) = focusable.first().copied() else {
            return;
        };
        let focused = context.memory(|memory| memory.focused());
        let index = focused.and_then(|focused| focusable.iter().position(|id| *id == focused));
        let offset = context.input_mut(|input| {
            if input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) {
                Some(focusable.len() - 1)
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                Some(1)
            } else {
                None
            }
        });
        let target = match (index, offset) {
            (Some(index), Some(offset)) => focusable[(index + offset) % focusable.len()],
            (Some(index), None) => focusable[index],
            (None, Some(_) | None) => first,
        };
        if focused != Some(target) {
            context.memory_mut(|memory| memory.request_focus(target));
        }
    }

    const fn promotion_key(kind: BoardEnumPieceKind) -> Option<egui::Key> {
        match kind {
            BoardEnumPieceKind::Queen => Some(egui::Key::Q),
            BoardEnumPieceKind::Rook => Some(egui::Key::R),
            BoardEnumPieceKind::Bishop => Some(egui::Key::B),
            BoardEnumPieceKind::Knight => Some(egui::Key::N),
            BoardEnumPieceKind::Pawn | BoardEnumPieceKind::King => None,
        }
    }
}
