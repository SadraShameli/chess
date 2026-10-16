use crate::board::position::BoardPosition;
use crate::clock::duration::ClockDuration;
use crate::engine::analysis::EngineAnalysisStaticEvaluation;
use crate::engine::package::manifest::EnginePackageEnumTarget;
use crate::workbench::enums::{
    WorkbenchEnumOperation, WorkbenchEnumParameters, WorkbenchEnumResult,
};
use crate::workbench::job::WorkbenchJobFailure;
use crate::workbench::parameters::WorkbenchBenchParameters;
use crate::workbench::result::{
    WorkbenchBenchResult, WorkbenchCompilerResult, WorkbenchPerftResult, WorkbenchPerftRoot,
};

pub(super) struct WorkbenchOutput {
    stage: usize,
    compiler: Option<String>,
    roots: Vec<WorkbenchPerftRoot>,
    nodes: u64,
    elapsed: ClockDuration,
    nps: u64,
    positions: u32,
}

impl Default for WorkbenchOutput {
    fn default() -> Self {
        Self {
            stage: 0,
            compiler: None,
            roots: Vec::new(),
            nodes: 0,
            elapsed: ClockDuration::ZERO,
            nps: 0,
            positions: 0,
        }
    }
}

impl WorkbenchOutput {
    pub(super) fn positions(&self) -> u32 {
        self.positions
    }

    pub(super) fn consume(
        &mut self,
        parameters: &WorkbenchEnumParameters,
        target: EnginePackageEnumTarget,
        text: &str,
    ) -> Result<Option<WorkbenchEnumResult>, WorkbenchJobFailure> {
        let operation = parameters.operation();
        let invalid = || WorkbenchJobFailure::InvalidResponse(operation);
        match parameters {
            WorkbenchEnumParameters::Inspector(position) => {
                let expected = Self::inspector(position.current());
                if expected.get(self.stage).map(String::as_str) != Some(text) {
                    return Err(invalid());
                }
                self.stage += 1;
                if self.stage == expected.len() {
                    Ok(Some(WorkbenchEnumResult::Inspector(
                        position.current().clone(),
                    )))
                } else {
                    Ok(None)
                }
            }
            WorkbenchEnumParameters::RawEvaluation(_) => Ok(Some(
                WorkbenchEnumResult::RawEvaluation(EngineAnalysisStaticEvaluation::parse(text)?),
            )),
            WorkbenchEnumParameters::Compiler => {
                let result = match self.stage {
                    0 => {
                        self.compiler = Some(
                            text.strip_prefix("Compiler Version: ")
                                .filter(|value| !value.is_empty())
                                .ok_or_else(invalid)?
                                .to_owned(),
                        );
                        None
                    }
                    1 => {
                        if text.strip_prefix("Compiler Target: ") != Some(target.triple()) {
                            return Err(invalid());
                        }
                        None
                    }
                    2 => {
                        let features = text
                            .strip_prefix("Compiler Features: ")
                            .ok_or_else(invalid)?;
                        Some(WorkbenchEnumResult::Compiler(WorkbenchCompilerResult::new(
                            self.compiler.take().ok_or_else(invalid)?,
                            target,
                            features.to_owned(),
                        )?))
                    }
                    _ => return Err(invalid()),
                };
                self.stage += 1;
                Ok(result)
            }
            WorkbenchEnumParameters::Perft(parameters) => {
                if !self.table(text, operation, Some(parameters.position().current()))? {
                    return Ok(None);
                }
                Ok(Some(WorkbenchEnumResult::Perft(WorkbenchPerftResult::new(
                    parameters,
                    std::mem::take(&mut self.roots),
                    self.nodes,
                    self.elapsed,
                    self.nps,
                )?)))
            }
            WorkbenchEnumParameters::Bench(_) => {
                if !self.table(text, operation, None)? {
                    return Ok(None);
                }
                Ok(Some(WorkbenchEnumResult::Bench(WorkbenchBenchResult::new(
                    self.positions,
                    self.nodes,
                    self.elapsed,
                    self.nps,
                )?)))
            }
        }
    }

    fn inspector(position: &BoardPosition) -> Vec<String> {
        let separator = " +---+---+---+---+---+---+---+---+";
        let mut lines = vec![separator.to_owned()];
        for (index, rank) in position.placement().split('/').enumerate() {
            let mut row = " |".to_owned();
            for symbol in rank.chars() {
                if let Some(empty) = symbol.to_digit(10) {
                    for _ in 0..empty {
                        row.push_str("   |");
                    }
                } else {
                    row.push_str(&format!(" {symbol} |"));
                }
            }
            row.push_str(&format!(" {}", 8 - index));
            lines.push(row);
            lines.push(separator.to_owned());
        }
        lines.extend([
            "   a   b   c   d   e   f   g   h".to_owned(),
            String::new(),
            format!("FEN: {}", position.fen()),
        ]);
        lines
    }

    fn table(
        &mut self,
        text: &str,
        operation: WorkbenchEnumOperation,
        position: Option<&BoardPosition>,
    ) -> Result<bool, WorkbenchJobFailure> {
        let invalid = || WorkbenchJobFailure::InvalidResponse(operation);
        let perft = operation == WorkbenchEnumOperation::Perft;
        let separator = "-".repeat(if perft { 60 } else { 50 });
        let columns = text.split_whitespace().collect::<Vec<_>>();
        match self.stage {
            0 | 2 | 5 => {
                if text != separator {
                    return Err(invalid());
                }
                self.stage += 1;
                Ok(self.stage == 6)
            }
            1 => {
                let expected: &[&str] = if perft {
                    &["Move", "Nodes", "Elapsed", "NPS"]
                } else {
                    &["Nodes", "Elapsed", "NPS"]
                };
                if columns != expected {
                    return Err(invalid());
                }
                self.stage += 1;
                Ok(false)
            }
            3 if text == separator => {
                self.stage = 4;
                Ok(false)
            }
            3 => {
                if perft {
                    let [index, movement, nodes, elapsed, nps, "kN/s"] = columns.as_slice() else {
                        return Err(invalid());
                    };
                    if index.parse::<usize>().ok() != self.roots.len().checked_add(1) {
                        return Err(invalid());
                    }
                    let nodes = nodes.parse::<u64>().map_err(|_error| invalid())?;
                    Self::decimal(elapsed.strip_suffix('s').ok_or_else(invalid)?, operation)?;
                    Self::decimal(nps, operation)?;
                    if self.roots.len() >= position.ok_or_else(invalid)?.legal_moves().len() {
                        return Err(invalid());
                    }
                    self.roots.push(WorkbenchPerftRoot::new(
                        position
                            .ok_or_else(invalid)?
                            .parse_uci(movement)
                            .map_err(|_error| invalid())?,
                        nodes,
                    ));
                } else {
                    let [index, nodes, elapsed, nps, "N/s"] = columns.as_slice() else {
                        return Err(invalid());
                    };
                    if index.parse::<u32>().ok() != Some(self.positions)
                        || self.positions >= WorkbenchBenchParameters::POSITIONS
                    {
                        return Err(invalid());
                    }
                    self.nodes = self
                        .nodes
                        .checked_add(nodes.parse::<u64>().map_err(|_error| invalid())?)
                        .ok_or_else(invalid)?;
                    Self::decimal(elapsed.strip_suffix('s').ok_or_else(invalid)?, operation)?;
                    nps.parse::<u64>().map_err(|_error| invalid())?;
                    self.positions += 1;
                }
                Ok(false)
            }
            4 => {
                let (nodes, elapsed, nps) = if perft {
                    let ["Total", nodes, elapsed, nps, "kN/s"] = columns.as_slice() else {
                        return Err(invalid());
                    };
                    (*nodes, *elapsed, Self::decimal(nps, operation)?)
                } else {
                    let [nodes, elapsed, nps, "N/s"] = columns.as_slice() else {
                        return Err(invalid());
                    };
                    (
                        *nodes,
                        *elapsed,
                        nps.parse::<u64>().map_err(|_error| invalid())?,
                    )
                };
                let nodes = nodes.parse::<u64>().map_err(|_error| invalid())?;
                if !perft
                    && (self.nodes != nodes
                        || self.positions != WorkbenchBenchParameters::POSITIONS)
                {
                    return Err(invalid());
                }
                self.nodes = nodes;
                self.elapsed = ClockDuration::from_millis(Self::decimal(
                    elapsed.strip_suffix('s').ok_or_else(invalid)?,
                    operation,
                )?)
                .map_err(|_error| invalid())?;
                self.nps = nps;
                self.stage = 5;
                Ok(false)
            }
            _ => Err(invalid()),
        }
    }

    fn decimal(text: &str, operation: WorkbenchEnumOperation) -> Result<u64, WorkbenchJobFailure> {
        let invalid = || WorkbenchJobFailure::InvalidResponse(operation);
        let (whole, fraction) = text.split_once('.').ok_or_else(invalid)?;
        if fraction.len() != 3
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return Err(invalid());
        }
        whole
            .parse::<u64>()
            .ok()
            .and_then(|value| value.checked_mul(1000))
            .and_then(|value| value.checked_add(fraction.parse::<u64>().ok()?))
            .ok_or_else(invalid)
    }
}
