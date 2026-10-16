use std::num::NonZeroUsize;

use rand::{RngExt, SeedableRng, rngs::StdRng};

pub trait PuzzleRandomProvider {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize;
}

pub struct PuzzleRandomProviderSeeded {
    random: StdRng,
}

impl PuzzleRandomProviderSeeded {
    pub fn new(seed: u64) -> Self {
        Self {
            random: StdRng::seed_from_u64(seed),
        }
    }
}

impl PuzzleRandomProvider for PuzzleRandomProviderSeeded {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize {
        self.random.random_range(0..alternatives.get())
    }
}
