use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::engine::capabilities::persistence::{EnginePersistenceError, EngineRecordJson};
use crate::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchGeneration, EngineSearchId,
    EngineSearchRevision, EngineSearchTag,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineSearchTagRecordData {
    id: Uuid,
    process_generation: Uuid,
    game: Uuid,
    revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSearchTagRecord {
    data: EngineSearchTagRecordData,
}

impl EngineSearchTagRecord {
    pub const MAX_BYTES: usize = 1024;

    pub const fn capture(tag: EngineSearchTag) -> Self {
        Self {
            data: EngineSearchTagRecordData {
                id: tag.id().value(),
                process_generation: tag.generation().value(),
                game: tag.context().game().value(),
                revision: tag.context().revision().value(),
            },
        }
    }

    pub fn restore(&self) -> Result<EngineSearchTag, EnginePersistenceError> {
        if self.data.id.is_nil() || self.data.process_generation.is_nil() {
            return Err(EnginePersistenceError::Context);
        }
        let game = EngineSearchGameId::try_from(self.data.game)?;
        Ok(EngineSearchTag {
            id: EngineSearchId(self.data.id),
            generation: EngineSearchGeneration(self.data.process_generation),
            context: EngineSearchContext::new(game, EngineSearchRevision::new(self.data.revision)),
        })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, EnginePersistenceError> {
        EngineRecordJson::parse::<Self, { Self::MAX_BYTES }>(bytes)
    }
}

impl Serialize for EngineSearchTagRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EngineRecordJson::serialize(&self.data, serializer)
    }
}

impl<'de> Deserialize<'de> for EngineSearchTagRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let record = Self {
            data: EngineRecordJson::deserialize::<D, EngineSearchTagRecordData, { Self::MAX_BYTES }>(
                deserializer,
            )?,
        };
        record.restore().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}
