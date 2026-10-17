use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::clock::duration::ClockDuration;
use crate::engine::capabilities::persistence::{EnginePersistenceError, EngineRecordJson};
use crate::engine::search::limit::{EngineSearchClock, EngineSearchEnumLimit, EngineSearchLimit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineSearchRecordClock {
    white_time: u32,
    black_time: u32,
    white_increment: u32,
    black_increment: u32,
    moves_to_go: Option<NonZeroU32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum EngineSearchRecordEnumLimit {
    Infinite,
    Depth(u32),
    Nodes(u32),
    MoveTime(u32),
    Clock(EngineSearchRecordClock),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct EngineSearchLimitRecordData(EngineSearchRecordEnumLimit);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSearchLimitRecord {
    data: EngineSearchLimitRecordData,
}

impl EngineSearchLimitRecord {
    pub const MAX_BYTES: usize = 2048;

    pub const fn capture(limit: EngineSearchLimit) -> Self {
        Self {
            data: EngineSearchLimitRecordData(match limit.0 {
                EngineSearchEnumLimit::Infinite => EngineSearchRecordEnumLimit::Infinite,
                EngineSearchEnumLimit::Depth(value) => EngineSearchRecordEnumLimit::Depth(value),
                EngineSearchEnumLimit::Nodes(value) => EngineSearchRecordEnumLimit::Nodes(value),
                EngineSearchEnumLimit::MoveTime(value) => {
                    EngineSearchRecordEnumLimit::MoveTime(value)
                }
                EngineSearchEnumLimit::Clock(clock) => {
                    EngineSearchRecordEnumLimit::Clock(EngineSearchRecordClock {
                        white_time: clock.white_time,
                        black_time: clock.black_time,
                        white_increment: clock.white_increment,
                        black_increment: clock.black_increment,
                        moves_to_go: clock.moves_to_go,
                    })
                }
            }),
        }
    }

    pub fn restore(&self) -> Result<EngineSearchLimit, EnginePersistenceError> {
        Ok(match self.data.0 {
            EngineSearchRecordEnumLimit::Infinite => EngineSearchLimit::infinite(),
            EngineSearchRecordEnumLimit::Depth(value) => EngineSearchLimit::depth(value)?,
            EngineSearchRecordEnumLimit::Nodes(value) => {
                EngineSearchLimit::nodes(u64::from(value))?
            }
            EngineSearchRecordEnumLimit::MoveTime(value) => {
                EngineSearchLimit::move_time(ClockDuration::from_millis(u64::from(value))?)?
            }
            EngineSearchRecordEnumLimit::Clock(clock) => {
                EngineSearchLimit::clock(EngineSearchClock::new(
                    ClockDuration::from_millis(u64::from(clock.white_time))?,
                    ClockDuration::from_millis(u64::from(clock.black_time))?,
                    ClockDuration::from_millis(u64::from(clock.white_increment))?,
                    ClockDuration::from_millis(u64::from(clock.black_increment))?,
                    clock.moves_to_go,
                )?)
            }
        })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, EnginePersistenceError> {
        EngineRecordJson::parse::<Self, { Self::MAX_BYTES }>(bytes)
    }
}

impl Serialize for EngineSearchLimitRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EngineRecordJson::serialize(&self.data, serializer)
    }
}

impl<'de> Deserialize<'de> for EngineSearchLimitRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let record = Self {
            data: EngineRecordJson::deserialize::<
                D,
                EngineSearchLimitRecordData,
                { Self::MAX_BYTES },
            >(deserializer)?,
        };
        record.restore().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}
