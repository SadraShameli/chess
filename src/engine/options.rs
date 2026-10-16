use std::num::{NonZeroU32, TryFromIntError};

use thiserror::Error;

use crate::board::enums::BoardEnumVariant;
use crate::clock::duration::ClockDuration;
use crate::engine::capabilities::{
    EngineCapabilities, EngineCapabilitiesError, EngineCapabilitiesVersion,
};
use crate::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};
use crate::engine::process_provider::EngineProcessLimits;
use crate::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};

#[derive(Debug, Error)]
pub enum EngineOptionsError {
    #[error("The engine did not advertise option {0:?}")]
    MissingOption(EngineEnumOption),
    #[error("Engine option {0:?} has an incompatible descriptor")]
    Descriptor(EngineEnumOption),
    #[error(
        "A Syzygy path must be nonempty single-line text without whitespace or control characters"
    )]
    PathSyntax,
    #[error("The Syzygy path exceeds the engine command bound")]
    PathLength,
    #[error(transparent)]
    Capabilities(#[from] EngineCapabilitiesError),
    #[error(transparent)]
    Resources(#[from] EngineResourceConfigError),
    #[error("The engine option value cannot be represented: {0}")]
    Conversion(#[from] TryFromIntError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOptionsSyzygyPath(Box<str>);

impl EngineOptionsSyzygyPath {
    pub fn new(value: String) -> Result<Self, EngineOptionsError> {
        if value.is_empty()
            || value.contains(|character: char| character.is_whitespace() || character.is_control())
        {
            return Err(EngineOptionsError::PathSyntax);
        }
        if EngineEnumOption::SyzygyPath
            .set_value_command("")
            .len()
            .checked_add(value.len())
            .is_none_or(|length| length > EngineProcessLimits::LINE_BYTES)
        {
            return Err(EngineOptionsError::PathLength);
        }
        Ok(Self(value.into_boxed_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineOptionsEnumValue {
    Hash(u32),
    Threads(u32),
    MoveOverhead(ClockDuration),
    Minimal(bool),
    ClearHash,
    Chess960(BoardEnumVariant),
    MultiPv(u32),
    SyzygyPath(EngineOptionsSyzygyPath),
}

impl EngineOptionsEnumValue {
    pub const fn option(&self) -> EngineEnumOption {
        match self {
            Self::Hash(_) => EngineEnumOption::Hash,
            Self::Threads(_) => EngineEnumOption::Threads,
            Self::MoveOverhead(_) => EngineEnumOption::MoveOverhead,
            Self::Minimal(_) => EngineEnumOption::Minimal,
            Self::ClearHash => EngineEnumOption::ClearHash,
            Self::Chess960(_) => EngineEnumOption::Chess960,
            Self::MultiPv(_) => EngineEnumOption::MultiPv,
            Self::SyzygyPath(_) => EngineEnumOption::SyzygyPath,
        }
    }

    fn formatted(&self) -> Option<String> {
        match self {
            Self::Hash(value) | Self::Threads(value) | Self::MultiPv(value) => {
                Some(value.to_string())
            }
            Self::MoveOverhead(value) => Some(value.as_millis().to_string()),
            Self::Minimal(value) => Some(value.to_string()),
            Self::ClearHash => None,
            Self::Chess960(variant) => Some((*variant == BoardEnumVariant::Chess960).to_string()),
            Self::SyzygyPath(value) => Some(value.as_str().to_owned()),
        }
    }

    pub(crate) fn is_resource(&self) -> bool {
        matches!(
            self,
            Self::Hash(_) | Self::Threads(_) | Self::MoveOverhead(_)
        )
    }

    fn validate(&self, capabilities: &EngineCapabilities) -> Result<(), EngineOptionsError> {
        let option = self.option();
        let descriptor = capabilities
            .descriptor(option)
            .ok_or(EngineOptionsError::MissingOption(option))?;
        match self {
            Self::Hash(value) | Self::Threads(value) | Self::MultiPv(value) => {
                EngineResourceConfig::number(option, u64::from(*value), descriptor)?;
            }
            Self::MoveOverhead(value) => {
                EngineResourceConfig::validate_move_overhead(*value)?;
                EngineResourceConfig::number(option, value.as_millis(), descriptor)?;
            }
            Self::Minimal(_) | Self::Chess960(_) => {
                if !matches!(descriptor, EngineOptionEnumDescriptor::Check(_)) {
                    return Err(EngineOptionsError::Descriptor(option));
                }
            }
            Self::ClearHash => {
                if descriptor != EngineOptionEnumDescriptor::Button {
                    return Err(EngineOptionsError::Descriptor(option));
                }
            }
            Self::SyzygyPath(_) => {
                if descriptor != EngineOptionEnumDescriptor::String {
                    return Err(EngineOptionsError::Descriptor(option));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOptionsSetting {
    version: EngineCapabilitiesVersion,
    value: EngineOptionsEnumValue,
}

impl EngineOptionsSetting {
    pub fn new(
        value: EngineOptionsEnumValue,
        capabilities: &EngineCapabilities,
    ) -> Result<Self, EngineOptionsError> {
        value.validate(capabilities)?;
        Ok(Self {
            version: capabilities.version(),
            value,
        })
    }

    pub(crate) fn resource(
        option: EngineEnumOption,
        value: u64,
        capabilities: &EngineCapabilities,
    ) -> Result<Self, EngineOptionsError> {
        let value = match option {
            EngineEnumOption::Hash => EngineOptionsEnumValue::Hash(u32::try_from(value)?),
            EngineEnumOption::Threads => EngineOptionsEnumValue::Threads(u32::try_from(value)?),
            EngineEnumOption::MoveOverhead => EngineOptionsEnumValue::MoveOverhead(
                ClockDuration::from_millis(value).map_err(EngineResourceConfigError::from)?,
            ),
            EngineEnumOption::Minimal
            | EngineEnumOption::ClearHash
            | EngineEnumOption::Chess960
            | EngineEnumOption::MultiPv
            | EngineEnumOption::SyzygyPath => return Err(EngineOptionsError::Descriptor(option)),
        };
        Self::new(value, capabilities)
    }

    pub const fn version(&self) -> EngineCapabilitiesVersion {
        self.version
    }
    pub const fn option(&self) -> EngineEnumOption {
        self.value.option()
    }
    pub const fn value(&self) -> &EngineOptionsEnumValue {
        &self.value
    }

    pub fn command(&self) -> String {
        let option = self.option();
        match self.value.formatted() {
            Some(value) => option.set_value_command(value),
            None => format!("setoption name {}", option.name()),
        }
    }

    pub(crate) fn validate(
        &self,
        capabilities: &EngineCapabilities,
    ) -> Result<(), EngineOptionsError> {
        if self.version != capabilities.version() {
            return Err(EngineCapabilitiesError::UnsupportedVersion(self.version.get()).into());
        }
        self.value.validate(capabilities)
    }

    pub(crate) fn acknowledgment(
        &self,
    ) -> Result<EngineOptionsEnumAcknowledgment, EngineOptionsError> {
        Ok(match self.option() {
            EngineEnumOption::Hash
            | EngineEnumOption::Threads
            | EngineEnumOption::MoveOverhead
            | EngineEnumOption::Chess960
            | EngineEnumOption::MultiPv => {
                let value = self
                    .value
                    .formatted()
                    .ok_or(EngineOptionsError::Descriptor(self.option()))?;
                let suffix = match self.value {
                    EngineOptionsEnumValue::Hash(_) => " MB",
                    EngineOptionsEnumValue::MoveOverhead(_) => " ms",
                    EngineOptionsEnumValue::Threads(_)
                    | EngineOptionsEnumValue::Minimal(_)
                    | EngineOptionsEnumValue::ClearHash
                    | EngineOptionsEnumValue::Chess960(_)
                    | EngineOptionsEnumValue::MultiPv(_)
                    | EngineOptionsEnumValue::SyzygyPath(_) => "",
                };
                EngineOptionsEnumAcknowledgment::Exact(format!(
                    "info string set {} to {value}{suffix}",
                    self.option().name()
                ))
            }
            EngineEnumOption::Minimal => EngineOptionsEnumAcknowledgment::Readiness,
            EngineEnumOption::ClearHash => {
                EngineOptionsEnumAcknowledgment::Exact("info string Hash cleared".to_owned())
            }
            EngineEnumOption::SyzygyPath => EngineOptionsEnumAcknowledgment::Tablebases,
        })
    }

    pub(crate) fn updated_resources(
        &self,
        current: Option<EngineResourceConfig>,
    ) -> Result<Option<EngineResourceConfig>, EngineOptionsError> {
        current
            .map(|current| {
                let (threads, hash, overhead) = match self.value {
                    EngineOptionsEnumValue::Hash(value) => {
                        (current.threads(), value, current.move_overhead())
                    }
                    EngineOptionsEnumValue::Threads(value) => {
                        (value, current.hash_mib(), current.move_overhead())
                    }
                    EngineOptionsEnumValue::MoveOverhead(value) => {
                        (current.threads(), current.hash_mib(), value)
                    }
                    EngineOptionsEnumValue::Minimal(_)
                    | EngineOptionsEnumValue::ClearHash
                    | EngineOptionsEnumValue::Chess960(_)
                    | EngineOptionsEnumValue::MultiPv(_)
                    | EngineOptionsEnumValue::SyzygyPath(_) => return Ok(current),
                };
                Ok(EngineResourceConfig::new(
                    usize::try_from(threads)?,
                    hash,
                    overhead,
                )?)
            })
            .transpose()
    }
}

pub(crate) enum EngineOptionsEnumAcknowledgment {
    Exact(String),
    Readiness,
    Tablebases,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineOptionsEnumTablebases {
    Unconfigured,
    Loaded {
        path: EngineOptionsSyzygyPath,
        pieces: NonZeroU32,
    },
    Unavailable {
        path: EngineOptionsSyzygyPath,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineOptionsEnumOutcome {
    Applied(EngineEnumOption),
    Tablebases(EngineOptionsEnumTablebases),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOptionsApplied {
    resources: Option<EngineResourceConfig>,
    variant: BoardEnumVariant,
    minimal: bool,
    multi_pv: NonZeroU32,
    tablebases: EngineOptionsEnumTablebases,
}

impl EngineOptionsApplied {
    pub(crate) fn new(capabilities: &EngineCapabilities) -> Result<Self, EngineOptionsError> {
        let Some(EngineOptionEnumDescriptor::Check(minimal)) =
            capabilities.descriptor(EngineEnumOption::Minimal)
        else {
            return Err(EngineOptionsError::Descriptor(EngineEnumOption::Minimal));
        };
        let Some(EngineOptionEnumDescriptor::Check(chess960)) =
            capabilities.descriptor(EngineEnumOption::Chess960)
        else {
            return Err(EngineOptionsError::Descriptor(EngineEnumOption::Chess960));
        };
        let Some(EngineOptionEnumDescriptor::Spin(multi_pv)) =
            capabilities.descriptor(EngineEnumOption::MultiPv)
        else {
            return Err(EngineOptionsError::Descriptor(EngineEnumOption::MultiPv));
        };
        let multi_pv = NonZeroU32::new(multi_pv.default_value())
            .ok_or(EngineOptionsError::Descriptor(EngineEnumOption::MultiPv))?;
        Ok(Self {
            resources: None,
            variant: if chess960 {
                BoardEnumVariant::Chess960
            } else {
                BoardEnumVariant::Standard
            },
            minimal,
            multi_pv,
            tablebases: EngineOptionsEnumTablebases::Unconfigured,
        })
    }

    pub const fn resources(&self) -> Option<EngineResourceConfig> {
        self.resources
    }
    pub const fn variant(&self) -> BoardEnumVariant {
        self.variant
    }
    pub const fn is_minimal(&self) -> bool {
        self.minimal
    }
    pub const fn multi_pv(&self) -> NonZeroU32 {
        self.multi_pv
    }
    pub const fn tablebases(&self) -> &EngineOptionsEnumTablebases {
        &self.tablebases
    }

    pub(crate) fn set_resources(&mut self, resources: Option<EngineResourceConfig>) {
        self.resources = resources;
    }

    pub(crate) fn apply(
        &mut self,
        setting: &EngineOptionsSetting,
        outcome: &EngineOptionsEnumOutcome,
        resources: Option<EngineResourceConfig>,
    ) -> Result<(), EngineOptionsError> {
        match setting.value() {
            EngineOptionsEnumValue::Hash(_)
            | EngineOptionsEnumValue::Threads(_)
            | EngineOptionsEnumValue::MoveOverhead(_) => self.resources = resources,
            EngineOptionsEnumValue::Minimal(value) => self.minimal = *value,
            EngineOptionsEnumValue::Chess960(value) => self.variant = *value,
            EngineOptionsEnumValue::MultiPv(value) => {
                self.multi_pv = NonZeroU32::new(*value)
                    .ok_or(EngineOptionsError::Descriptor(EngineEnumOption::MultiPv))?
            }
            EngineOptionsEnumValue::ClearHash => {}
            EngineOptionsEnumValue::SyzygyPath(_) => {
                let EngineOptionsEnumOutcome::Tablebases(status) = outcome else {
                    return Err(EngineOptionsError::Descriptor(EngineEnumOption::SyzygyPath));
                };
                self.tablebases = status.clone();
            }
        }
        Ok(())
    }
}
