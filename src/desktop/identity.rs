use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
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

impl JsonSchema for DesktopRequestId {
    fn schema_name() -> Cow<'static, str> {
        "DesktopRequestId".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "pattern": "^(?!00000000-0000-0000-0000-000000000000$)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"})
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesktopRevision(u64);

impl DesktopRevision {
    pub const ZERO: Self = Self(0);

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

    fn schema_pattern() -> String {
        let maximum = u64::MAX.to_string();
        let mut alternatives = vec![
            "0".to_owned(),
            format!("[1-9][0-9]{{0,{}}}", maximum.len() - 2),
        ];
        let mut prefix = String::new();
        for (index, byte) in maximum.bytes().enumerate() {
            let minimum = if index == 0 { b'1' } else { b'0' };
            if byte > minimum {
                alternatives.push(format!(
                    "{prefix}[{}-{}][0-9]{{{}}}",
                    char::from(minimum),
                    char::from(byte - 1),
                    maximum.len() - index - 1
                ));
            }
            prefix.push(char::from(byte));
        }
        alternatives.push(maximum);
        format!("^(?:{})$", alternatives.join("|"))
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

impl JsonSchema for DesktopRevision {
    fn schema_name() -> Cow<'static, str> {
        "DesktopRevision".into()
    }
    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "pattern": Self::schema_pattern()})
    }
}
