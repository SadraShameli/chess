use std::num::NonZeroUsize;

use rand::{RngExt, SeedableRng, rngs::Xoshiro256PlusPlus};

pub trait PuzzleRandomProvider {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize;
}

#[derive(Clone)]
pub struct PuzzleRandomProviderSeeded {
    random: Xoshiro256PlusPlus,
    seed: u64,
    fresh: bool,
}

impl PuzzleRandomProviderSeeded {
    pub fn new(seed: u64) -> Self {
        Self {
            random: Xoshiro256PlusPlus::seed_from_u64(seed),
            seed,
            fresh: true,
        }
    }

    pub const ALGORITHM: &str = "Xoshiro256PlusPlus";
    pub const ALGORITHM_VERSION: u32 = 1;

    pub const fn seed(&self) -> u64 {
        self.seed
    }

    pub const fn is_fresh(&self) -> bool {
        self.fresh
    }
}

impl PuzzleRandomProvider for PuzzleRandomProviderSeeded {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize {
        self.fresh = false;
        self.random.random_range(0..alternatives.get())
    }
}
