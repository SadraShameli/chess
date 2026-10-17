use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hash};
use std::num::NonZeroUsize;

use crate::opening::corpus::OpeningCorpusError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningCorpusLimits {
    max_records: NonZeroUsize,
    max_plies: NonZeroUsize,
    max_bytes: NonZeroUsize,
    max_steps: NonZeroUsize,
}

impl OpeningCorpusLimits {
    const CANONICAL_TEXT_RESERVATION_FACTOR: usize = 32;
    pub(crate) const CANONICAL_MOVE_RESERVATION_BYTES: usize = 32;

    pub const fn new(
        max_records: NonZeroUsize,
        max_plies: NonZeroUsize,
        max_bytes: NonZeroUsize,
        max_steps: NonZeroUsize,
    ) -> Self {
        Self {
            max_records,
            max_plies,
            max_bytes,
            max_steps,
        }
    }
    pub const fn max_records(self) -> NonZeroUsize {
        self.max_records
    }
    pub const fn max_plies(self) -> NonZeroUsize {
        self.max_plies
    }
    pub const fn max_bytes(self) -> NonZeroUsize {
        self.max_bytes
    }
    pub const fn max_steps(self) -> NonZeroUsize {
        self.max_steps
    }

    pub(crate) fn validate_records(self, actual: usize) -> Result<(), OpeningCorpusError> {
        if actual > self.max_records.get() {
            return Err(OpeningCorpusError::RecordLimit {
                actual,
                limit: self.max_records.get(),
            });
        }
        Ok(())
    }

    pub(crate) fn validate_plies(self, actual: usize) -> Result<(), OpeningCorpusError> {
        if actual > self.max_plies.get() {
            return Err(OpeningCorpusError::PlyLimit {
                actual,
                limit: self.max_plies.get(),
            });
        }
        Ok(())
    }

    pub(crate) fn canonical_text_bytes(input_bytes: usize) -> Result<usize, OpeningCorpusError> {
        input_bytes
            .checked_mul(Self::CANONICAL_TEXT_RESERVATION_FACTOR)
            .and_then(|bytes| bytes.checked_add(64))
            .ok_or(OpeningCorpusError::Overflow)
    }
}

pub(crate) struct OpeningCorpusBudget<F> {
    limits: OpeningCorpusLimits,
    bytes: usize,
    steps: usize,
    cancelled: F,
}

impl<F: FnMut() -> bool> OpeningCorpusBudget<F> {
    pub(crate) const fn new(limits: OpeningCorpusLimits, cancelled: F) -> Self {
        Self {
            limits,
            bytes: 0,
            steps: 0,
            cancelled,
        }
    }

    pub(crate) fn checkpoint(&mut self) -> Result<(), OpeningCorpusError> {
        if (self.cancelled)() {
            return Err(OpeningCorpusError::Cancelled);
        }
        self.steps = self
            .steps
            .checked_add(1)
            .ok_or(OpeningCorpusError::Overflow)?;
        if self.steps > self.limits.max_steps.get() {
            return Err(OpeningCorpusError::WorkLimit);
        }
        Ok(())
    }

    pub(crate) fn charge(&mut self, bytes: usize) -> Result<(), OpeningCorpusError> {
        let required = self
            .bytes
            .checked_add(bytes)
            .ok_or(OpeningCorpusError::Overflow)?;
        if required > self.limits.max_bytes.get() {
            return Err(OpeningCorpusError::MemoryLimit {
                required,
                limit: self.limits.max_bytes.get(),
            });
        }
        self.bytes = required;
        Ok(())
    }

    pub(crate) fn reserve<T>(
        &mut self,
        values: &mut Vec<T>,
        additional: usize,
    ) -> Result<(), OpeningCorpusError> {
        let required = values
            .len()
            .checked_add(additional)
            .ok_or(OpeningCorpusError::Overflow)?;
        if required <= values.capacity() {
            return Ok(());
        }
        let required = required
            .checked_next_power_of_two()
            .ok_or(OpeningCorpusError::Overflow)?;
        let previous = values.capacity();
        self.charge(
            required
                .checked_sub(previous)
                .and_then(|count| count.checked_mul(std::mem::size_of::<T>()))
                .and_then(|bytes| bytes.checked_mul(2))
                .and_then(|bytes| bytes.checked_add(64))
                .ok_or(OpeningCorpusError::Overflow)?,
        )?;
        values.try_reserve_exact(required - values.len())?;
        if values.capacity() > required {
            self.charge(
                (values.capacity() - required)
                    .checked_mul(std::mem::size_of::<T>())
                    .and_then(|bytes| bytes.checked_mul(2))
                    .ok_or(OpeningCorpusError::Overflow)?,
            )?;
        }
        Ok(())
    }

    pub(crate) fn reserve_map<K: Hash + Eq, V, S: BuildHasher>(
        &mut self,
        values: &mut HashMap<K, V, S>,
        count: usize,
    ) -> Result<(), OpeningCorpusError> {
        let buckets = count
            .checked_next_power_of_two()
            .and_then(|count| count.checked_mul(4))
            .ok_or(OpeningCorpusError::Overflow)?;
        self.charge(
            buckets
                .checked_mul(std::mem::size_of::<(K, V)>() + 1)
                .and_then(|bytes| bytes.checked_add(64))
                .ok_or(OpeningCorpusError::Overflow)?,
        )?;
        values.try_reserve(count)?;
        Ok(())
    }

    pub(crate) fn sort<T: Copy>(
        &mut self,
        values: &mut [T],
        mut compare: impl FnMut(&T, &T) -> Ordering,
    ) -> Result<(), OpeningCorpusError> {
        if values.len() < 2 {
            return Ok(());
        }
        let mut scratch = Vec::new();
        self.reserve(&mut scratch, values.len())?;
        for value in values.iter() {
            self.checkpoint()?;
            scratch.push(*value);
        }
        let mut width = 1_usize;
        while width < values.len() {
            let mut start = 0;
            while start < values.len() {
                let middle = start.saturating_add(width).min(values.len());
                let end = middle.saturating_add(width).min(values.len());
                let (mut left, mut right) = (start, middle);
                for target in &mut scratch[start..end] {
                    self.checkpoint()?;
                    let take_left = right == end
                        || (left < middle
                            && compare(&values[left], &values[right]) != Ordering::Greater);
                    *target = if take_left {
                        let value = values[left];
                        left += 1;
                        value
                    } else {
                        let value = values[right];
                        right += 1;
                        value
                    };
                }
                start = end;
            }
            for (target, source) in values.iter_mut().zip(&scratch) {
                self.checkpoint()?;
                *target = *source;
            }
            width = width.saturating_mul(2);
        }
        Ok(())
    }
}
