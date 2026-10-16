use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use crate::board::position::BoardPosition;
use crate::board::square::BoardSquare;
use crate::desktop::envelope::DesktopProtocolVersion;
use crate::desktop::file_handle::{DesktopFileEnumPurpose, DesktopFileHandleId};
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopEnumRequest {
    Bootstrap,
    FileSelect {
        purpose: DesktopFileEnumPurpose,
    },
    StreamSubscribe,
    StreamAcknowledge {
        subscription_id: DesktopRequestId,
        sequence: DesktopRevision,
    },
    StreamResync {
        subscription_id: DesktopRequestId,
    },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
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

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopBoardMove {
    origin: Option<BoardSquare>,
    destination: BoardSquare,
    promotion: Option<BoardEnumPieceKind>,
    castling_rook: Option<BoardSquare>,
}

impl DesktopBoardMove {
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

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopBoardSnapshot {
    #[schemars(length(max = BoardPosition::MAX_FEN_BYTES))]
    fen: String,
    variant: BoardEnumVariant,
    turn: BoardEnumColor,
    is_check: bool,
    #[schemars(length(max = 64))]
    pieces: Vec<DesktopBoardPiece>,
    #[schemars(length(max = 256))]
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
                    origin: movement.origin(),
                    destination: movement.destination(),
                    promotion: movement.promotion(),
                    castling_rook: movement.castling_rook(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopEngineIdentity {
    #[schemars(length(max = 256))]
    name: String,
    #[schemars(length(max = 256))]
    author: String,
    #[schemars(length(max = 128))]
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

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopEnumEngineSnapshot {
    Ready {
        identity: Box<DesktopEngineIdentity>,
    },
    Unavailable {
        #[schemars(length(max = 1024))]
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopBootstrapSnapshot {
    protocol_version: DesktopProtocolVersion,
    #[schemars(length(max = 64))]
    app_version: String,
    revision: DesktopRevision,
    board: DesktopBoardSnapshot,
    engine: DesktopEnumEngineSnapshot,
    #[schemars(schema_with = "DesktopBootstrapSnapshot::cpu_schema")]
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

    fn cpu_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "integer", "minimum": 1, "maximum": u32::MAX})
    }

    pub fn new(
        board: &BoardPosition,
        cpu_threads: std::num::NonZeroU32,
        engine: DesktopEnumEngineSnapshot,
    ) -> Self {
        Self {
            protocol_version: DesktopProtocolVersion::CURRENT,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            revision: DesktopRevision::ZERO,
            board: DesktopBoardSnapshot::from_position(board),
            engine,
            cpu_threads: cpu_threads.get(),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(
    tag = "operation",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DesktopEnumValue {
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

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DesktopEnumEvent {
    Snapshot(DesktopBootstrapSnapshot),
    Failure(crate::desktop::error::DesktopFailure),
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopEnumResponseResult {
    Success {
        value: DesktopEnumValue,
        revision: Option<DesktopRevision>,
    },
    Failure {
        error: crate::desktop::error::DesktopFailure,
    },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopResponse {
    protocol_version: DesktopProtocolVersion,
    request_id: DesktopRequestId,
    result: DesktopEnumResponseResult,
}

impl DesktopResponse {
    pub fn new(request_id: DesktopRequestId, result: DesktopEnumResponseResult) -> Self {
        Self {
            protocol_version: DesktopProtocolVersion::CURRENT,
            request_id,
            result,
        }
    }
}
