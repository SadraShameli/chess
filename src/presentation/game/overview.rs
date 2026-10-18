use bevy_egui::egui::{
    self, Align, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use strum::IntoEnumIterator;

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::clock::duration::ClockInstant;
use crate::clock::state::{ClockStateEnumStatus, ClockStateError};
use crate::desktop::game_snapshot::{DesktopGameSnapshot, DesktopGameSnapshotEnumAdmission};
use crate::game::configuration::GameConfigurationEnumPreset;
use crate::game::enums::{GameEnumMode, GameEnumPhase, GameEnumSeat, GameId};
use crate::presentation::game::play::PresentationGamePlayClock;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumTone,
};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::Display)]
pub enum PresentationGameOverviewEnumStatus {
    #[strum(serialize = "Running")]
    Running,
    #[strum(serialize = "Waiting for engine capacity")]
    WaitingForCapacity,
    #[strum(serialize = "Paused")]
    Paused,
    #[strum(serialize = "Finished")]
    Finished,
}

impl PresentationGameOverviewEnumStatus {
    pub fn label(self) -> &'static str {
        self.into()
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::WaitingForCapacity => "Queued",
            Self::Paused => "Paused",
            Self::Finished => "Finished",
        }
    }

    pub const fn tone(self) -> PresentationThemeEnumTone {
        match self {
            Self::Running => PresentationThemeEnumTone::Run,
            Self::WaitingForCapacity => PresentationThemeEnumTone::Low,
            Self::Paused | Self::Finished => PresentationThemeEnumTone::Neutral,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationGameOverviewPiece {
    square: u8,
    color: BoardEnumColor,
    kind: BoardEnumPieceKind,
}

impl PresentationGameOverviewPiece {
    const fn file(self) -> u8 {
        self.square % 8
    }

    const fn rank(self) -> u8 {
        self.square / 8
    }

    const fn glyph(self) -> &'static str {
        match self.kind {
            BoardEnumPieceKind::Pawn => "P",
            BoardEnumPieceKind::Knight => "N",
            BoardEnumPieceKind::Bishop => "B",
            BoardEnumPieceKind::Rook => "R",
            BoardEnumPieceKind::Queen => "Q",
            BoardEnumPieceKind::King => "K",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationGameOverviewRow {
    id: GameId,
    ordinal: usize,
    turn: BoardEnumColor,
    clock: String,
    status: PresentationGameOverviewEnumStatus,
    unsaved: bool,
    resources: Option<String>,
    control: &'static str,
    white_clock: String,
    black_clock: String,
    mode: GameEnumMode,
    white_seat: GameEnumSeat,
    black_seat: GameEnumSeat,
    moves: usize,
    pieces: Vec<PresentationGameOverviewPiece>,
}

impl PresentationGameOverviewRow {
    pub const fn id(&self) -> GameId {
        self.id
    }

    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    pub const fn turn(&self) -> BoardEnumColor {
        self.turn
    }

    pub fn clock(&self) -> &str {
        &self.clock
    }

    pub const fn status(&self) -> PresentationGameOverviewEnumStatus {
        self.status
    }

    pub const fn is_unsaved(&self) -> bool {
        self.unsaved
    }

    pub fn resources(&self) -> Option<&str> {
        self.resources.as_deref()
    }

    pub fn name(&self) -> String {
        format!("Game {}", self.ordinal)
    }

    pub const fn control(&self) -> &'static str {
        self.control
    }

    pub fn side_clock(&self, color: BoardEnumColor) -> &str {
        match color {
            BoardEnumColor::White => &self.white_clock,
            BoardEnumColor::Black => &self.black_clock,
        }
    }

    pub const fn player(&self, color: BoardEnumColor) -> &'static str {
        let seat = match color {
            BoardEnumColor::White => self.white_seat,
            BoardEnumColor::Black => self.black_seat,
        };
        match (seat, self.mode, color) {
            (GameEnumSeat::Reckless, _, _) => "Reckless",
            (GameEnumSeat::Human, GameEnumMode::HumanVersusEngine, _) => "You",
            (GameEnumSeat::Human, GameEnumMode::LocalTwoPlayer, BoardEnumColor::White) => {
                "White player"
            }
            (GameEnumSeat::Human, GameEnumMode::LocalTwoPlayer, BoardEnumColor::Black) => {
                "Black player"
            }
        }
    }

    pub const fn is_running(&self) -> bool {
        matches!(self.status, PresentationGameOverviewEnumStatus::Running)
    }

    pub fn is_waiting_on_you(&self) -> bool {
        let seat = match self.turn {
            BoardEnumColor::White => self.white_seat,
            BoardEnumColor::Black => self.black_seat,
        };
        self.mode == GameEnumMode::HumanVersusEngine
            && seat == GameEnumSeat::Human
            && self.status != PresentationGameOverviewEnumStatus::Finished
    }

    pub fn meta(&self) -> String {
        let mut meta = format!("{} \u{b7} {} moves", self.control, self.moves);
        if let Some(resources) = &self.resources {
            meta.push_str(" \u{b7} ");
            meta.push_str(resources);
        }
        meta
    }

    pub fn tab_label(&self) -> String {
        let mut label = format!("Game {}", self.ordinal);
        if self.unsaved {
            label.push_str(" *");
        }
        label
    }

    pub fn summary(&self) -> String {
        let turn = match self.turn {
            BoardEnumColor::White => "White",
            BoardEnumColor::Black => "Black",
        };
        let mut summary = format!(
            "{}: {} to move, {}, {}",
            self.tab_label(),
            turn,
            self.clock,
            self.status.label()
        );
        if self.unsaved {
            summary.push_str(", unsaved changes");
        }
        if let Some(resources) = &self.resources {
            summary.push_str(", ");
            summary.push_str(resources);
        }
        summary
    }
}

pub struct PresentationGameOverview;

impl PresentationGameOverview {
    pub fn rows<'a>(
        snapshots: impl IntoIterator<Item = &'a DesktopGameSnapshot>,
        now: ClockInstant,
    ) -> Result<Vec<PresentationGameOverviewRow>, ClockStateError> {
        let mut rows = Vec::new();
        for (index, snapshot) in snapshots.into_iter().enumerate() {
            rows.push(Self::row(index.saturating_add(1), snapshot, now)?);
        }
        Ok(rows)
    }

    pub fn row(
        ordinal: usize,
        snapshot: &DesktopGameSnapshot,
        now: ClockInstant,
    ) -> Result<PresentationGameOverviewRow, ClockStateError> {
        let view = snapshot.clock().view(now)?;
        let turn = view.active();
        let clock = PresentationGamePlayClock::text(view.side(turn).displayed());
        let white_clock =
            PresentationGamePlayClock::text(view.side(BoardEnumColor::White).displayed());
        let black_clock =
            PresentationGamePlayClock::text(view.side(BoardEnumColor::Black).displayed());
        let control = GameConfigurationEnumPreset::iter()
            .find(|preset| {
                preset
                    .time_control()
                    .is_ok_and(|control| &control == snapshot.clock().control())
            })
            .map_or("Custom", GameConfigurationEnumPreset::label);
        let seats = snapshot.seats();
        let pieces = snapshot
            .board()
            .pieces()
            .iter()
            .map(|piece| PresentationGameOverviewPiece {
                square: piece.square().into(),
                color: piece.color(),
                kind: piece.kind(),
            })
            .collect();
        let status = if snapshot.phase() == GameEnumPhase::Completed {
            PresentationGameOverviewEnumStatus::Finished
        } else if snapshot.admission() == DesktopGameSnapshotEnumAdmission::WaitingForCapacity {
            PresentationGameOverviewEnumStatus::WaitingForCapacity
        } else if snapshot.phase() == GameEnumPhase::Active
            && view.status() == ClockStateEnumStatus::Running
        {
            PresentationGameOverviewEnumStatus::Running
        } else {
            PresentationGameOverviewEnumStatus::Paused
        };
        let resources = snapshot.allocation().map(|allocation| {
            format!(
                "{} threads, {} MiB hash",
                allocation.workers(),
                allocation.hash_mib()
            )
        });
        Ok(PresentationGameOverviewRow {
            id: snapshot.id(),
            ordinal,
            turn,
            clock,
            status,
            unsaved: snapshot.has_unsaved_changes(),
            resources,
            control,
            white_clock,
            black_clock,
            mode: seats.mode(),
            white_seat: seats.seat(BoardEnumColor::White),
            black_seat: seats.seat(BoardEnumColor::Black),
            moves: snapshot.moves().len(),
            pieces,
        })
    }

    pub fn show(
        ui: &mut Ui,
        rows: &[PresentationGameOverviewRow],
    ) -> PresentationGameOverviewAction {
        let colors = PresentationThemeColors::of(ui.ctx());
        let mut action = PresentationGameOverviewAction::default();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                PresentationThemeComponents::title(ui, "All games");
                PresentationThemeComponents::caption(ui, Self::summary_line(rows));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.horizontal(|ui| {
                    PresentationThemeComponents::kbd(ui, "N");
                    if PresentationThemeEnumButton::Primary
                        .show(ui, "New game")
                        .clicked()
                    {
                        action.new_game = true;
                    }
                });
                if PresentationThemeEnumButton::Secondary
                    .show(ui, "Pause all")
                    .clicked()
                {
                    action.pause_all = true;
                }
            });
        });
        ui.add_space(PresentationThemeTokens::SPACE_5);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(PresentationThemeTokens::SPACE_4);
            for row in rows {
                if Self::show_card(ui, row, &colors).clicked() {
                    action.open = Some(row.id());
                }
            }
            if Self::show_new_card(ui, &colors).clicked() {
                action.new_game = true;
            }
        });
        action
    }

    fn summary_line(rows: &[PresentationGameOverviewRow]) -> String {
        let open = rows
            .iter()
            .filter(|row| row.status != PresentationGameOverviewEnumStatus::Finished)
            .count();
        let waiting = rows.iter().filter(|row| row.is_waiting_on_you()).count();
        let running = rows.iter().filter(|row| row.is_running()).count();
        format!("{open} open \u{b7} {waiting} waiting on you \u{b7} {running} running")
    }

    fn show_card(
        ui: &mut Ui,
        row: &PresentationGameOverviewRow,
        colors: &PresentationThemeColors,
    ) -> egui::Response {
        let inner = ui.allocate_ui_with_layout(
            Vec2::new(PresentationGameOverviewCard::WIDTH, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(PresentationGameOverviewCard::WIDTH);
                PresentationThemeComponents::card(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        PresentationThemeComponents::subhead(ui, row.name());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            PresentationThemeComponents::pill(
                                ui,
                                row.status.short_label(),
                                row.status.tone(),
                            );
                        });
                    });
                    ui.add_space(PresentationThemeTokens::SPACE_3);
                    PresentationGameOverviewCard::board(ui, row);
                    ui.add_space(PresentationThemeTokens::SPACE_3);
                    for color in [BoardEnumColor::Black, BoardEnumColor::White] {
                        PresentationGameOverviewCard::player(ui, row, color, colors);
                    }
                    ui.add_space(PresentationThemeTokens::SPACE_2);
                    PresentationThemeComponents::caption(ui, row.meta());
                });
            },
        );
        let response = ui.interact(
            inner.response.rect,
            ui.id().with(("overview_card", row.id())),
            Sense::click(),
        );
        let summary = row.summary();
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, summary.clone())
        });
        response
    }

    fn show_new_card(ui: &mut Ui, colors: &PresentationThemeColors) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(
                PresentationGameOverviewCard::WIDTH,
                PresentationGameOverviewCard::NEW_HEIGHT,
            ),
            Sense::click(),
        );
        let stroke_color = if response.hovered() {
            colors.accent
        } else {
            colors.line_strong
        };
        let path = [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.left_top(),
        ];
        ui.painter().extend(egui::Shape::dashed_line(
            &path,
            Stroke::new(1.0, stroke_color),
            6.0,
            4.0,
        ));
        ui.painter().text(
            rect.center() - Vec2::new(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            "New game",
            egui::FontId::proportional(PresentationThemeTypography::SUBHEAD_POINTS),
            colors.text,
        );
        ui.painter().text(
            rect.center() + Vec2::new(0.0, 12.0),
            egui::Align2::CENTER_CENTER,
            "Opens in a new tab",
            egui::FontId::proportional(PresentationThemeTypography::CAPTION_POINTS),
            colors.text_3,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                "New game, opens in a new tab",
            )
        });
        response
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PresentationGameOverviewAction {
    pub open: Option<GameId>,
    pub new_game: bool,
    pub pause_all: bool,
}

struct PresentationGameOverviewCard;

impl PresentationGameOverviewCard {
    const WIDTH: f32 = 340.0;
    const NEW_HEIGHT: f32 = 340.0;
    const SQUARE: f32 = 22.0;
    const DARK_LIGHT: Color32 = Color32::from_rgb(0xDD, 0xCF, 0xB3);
    const DARK_DARK: Color32 = Color32::from_rgb(0x93, 0x76, 0x58);
    const LIGHT_LIGHT: Color32 = Color32::from_rgb(0xE6, 0xDA, 0xC2);
    const LIGHT_DARK: Color32 = Color32::from_rgb(0xA3, 0x84, 0x5F);

    fn board(ui: &mut Ui, row: &PresentationGameOverviewRow) {
        let (light, dark) = if ui.visuals().dark_mode {
            (Self::DARK_LIGHT, Self::DARK_DARK)
        } else {
            (Self::LIGHT_LIGHT, Self::LIGHT_DARK)
        };
        let side = Self::SQUARE * 8.0;
        let width = ui.available_width();
        let (area, _) = ui.allocate_exact_size(Vec2::new(width, side), Sense::hover());
        let origin = Pos2::new(area.center().x - side / 2.0, area.min.y);
        let painter = ui.painter();
        let mut file = 0.0_f32;
        for column in 0_u8..8 {
            let mut rank = 0.0_f32;
            for line in 0_u8..8 {
                let color = if (column + line) % 2 == 0 {
                    dark
                } else {
                    light
                };
                painter.rect_filled(Self::square(origin, file, rank), 0.0, color);
                rank += 1.0;
            }
            file += 1.0;
        }
        for piece in &row.pieces {
            let rect = Self::square(origin, f32::from(piece.file()), f32::from(piece.rank()));
            let (fill, text) = match piece.color {
                BoardEnumColor::White => (
                    Color32::from_rgb(0xF7, 0xF4, 0xEE),
                    Color32::from_rgb(0x1E, 0x1B, 0x16),
                ),
                BoardEnumColor::Black => (
                    Color32::from_rgb(0x1E, 0x1B, 0x16),
                    Color32::from_rgb(0xF7, 0xF4, 0xEE),
                ),
            };
            painter.circle(
                rect.center(),
                Self::SQUARE * 0.38,
                fill,
                Stroke::new(1.0, Color32::from_black_alpha(90)),
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                piece.glyph(),
                egui::FontId::proportional(PresentationThemeTypography::MICRO_POINTS),
                text,
            );
        }
    }

    fn square(origin: Pos2, file: f32, rank: f32) -> Rect {
        let min = origin + Vec2::new(file * Self::SQUARE, (7.0 - rank) * Self::SQUARE);
        Rect::from_min_size(min, Vec2::splat(Self::SQUARE))
    }

    fn player(
        ui: &mut Ui,
        row: &PresentationGameOverviewRow,
        color: BoardEnumColor,
        colors: &PresentationThemeColors,
    ) {
        let running = row.is_running() && row.turn == color;
        ui.horizontal(|ui| {
            let (swatch, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            let fill = match color {
                BoardEnumColor::White => Color32::from_rgb(0xF7, 0xF4, 0xEE),
                BoardEnumColor::Black => Color32::from_rgb(0x1E, 0x1B, 0x16),
            };
            ui.painter().rect(
                swatch,
                2.0,
                fill,
                Stroke::new(1.0, colors.line_strong),
                egui::StrokeKind::Inside,
            );
            ui.label(RichText::new(row.player(color)).color(colors.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let clock_color = if running { colors.text } else { colors.text_3 };
                let response = ui.label(
                    RichText::new(row.side_clock(color))
                        .monospace()
                        .strong()
                        .color(clock_color),
                );
                if running {
                    let rect = response.rect;
                    ui.painter().hline(
                        rect.x_range(),
                        rect.max.y + 1.0,
                        Stroke::new(2.0, colors.text),
                    );
                }
            });
        });
    }
}
