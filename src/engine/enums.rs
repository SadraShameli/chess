#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EngineEnumOption {
    Hash,
    Threads,
    MoveOverhead,
    Minimal,
    ClearHash,
    Chess960,
    MultiPv,
    SyzygyPath,
}

impl EngineEnumOption {
    const THREADS_BOUND_MULTIPLIER: u32 = 4;

    pub const ALL: [Self; 8] = [
        Self::Hash,
        Self::Threads,
        Self::MoveOverhead,
        Self::Minimal,
        Self::ClearHash,
        Self::Chess960,
        Self::MultiPv,
        Self::SyzygyPath,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Hash => "Hash",
            Self::Threads => "Threads",
            Self::MoveOverhead => "MoveOverhead",
            Self::Minimal => "Minimal",
            Self::ClearHash => "Clear Hash",
            Self::Chess960 => "UCI_Chess960",
            Self::MultiPv => "MultiPV",
            Self::SyzygyPath => "SyzygyPath",
        }
    }

    pub const fn typed_descriptor(self) -> EngineOptionEnumDescriptor {
        match self {
            Self::Hash => {
                EngineOptionEnumDescriptor::Spin(EngineOptionSpinDescriptor::new(16, 1, 262_144))
            }
            Self::Threads => {
                EngineOptionEnumDescriptor::Spin(EngineOptionSpinDescriptor::new(1, 1, 512))
            }
            Self::MoveOverhead => {
                EngineOptionEnumDescriptor::Spin(EngineOptionSpinDescriptor::new(100, 0, 2_000))
            }
            Self::Minimal | Self::Chess960 => EngineOptionEnumDescriptor::Check(false),
            Self::ClearHash => EngineOptionEnumDescriptor::Button,
            Self::MultiPv => {
                EngineOptionEnumDescriptor::Spin(EngineOptionSpinDescriptor::new(1, 1, 256))
            }
            Self::SyzygyPath => EngineOptionEnumDescriptor::String,
        }
    }

    pub fn descriptor(self) -> String {
        self.typed_descriptor().descriptor()
    }

    pub(crate) fn parse_descriptor(self, advertised: &str) -> Option<EngineOptionEnumDescriptor> {
        let mut descriptor = self.typed_descriptor();
        if self == Self::Threads {
            let EngineOptionEnumDescriptor::Spin(range) = descriptor else {
                return None;
            };
            let prefix = format!(
                "spin default {} min {} max ",
                range.default_value(),
                range.minimum()
            );
            let maximum: u32 = advertised.strip_prefix(&prefix)?.parse().ok()?;
            if maximum < range.maximum() || !maximum.is_multiple_of(Self::THREADS_BOUND_MULTIPLIER)
            {
                return None;
            }
            descriptor = EngineOptionEnumDescriptor::Spin(EngineOptionSpinDescriptor::new(
                range.default_value(),
                range.minimum(),
                maximum,
            ));
        }
        (descriptor.descriptor() == advertised).then_some(descriptor)
    }

    pub(crate) fn set_value_command(self, value: impl Display) -> String {
        format!("setoption name {} value {value}", self.name())
    }

    pub(crate) fn acknowledgment(self, value: impl Display) -> String {
        let suffix = match self {
            Self::Hash => " MB",
            Self::MoveOverhead => " ms",
            Self::Threads
            | Self::Minimal
            | Self::ClearHash
            | Self::Chess960
            | Self::MultiPv
            | Self::SyzygyPath => "",
        };
        format!("info string set {} to {value}{suffix}", self.name())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineOptionSpinDescriptor {
    default: u32,
    minimum: u32,
    maximum: u32,
}

impl EngineOptionSpinDescriptor {
    const fn new(default: u32, minimum: u32, maximum: u32) -> Self {
        Self {
            default,
            minimum,
            maximum,
        }
    }

    pub const fn default_value(self) -> u32 {
        self.default
    }

    pub const fn minimum(self) -> u32 {
        self.minimum
    }

    pub const fn maximum(self) -> u32 {
        self.maximum
    }

    pub fn contains(self, value: u64) -> bool {
        (u64::from(self.minimum)..=u64::from(self.maximum)).contains(&value)
    }

    fn descriptor(self) -> String {
        format!(
            "spin default {} min {} max {}",
            self.default, self.minimum, self.maximum
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineOptionEnumDescriptor {
    Spin(EngineOptionSpinDescriptor),
    Check(bool),
    Button,
    String,
}

impl EngineOptionEnumDescriptor {
    fn descriptor(self) -> String {
        match self {
            Self::Spin(spin) => spin.descriptor(),
            Self::Check(default) => format!("check default {default}"),
            Self::Button => "button".to_owned(),
            Self::String => "string default".to_owned(),
        }
    }
}
use std::fmt::Display;
