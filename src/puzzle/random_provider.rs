use std::num::NonZeroUsize;

use rand::{RngExt, SeedableRng, rngs::Xoshiro256PlusPlus};

pub trait PuzzleRandomProvider {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize;
}

#[derive(Clone)]
pub struct PuzzleRandomProviderSeeded {
    random: Xoshiro256PlusPlus,
}

impl PuzzleRandomProviderSeeded {
    pub fn new(seed: u64) -> Self {
        Self {
            random: Xoshiro256PlusPlus::seed_from_u64(seed),
        }
    }
}

impl PuzzleRandomProvider for PuzzleRandomProviderSeeded {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize {
        self.random.random_range(0..alternatives.get())
    }
}
