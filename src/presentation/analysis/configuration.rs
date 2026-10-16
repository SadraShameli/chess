use std::num::NonZeroU32;

use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError};
use crate::engine::search::limit::{EngineSearchClock, EngineSearchLimit, EngineSearchLimitError};

#[derive(Debug, Error)]
pub enum PresentationAnalysisConfigurationError {
    #[error("{field}: enter a whole non-negative number in {unit}")]
    Number {
        field: &'static str,
        unit: &'static str,
    },
    #[error("Moves to go must be positive, or empty when unspecified")]
    MovesToGo,
    #[error("{field}: {source}")]
    Limit {
        field: &'static str,
        source: EngineSearchLimitError,
    },
    #[error("{field}: {source}")]
    Duration {
        field: &'static str,
        source: ClockDurationError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationAnalysisConfigurationEnumMode {
    Infinite,
    Depth,
    Nodes,
    MoveTime,
    Clock,
}

impl PresentationAnalysisConfigurationEnumMode {
    pub const ALL: [Self; 5] = [
        Self::Infinite,
        Self::Depth,
        Self::Nodes,
        Self::MoveTime,
        Self::Clock,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Infinite => "Infinite",
            Self::Depth => "Depth",
            Self::Nodes => "Nodes",
            Self::MoveTime => "Move time",
            Self::Clock => "Clock",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationAnalysisConfigurationClock {
    pub white_time: String,
    pub black_time: String,
    pub white_increment: String,
    pub black_increment: String,
    pub moves_to_go: String,
}

impl Default for PresentationAnalysisConfigurationClock {
    fn default() -> Self {
        Self {
            white_time: "600000".to_owned(),
            black_time: "600000".to_owned(),
            white_increment: "5000".to_owned(),
            black_increment: "5000".to_owned(),
            moves_to_go: String::new(),
        }
    }
}

impl PresentationAnalysisConfigurationClock {
    fn validate(&self) -> Result<EngineSearchLimit, PresentationAnalysisConfigurationError> {
        let moves = if self.moves_to_go.trim().is_empty() {
            None
        } else {
            Some(
                NonZeroU32::new(PresentationAnalysisConfiguration::number(
                    &self.moves_to_go,
                    "Moves to go",
                    "moves",
                )?)
                .ok_or(PresentationAnalysisConfigurationError::MovesToGo)?,
            )
        };
        let clock = EngineSearchClock::new(
            PresentationAnalysisConfiguration::duration(&self.white_time, "White time")?,
            PresentationAnalysisConfiguration::duration(&self.black_time, "Black time")?,
            PresentationAnalysisConfiguration::duration(&self.white_increment, "White increment")?,
            PresentationAnalysisConfiguration::duration(&self.black_increment, "Black increment")?,
            moves,
        )
        .map_err(|source| PresentationAnalysisConfigurationError::Limit {
            field: "Clock",
            source,
        })?;
        Ok(EngineSearchLimit::clock(clock))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PresentationAnalysisConfiguration {
    #[default]
    Infinite,
    Depth(String),
    Nodes(String),
    MoveTime(String),
    Clock(PresentationAnalysisConfigurationClock),
}

impl PresentationAnalysisConfiguration {
    pub fn for_mode(mode: PresentationAnalysisConfigurationEnumMode) -> Self {
        match mode {
            PresentationAnalysisConfigurationEnumMode::Infinite => Self::Infinite,
            PresentationAnalysisConfigurationEnumMode::Depth => Self::Depth("16".to_owned()),
            PresentationAnalysisConfigurationEnumMode::Nodes => Self::Nodes("100000".to_owned()),
            PresentationAnalysisConfigurationEnumMode::MoveTime => {
                Self::MoveTime("3000".to_owned())
            }
            PresentationAnalysisConfigurationEnumMode::Clock => {
                Self::Clock(PresentationAnalysisConfigurationClock::default())
            }
        }
    }

    pub const fn mode(&self) -> PresentationAnalysisConfigurationEnumMode {
        match self {
            Self::Infinite => PresentationAnalysisConfigurationEnumMode::Infinite,
            Self::Depth(_) => PresentationAnalysisConfigurationEnumMode::Depth,
            Self::Nodes(_) => PresentationAnalysisConfigurationEnumMode::Nodes,
            Self::MoveTime(_) => PresentationAnalysisConfigurationEnumMode::MoveTime,
            Self::Clock(_) => PresentationAnalysisConfigurationEnumMode::Clock,
        }
    }

    pub fn validate(&self) -> Result<EngineSearchLimit, PresentationAnalysisConfigurationError> {
        let (field, result) = match self {
            Self::Infinite => return Ok(EngineSearchLimit::infinite()),
            Self::Depth(value) => (
                "Depth",
                EngineSearchLimit::depth(Self::number(value, "Depth", "plies")?),
            ),
            Self::Nodes(value) => (
                "Nodes",
                EngineSearchLimit::nodes(Self::number(value, "Nodes", "nodes")?),
            ),
            Self::MoveTime(value) => (
                "Move time",
                EngineSearchLimit::move_time(Self::duration(value, "Move time")?),
            ),
            Self::Clock(clock) => return clock.validate(),
        };
        result.map_err(|source| PresentationAnalysisConfigurationError::Limit { field, source })
    }

    fn number<T: std::str::FromStr>(
        value: &str,
        field: &'static str,
        unit: &'static str,
    ) -> Result<T, PresentationAnalysisConfigurationError> {
        let text = value.trim();
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(PresentationAnalysisConfigurationError::Number { field, unit });
        }
        text.parse()
            .map_err(|_error| PresentationAnalysisConfigurationError::Number { field, unit })
    }

    fn duration(
        value: &str,
        field: &'static str,
    ) -> Result<ClockDuration, PresentationAnalysisConfigurationError> {
        ClockDuration::from_millis(Self::number(value, field, "milliseconds")?)
            .map_err(|source| PresentationAnalysisConfigurationError::Duration { field, source })
    }
}
