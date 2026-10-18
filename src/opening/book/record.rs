use crate::board::enums::BoardEnumPieceKind;
use crate::board::r#move::BoardMove;
use crate::opening::book::OpeningBookError;

#[derive(Clone, Copy)]
pub(super) struct OpeningBookRecord {
    key: u64,
    encoded_move: u16,
    weight: u16,
    learn: u32,
}

impl OpeningBookRecord {
    pub(super) const BYTES: usize = 16;
    pub(super) const ENCODING_COUNT: usize = 1 << 15;
    const SQUARE_BITS: u32 = 6;
    const PROMOTION_BITS: u32 = 12;
    const MAX_PROMOTION: u16 = 4;

    pub(super) fn decode(bytes: &[u8]) -> Result<Self, OpeningBookError> {
        let bytes: &[u8; Self::BYTES] = bytes
            .try_into()
            .map_err(|_error| OpeningBookError::Truncated { bytes: bytes.len() })?;
        Ok(Self {
            key: u64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            encoded_move: u16::from_be_bytes([bytes[8], bytes[9]]),
            weight: u16::from_be_bytes([bytes[10], bytes[11]]),
            learn: u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        })
    }

    pub(super) fn encode(movement: BoardMove) -> Result<u16, OpeningBookError> {
        let from = movement
            .origin()
            .ok_or(OpeningBookError::BoardMoveEncoding)?;
        let to = movement.castling_rook().unwrap_or(movement.destination());
        let promotion = match movement.promotion() {
            None => 0,
            Some(BoardEnumPieceKind::Knight) => 1,
            Some(BoardEnumPieceKind::Bishop) => 2,
            Some(BoardEnumPieceKind::Rook) => 3,
            Some(BoardEnumPieceKind::Queen) => Self::MAX_PROMOTION,
            Some(BoardEnumPieceKind::Pawn | BoardEnumPieceKind::King) => {
                return Err(OpeningBookError::BoardMoveEncoding);
            }
        };
        Ok(u16::from(u8::from(to))
            | (u16::from(u8::from(from)) << Self::SQUARE_BITS)
            | (promotion << Self::PROMOTION_BITS))
    }

    pub(super) fn validate_encoding(self, record: usize) -> Result<(), OpeningBookError> {
        if usize::from(self.encoded_move) >= Self::ENCODING_COUNT
            || self.encoded_move >> Self::PROMOTION_BITS > Self::MAX_PROMOTION
        {
            return Err(OpeningBookError::MoveEncoding {
                record,
                encoded_move: self.encoded_move,
            });
        }
        Ok(())
    }

    pub(super) const fn key(self) -> u64 {
        self.key
    }

    pub(super) const fn encoded_move(self) -> u16 {
        self.encoded_move
    }

    pub(super) const fn weight(self) -> u16 {
        self.weight
    }

    pub(super) const fn learn(self) -> u32 {
        self.learn
    }
}
