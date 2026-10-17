use thiserror::Error;

use crate::clock::duration::ClockDuration;
use crate::engine::capabilities::EngineCapabilities;
use crate::engine::enums::EngineEnumOption;
use crate::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};

#[derive(Debug, Error)]
pub enum EngineConfigurationError {
    #[error(transparent)]
    Resource(#[from] EngineResourceConfigError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineConfigurationEnumBoundary {
    Idle,
    SearchActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineConfigurationEnumStatus {
    InSync,
    PendingNextBoundary(EngineResourceConfig),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfigurationOverrides {
    pub threads: Option<u32>,
    pub hash_mib: Option<u32>,
    pub move_overhead_ms: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfiguration {
    effective: EngineResourceConfig,
    pending: Option<EngineResourceConfig>,
}

impl EngineConfiguration {
    pub const fn new(effective: EngineResourceConfig) -> Self {
        Self {
            effective,
            pending: None,
        }
    }

    pub fn resolve(
        overrides: EngineConfigurationOverrides,
        capabilities: &EngineCapabilities,
    ) -> Result<EngineResourceConfig, EngineConfigurationError> {
        let threads = match overrides.threads {
            Some(value) => value,
            None => {
                EngineResourceConfig::advertised_default(capabilities, EngineEnumOption::Threads)?
            }
        };
        let hash_mib = match overrides.hash_mib {
            Some(value) => value,
            None => EngineResourceConfig::advertised_default(capabilities, EngineEnumOption::Hash)?,
        };
        let overhead_ms = match overrides.move_overhead_ms {
            Some(value) => value,
            None => EngineResourceConfig::advertised_default(
                capabilities,
                EngineEnumOption::MoveOverhead,
            )?,
        };
        let move_overhead = ClockDuration::from_millis(u64::from(overhead_ms))
            .map_err(EngineResourceConfigError::from)?;
        let config = EngineResourceConfig::new(
            usize::try_from(threads).map_err(EngineResourceConfigError::from)?,
            hash_mib,
            move_overhead,
        )?;
        config.validate(capabilities)?;
        Ok(config)
    }

    pub const fn effective(&self) -> EngineResourceConfig {
        self.effective
    }

    pub const fn snapshot(&self) -> EngineResourceConfig {
        self.effective
    }

    pub const fn pending(&self) -> Option<EngineResourceConfig> {
        self.pending
    }

    pub const fn status(&self) -> EngineConfigurationEnumStatus {
        match self.pending {
            Some(pending) => EngineConfigurationEnumStatus::PendingNextBoundary(pending),
            None => EngineConfigurationEnumStatus::InSync,
        }
    }

    pub fn request(
        &mut self,
        requested: EngineResourceConfig,
        boundary: EngineConfigurationEnumBoundary,
    ) -> EngineConfigurationEnumStatus {
        if requested == self.effective {
            self.pending = None;
            return EngineConfigurationEnumStatus::InSync;
        }
        match boundary {
            EngineConfigurationEnumBoundary::Idle => {
                self.effective = requested;
                self.pending = None;
            }
            EngineConfigurationEnumBoundary::SearchActive => {
                self.pending = Some(requested);
            }
        }
        self.status()
    }

    pub fn request_validated(
        &mut self,
        requested: EngineResourceConfig,
        capabilities: &EngineCapabilities,
        boundary: EngineConfigurationEnumBoundary,
    ) -> Result<EngineConfigurationEnumStatus, EngineConfigurationError> {
        requested.validate(capabilities)?;
        Ok(self.request(requested, boundary))
    }

    pub fn reach_boundary(&mut self) -> Option<EngineResourceConfig> {
        let applied = self.pending.take()?;
        self.effective = applied;
        Some(applied)
    }
}
