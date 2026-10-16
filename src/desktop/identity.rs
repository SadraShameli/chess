use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::desktop::error::DesktopError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesktopRequestId(Uuid);

impl DesktopRequestId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub(crate) const fn for_game(game: crate::game::enums::GameId) -> Self {
        Self(game.as_uuid())
    }
}

impl Default for DesktopRequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl FromStr for DesktopRequestId {
    type Err = DesktopError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(value)?;
        if uuid.is_nil() || uuid.to_string() != value {
            return Err(DesktopError::NoncanonicalIdentity);
        }
        Ok(Self(uuid))
    }
}

impl TryFrom<String> for DesktopRequestId {
    type Error = DesktopError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<DesktopRequestId> for String {
    fn from(value: DesktopRequestId) -> Self {
        value.0.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesktopRevision(u64);

impl DesktopRevision {
    pub const ZERO: Self = Self(0);

    pub const fn for_game(revision: crate::game::enums::GameRevision) -> Self {
        Self(revision.value())
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, DesktopError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(DesktopError::RevisionOverflow)
    }

    pub fn check_expected(self, expected: Self) -> Result<(), DesktopError> {
        if self == expected {
            Ok(())
        } else {
            Err(DesktopError::StaleRevision {
                expected,
                actual: self,
            })
        }
    }
}

impl FromStr for DesktopRevision {
    type Err = DesktopError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed: u64 = value
            .parse()
            .map_err(|_error| DesktopError::InvalidRevision)?;
        if parsed.to_string() != value {
            return Err(DesktopError::InvalidRevision);
        }
        Ok(Self(parsed))
    }
}

impl TryFrom<String> for DesktopRevision {
    type Error = DesktopError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<DesktopRevision> for String {
    fn from(value: DesktopRevision) -> Self {
        value.0.to_string()
    }
}

impl Display for DesktopRevision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}
