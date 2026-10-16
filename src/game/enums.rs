use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::board::enums::BoardEnumColor;

#[derive(Debug, Error)]
pub enum GameIdentityError {
    #[error("Invalid game identifier: {0}")]
    InvalidUuid(#[from] uuid::Error),
    #[error("Game identifiers must be non-nil lowercase canonical UUID strings")]
    Noncanonical,
}

struct GameIdentity;

impl GameIdentity {
    fn parse(value: &str) -> Result<Uuid, GameIdentityError> {
        let parsed = Uuid::parse_str(value)?;
        if parsed.is_nil() || parsed.to_string() != value {
            return Err(GameIdentityError::Noncanonical);
        }
        Ok(parsed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GameId(Uuid);

impl GameId {
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl FromStr for GameId {
    type Err = GameIdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        GameIdentity::parse(value).map(Self)
    }
}

impl TryFrom<String> for GameId {
    type Error = GameIdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<GameId> for String {
    fn from(value: GameId) -> Self {
        value.0.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GameMoveId(Uuid);

impl GameMoveId {
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl FromStr for GameMoveId {
    type Err = GameIdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        GameIdentity::parse(value).map(Self)
    }
}

impl TryFrom<String> for GameMoveId {
    type Error = GameIdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<GameMoveId> for String {
    fn from(value: GameMoveId) -> Self {
        value.0.to_string()
    }
}

#[derive(Debug, Error)]
pub enum GameRevisionError {
    #[error("Game revisions must be canonical unsigned decimal strings")]
    Noncanonical,
    #[error("The game revision counter is exhausted")]
    Exhausted,
    #[error("The game changed: expected revision {expected}, current revision {actual}")]
    Stale {
        expected: GameRevision,
        actual: GameRevision,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GameRevision(u64);

impl GameRevision {
    pub const ZERO: Self = Self(0);

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, GameRevisionError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(GameRevisionError::Exhausted)
    }

    pub fn check_expected(self, expected: Self) -> Result<(), GameRevisionError> {
        if self == expected {
            Ok(())
        } else {
            Err(GameRevisionError::Stale {
                expected,
                actual: self,
            })
        }
    }
}

impl FromStr for GameRevision {
    type Err = GameRevisionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed: u64 = value
            .parse()
            .map_err(|_error| GameRevisionError::Noncanonical)?;
        if parsed.to_string() != value {
            return Err(GameRevisionError::Noncanonical);
        }
        Ok(Self(parsed))
    }
}

impl TryFrom<String> for GameRevision {
    type Error = GameRevisionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<GameRevision> for String {
    fn from(value: GameRevision) -> Self {
        value.0.to_string()
    }
}

impl Display for GameRevision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameEnumMode {
    HumanVersusEngine,
    LocalTwoPlayer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameEnumSeat {
    Human,
    Reckless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEnumEngineReadiness {
    NotRequired,
    Ready,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameEnumPhase {
    Setup,
    Active,
    Interrupted,
    Completed,
    Analysis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameEnumResult {
    Victory(BoardEnumColor),
    Draw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameEnumOutcomeReason {
    Checkmate,
    Stalemate,
    DeadPosition,
    FivefoldRepetition,
    SeventyFiveMove,
    ThreefoldRepetition,
    FiftyMove,
    Resignation,
    Flagfall,
    Agreement,
    ConsensualAdjudication,
}

impl GameEnumOutcomeReason {
    pub(crate) fn permits(self, result: GameEnumResult) -> bool {
        match self {
            Self::Checkmate | Self::Resignation => matches!(result, GameEnumResult::Victory(_)),
            Self::Flagfall | Self::ConsensualAdjudication => true,
            Self::Stalemate
            | Self::DeadPosition
            | Self::FivefoldRepetition
            | Self::SeventyFiveMove
            | Self::ThreefoldRepetition
            | Self::FiftyMove
            | Self::Agreement => result == GameEnumResult::Draw,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEnumTransitionKind {
    Started,
    MoveAccepted,
    FlagFall,
    ResultAccepted,
    Interrupted,
    Resumed,
    AnalysisStarted,
    RecoveryAccepted,
    TakebackRequested,
    TakebackAccepted,
    TakebackDeclined,
    TakebackCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameEnumMarkingColor {
    Green,
    Red,
    Blue,
    Yellow,
}
