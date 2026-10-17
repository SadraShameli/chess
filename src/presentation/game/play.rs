use std::time::Duration;

use bevy_egui::egui::{self, Ui};
use thiserror::Error;

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::r#move::BoardMove;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::state::{ClockState, ClockStateEnumStatus, ClockStateError, ClockStateView};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::game::draw_claim::{GameDrawClaimEnumIntent, GameDrawClaimEnumKind};
use crate::game::enums::{
    GameEnumMode, GameEnumOutcomeReason, GameEnumPhase, GameEnumResult, GameEnumSeat, GameId,
    GameMoveId, GameRevision,
};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use crate::game::session::GameSessionEnumInterruption;
use crate::game::takeback::GameTakebackEnumResponse;
use crate::presentation::board::figurines::PresentationBoardFigurines;
use crate::presentation::theme::tokens::PresentationThemeTokens;

#[derive(Debug, Error)]
pub enum PresentationGamePlayError {
    #[error(transparent)]
    Clock(#[from] ClockStateError),
    #[error(transparent)]
    Time(#[from] ClockDurationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationGamePlayEnumOperation {
    Navigate(GameMoveId),
    ReturnToLive,
    Pause,
    Resume,
    Retry,
    Takeback {
        requester: BoardEnumColor,
    },
    RespondTakeback {
        responder: BoardEnumColor,
        response: GameTakebackEnumResponse,
    },
    ClaimDraw {
        claimant: BoardEnumColor,
        kind: GameDrawClaimEnumKind,
        intent: GameDrawClaimEnumIntent,
    },
    Resign {
        resigning: BoardEnumColor,
    },
    CancelPremove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationGamePlayIntent {
    game: GameId,
    revision: GameRevision,
    operation: PresentationGamePlayEnumOperation,
}

impl PresentationGamePlayIntent {
    pub const fn new(
        game: GameId,
        revision: GameRevision,
        operation: PresentationGamePlayEnumOperation,
    ) -> Self {
        Self {
            game,
            revision,
            operation,
        }
    }

    pub const fn game(self) -> GameId {
        self.game
    }
    pub const fn revision(self) -> GameRevision {
        self.revision
    }
    pub const fn operation(self) -> PresentationGamePlayEnumOperation {
        self.operation
    }
}

pub struct PresentationGamePlayClock;

impl PresentationGamePlayClock {
    const SECOND: ClockDuration = ClockDuration::from_nanos(1_000_000_000);
    const TENTHS_PER_SECOND: u64 = 10;
    const UNITS_PER_TIME_PART: u64 = 60;

    pub fn text(duration: Option<ClockDuration>) -> String {
        let Some(duration) = duration else {
            return "Untimed".to_owned();
        };
        if Self::is_low_time(Some(duration)) {
            let ticks = duration.as_nanos().div_ceil(Self::tenth().as_nanos());
            let seconds = ticks / Self::TENTHS_PER_SECOND;
            let tenths = ticks % Self::TENTHS_PER_SECOND;
            return format!("0:{seconds:02}.{tenths}");
        }
        let total_seconds = duration.as_nanos().div_ceil(Self::SECOND.as_nanos());
        let total_minutes = total_seconds / Self::UNITS_PER_TIME_PART;
        let seconds = total_seconds % Self::UNITS_PER_TIME_PART;
        let minutes = total_minutes % Self::UNITS_PER_TIME_PART;
        let hours = total_minutes / Self::UNITS_PER_TIME_PART;
        if hours == 0 {
            format!("{minutes}:{seconds:02}")
        } else {
            format!("{hours}:{minutes:02}:{seconds:02}")
        }
    }

    pub fn is_low_time(duration: Option<ClockDuration>) -> bool {
        duration.is_some_and(|duration| duration < ClockState::LOW_TIME)
    }

    const fn tenth() -> ClockDuration {
        ClockDuration::from_nanos(Self::SECOND.as_nanos() / Self::TENTHS_PER_SECOND)
    }

    fn next_refresh(duration: Option<ClockDuration>) -> Option<Duration> {
        let duration = duration?;
        if duration == ClockDuration::ZERO {
            return None;
        }
        let quantum = if duration <= ClockState::LOW_TIME {
            Self::tenth()
        } else {
            Self::SECOND
        };
        let remainder = duration.as_nanos() % quantum.as_nanos();
        Some(Duration::from_nanos(if remainder == 0 {
            quantum.as_nanos()
        } else {
            remainder
        }))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PresentationGamePlayClaimContext {
    game: GameId,
    revision: GameRevision,
    view: GameMoveId,
    phase: GameEnumPhase,
    is_save_required: bool,
    is_busy: bool,
}

impl PresentationGamePlayClaimContext {
    fn new(snapshot: &DesktopGameSnapshot, is_busy: bool) -> Self {
        Self {
            game: snapshot.id(),
            revision: snapshot.revision(),
            view: snapshot.view(),
            phase: snapshot.phase(),
            is_save_required: snapshot.is_save_required(),
            is_busy,
        }
    }
}

#[derive(Default)]
pub struct PresentationGamePlay {
    resignation: Option<PresentationGamePlayIntent>,
    intended_move: Option<BoardMove>,
    claim_context: Option<PresentationGamePlayClaimContext>,
    submitted_frame: Option<(u64, GameId)>,
}

impl PresentationGamePlay {
    const DRAW_CLAIMS: [(GameDrawClaimEnumKind, &str, &str); 2] = [
        (
            GameDrawClaimEnumKind::ThreefoldRepetition,
            "Claim Threefold Draw",
            "Claim Threefold with Intended Move",
        ),
        (
            GameDrawClaimEnumKind::FiftyMove,
            "Claim Fifty-Move Draw",
            "Claim Fifty-Move with Intended Move",
        ),
    ];

    pub fn new() -> Self {
        Self::default()
    }

    pub const fn is_resignation_confirmation_open(&self) -> bool {
        self.resignation.is_some()
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        now: ClockInstant,
        is_busy: bool,
    ) -> Result<Option<PresentationGamePlayIntent>, PresentationGamePlayError> {
        now.elapsed_since(snapshot.captured_at())?;
        let clock = snapshot.clock().view(now)?;
        let claim_context = PresentationGamePlayClaimContext::new(snapshot, is_busy);
        if self.claim_context != Some(claim_context) {
            self.intended_move = None;
            self.claim_context = Some(claim_context);
        }
        if self.resignation.is_some_and(|pending| {
            pending.game() != snapshot.id() || pending.revision() != snapshot.revision()
        }) {
            self.resignation = None;
        }
        self.show_players(ui, snapshot, clock);
        Self::show_status(ui, snapshot, clock);
        if is_busy {
            ui.weak("Waiting for the game to accept your action.");
        }
        let mut operation = None;
        ui.separator();
        self.show_controls(ui, snapshot, is_busy, &mut operation);
        self.show_resignation(ui, snapshot, is_busy, &mut operation);
        ui.separator();
        Self::show_moves(ui, snapshot, is_busy, &mut operation);
        if clock.status() == ClockStateEnumStatus::Running
            && let Some(delay) =
                PresentationGamePlayClock::next_refresh(clock.side(clock.active()).displayed())
        {
            ui.ctx().request_repaint_after(delay);
        }
        let frame = (ui.ctx().cumulative_frame_nr(), snapshot.id());
        if self.submitted_frame == Some(frame) {
            return Ok(None);
        }
        Ok(operation.map(|operation| {
            self.submitted_frame = Some(frame);
            PresentationGamePlayIntent::new(snapshot.id(), snapshot.revision(), operation)
        }))
    }

    fn show_players(&self, ui: &mut Ui, snapshot: &DesktopGameSnapshot, clock: ClockStateView) {
        for color in [BoardEnumColor::Black, BoardEnumColor::White] {
            let side = clock.side(color);
            ui.group(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(Self::color_label(color));
                    let name = match color {
                        BoardEnumColor::White => snapshot.metadata().known().white.as_ref(),
                        BoardEnumColor::Black => snapshot.metadata().known().black.as_ref(),
                    };
                    if let Some(name) = name {
                        ui.label(name.as_str());
                    } else {
                        ui.label(match snapshot.seats().seat(color) {
                            GameEnumSeat::Human => "Human",
                            GameEnumSeat::Reckless => "Reckless",
                        });
                    }
                    if clock.active() == color && clock.status() == ClockStateEnumStatus::Running {
                        ui.strong("To move");
                    }
                });
                let text = egui::RichText::new(PresentationGamePlayClock::text(side.displayed()))
                    .monospace()
                    .strong();
                let text = if PresentationGamePlayClock::is_low_time(side.displayed()) {
                    text.color(ui.visuals().warn_fg_color)
                } else {
                    text
                };
                ui.label(text);
                if PresentationGamePlayClock::is_low_time(side.displayed()) {
                    ui.small("Low time");
                }
                if side.delay_remaining() != ClockDuration::ZERO {
                    ui.small(format!(
                        "Delay: {}",
                        PresentationGamePlayClock::text(Some(side.delay_remaining()))
                    ));
                }
                if side.pending_refund() != ClockDuration::ZERO {
                    ui.small(format!(
                        "Pending Bronstein refund: {}",
                        PresentationGamePlayClock::text(Some(side.pending_refund()))
                    ));
                }
            });
        }
    }

    fn show_status(ui: &mut Ui, snapshot: &DesktopGameSnapshot, clock: ClockStateView) {
        if let Some(outcome) = snapshot.outcome() {
            let result = match outcome.result() {
                GameEnumResult::Victory(BoardEnumColor::White) => "White wins (1-0)",
                GameEnumResult::Victory(BoardEnumColor::Black) => "Black wins (0-1)",
                GameEnumResult::Draw => "Draw (1/2-1/2)",
            };
            ui.strong(result);
            ui.label(Self::outcome_reason(outcome.reason()));
        } else if snapshot.is_save_required() {
            let message =
                if snapshot.interruption() == Some(GameSessionEnumInterruption::PendingResult) {
                    Self::interruption_label(GameSessionEnumInterruption::PendingResult)
                } else {
                    "This game is unsaved. Retry Save before continuing."
                };
            ui.colored_label(ui.visuals().error_fg_color, message);
        } else if let Some(interruption) = snapshot.interruption() {
            ui.label(Self::interruption_label(interruption));
        } else {
            match snapshot.phase() {
                GameEnumPhase::Active => {
                    if matches!(clock.status(), ClockStateEnumStatus::Flagged(_)) {
                        ui.label("Clock expired. Waiting for the native game result.");
                    } else {
                        ui.label("Game in progress");
                    }
                }
                GameEnumPhase::Setup => {
                    ui.label("Game setup");
                }
                GameEnumPhase::Interrupted => {
                    ui.label("Game interrupted");
                }
                GameEnumPhase::Completed => {
                    ui.label("Game completed");
                }
                GameEnumPhase::Analysis => {
                    ui.label("Game review");
                }
            }
        }
        if !snapshot.is_viewing_live() {
            ui.strong("Viewing history");
            ui.small("The live game, its clocks, and Reckless continue independently.");
        }
    }

    fn show_controls(
        &mut self,
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        is_busy: bool,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        let recovery = if snapshot.is_save_required() {
            Some(("Retry Save", PresentationGamePlayEnumOperation::Retry))
        } else {
            snapshot
                .interruption()
                .map(|interruption| match interruption {
                    GameSessionEnumInterruption::RecoveryRequired(fault) => (
                        Self::retry_label(fault),
                        PresentationGamePlayEnumOperation::Retry,
                    ),
                    GameSessionEnumInterruption::Adjudication { .. } => (
                        "Retry Adjudication",
                        PresentationGamePlayEnumOperation::Retry,
                    ),
                    GameSessionEnumInterruption::Paused(_)
                    | GameSessionEnumInterruption::Recovered(_) => {
                        ("Resume Game", PresentationGamePlayEnumOperation::Resume)
                    }
                    GameSessionEnumInterruption::PendingResult => {
                        ("Retry Save", PresentationGamePlayEnumOperation::Retry)
                    }
                })
        };
        if let Some((label, action)) = recovery {
            Self::action(ui, label, !is_busy, action, operation);
        } else if snapshot.phase() == GameEnumPhase::Active {
            Self::action(
                ui,
                "Pause Game",
                !is_busy,
                PresentationGamePlayEnumOperation::Pause,
                operation,
            );
        }
        let can_takeback = snapshot.takeback_target().is_some()
            && snapshot.takeback_pending().is_none()
            && !is_busy
            && !snapshot.is_save_required();
        if snapshot.takeback_pending().is_none() {
            match snapshot.seats().mode() {
                GameEnumMode::HumanVersusEngine => {
                    Self::action(
                        ui,
                        "Takeback",
                        can_takeback,
                        PresentationGamePlayEnumOperation::Takeback {
                            requester: Self::human_color(snapshot),
                        },
                        operation,
                    );
                }
                GameEnumMode::LocalTwoPlayer => {
                    ui.horizontal_wrapped(|ui| {
                        for requester in [BoardEnumColor::White, BoardEnumColor::Black] {
                            Self::action(
                                ui,
                                Self::takeback_label(requester),
                                can_takeback,
                                PresentationGamePlayEnumOperation::Takeback { requester },
                                operation,
                            );
                        }
                    });
                }
            }
        } else {
            Self::show_takeback_consent(
                ui,
                snapshot,
                !is_busy && !snapshot.is_save_required(),
                operation,
            );
        }
        self.show_claims(
            ui,
            snapshot,
            !is_busy && !snapshot.is_save_required(),
            operation,
        );
        if let Some(premove) = snapshot.premove() {
            ui.strong("Queued Premove");
            let promotion = match premove.promotion() {
                None => String::new(),
                Some(kind) => format!(" = {}", Self::piece_label(kind)),
            };
            ui.label(format!(
                "{} to {}{promotion}",
                premove.origin(),
                premove.destination()
            ));
            Self::action(
                ui,
                "Cancel Premove",
                !is_busy && !snapshot.is_save_required(),
                PresentationGamePlayEnumOperation::CancelPremove,
                operation,
            );
        }
        let can_resign = matches!(
            snapshot.phase(),
            GameEnumPhase::Active | GameEnumPhase::Interrupted
        ) && !is_busy
            && !snapshot.is_save_required();
        match snapshot.seats().mode() {
            GameEnumMode::HumanVersusEngine => {
                if ui
                    .add_enabled(can_resign, egui::Button::new("Resign"))
                    .clicked()
                {
                    self.resignation = Some(PresentationGamePlayIntent::new(
                        snapshot.id(),
                        snapshot.revision(),
                        PresentationGamePlayEnumOperation::Resign {
                            resigning: Self::human_color(snapshot),
                        },
                    ));
                }
            }
            GameEnumMode::LocalTwoPlayer => {
                ui.horizontal_wrapped(|ui| {
                    for resigning in [BoardEnumColor::White, BoardEnumColor::Black] {
                        if ui
                            .add_enabled(
                                can_resign,
                                egui::Button::new(Self::resign_label(resigning)),
                            )
                            .clicked()
                        {
                            self.resignation = Some(PresentationGamePlayIntent::new(
                                snapshot.id(),
                                snapshot.revision(),
                                PresentationGamePlayEnumOperation::Resign { resigning },
                            ));
                        }
                    }
                });
            }
        }
    }

    fn show_takeback_consent(
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        is_enabled: bool,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        let Some(pending) = snapshot.takeback_pending() else {
            return;
        };
        let requester = pending.requester();
        let responder = requester.opposite();
        ui.label(format!(
            "{} requests a takeback.",
            Self::color_label(requester)
        ));
        ui.small("Clocks keep running until the request is accepted.");
        ui.horizontal_wrapped(|ui| {
            for response in [
                GameTakebackEnumResponse::Accept,
                GameTakebackEnumResponse::Decline,
                GameTakebackEnumResponse::Cancel,
            ] {
                let responder = match response {
                    GameTakebackEnumResponse::Accept | GameTakebackEnumResponse::Decline => {
                        responder
                    }
                    GameTakebackEnumResponse::Cancel => requester,
                };
                Self::action(
                    ui,
                    Self::response_label(responder, response),
                    is_enabled,
                    PresentationGamePlayEnumOperation::RespondTakeback {
                        responder,
                        response,
                    },
                    operation,
                );
            }
        });
    }

    fn show_claims(
        &mut self,
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        is_enabled: bool,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        let claimant = snapshot.live_turn();
        let human_turn = snapshot.seats().seat(claimant) == GameEnumSeat::Human;
        let can_claim = is_enabled && human_turn && snapshot.phase() == GameEnumPhase::Active;
        ui.horizontal_wrapped(|ui| {
            for (kind, label, _) in Self::DRAW_CLAIMS {
                Self::action(
                    ui,
                    label,
                    can_claim && snapshot.claimable().contains(kind),
                    PresentationGamePlayEnumOperation::ClaimDraw {
                        claimant,
                        kind,
                        intent: GameDrawClaimEnumIntent::Current,
                    },
                    operation,
                );
            }
        });
        ui.add_enabled_ui(can_claim && snapshot.is_viewing_live(), |ui| {
            let variant = snapshot.board().variant();
            let movement_label = |movement: Option<BoardMove>| {
                movement.map_or_else(
                    || "Choose intended move".to_owned(),
                    |movement| movement.uci(variant),
                )
            };
            egui::ComboBox::from_label("Intended Move")
                .selected_text(movement_label(self.intended_move))
                .show_ui(ui, |ui| {
                    let movements = std::iter::once(None).chain(
                        snapshot
                            .board()
                            .legal_moves()
                            .iter()
                            .map(|movement| Some(movement.movement())),
                    );
                    for movement in movements {
                        if ui
                            .selectable_value(
                                &mut self.intended_move,
                                movement,
                                movement_label(movement),
                            )
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
            ui.horizontal_wrapped(|ui| {
                for (kind, _, label) in Self::DRAW_CLAIMS {
                    if ui
                        .add_enabled(self.intended_move.is_some(), egui::Button::new(label))
                        .clicked()
                        && let Some(movement) = self.intended_move
                    {
                        *operation = Some(PresentationGamePlayEnumOperation::ClaimDraw {
                            claimant,
                            kind,
                            intent: GameDrawClaimEnumIntent::Intended(movement),
                        });
                    }
                }
            });
        });
    }

    fn show_resignation(
        &mut self,
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        is_busy: bool,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        let Some(intent) = self.resignation else {
            return;
        };
        ui.group(|ui| {
            ui.strong("Resign this game?");
            if let PresentationGamePlayEnumOperation::Resign { resigning } = intent.operation() {
                ui.label(format!(
                    "{} will resign the live game.",
                    Self::color_label(resigning)
                ));
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !is_busy && !snapshot.is_save_required(),
                        egui::Button::new("Confirm Resignation"),
                    )
                    .clicked()
                {
                    *operation = Some(intent.operation());
                    self.resignation = None;
                }
                if ui.button("Keep Playing").clicked() {
                    self.resignation = None;
                }
            });
        });
    }

    fn show_moves(
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        is_busy: bool,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        ui.strong("Moves");
        if !snapshot.is_viewing_live() {
            Self::action(
                ui,
                "Return to Live",
                !is_busy,
                PresentationGamePlayEnumOperation::ReturnToLive,
                operation,
            );
        }
        egui::ScrollArea::vertical()
            .id_salt("live_game_move_list")
            .max_height(PresentationThemeTokens::PANEL_WIDTH)
            .show(ui, |ui| {
                ui.add_enabled_ui(!is_busy, |ui| {
                    if ui
                        .selectable_label(snapshot.view() == snapshot.root(), "Starting Position")
                        .clicked()
                    {
                        *operation =
                            Some(PresentationGamePlayEnumOperation::Navigate(snapshot.root()));
                    }
                    for movement in snapshot.moves() {
                        let separator = match movement.mover() {
                            BoardEnumColor::White => ".",
                            BoardEnumColor::Black => "...",
                        };
                        let label = format!(
                            "{}{separator} {}",
                            movement.number(),
                            PresentationBoardFigurines::display(
                                ui.ctx(),
                                movement.san(),
                                Some(movement.mover())
                            )
                        );
                        if ui
                            .selectable_label(snapshot.view() == movement.id(), label)
                            .clicked()
                        {
                            *operation =
                                Some(PresentationGamePlayEnumOperation::Navigate(movement.id()));
                        }
                    }
                });
            });
    }

    fn action(
        ui: &mut Ui,
        label: &str,
        is_enabled: bool,
        requested: PresentationGamePlayEnumOperation,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        if ui
            .add_enabled(is_enabled, egui::Button::new(label))
            .clicked()
        {
            *operation = Some(requested);
        }
    }

    fn human_color(snapshot: &DesktopGameSnapshot) -> BoardEnumColor {
        if snapshot.seats().seat(BoardEnumColor::White) == GameEnumSeat::Human {
            BoardEnumColor::White
        } else {
            BoardEnumColor::Black
        }
    }

    const fn color_label(color: BoardEnumColor) -> &'static str {
        match color {
            BoardEnumColor::White => "White",
            BoardEnumColor::Black => "Black",
        }
    }

    const fn resign_label(color: BoardEnumColor) -> &'static str {
        match color {
            BoardEnumColor::White => "White: Resign",
            BoardEnumColor::Black => "Black: Resign",
        }
    }

    const fn takeback_label(color: BoardEnumColor) -> &'static str {
        match color {
            BoardEnumColor::White => "White: Request Takeback",
            BoardEnumColor::Black => "Black: Request Takeback",
        }
    }

    const fn response_label(
        color: BoardEnumColor,
        response: GameTakebackEnumResponse,
    ) -> &'static str {
        match (color, response) {
            (BoardEnumColor::White, GameTakebackEnumResponse::Accept) => "White: Accept Takeback",
            (BoardEnumColor::Black, GameTakebackEnumResponse::Accept) => "Black: Accept Takeback",
            (BoardEnumColor::White, GameTakebackEnumResponse::Decline) => "White: Decline Takeback",
            (BoardEnumColor::Black, GameTakebackEnumResponse::Decline) => "Black: Decline Takeback",
            (BoardEnumColor::White, GameTakebackEnumResponse::Cancel) => "White: Cancel Request",
            (BoardEnumColor::Black, GameTakebackEnumResponse::Cancel) => "Black: Cancel Request",
        }
    }

    const fn retry_label(fault: GamePauseEnumFault) -> &'static str {
        match fault {
            GamePauseEnumFault::Engine => "Retry Engine",
            GamePauseEnumFault::Graphics => "Retry Renderer",
            GamePauseEnumFault::Save => "Retry Save",
        }
    }

    const fn interruption_label(interruption: GameSessionEnumInterruption) -> &'static str {
        match interruption {
            GameSessionEnumInterruption::PendingResult => {
                "Result pending. Retry Save to accept and save the result."
            }
            GameSessionEnumInterruption::Adjudication { .. } => {
                "Game paused: the mating proof is unresolved. Retry to run the bounded proof again."
            }
            GameSessionEnumInterruption::RecoveryRequired(GamePauseEnumFault::Engine) => {
                "Game paused after an engine failure. Retry Engine, then explicitly Resume."
            }
            GameSessionEnumInterruption::RecoveryRequired(GamePauseEnumFault::Graphics) => {
                "Game paused after a graphics failure. Retry Renderer, then explicitly Resume."
            }
            GameSessionEnumInterruption::RecoveryRequired(GamePauseEnumFault::Save) => {
                "Game paused after a save failure. Retry Save, then explicitly Resume."
            }
            GameSessionEnumInterruption::Recovered(_) => {
                "Recovery succeeded. The game remains paused until you Resume."
            }
            GameSessionEnumInterruption::Paused(GamePauseEnumCause::Manual) => {
                "Game paused manually. Resume when ready."
            }
            GameSessionEnumInterruption::Paused(GamePauseEnumCause::ComputerSleep) => {
                "Game paused for computer sleep. Resume when ready."
            }
            GameSessionEnumInterruption::Paused(GamePauseEnumCause::Relaunch) => {
                "Saved game restored paused. Resume when ready."
            }
            GameSessionEnumInterruption::Paused(GamePauseEnumCause::AutosaveFailure) => {
                "Game paused after an autosave failure. Resume after saving succeeds."
            }
            GameSessionEnumInterruption::Paused(
                GamePauseEnumCause::TabClosed
                | GamePauseEnumCause::ApplicationQuit
                | GamePauseEnumCause::UpdateInstall,
            ) => "Saved game paused. Resume when ready.",
            GameSessionEnumInterruption::Paused(GamePauseEnumCause::Fault(_)) => {
                "Game paused after a resource failure."
            }
        }
    }

    const fn outcome_reason(reason: GameEnumOutcomeReason) -> &'static str {
        match reason {
            GameEnumOutcomeReason::Checkmate => "Checkmate",
            GameEnumOutcomeReason::Stalemate => "Stalemate",
            GameEnumOutcomeReason::DeadPosition => "Dead position",
            GameEnumOutcomeReason::FivefoldRepetition => "Fivefold repetition",
            GameEnumOutcomeReason::SeventyFiveMove => "Seventy-five-move rule",
            GameEnumOutcomeReason::ThreefoldRepetition => "Claimed threefold repetition",
            GameEnumOutcomeReason::FiftyMove => "Claimed fifty-move draw",
            GameEnumOutcomeReason::Resignation => "Resignation",
            GameEnumOutcomeReason::Flagfall => "Time forfeit",
            GameEnumOutcomeReason::Agreement => "Draw agreed by both players",
            GameEnumOutcomeReason::ConsensualAdjudication => "Result confirmed by both players",
        }
    }

    const fn piece_label(kind: BoardEnumPieceKind) -> &'static str {
        match kind {
            BoardEnumPieceKind::Pawn => "pawn",
            BoardEnumPieceKind::Knight => "knight",
            BoardEnumPieceKind::Bishop => "bishop",
            BoardEnumPieceKind::Rook => "rook",
            BoardEnumPieceKind::Queen => "queen",
            BoardEnumPieceKind::King => "king",
        }
    }
}
