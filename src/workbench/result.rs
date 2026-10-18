use thiserror::Error;

use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::clock::duration::ClockDuration;
use crate::engine::package::manifest::EnginePackageEnumTarget;
use crate::engine::process_provider::EngineProcessLimits;
use crate::workbench::parameters::{
    WorkbenchBenchParameters, WorkbenchPerftDepth, WorkbenchPerftParameters,
};

#[derive(Debug, Error)]
pub enum WorkbenchResultError {
    #[error("Compiler diagnostics must contain bounded single-line compiler and feature fields")]
    CompilerText,
    #[error("Perft divide must contain each legal root move exactly once")]
    PerftRoots,
    #[error("Perft root counts overflow or do not match the reported total")]
    PerftTotal,
    #[error("Perft at depth one must count each root exactly once")]
    PerftDepthOne,
    #[error(
        "A complete built-in benchmark must report all {} positions",
        WorkbenchBenchParameters::POSITIONS
    )]
    BenchPositions,
}

#[derive(Debug)]
pub struct WorkbenchCompilerResult {
    compiler: Box<str>,
    target: EnginePackageEnumTarget,
    target_features: Box<str>,
}

impl WorkbenchCompilerResult {
    pub fn new(
        compiler: String,
        target: EnginePackageEnumTarget,
        target_features: String,
    ) -> Result<Self, WorkbenchResultError> {
        if compiler.trim().is_empty()
            || compiler.contains(['\r', '\n'])
            || target_features.contains(['\r', '\n'])
            || compiler.len().saturating_add(target_features.len())
                > EngineProcessLimits::LINE_BYTES
        {
            return Err(WorkbenchResultError::CompilerText);
        }
        Ok(Self {
            compiler: compiler.into_boxed_str(),
            target,
            target_features: target_features.into_boxed_str(),
        })
    }

    pub fn compiler(&self) -> &str {
        &self.compiler
    }

    pub const fn target(&self) -> EnginePackageEnumTarget {
        self.target
    }

    pub fn target_features(&self) -> &str {
        &self.target_features
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkbenchPerftRoot {
    movement: BoardMove,
    nodes: u64,
}

impl WorkbenchPerftRoot {
    pub const fn new(movement: BoardMove, nodes: u64) -> Self {
        Self { movement, nodes }
    }

    pub const fn movement(self) -> BoardMove {
        self.movement
    }

    pub const fn nodes(self) -> u64 {
        self.nodes
    }
}

#[derive(Debug)]
pub struct WorkbenchPerftResult {
    position: BoardPosition,
    depth: WorkbenchPerftDepth,
    roots: Box<[WorkbenchPerftRoot]>,
    nodes: u64,
    elapsed: ClockDuration,
    nodes_per_second: u64,
}

impl WorkbenchPerftResult {
    pub fn new(
        parameters: &WorkbenchPerftParameters,
        roots: Vec<WorkbenchPerftRoot>,
        nodes: u64,
        elapsed: ClockDuration,
        nodes_per_second: u64,
    ) -> Result<Self, WorkbenchResultError> {
        let position = parameters.position().current();
        let legal = position.legal_moves();
        if roots.len() != legal.len()
            || legal.iter().any(|movement| {
                roots
                    .iter()
                    .filter(|root| root.movement == *movement)
                    .count()
                    != 1
            })
        {
            return Err(WorkbenchResultError::PerftRoots);
        }
        let total = roots.iter().try_fold(0_u64, |sum, root| {
            sum.checked_add(root.nodes)
                .ok_or(WorkbenchResultError::PerftTotal)
        })?;
        if total != nodes {
            return Err(WorkbenchResultError::PerftTotal);
        }
        if parameters.depth().value() == WorkbenchPerftDepth::MINIMUM
            && roots.iter().any(|root| root.nodes != 1)
        {
            return Err(WorkbenchResultError::PerftDepthOne);
        }
        Ok(Self {
            position: position.clone(),
            depth: parameters.depth(),
            roots: roots.into_boxed_slice(),
            nodes,
            elapsed,
            nodes_per_second,
        })
    }

    pub const fn position(&self) -> &BoardPosition {
        &self.position
    }

    pub const fn depth(&self) -> WorkbenchPerftDepth {
        self.depth
    }

    pub fn roots(&self) -> &[WorkbenchPerftRoot] {
        &self.roots
    }

    pub const fn nodes(&self) -> u64 {
        self.nodes
    }

    pub const fn elapsed(&self) -> ClockDuration {
        self.elapsed
    }

    pub const fn nodes_per_second(&self) -> u64 {
        self.nodes_per_second
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkbenchBenchResult {
    positions: u32,
    nodes: u64,
    elapsed: ClockDuration,
    nodes_per_second: u64,
}

impl WorkbenchBenchResult {
    pub fn new(
        positions: u32,
        nodes: u64,
        elapsed: ClockDuration,
        nodes_per_second: u64,
    ) -> Result<Self, WorkbenchResultError> {
        if positions != WorkbenchBenchParameters::POSITIONS {
            return Err(WorkbenchResultError::BenchPositions);
        }
        Ok(Self {
            positions,
            nodes,
            elapsed,
            nodes_per_second,
        })
    }

    pub const fn positions(self) -> u32 {
        self.positions
    }

    pub const fn nodes(self) -> u64 {
        self.nodes
    }

    pub const fn elapsed(self) -> ClockDuration {
        self.elapsed
    }

    pub const fn nodes_per_second(self) -> u64 {
        self.nodes_per_second
    }
}
