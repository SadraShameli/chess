use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresentationBoardFigurinesEnumNotation {
    #[default]
    Letters,
    Figurines,
}

impl PresentationBoardFigurinesEnumNotation {
    pub const ALL: [Self; 2] = [Self::Letters, Self::Figurines];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Letters => "Letters",
            Self::Figurines => "Figurines",
        }
    }
}

pub struct PresentationBoardFigurines;

impl PresentationBoardFigurines {
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
        notation: PresentationBoardFigurinesEnumNotation,
        mover: BoardEnumColor,
    ) -> String {
        match notation {
            PresentationBoardFigurinesEnumNotation::Letters => san.to_owned(),
            PresentationBoardFigurinesEnumNotation::Figurines => {
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
