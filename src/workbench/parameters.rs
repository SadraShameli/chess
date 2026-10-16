use std::num::TryFromIntError;

use thiserror::Error;

use crate::engine::capabilities::EngineCapabilities;
use crate::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};
use crate::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};
use crate::engine::search::limit::{EngineSearchLimit, EngineSearchLimitError};
use crate::engine::search::position::EngineSearchPosition;

#[derive(Debug, Error)]
pub enum WorkbenchParametersError {
    #[error("Workbench perft depth {0} must be within the supported range")]
    PerftDepth(u32),
    #[error(transparent)]
    Resources(#[from] EngineResourceConfigError),
    #[error(transparent)]
    SearchLimit(#[from] EngineSearchLimitError),
    #[error("The workbench resource count is out of range: {0}")]
    Conversion(#[from] TryFromIntError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkbenchPerftDepth(u32);

impl Default for WorkbenchPerftDepth {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

impl WorkbenchPerftDepth {
    pub const MINIMUM: u32 = 1;
    pub const MAXIMUM: u32 = 12;
    pub const DEFAULT: u32 = 4;

    pub fn new(depth: u32) -> Result<Self, WorkbenchParametersError> {
        if !(Self::MINIMUM..=Self::MAXIMUM).contains(&depth) {
            return Err(WorkbenchParametersError::PerftDepth(depth));
        }
        Ok(Self(depth))
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Debug)]
pub struct WorkbenchPerftParameters {
    position: EngineSearchPosition,
    depth: WorkbenchPerftDepth,
}

impl WorkbenchPerftParameters {
    pub const fn new(position: EngineSearchPosition, depth: WorkbenchPerftDepth) -> Self {
        Self { position, depth }
    }

    pub const fn position(&self) -> &EngineSearchPosition {
        &self.position
    }

    pub const fn depth(&self) -> WorkbenchPerftDepth {
        self.depth
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkbenchBenchParameters {
    hash_mib: u32,
    threads: u32,
    workers: usize,
    depth: u32,
}

impl WorkbenchBenchParameters {
    pub const DEFAULT_DEPTH: u32 = 12;
    pub const POSITIONS: u32 = 45;

    pub fn new(
        capabilities: &EngineCapabilities,
        hash_mib: Option<u32>,
        threads: Option<usize>,
        depth: Option<u32>,
    ) -> Result<Self, WorkbenchParametersError> {
        let hash_mib = Self::resource(
            capabilities,
            EngineEnumOption::Hash,
            hash_mib.map(u64::from),
        )?;
        let threads = Self::resource(
            capabilities,
            EngineEnumOption::Threads,
            threads.map(u64::try_from).transpose()?,
        )?;
        let depth = depth.unwrap_or(Self::DEFAULT_DEPTH);
        EngineSearchLimit::depth(depth)?;
        Ok(Self {
            hash_mib,
            threads,
            workers: usize::try_from(threads)?,
            depth,
        })
    }

    pub const fn hash_mib(self) -> u32 {
        self.hash_mib
    }

    pub const fn threads(self) -> u32 {
        self.threads
    }

    pub const fn workers(self) -> usize {
        self.workers
    }

    pub const fn depth(self) -> u32 {
        self.depth
    }

    fn resource(
        capabilities: &EngineCapabilities,
        option: EngineEnumOption,
        value: Option<u64>,
    ) -> Result<u32, EngineResourceConfigError> {
        let descriptor = capabilities
            .descriptor(option)
            .ok_or(EngineResourceConfigError::MissingOption(option))?;
        let EngineOptionEnumDescriptor::Spin(range) = descriptor else {
            return Err(EngineResourceConfigError::NonNumericOption(option));
        };
        EngineResourceConfig::number(
            option,
            value.unwrap_or(u64::from(range.default_value())),
            descriptor,
        )
    }
}
