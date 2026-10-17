use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ToolsBenchmarkRunError {
    #[error("Reading the measurement file failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The measurement file is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("The measurements do not satisfy the acceptance protocol:\n{0}")]
    Failed(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolsBenchmarkRunMeasurements {
    input_ms: Vec<Vec<f64>>,
    frame_ms: Vec<Vec<f64>>,
    stop_ms: Vec<Vec<f64>>,
    startup_ms: Vec<f64>,
    idle_cpu_percent: Vec<f64>,
}

pub(crate) struct ToolsBenchmarkRun;

impl ToolsBenchmarkRun {
    const RUNS: usize = 5;
    const MINIMUM_SAMPLES: usize = 200;
    const INPUT_LIMIT_MS: f64 = 100.0;
    const FRAME_LIMIT_MS: f64 = 16.7;
    const STOP_LIMIT_MS: f64 = 1_000.0;
    const STARTUP_LIMIT_MS: f64 = 3_000.0;
    const IDLE_LIMIT_PERCENT: f64 = 1.0;

    pub(crate) fn evaluate(path: &Path) -> Result<String, ToolsBenchmarkRunError> {
        let measurements: ToolsBenchmarkRunMeasurements =
            serde_json::from_slice(&std::fs::read(path)?)?;
        Self::check(&measurements)
    }

    pub(crate) fn check(
        measurements: &ToolsBenchmarkRunMeasurements,
    ) -> Result<String, ToolsBenchmarkRunError> {
        let mut report = String::new();
        let mut failures = String::new();
        Self::series(
            "input",
            &measurements.input_ms,
            Self::MINIMUM_SAMPLES,
            Self::INPUT_LIMIT_MS,
            &mut report,
            &mut failures,
        );
        Self::series(
            "active frame",
            &measurements.frame_ms,
            1,
            Self::FRAME_LIMIT_MS,
            &mut report,
            &mut failures,
        );
        Self::series(
            "stop",
            &measurements.stop_ms,
            Self::MINIMUM_SAMPLES,
            Self::STOP_LIMIT_MS,
            &mut report,
            &mut failures,
        );
        Self::scalars(
            "startup",
            &measurements.startup_ms,
            Self::STARTUP_LIMIT_MS,
            true,
            &mut report,
            &mut failures,
        );
        Self::scalars(
            "idle CPU",
            &measurements.idle_cpu_percent,
            Self::IDLE_LIMIT_PERCENT,
            false,
            &mut report,
            &mut failures,
        );
        if failures.is_empty() {
            Ok(report)
        } else {
            Err(ToolsBenchmarkRunError::Failed(failures))
        }
    }

    pub(crate) fn nearest_rank(samples: &[f64], percentile: usize) -> Option<f64> {
        let mut sorted: Vec<f64> = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let rank = percentile.checked_mul(sorted.len())?.div_ceil(100);
        sorted.get(rank.checked_sub(1)?).copied()
    }

    fn series(
        name: &str,
        runs: &[Vec<f64>],
        minimum: usize,
        limit: f64,
        report: &mut String,
        failures: &mut String,
    ) {
        if runs.len() != Self::RUNS {
            failures.push_str(&format!("{name}: {} runs supplied, {} required\n", runs.len(), Self::RUNS));
        }
        for (index, samples) in runs.iter().enumerate() {
            let run = index + 1;
            if samples.len() < minimum {
                failures.push_str(&format!("{name} run {run}: {} samples, at least {minimum} required\n",
                    samples.len()));
                continue;
            }
            match Self::nearest_rank(samples, 95) {
                Some(p95) if p95 <= limit => {
                    report.push_str(&format!("{name} run {run}: p95 {p95:.3} ms <= {limit} ms\n"));
                }
                Some(p95) => {
                    failures.push_str(&format!("{name} run {run}: p95 {p95:.3} ms exceeds {limit} ms\n"));
                }
                None => {
                    failures.push_str(&format!("{name} run {run}: no samples\n"));
                }
            }
        }
    }

    fn scalars(
        name: &str,
        values: &[f64],
        limit: f64,
        inclusive: bool,
        report: &mut String,
        failures: &mut String,
    ) {
        if values.len() != Self::RUNS {
            failures.push_str(&format!("{name}: {} values supplied, {} required\n", values.len(), Self::RUNS));
        }
        for (index, value) in values.iter().enumerate() {
            let run = index + 1;
            let passed = if inclusive { *value <= limit } else { *value < limit };
            let line = format!("{name} run {run}: {value:.3} against {limit}");
            let target = if passed { &mut *report } else { &mut *failures };
            target.push_str(&format!("{line}\n"));
        }
    }
}
