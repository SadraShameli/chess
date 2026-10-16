use serde::Serialize;

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::board::square::BoardSquare;
use crate::desktop::file_handle::DesktopFileHandleId;
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopBoardPiece {
    square: BoardSquare,
    color: BoardEnumColor,
    kind: BoardEnumPieceKind,
}

impl DesktopBoardPiece {
    pub fn square(&self) -> BoardSquare {
        self.square
    }

    pub fn color(&self) -> BoardEnumColor {
        self.color
    }

    pub fn kind(&self) -> BoardEnumPieceKind {
        self.kind
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopBoardMove {
    #[serde(skip)]
    movement: BoardMove,
    origin: Option<BoardSquare>,
    destination: BoardSquare,
    promotion: Option<BoardEnumPieceKind>,
    castling_rook: Option<BoardSquare>,
}

impl DesktopBoardMove {
    pub const fn movement(&self) -> BoardMove {
        self.movement
    }

    pub fn origin(&self) -> Option<BoardSquare> {
        self.origin
    }

    pub fn destination(&self) -> BoardSquare {
        self.destination
    }

    pub fn promotion(&self) -> Option<BoardEnumPieceKind> {
        self.promotion
    }

    pub fn castling_rook(&self) -> Option<BoardSquare> {
        self.castling_rook
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopBoardSnapshot {
    fen: String,
    variant: BoardEnumVariant,
    turn: BoardEnumColor,
    is_check: bool,
    pieces: Vec<DesktopBoardPiece>,
    legal_moves: Vec<DesktopBoardMove>,
}

impl DesktopBoardSnapshot {
    pub fn fen(&self) -> &str {
        &self.fen
    }

    pub fn variant(&self) -> BoardEnumVariant {
        self.variant
    }

    pub fn turn(&self) -> BoardEnumColor {
        self.turn
    }

    pub fn is_check(&self) -> bool {
        self.is_check
    }

    pub fn pieces(&self) -> &[DesktopBoardPiece] {
        &self.pieces
    }

    pub fn legal_moves(&self) -> &[DesktopBoardMove] {
        &self.legal_moves
    }

    pub fn from_position(position: &BoardPosition) -> Self {
        Self {
            fen: position.fen(),
            variant: position.variant(),
            turn: position.turn(),
            is_check: position.is_check(),
            pieces: position
                .pieces()
                .map(|(square, piece)| DesktopBoardPiece {
                    square,
                    color: piece.color(),
                    kind: piece.kind(),
                })
                .collect(),
            legal_moves: position
                .legal_moves()
                .into_iter()
                .map(|movement| DesktopBoardMove {
                    movement,
                    origin: movement.origin(),
                    destination: movement.destination(),
                    promotion: movement.promotion(),
                    castling_rook: movement.castling_rook(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopEngineIdentity {
    name: String,
    author: String,
    supported_options: Vec<String>,
}

impl DesktopEngineIdentity {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn supported_options(&self) -> &[String] {
        &self.supported_options
    }

    pub fn new(name: String, author: String, supported_options: Vec<String>) -> Self {
        Self {
            name,
            author,
            supported_options,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopEnumEngineSnapshot {
    Ready {
        identity: Box<DesktopEngineIdentity>,
    },
    Unavailable {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopBootstrapSnapshot {
    app_version: String,
    revision: DesktopRevision,
    board: DesktopBoardSnapshot,
    engine: DesktopEnumEngineSnapshot,
    cpu_threads: u32,
}

impl DesktopBootstrapSnapshot {
    pub fn app_version(&self) -> &str {
        &self.app_version
    }

    pub fn revision(&self) -> DesktopRevision {
        self.revision
    }

    pub fn board(&self) -> &DesktopBoardSnapshot {
        &self.board
    }

    pub fn engine(&self) -> &DesktopEnumEngineSnapshot {
        &self.engine
    }

    pub fn cpu_threads(&self) -> u32 {
        self.cpu_threads
    }

    pub fn new(
        board: &BoardPosition,
        cpu_threads: std::num::NonZeroU32,
        engine: DesktopEnumEngineSnapshot,
    ) -> Self {
        Self {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            revision: DesktopRevision::ZERO,
            board: DesktopBoardSnapshot::from_position(board),
            engine,
            cpu_threads: cpu_threads.get(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DesktopEnumValue {
    Game(Box<crate::desktop::game_snapshot::DesktopGameSnapshot>),
    Bootstrap(DesktopBootstrapSnapshot),
    FileSelect(Option<DesktopFileHandleId>),
    StreamSubscribe {
        subscription_id: DesktopRequestId,
        sequence: DesktopRevision,
        snapshot: DesktopBootstrapSnapshot,
    },
    StreamAcknowledge,
    StreamResync {
        subscription_id: DesktopRequestId,
        sequence: DesktopRevision,
        snapshot: DesktopBootstrapSnapshot,
    },
}

#[derive(Debug, Clone)]
pub enum DesktopEnumEvent {
    Games(Vec<crate::desktop::game_snapshot::DesktopGameSnapshot>),
    Snapshot(DesktopBootstrapSnapshot),
    Failure(crate::desktop::error::DesktopFailure),
}
