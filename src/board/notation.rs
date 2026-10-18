use shakmaty::{Color, EnPassantMode, Position};

use crate::board::position::BoardPosition;

impl BoardPosition {
    const EMPTY_FEN_FIELD: &str = "-";

    pub fn shredder_fen(&self) -> String {
        let mut rights = String::with_capacity(4);
        for color in Color::ALL {
            for rook in (self.chess.castles().castling_rights() & color.backrank())
                .into_iter()
                .rev()
            {
                let file = rook.file().char();
                rights.push(match color {
                    Color::White => file.to_ascii_uppercase(),
                    Color::Black => file,
                });
            }
        }
        if rights.is_empty() {
            rights.push_str(Self::EMPTY_FEN_FIELD);
        }
        let en_passant = self.chess.ep_square(EnPassantMode::Always).map_or_else(
            || Self::EMPTY_FEN_FIELD.to_owned(),
            |square| square.to_string(),
        );
        format!(
            "{} {} {} {} {} {}",
            self.chess.board(),
            self.chess.turn().char(),
            rights,
            en_passant,
            self.chess.halfmoves(),
            self.chess.fullmoves()
        )
    }
}
