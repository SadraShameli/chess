use bevy_egui::egui::{Context, Id};

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::desktop::preferences::DesktopPreferencesEnumNotation;

pub struct PresentationBoardFigurines;

impl PresentationBoardFigurines {
    const PREFERENCE_KEY: &'static str = "chess.notation.preference";

    pub fn publish(context: &Context, notation: DesktopPreferencesEnumNotation) {
        context.data_mut(|data| data.insert_temp(Id::new(Self::PREFERENCE_KEY), notation));
    }

    pub fn notation(context: &Context) -> DesktopPreferencesEnumNotation {
        context
            .data(|data| data.get_temp(Id::new(Self::PREFERENCE_KEY)))
            .unwrap_or_default()
    }

    pub fn display(context: &Context, san: &str, mover: Option<BoardEnumColor>) -> String {
        Self::display_san(
            san,
            Self::notation(context),
            mover.unwrap_or(BoardEnumColor::White),
        )
    }

    pub fn display_line(context: &Context, line: &str) -> String {
        match Self::notation(context) {
            DesktopPreferencesEnumNotation::Letters => line.to_owned(),
            DesktopPreferencesEnumNotation::Figurines => line
                .split(' ')
                .map(|token| {
                    if token.ends_with('.') || token.starts_with(|first: char| first.is_ascii_digit()) {
                        token.to_owned()
                    } else {
                        Self::display_san(
                            token,
                            DesktopPreferencesEnumNotation::Figurines,
                            BoardEnumColor::White,
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    pub const fn glyph(color: BoardEnumColor, kind: BoardEnumPieceKind) -> char {
        match (color, kind) {
            (BoardEnumColor::White, BoardEnumPieceKind::King) => '\u{2654}',
            (BoardEnumColor::White, BoardEnumPieceKind::Queen) => '\u{2655}',
            (BoardEnumColor::White, BoardEnumPieceKind::Rook) => '\u{2656}',
            (BoardEnumColor::White, BoardEnumPieceKind::Bishop) => '\u{2657}',
            (BoardEnumColor::White, BoardEnumPieceKind::Knight) => '\u{2658}',
            (BoardEnumColor::White, BoardEnumPieceKind::Pawn) => '\u{2659}',
            (BoardEnumColor::Black, BoardEnumPieceKind::King) => '\u{265a}',
            (BoardEnumColor::Black, BoardEnumPieceKind::Queen) => '\u{265b}',
            (BoardEnumColor::Black, BoardEnumPieceKind::Rook) => '\u{265c}',
            (BoardEnumColor::Black, BoardEnumPieceKind::Bishop) => '\u{265d}',
            (BoardEnumColor::Black, BoardEnumPieceKind::Knight) => '\u{265e}',
            (BoardEnumColor::Black, BoardEnumPieceKind::Pawn) => '\u{265f}',
        }
    }

    pub const fn kind_for_letter(letter: char) -> Option<BoardEnumPieceKind> {
        match letter {
            'K' => Some(BoardEnumPieceKind::King),
            'Q' => Some(BoardEnumPieceKind::Queen),
            'R' => Some(BoardEnumPieceKind::Rook),
            'B' => Some(BoardEnumPieceKind::Bishop),
            'N' => Some(BoardEnumPieceKind::Knight),
            _ => None,
        }
    }

    pub const fn spoken_name(kind: BoardEnumPieceKind) -> &'static str {
        match kind {
            BoardEnumPieceKind::King => "king",
            BoardEnumPieceKind::Queen => "queen",
            BoardEnumPieceKind::Rook => "rook",
            BoardEnumPieceKind::Bishop => "bishop",
            BoardEnumPieceKind::Knight => "knight",
            BoardEnumPieceKind::Pawn => "pawn",
        }
    }

    pub fn display_san(
        san: &str,
        notation: DesktopPreferencesEnumNotation,
        mover: BoardEnumColor,
    ) -> String {
        match notation {
            DesktopPreferencesEnumNotation::Letters => san.to_owned(),
            DesktopPreferencesEnumNotation::Figurines => {
                let mut previous = None;
                san.chars()
                    .enumerate()
                    .map(|(index, character)| {
                        let replace = index == 0 || previous == Some('=');
                        previous = Some(character);
                        match Self::kind_for_letter(character) {
                            Some(kind) if replace => Self::glyph(mover, kind),
                            Some(_) | None => character,
                        }
                    })
                    .collect()
            }
        }
    }

    pub fn spoken_san(san: &str) -> String {
        san.chars()
            .map(|character| match Self::kind_for_letter(character) {
                Some(kind) => format!("{} ", Self::spoken_name(kind)),
                None => character.to_string(),
            })
            .collect::<String>()
            .trim()
            .to_owned()
    }
}
