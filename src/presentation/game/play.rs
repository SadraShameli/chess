use std::time::Duration;

use bevy_egui::egui::{
    self, Align2, Color32, CornerRadius, FontId, Rect, Response, RichText, Sense, Stroke, Ui, Vec2,
};
use thiserror::Error;

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::r#move::BoardMove;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::state::{ClockState, ClockStateEnumStatus, ClockStateError, ClockStateView};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::game_snapshot::DesktopGameSnapshotMove;
use crate::game::draw_claim::{GameDrawClaimEnumIntent, GameDrawClaimEnumKind};
use crate::game::enums::{
    GameEnumMode, GameEnumOutcomeReason, GameEnumPhase, GameEnumResult, GameEnumSeat, GameId,
    GameMoveId, GameRevision,
};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use crate::game::session::GameSessionEnumInterruption;
use crate::game::takeback::GameTakebackEnumResponse;
use crate::presentation::board::figurines::PresentationBoardFigurines;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
    PresentationThemeEnumTone,
};
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PresentationGamePlayEnumTab {
    #[default]
    Moves,
    Info,
    Notes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationGamePlayEnumMark {
    Icon(PresentationThemeEnumIcon),
    Glyph(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationGamePlayEnumTileOutcome {
    Operation(PresentationGamePlayEnumOperation),
    Resign(BoardEnumColor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PresentationGamePlayTile {
    label: &'static str,
    mark: PresentationGamePlayEnumMark,
    is_enabled: bool,
    outcome: PresentationGamePlayEnumTileOutcome,
}

impl PresentationGamePlayTile {
    const fn operation(
        label: &'static str,
        mark: PresentationGamePlayEnumMark,
        is_enabled: bool,
        operation: PresentationGamePlayEnumOperation,
    ) -> Self {
        Self {
            label,
            mark,
            is_enabled,
            outcome: PresentationGamePlayEnumTileOutcome::Operation(operation),
        }
    }

    const fn resign(label: &'static str, is_enabled: bool, resigning: BoardEnumColor) -> Self {
        Self {
            label,
            mark: PresentationGamePlayEnumMark::Icon(PresentationThemeEnumIcon::Resign),
            is_enabled,
            outcome: PresentationGamePlayEnumTileOutcome::Resign(resigning),
        }
    }
}

struct PresentationGamePlayMoveRow<'a> {
    number: u32,
    white: Option<&'a DesktopGameSnapshotMove>,
    black: Option<&'a DesktopGameSnapshotMove>,
}

#[derive(Default)]
pub struct PresentationGamePlay {
    tab: PresentationGamePlayEnumTab,
    resignation: Option<PresentationGamePlayIntent>,
    intended_move: Option<BoardMove>,
    claim_context: Option<PresentationGamePlayClaimContext>,
    submitted_frame: Option<(u64, GameId)>,
}

impl PresentationGamePlay {
    const NUMBER_COLUMN: f32 = 40.0;
    const MOVE_ROW: f32 = 30.0;
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
        let mut operation = None;
        ui.spacing_mut().item_spacing.y = PresentationThemeTokens::SPACE_2;
        Self::show_header(ui, snapshot, clock);
        Self::show_history_banner(ui, snapshot, is_busy, &mut operation);
        Self::show_status(ui, snapshot, clock);
        if is_busy {
            PresentationThemeComponents::caption(ui, "Waiting for the game to accept your action.");
        }
        PresentationThemeComponents::segmented(
            ui,
            &mut self.tab,
            &[
                (PresentationGamePlayEnumTab::Moves, "Moves"),
                (PresentationGamePlayEnumTab::Info, "Info"),
                (PresentationGamePlayEnumTab::Notes, "Notes"),
            ],
            true,
        );
        match self.tab {
            PresentationGamePlayEnumTab::Moves => {
                Self::show_moves(ui, snapshot, is_busy, &mut operation);
                Self::show_navigation(ui, snapshot, is_busy, &mut operation);
            }
            PresentationGamePlayEnumTab::Info => Self::show_info(ui, snapshot),
            PresentationGamePlayEnumTab::Notes => Self::show_notes(ui, snapshot),
        }
        self.show_players(ui, snapshot, clock);
        PresentationThemeComponents::divider(ui);
        self.show_controls(ui, snapshot, is_busy, &mut operation);
        self.show_resignation(ui, snapshot, is_busy, &mut operation);
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

    fn show_header(ui: &mut Ui, snapshot: &DesktopGameSnapshot, clock: ClockStateView) {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(Self::title(snapshot))
                    .size(PresentationThemeTypography::HEADING_POINTS)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, tone) = Self::run_state(snapshot, clock);
                PresentationThemeComponents::pill(ui, text, tone);
            });
        });
        let known = snapshot.metadata().known();
        let opening = match (known.opening.as_ref(), known.variation.as_ref()) {
            (Some(opening), Some(variation)) => {
                Some(format!("{}, {}", opening.as_str(), variation.as_str()))
            }
            (Some(opening), None) => Some(opening.as_str().to_owned()),
            (None, Some(_) | None) => None,
        };
        let eco = known.eco.as_ref();
        if opening.is_some() || eco.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_1;
                if let Some(opening) = opening {
                    ui.label(
                        RichText::new(opening)
                            .size(PresentationThemeTypography::CAPTION_POINTS)
                            .color(colors.text_2),
                    );
                }
                if let Some(eco) = eco {
                    ui.label(
                        RichText::new(eco.as_str())
                            .size(PresentationThemeTypography::CAPTION_POINTS)
                            .monospace()
                            .color(colors.text_3),
                    );
                }
            });
        }
    }

    fn title(snapshot: &DesktopGameSnapshot) -> &'static str {
        match snapshot.seats().mode() {
            GameEnumMode::HumanVersusEngine => "vs Reckless",
            GameEnumMode::LocalTwoPlayer => "Local game",
        }
    }

    fn run_state(
        snapshot: &DesktopGameSnapshot,
        clock: ClockStateView,
    ) -> (&'static str, PresentationThemeEnumTone) {
        if snapshot.outcome().is_some() {
            return ("Game over", PresentationThemeEnumTone::Neutral);
        }
        if snapshot.is_save_required() {
            return ("Unsaved", PresentationThemeEnumTone::Unsaved);
        }
        if snapshot.interruption().is_some() {
            return ("Paused", PresentationThemeEnumTone::Low);
        }
        if snapshot.phase() != GameEnumPhase::Active {
            return ("Not running", PresentationThemeEnumTone::Neutral);
        }
        let turn = snapshot.live_turn();
        let is_low = PresentationGamePlayClock::is_low_time(clock.side(turn).displayed());
        match (snapshot.seats().mode(), snapshot.seats().seat(turn), is_low) {
            (GameEnumMode::HumanVersusEngine, GameEnumSeat::Human, false) => {
                ("Your move", PresentationThemeEnumTone::Run)
            }
            (GameEnumMode::HumanVersusEngine, GameEnumSeat::Human, true) => {
                ("Your move", PresentationThemeEnumTone::Low)
            }
            (GameEnumMode::HumanVersusEngine, GameEnumSeat::Reckless, _) => {
                ("Reckless is thinking", PresentationThemeEnumTone::Accent)
            }
            (GameEnumMode::LocalTwoPlayer, _, true) => match turn {
                BoardEnumColor::White => ("White to move", PresentationThemeEnumTone::Low),
                BoardEnumColor::Black => ("Black to move", PresentationThemeEnumTone::Low),
            },
            (GameEnumMode::LocalTwoPlayer, _, false) => match turn {
                BoardEnumColor::White => ("White to move", PresentationThemeEnumTone::Run),
                BoardEnumColor::Black => ("Black to move", PresentationThemeEnumTone::Run),
            },
        }
    }

    fn show_history_banner(
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        is_busy: bool,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        if snapshot.is_viewing_live() {
            return;
        }
        let colors = PresentationThemeColors::of(ui.ctx());
        PresentationThemeComponents::card(ui)
            .fill(colors.surface_2)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    PresentationThemeIcon::show(
                        ui,
                        PresentationThemeEnumIcon::Clock,
                        PresentationThemeTypography::BODY_POINTS,
                        colors.accent,
                    );
                    ui.strong("Viewing history");
                });
                let viewed = snapshot
                    .moves()
                    .iter()
                    .find(|movement| movement.id() == snapshot.view())
                    .map_or_else(
                        || "the starting position".to_owned(),
                        |movement| Self::move_text(ui.ctx(), movement),
                    );
                ui.label(format!("Viewing {viewed}. The game continues live."));
                PresentationThemeComponents::caption(
                    ui,
                    "The live game, its clocks, and Reckless continue independently.",
                );
                if ui
                    .add_enabled(
                        !is_busy,
                        PresentationThemeEnumButton::Primary.widget(
                            ui,
                            "Return to Live",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
                    .clicked()
                {
                    *operation = Some(PresentationGamePlayEnumOperation::ReturnToLive);
                }
            });
    }

    fn move_text(context: &egui::Context, movement: &DesktopGameSnapshotMove) -> String {
        let separator = match movement.mover() {
            BoardEnumColor::White => ".",
            BoardEnumColor::Black => "\u{2026}",
        };
        format!(
            "{}{separator}{}",
            movement.number(),
            PresentationBoardFigurines::display(context, movement.san(), Some(movement.mover()))
        )
    }

    fn show_info(ui: &mut Ui, snapshot: &DesktopGameSnapshot) {
        let known = snapshot.metadata().known();
        let rows = [
            (
                "Event",
                known.event.as_ref().map(|text| text.as_str().to_owned()),
            ),
            (
                "Site",
                known.site.as_ref().map(|text| text.as_str().to_owned()),
            ),
            (
                "Round",
                known.round.as_ref().map(|text| text.as_str().to_owned()),
            ),
            (
                "Time control",
                Some(if snapshot.clock().control().is_timed() {
                    "Timed".to_owned()
                } else {
                    "Untimed".to_owned()
                }),
            ),
            ("Moves played", Some(snapshot.moves().len().to_string())),
        ];
        egui::Grid::new("live_game_info")
            .num_columns(2)
            .spacing([
                PresentationThemeTokens::SPACE_3,
                PresentationThemeTokens::SPACE_1,
            ])
            .show(ui, |ui| {
                for (name, value) in rows {
                    let Some(value) = value else {
                        continue;
                    };
                    PresentationThemeComponents::caption(ui, name);
                    ui.label(value);
                    ui.end_row();
                }
            });
    }

    fn show_notes(ui: &mut Ui, snapshot: &DesktopGameSnapshot) {
        match snapshot.metadata().known().annotator.as_ref() {
            Some(annotator) => {
                ui.label(format!("Annotated by {}", annotator.as_str()));
            }
            None => {
                PresentationThemeComponents::caption(ui, "No notes for this game yet.");
            }
        }
    }

    fn show_navigation(
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        is_busy: bool,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        let moves = snapshot.moves();
        let index = moves
            .iter()
            .position(|movement| movement.id() == snapshot.view());
        let previous = match index {
            None => None,
            Some(0) => Some(snapshot.root()),
            Some(index) => moves
                .get(index.saturating_sub(1))
                .map(DesktopGameSnapshotMove::id),
        };
        let next = match index {
            None => moves.first(),
            Some(index) => moves.get(index.saturating_add(1)),
        }
        .map(DesktopGameSnapshotMove::id);
        let last = moves.last().map(DesktopGameSnapshotMove::id);
        let first = (snapshot.view() != snapshot.root()).then_some(snapshot.root());
        PresentationThemeComponents::divider(ui);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_1;
            for (icon, label, target) in [
                (PresentationThemeEnumIcon::First, "First move", first),
                (
                    PresentationThemeEnumIcon::Previous,
                    "Previous move",
                    previous,
                ),
                (PresentationThemeEnumIcon::Next, "Next move", next),
                (
                    PresentationThemeEnumIcon::Last,
                    "Last move",
                    last.filter(|last| *last != snapshot.view()),
                ),
            ] {
                let response = ui
                    .add_enabled_ui(!is_busy && target.is_some(), |ui| {
                        PresentationThemeIcon::button(ui, icon, label)
                    })
                    .inner;
                if response.clicked()
                    && let Some(target) = target
                {
                    *operation = Some(PresentationGamePlayEnumOperation::Navigate(target));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                PresentationThemeComponents::caption(
                    ui,
                    if snapshot.has_unsaved_changes() {
                        "Saving to library"
                    } else {
                        "Saved to library"
                    },
                );
            });
        });
    }

    fn show_players(&self, ui: &mut Ui, snapshot: &DesktopGameSnapshot, clock: ClockStateView) {
        let colors = PresentationThemeColors::of(ui.ctx());
        PresentationThemeComponents::divider(ui);
        for color in [BoardEnumColor::Black, BoardEnumColor::White] {
            let side = clock.side(color);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_2;
                ui.label(
                    RichText::new(Self::color_label(color))
                        .size(PresentationThemeTypography::CAPTION_POINTS)
                        .color(colors.text_3),
                );
                let name = match color {
                    BoardEnumColor::White => snapshot.metadata().known().white.as_ref(),
                    BoardEnumColor::Black => snapshot.metadata().known().black.as_ref(),
                };
                if let Some(name) = name {
                    ui.label(RichText::new(name.as_str()).strong());
                } else {
                    ui.label(
                        RichText::new(match snapshot.seats().seat(color) {
                            GameEnumSeat::Human => "Human",
                            GameEnumSeat::Reckless => "Reckless",
                        })
                        .strong(),
                    );
                }
                if clock.active() == color && clock.status() == ClockStateEnumStatus::Running {
                    PresentationThemeComponents::pill(
                        ui,
                        "To move",
                        PresentationThemeEnumTone::Run,
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let is_low = PresentationGamePlayClock::is_low_time(side.displayed());
                    let text = RichText::new(PresentationGamePlayClock::text(side.displayed()))
                        .monospace()
                        .size(PresentationThemeTypography::BODY_POINTS)
                        .strong()
                        .color(if is_low { colors.low } else { colors.text });
                    ui.label(text);
                    if is_low {
                        PresentationThemeComponents::pill(
                            ui,
                            "Low time",
                            PresentationThemeEnumTone::Low,
                        );
                    }
                });
            });
            if side.delay_remaining() != ClockDuration::ZERO {
                PresentationThemeComponents::caption(
                    ui,
                    format!(
                        "Delay: {}",
                        PresentationGamePlayClock::text(Some(side.delay_remaining()))
                    ),
                );
            }
            if side.pending_refund() != ClockDuration::ZERO {
                PresentationThemeComponents::caption(
                    ui,
                    format!(
                        "Pending Bronstein refund: {}",
                        PresentationGamePlayClock::text(Some(side.pending_refund()))
                    ),
                );
            }
        }
    }

    fn show_status(ui: &mut Ui, snapshot: &DesktopGameSnapshot, clock: ClockStateView) {
        let colors = PresentationThemeColors::of(ui.ctx());
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
            ui.colored_label(colors.err, message);
        } else if let Some(interruption) = snapshot.interruption() {
            ui.label(Self::interruption_label(interruption));
        } else {
            let text = match snapshot.phase() {
                GameEnumPhase::Active => {
                    if matches!(clock.status(), ClockStateEnumStatus::Flagged(_)) {
                        "Clock expired. Waiting for the native game result."
                    } else {
                        "Game in progress"
                    }
                }
                GameEnumPhase::Setup => "Game setup",
                GameEnumPhase::Interrupted => "Game interrupted",
                GameEnumPhase::Completed => "Game completed",
                GameEnumPhase::Analysis => "Game review",
            };
            PresentationThemeComponents::caption(ui, text);
        }
    }

    fn show_controls(
        &mut self,
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        is_busy: bool,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        let can_act = !is_busy && !snapshot.is_save_required();
        let mut tiles = Vec::new();
        let can_takeback = snapshot.takeback_target().is_some()
            && snapshot.takeback_pending().is_none()
            && can_act;
        if snapshot.takeback_pending().is_none() {
            match snapshot.seats().mode() {
                GameEnumMode::HumanVersusEngine => tiles.push(PresentationGamePlayTile::operation(
                    "Takeback",
                    PresentationGamePlayEnumMark::Icon(PresentationThemeEnumIcon::Takeback),
                    can_takeback,
                    PresentationGamePlayEnumOperation::Takeback {
                        requester: Self::human_color(snapshot),
                    },
                )),
                GameEnumMode::LocalTwoPlayer => {
                    for requester in [BoardEnumColor::White, BoardEnumColor::Black] {
                        tiles.push(PresentationGamePlayTile::operation(
                            Self::takeback_label(requester),
                            PresentationGamePlayEnumMark::Icon(PresentationThemeEnumIcon::Takeback),
                            can_takeback,
                            PresentationGamePlayEnumOperation::Takeback { requester },
                        ));
                    }
                }
            }
        }
        if let Some((label, action, icon)) = Self::recovery(snapshot) {
            tiles.push(PresentationGamePlayTile::operation(
                label,
                PresentationGamePlayEnumMark::Icon(icon),
                !is_busy,
                action,
            ));
        } else if snapshot.phase() == GameEnumPhase::Active {
            tiles.push(PresentationGamePlayTile::operation(
                "Pause Game",
                PresentationGamePlayEnumMark::Icon(PresentationThemeEnumIcon::Pause),
                !is_busy,
                PresentationGamePlayEnumOperation::Pause,
            ));
        }
        let claimant = snapshot.live_turn();
        let can_claim = can_act
            && snapshot.seats().seat(claimant) == GameEnumSeat::Human
            && snapshot.phase() == GameEnumPhase::Active;
        for (kind, label, _) in Self::DRAW_CLAIMS {
            tiles.push(PresentationGamePlayTile::operation(
                label,
                PresentationGamePlayEnumMark::Glyph("\u{bd}"),
                can_claim && snapshot.claimable().contains(kind),
                PresentationGamePlayEnumOperation::ClaimDraw {
                    claimant,
                    kind,
                    intent: GameDrawClaimEnumIntent::Current,
                },
            ));
        }
        let can_resign = matches!(
            snapshot.phase(),
            GameEnumPhase::Active | GameEnumPhase::Interrupted
        ) && can_act;
        match snapshot.seats().mode() {
            GameEnumMode::HumanVersusEngine => tiles.push(PresentationGamePlayTile::resign(
                "Resign",
                can_resign,
                Self::human_color(snapshot),
            )),
            GameEnumMode::LocalTwoPlayer => {
                for resigning in [BoardEnumColor::White, BoardEnumColor::Black] {
                    tiles.push(PresentationGamePlayTile::resign(
                        Self::resign_label(resigning),
                        can_resign,
                        resigning,
                    ));
                }
            }
        }
        self.show_tiles(ui, snapshot, &tiles, operation);
        if snapshot.takeback_pending().is_some() {
            Self::show_takeback_consent(ui, snapshot, can_act, operation);
        }
        self.show_intended_claims(ui, snapshot, can_claim, operation);
        if let Some(premove) = snapshot.premove() {
            PresentationThemeComponents::card(ui)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
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
                        can_act,
                        PresentationGamePlayEnumOperation::CancelPremove,
                        operation,
                    );
                });
        }
    }

    fn recovery(
        snapshot: &DesktopGameSnapshot,
    ) -> Option<(
        &'static str,
        PresentationGamePlayEnumOperation,
        PresentationThemeEnumIcon,
    )> {
        if snapshot.is_save_required() {
            return Some((
                "Retry Save",
                PresentationGamePlayEnumOperation::Retry,
                PresentationThemeEnumIcon::ResetCamera,
            ));
        }
        snapshot
            .interruption()
            .map(|interruption| match interruption {
                GameSessionEnumInterruption::RecoveryRequired(fault) => (
                    Self::retry_label(fault),
                    PresentationGamePlayEnumOperation::Retry,
                    PresentationThemeEnumIcon::ResetCamera,
                ),
                GameSessionEnumInterruption::Adjudication { .. } => (
                    "Retry Adjudication",
                    PresentationGamePlayEnumOperation::Retry,
                    PresentationThemeEnumIcon::ResetCamera,
                ),
                GameSessionEnumInterruption::Paused(_)
                | GameSessionEnumInterruption::Recovered(_) => (
                    "Resume Game",
                    PresentationGamePlayEnumOperation::Resume,
                    PresentationThemeEnumIcon::Play,
                ),
                GameSessionEnumInterruption::PendingResult => (
                    "Retry Save",
                    PresentationGamePlayEnumOperation::Retry,
                    PresentationThemeEnumIcon::ResetCamera,
                ),
            })
    }

    fn show_tiles(
        &mut self,
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        tiles: &[PresentationGamePlayTile],
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        let gap = PresentationThemeTokens::SPACE_2;
        let width = ((ui.available_width() - gap) / 2.0).max(PresentationThemeTokens::CONTROL_LG);
        ui.spacing_mut().item_spacing = Vec2::splat(gap);
        for row in tiles.chunks(2) {
            ui.horizontal(|ui| {
                for tile in row {
                    if Self::tile(ui, tile, width).clicked() {
                        match tile.outcome {
                            PresentationGamePlayEnumTileOutcome::Operation(requested) => {
                                *operation = Some(requested);
                            }
                            PresentationGamePlayEnumTileOutcome::Resign(resigning) => {
                                self.resignation = Some(PresentationGamePlayIntent::new(
                                    snapshot.id(),
                                    snapshot.revision(),
                                    PresentationGamePlayEnumOperation::Resign { resigning },
                                ));
                            }
                        }
                    }
                }
            });
        }
    }

    fn tile(ui: &mut Ui, tile: &PresentationGamePlayTile, width: f32) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.add_enabled_ui(tile.is_enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(width, PresentationThemeTokens::CONTROL_MD),
                Sense::click(),
            );
            let is_danger = matches!(tile.outcome, PresentationGamePlayEnumTileOutcome::Resign(_));
            let text = match (is_danger, tile.is_enabled) {
                (_, false) => colors.text_3,
                (true, true) => colors.err,
                (false, true) => colors.text,
            };
            let fill = if response.hovered() && tile.is_enabled {
                colors.surface_3
            } else {
                colors.surface_2
            };
            ui.painter().rect(
                rect,
                CornerRadius::same(PresentationThemeTokens::RADIUS_MD),
                fill,
                Stroke::new(1.0, colors.line),
                egui::StrokeKind::Inside,
            );
            let font = FontId::proportional(PresentationThemeTypography::UI_POINTS);
            let galley = ui
                .painter()
                .layout_no_wrap(tile.label.to_owned(), font, text);
            let mark = PresentationThemeTypography::BODY_POINTS;
            let total = mark + PresentationThemeTokens::SPACE_2 + galley.size().x;
            let left = rect.center().x - total / 2.0;
            let mark_rect = Rect::from_center_size(
                egui::pos2(left + mark / 2.0, rect.center().y),
                Vec2::splat(mark),
            );
            match tile.mark {
                PresentationGamePlayEnumMark::Icon(icon) => {
                    PresentationThemeIcon::paint(ui.painter(), mark_rect, icon, text);
                }
                PresentationGamePlayEnumMark::Glyph(glyph) => {
                    ui.painter().text(
                        mark_rect.center(),
                        Align2::CENTER_CENTER,
                        glyph,
                        FontId::proportional(PresentationThemeTypography::BODY_POINTS),
                        text,
                    );
                }
            }
            ui.painter().galley(
                egui::pos2(
                    left + mark + PresentationThemeTokens::SPACE_2,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                text,
            );
            let label = tile.label.to_owned();
            let is_enabled = tile.is_enabled;
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, is_enabled, label.clone())
            });
            response
        })
        .inner
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

    fn show_intended_claims(
        &mut self,
        ui: &mut Ui,
        snapshot: &DesktopGameSnapshot,
        can_claim: bool,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        let claimant = snapshot.live_turn();
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
        PresentationThemeComponents::card(ui).show(ui, |ui| {
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
                        PresentationThemeEnumButton::DangerSolid.widget(
                            ui,
                            "Confirm Resignation",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
                    .clicked()
                {
                    *operation = Some(intent.operation());
                    self.resignation = None;
                }
                if PresentationThemeEnumButton::Ghost
                    .show(ui, "Keep Playing")
                    .clicked()
                {
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
        let mut rows: Vec<PresentationGamePlayMoveRow<'_>> = Vec::new();
        for movement in snapshot.moves() {
            match (movement.mover(), rows.last_mut()) {
                (BoardEnumColor::Black, Some(row))
                    if row.number == movement.number() && row.black.is_none() =>
                {
                    row.black = Some(movement);
                }
                (BoardEnumColor::White, _) => rows.push(PresentationGamePlayMoveRow {
                    number: movement.number(),
                    white: Some(movement),
                    black: None,
                }),
                (BoardEnumColor::Black, _) => rows.push(PresentationGamePlayMoveRow {
                    number: movement.number(),
                    white: None,
                    black: Some(movement),
                }),
            }
        }
        egui::ScrollArea::vertical()
            .id_salt("live_game_move_list")
            .max_height(PresentationThemeTokens::PANEL_WIDTH)
            .show(ui, |ui| {
                ui.add_enabled_ui(!is_busy, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
                    let width = ui.available_width();
                    let cell = ((width - Self::NUMBER_COLUMN) / 2.0).max(0.0);
                    if Self::move_cell(
                        ui,
                        "Starting Position",
                        "Starting Position",
                        snapshot.view() == snapshot.root(),
                        width,
                    )
                    .clicked()
                    {
                        *operation =
                            Some(PresentationGamePlayEnumOperation::Navigate(snapshot.root()));
                    }
                    for row in rows {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let colors = PresentationThemeColors::of(ui.ctx());
                            let (number, _) = ui.allocate_exact_size(
                                Vec2::new(Self::NUMBER_COLUMN, Self::MOVE_ROW),
                                Sense::hover(),
                            );
                            ui.painter().text(
                                number.left_center() + Vec2::new(8.0, 0.0),
                                Align2::LEFT_CENTER,
                                format!("{}.", row.number),
                                FontId::proportional(PresentationThemeTypography::CAPTION_POINTS),
                                colors.text_3,
                            );
                            for movement in [row.white, row.black] {
                                let Some(movement) = movement else {
                                    ui.allocate_exact_size(
                                        Vec2::new(cell, Self::MOVE_ROW),
                                        Sense::hover(),
                                    );
                                    continue;
                                };
                                let separator = match movement.mover() {
                                    BoardEnumColor::White => ".",
                                    BoardEnumColor::Black => "...",
                                };
                                let display = PresentationBoardFigurines::display(
                                    ui.ctx(),
                                    movement.san(),
                                    Some(movement.mover()),
                                );
                                let label = format!("{}{separator} {display}", movement.number());
                                if Self::move_cell(
                                    ui,
                                    &display,
                                    &label,
                                    snapshot.view() == movement.id(),
                                    cell,
                                )
                                .clicked()
                                {
                                    *operation = Some(PresentationGamePlayEnumOperation::Navigate(
                                        movement.id(),
                                    ));
                                }
                            }
                        });
                    }
                });
            });
    }

    fn move_cell(ui: &mut Ui, text: &str, label: &str, selected: bool, width: f32) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, Self::MOVE_ROW), Sense::click());
        let fill = if selected {
            colors.accent_soft
        } else if response.hovered() {
            colors.surface_2
        } else {
            Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, PresentationThemeTokens::RADIUS_MD, fill);
        let font = FontId::proportional(PresentationThemeTypography::BODY_POINTS);
        ui.painter().text(
            rect.left_center() + Vec2::new(8.0, 0.0),
            Align2::LEFT_CENTER,
            text,
            font,
            if selected { colors.accent } else { colors.text },
        );
        let label = label.to_owned();
        let is_enabled = ui.is_enabled();
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                is_enabled,
                selected,
                label.clone(),
            )
        });
        response
    }

    fn action(
        ui: &mut Ui,
        label: &str,
        is_enabled: bool,
        requested: PresentationGamePlayEnumOperation,
        operation: &mut Option<PresentationGamePlayEnumOperation>,
    ) {
        if ui
            .add_enabled(
                is_enabled,
                PresentationThemeEnumButton::Secondary.widget(
                    ui,
                    label,
                    PresentationThemeEnumSize::Small,
                ),
            )
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
