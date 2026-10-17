use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::board::enums::BoardEnumVariant;
use crate::clock::duration::ClockDuration;
use crate::engine::capabilities::persistence::{
    EnginePersistenceError, EngineRecordJson, EngineRecordText,
};
use crate::engine::capabilities::{
    EngineCapabilities, EngineCapabilitiesError, EngineCapabilitiesVersion,
};
use crate::engine::enums::EngineEnumOption;
use crate::engine::options::{
    EngineOptionsApplied, EngineOptionsEnumOutcome, EngineOptionsEnumTablebases,
    EngineOptionsEnumValue, EngineOptionsSetting, EngineOptionsSyzygyPath,
};
use crate::engine::process_provider::EngineProcessLimits;
use crate::engine::resource_config::EngineResourceConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineOptionsRecordResources {
    threads: u32,
    hash_mib: u32,
    move_overhead_millis: u64,
}

impl EngineOptionsRecordResources {
    fn capture(resources: EngineResourceConfig) -> Self {
        Self {
            threads: resources.threads(),
            hash_mib: resources.hash_mib(),
            move_overhead_millis: resources.move_overhead().as_millis(),
        }
    }

    fn restore(self) -> Result<EngineResourceConfig, EnginePersistenceError> {
        Ok(EngineResourceConfig::new(
            usize::try_from(self.threads)?,
            self.hash_mib,
            ClockDuration::from_millis(self.move_overhead_millis)?,
        )?)
    }
}

struct EngineOptionsRecordPath;

impl EngineOptionsRecordPath {
    fn serialize<S: Serializer>(
        path: &EngineOptionsSyzygyPath,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        path.as_str().serialize(serializer)
    }

    fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<EngineOptionsSyzygyPath, D::Error> {
        let text =
            EngineRecordText::<{ EngineProcessLimits::LINE_BYTES }>::deserialize(deserializer)?;
        EngineOptionsSyzygyPath::new(text.into_string()).map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(
    remote = "EngineOptionsEnumTablebases",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum EngineOptionsRecordEnumTablebases {
    Unconfigured,
    Loaded {
        #[serde(with = "EngineOptionsRecordPath")]
        path: EngineOptionsSyzygyPath,
        pieces: NonZeroU32,
    },
    Unavailable {
        #[serde(with = "EngineOptionsRecordPath")]
        path: EngineOptionsSyzygyPath,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineOptionsAppliedRecordData {
    registry_version: u32,
    resources: Option<EngineOptionsRecordResources>,
    variant: BoardEnumVariant,
    minimal: bool,
    multi_pv: NonZeroU32,
    #[serde(with = "EngineOptionsRecordEnumTablebases")]
    tablebases: EngineOptionsEnumTablebases,
}

impl EngineOptionsAppliedRecordData {
    fn validate(&self) -> Result<(), EnginePersistenceError> {
        EngineCapabilitiesVersion::try_from(self.registry_version)?;
        if let Some(resources) = self.resources {
            resources.restore()?;
        }
        EngineResourceConfig::number(
            EngineEnumOption::MultiPv,
            u64::from(self.multi_pv.get()),
            EngineEnumOption::MultiPv.typed_descriptor(),
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOptionsAppliedRecord {
    data: EngineOptionsAppliedRecordData,
}

impl EngineOptionsAppliedRecord {
    pub const MAX_BYTES: usize = 512 * 1024;

    pub fn capture(applied: &EngineOptionsApplied) -> Result<Self, EnginePersistenceError> {
        let record = Self {
            data: EngineOptionsAppliedRecordData {
                registry_version: EngineCapabilitiesVersion::CURRENT.get(),
                resources: applied
                    .resources()
                    .map(EngineOptionsRecordResources::capture),
                variant: applied.variant(),
                minimal: applied.is_minimal(),
                multi_pv: applied.multi_pv(),
                tablebases: applied.tablebases().clone(),
            },
        };
        record.data.validate()?;
        Ok(record)
    }

    pub fn restore(
        &self,
        capabilities: &EngineCapabilities,
    ) -> Result<EngineOptionsApplied, EnginePersistenceError> {
        self.data.validate()?;
        capabilities.validate()?;
        if self.data.registry_version != capabilities.version().get() {
            return Err(
                EngineCapabilitiesError::UnsupportedVersion(self.data.registry_version).into(),
            );
        }
        let resources = self
            .data
            .resources
            .map(EngineOptionsRecordResources::restore)
            .transpose()?;
        if let Some(resources) = resources {
            resources.validate(capabilities)?;
        }
        let mut applied = EngineOptionsApplied::new(capabilities)?;
        applied.set_resources(resources);
        for value in [
            EngineOptionsEnumValue::Minimal(self.data.minimal),
            EngineOptionsEnumValue::Chess960(self.data.variant),
            EngineOptionsEnumValue::MultiPv(self.data.multi_pv.get()),
        ] {
            let setting = EngineOptionsSetting::new(value, capabilities)?;
            applied.apply(
                &setting,
                &EngineOptionsEnumOutcome::Applied(setting.option()),
                resources,
            )?;
        }
        match &self.data.tablebases {
            EngineOptionsEnumTablebases::Unconfigured => {}
            EngineOptionsEnumTablebases::Loaded { path, .. }
            | EngineOptionsEnumTablebases::Unavailable { path } => {
                let setting = EngineOptionsSetting::new(
                    EngineOptionsEnumValue::SyzygyPath(path.clone()),
                    capabilities,
                )?;
                applied.apply(
                    &setting,
                    &EngineOptionsEnumOutcome::Tablebases(self.data.tablebases.clone()),
                    resources,
                )?;
            }
        }
        Ok(applied)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, EnginePersistenceError> {
        EngineRecordJson::parse::<Self, { Self::MAX_BYTES }>(bytes)
    }
}

impl Serialize for EngineOptionsAppliedRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EngineRecordJson::serialize(&self.data, serializer)
    }
}

impl<'de> Deserialize<'de> for EngineOptionsAppliedRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let record = Self {
            data: EngineRecordJson::deserialize::<
                D,
                EngineOptionsAppliedRecordData,
                { Self::MAX_BYTES },
            >(deserializer)?,
        };
        record.data.validate().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}
