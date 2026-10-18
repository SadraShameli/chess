use std::collections::VecDeque;

use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};

#[cfg(test)]
#[path = "../../tests/presentation/timing.rs"]
mod tests;

#[derive(Debug, Error)]
pub enum PresentationTimingError {
    #[error(transparent)]
    Clock(#[from] ClockDurationError),
    #[error("A frame was presented before the previous frame")]
    Reordered,
}

#[derive(Debug, Default)]
pub struct PresentationTimingSampler {
    pending_inputs: VecDeque<ClockInstant>,
    last_presented: Option<ClockInstant>,
    input_latencies: Vec<ClockDuration>,
    frame_intervals: Vec<ClockDuration>,
}

impl PresentationTimingSampler {
    pub const fn new() -> Self {
        Self {
            pending_inputs: VecDeque::new(),
            last_presented: None,
            input_latencies: Vec::new(),
            frame_intervals: Vec::new(),
        }
    }

    pub fn record_input(&mut self, received: ClockInstant) {
        self.pending_inputs.push_back(received);
    }

    pub fn record_presented(
        &mut self,
        presented: ClockInstant,
        active: bool,
    ) -> Result<(), PresentationTimingError> {
        if let Some(previous) = self.last_presented
            && active
        {
            self.frame_intervals.push(
                presented
                    .elapsed_since(previous)
                    .map_err(|_error| PresentationTimingError::Reordered)?,
            );
        }
        self.last_presented = Some(presented);
        while let Some(received) = self.pending_inputs.pop_front() {
            self.input_latencies
                .push(presented.elapsed_since(received)?);
        }
        Ok(())
    }

    pub fn reset_frames(&mut self) {
        self.last_presented = None;
        self.frame_intervals.clear();
    }

    pub fn input_samples(&self) -> usize {
        self.input_latencies.len()
    }

    pub fn frame_samples(&self) -> usize {
        self.frame_intervals.len()
    }

    pub fn nearest_rank(samples: &[ClockDuration], percentile: usize) -> Option<ClockDuration> {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable_by_key(|sample| sample.as_nanos());
        let rank = percentile.checked_mul(sorted.len())?.div_ceil(100);
        sorted.get(rank.checked_sub(1)?).copied()
    }

    pub fn input_p95(&self) -> Option<ClockDuration> {
        Self::nearest_rank(&self.input_latencies, 95)
    }

    pub fn frame_p95(&self) -> Option<ClockDuration> {
        Self::nearest_rank(&self.frame_intervals, 95)
    }

    pub fn to_json(&self) -> String {
        let list = |samples: &[ClockDuration]| {
            let values: Vec<String> = samples
                .iter()
                .map(|sample| {
                    format!(
                        "{}.{:06}",
                        sample.as_nanos() / 1_000_000,
                        sample.as_nanos() % 1_000_000
                    )
                })
                .collect();
            format!("[{}]", values.join(","))
        };
        format!(
            "{{\"input_ms\":{},\"frame_ms\":{}}}",
            list(&self.input_latencies),
            list(&self.frame_intervals)
        )
    }
}
