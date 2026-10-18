use bevy_egui::egui::{self, Align2, CornerRadius, FontId, RichText, Sense, Ui, Vec2};

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::game::enums::GameEnumSeat;
use crate::presentation::board::figurines::PresentationBoardFigurines;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::game::play::PresentationGamePlayClock;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumClock,
};
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;

pub struct PresentationGameEdge;

impl PresentationGameEdge {
    const AVATAR: f32 = 36.0;
    const AVATAR_RADIUS: u8 = 8;
    const PERMILLE: u64 = 1000;
    const MATERIAL: [(BoardEnumPieceKind, u8, i32); 5] = [
        (BoardEnumPieceKind::Queen, 1, 9),
        (BoardEnumPieceKind::Rook, 2, 5),
        (BoardEnumPieceKind::Bishop, 2, 3),
        (BoardEnumPieceKind::Knight, 2, 3),
        (BoardEnumPieceKind::Pawn, 8, 1),
    ];

    pub fn show(
        ui: &mut Ui,
        color: BoardEnumColor,
        game: Option<&DesktopGameSnapshot>,
        now: ClockInstant,
        extra: impl FnOnce(&mut Ui),
    ) {
        ui.horizontal(|ui| {
            ui.set_min_height(PresentationThemeTokens::CONTROL_LG);
            ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_3;
            Self::avatar(ui, color, game);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                Self::identity(ui, color, game);
                if let Some(game) = game {
                    Self::detail(ui, color, game);
                }
                extra(ui);
            });
            let Some(view) = game.and_then(|game| game.clock().view(now).ok()) else {
                return;
            };
            let displayed = view.side(color).displayed();
            let state =
                if view.active() != color || !game.is_some_and(|game| game.clock().is_running()) {
                    PresentationThemeEnumClock::Idle
                } else if PresentationGamePlayClock::is_low_time(displayed) {
                    PresentationThemeEnumClock::Low
                } else {
                    PresentationThemeEnumClock::Running
                };
            let share = game.and_then(|game| Self::share(game, displayed));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                PresentationThemeComponents::clock(
                    ui,
                    &PresentationGamePlayClock::text(displayed),
                    state,
                    share,
                );
            });
        });
    }

    fn avatar(ui: &mut Ui, color: BoardEnumColor, game: Option<&DesktopGameSnapshot>) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(Self::AVATAR), Sense::hover());
        let seat = game.map(|game| game.seats().seat(color));
        match seat {
            Some(GameEnumSeat::Reckless) => {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(Self::AVATAR_RADIUS),
                    colors.surface_3,
                );
                PresentationThemeIcon::paint(
                    ui.painter(),
                    rect.shrink(8.0),
                    PresentationThemeEnumIcon::Engine,
                    colors.text_2,
                );
            }
            Some(GameEnumSeat::Human) | None => {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(Self::AVATAR_RADIUS),
                    colors.accent_soft,
                );
                let initial = Self::name(color, game)
                    .chars()
                    .next()
                    .map(|letter| letter.to_uppercase().to_string())
                    .unwrap_or_default();
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    initial,
                    FontId::proportional(PresentationThemeTypography::BODY_POINTS),
                    colors.accent,
                );
            }
        }
    }

    fn name(color: BoardEnumColor, game: Option<&DesktopGameSnapshot>) -> String {
        let Some(game) = game else {
            return PresentationBoardSemanticGrid::color_name(color).to_owned();
        };
        let known = game.metadata().known();
        let name = match color {
            BoardEnumColor::White => known.white.as_ref(),
            BoardEnumColor::Black => known.black.as_ref(),
        };
        name.map_or_else(
            || match game.seats().seat(color) {
                GameEnumSeat::Human => "Human".to_owned(),
                GameEnumSeat::Reckless => "Reckless".to_owned(),
            },
            |name| name.as_str().to_owned(),
        )
    }

    fn identity(ui: &mut Ui, color: BoardEnumColor, game: Option<&DesktopGameSnapshot>) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let color_name = PresentationBoardSemanticGrid::color_name(color);
        let rating = game.and_then(|game| {
            let known = game.metadata().known();
            match color {
                BoardEnumColor::White => known.white_elo,
                BoardEnumColor::Black => known.black_elo,
            }
        });
        let secondary = rating.map_or_else(
            || color_name.to_owned(),
            |rating| format!("{color_name} \u{b7} {rating}"),
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_1;
            if game.is_some() {
                ui.label(
                    RichText::new(Self::name(color, game))
                        .size(PresentationThemeTypography::SUBHEAD_POINTS)
                        .strong(),
                );
                ui.label(
                    RichText::new(secondary)
                        .size(PresentationThemeTypography::SUBHEAD_POINTS)
                        .color(colors.text_3),
                );
            } else {
                PresentationThemeComponents::subhead(ui, color_name);
            }
        });
    }

    fn detail(ui: &mut Ui, color: BoardEnumColor, game: &DesktopGameSnapshot) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let text = match game.premove() {
            Some(premove) if game.live_turn() != color => format!(
                "Premove ready: {}{}",
                premove.origin(),
                premove.destination()
            ),
            Some(_) | None => Self::material(color, game),
        };
        if text.is_empty() {
            return;
        }
        ui.label(
            RichText::new(text)
                .size(PresentationThemeTypography::CAPTION_POINTS)
                .color(colors.text_3),
        );
    }

    fn material(color: BoardEnumColor, game: &DesktopGameSnapshot) -> String {
        let opponent = color.opposite();
        let pieces = game.board().pieces();
        let count = |side: BoardEnumColor, kind: BoardEnumPieceKind| {
            pieces
                .iter()
                .filter(|piece| piece.color() == side && piece.kind() == kind)
                .count()
        };
        let mut captured = String::new();
        let mut balance = 0_i32;
        for (kind, start, value) in Self::MATERIAL {
            let missing = usize::from(start).saturating_sub(count(opponent, kind));
            for _ in 0..missing {
                captured.push(PresentationBoardFigurines::glyph(opponent, kind));
            }
            let own = i32::try_from(count(color, kind)).unwrap_or(i32::MAX);
            let theirs = i32::try_from(count(opponent, kind)).unwrap_or(i32::MAX);
            balance = balance.saturating_add(own.saturating_sub(theirs).saturating_mul(value));
        }
        if balance > 0 {
            captured.push_str(&format!(" +{balance}"));
        }
        captured
    }

    fn share(game: &DesktopGameSnapshot, displayed: Option<ClockDuration>) -> Option<f32> {
        let displayed = displayed?;
        let base = game.clock().control().stages().first()?.base_time();
        if base == ClockDuration::ZERO {
            return None;
        }
        let permille = displayed
            .as_millis()
            .saturating_mul(Self::PERMILLE)
            .checked_div(base.as_millis())?
            .min(Self::PERMILLE);
        let permille = u16::try_from(permille).ok()?;
        Some(f32::from(permille) / 1000.0)
    }
}
