use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;

use thiserror::Error;

use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::{PuzzleDefinitionDifficulty, PuzzleDefinitionTheme};
use crate::puzzle::random_provider::PuzzleRandomProvider;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PuzzleSelectionError {
    #[error("The minimum puzzle difficulty {minimum:?} exceeds maximum {maximum:?}")]
    InvalidDifficultyRange {
        minimum: PuzzleDefinitionDifficulty,
        maximum: PuzzleDefinitionDifficulty,
    },
    #[error("The puzzle filter theme {theme:?} is selected more than once")]
    DuplicateTheme { theme: PuzzleDefinitionTheme },
    #[error("The puzzle identity {identity:?} belongs to a different dataset source or version")]
    DatasetMismatch { identity: PuzzleDefinitionIdentity },
    #[error("The puzzle identity {identity:?} occurs more than once in the collection")]
    DuplicateIdentity { identity: PuzzleDefinitionIdentity },
    #[error("The available puzzles do not match the saved filtered collection")]
    CollectionMismatch,
    #[error("The saved puzzle order must contain every frozen identity exactly once")]
    InvalidOrder,
    #[error("The saved puzzle cursor exceeds the frozen collection")]
    InvalidCursor,
    #[error("The saved visited puzzles must be the order prefix before the cursor")]
    InvalidVisited,
    #[error("The saved shuffle decisions do not match the frozen puzzle order")]
    InvalidShuffleRandomness,
    #[error("The random provider selected {choice}, outside {alternatives} alternatives")]
    InvalidRandomChoice {
        alternatives: NonZeroUsize,
        choice: usize,
    },
    #[error("Random replay differs at draw {draw}: expected {expected}, received {actual}")]
    RandomReplayMismatch {
        draw: usize,
        expected: usize,
        actual: usize,
    },
    #[error("The puzzle collection must be exhausted before reshuffling")]
    CollectionNotExhausted,
    #[error("No puzzles match the frozen filters")]
    NoMatchingPuzzles,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleSelectionDataset {
    source: String,
    version: String,
}

impl PuzzleSelectionDataset {
    pub fn from_identity(identity: &PuzzleDefinitionIdentity) -> Self {
        Self {
            source: identity.source().to_owned(),
            version: identity.version().to_owned(),
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    fn validate(&self, identity: &PuzzleDefinitionIdentity) -> Result<(), PuzzleSelectionError> {
        if identity.source() != self.source || identity.version() != self.version {
            return Err(PuzzleSelectionError::DatasetMismatch {
                identity: identity.clone(),
            });
        }
        Ok(())
    }

    fn validate_collection<'a>(
        &self,
        identities: impl IntoIterator<Item = &'a PuzzleDefinitionIdentity>,
    ) -> Result<(), PuzzleSelectionError> {
        let identities = identities.into_iter();
        let mut seen = HashSet::with_capacity(identities.size_hint().0);
        for identity in identities {
            self.validate(identity)?;
            if !seen.insert(identity) {
                return Err(PuzzleSelectionError::DuplicateIdentity {
                    identity: identity.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleSelectionDifficulty {
    minimum: PuzzleDefinitionDifficulty,
    maximum: PuzzleDefinitionDifficulty,
}

impl PuzzleSelectionDifficulty {
    pub fn new(
        minimum: PuzzleDefinitionDifficulty,
        maximum: PuzzleDefinitionDifficulty,
    ) -> Result<Self, PuzzleSelectionError> {
        if minimum > maximum {
            return Err(PuzzleSelectionError::InvalidDifficultyRange { minimum, maximum });
        }
        Ok(Self { minimum, maximum })
    }

    pub const fn minimum(self) -> PuzzleDefinitionDifficulty {
        self.minimum
    }

    pub const fn maximum(self) -> PuzzleDefinitionDifficulty {
        self.maximum
    }

    fn contains(self, difficulty: PuzzleDefinitionDifficulty) -> bool {
        (self.minimum..=self.maximum).contains(&difficulty)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PuzzleSelectionFilter {
    difficulty: Option<PuzzleSelectionDifficulty>,
    themes: Vec<PuzzleDefinitionTheme>,
}

impl PuzzleSelectionFilter {
    pub fn new(
        difficulty: Option<PuzzleSelectionDifficulty>,
        themes: Vec<PuzzleDefinitionTheme>,
    ) -> Result<Self, PuzzleSelectionError> {
        let mut seen = HashSet::with_capacity(themes.len());
        for theme in &themes {
            if !seen.insert(theme) {
                return Err(PuzzleSelectionError::DuplicateTheme {
                    theme: theme.clone(),
                });
            }
        }
        Ok(Self { difficulty, themes })
    }

    pub const fn difficulty(&self) -> Option<PuzzleSelectionDifficulty> {
        self.difficulty
    }

    pub fn themes(&self) -> &[PuzzleDefinitionTheme] {
        &self.themes
    }

    fn matches(&self, definition: &PuzzleDefinition) -> bool {
        let metadata = definition.metadata();
        if let Some(range) = self.difficulty
            && !metadata
                .difficulty()
                .is_some_and(|difficulty| range.contains(difficulty))
        {
            return false;
        }
        self.themes.is_empty()
            || metadata
                .themes()
                .is_some_and(|themes| self.themes.iter().any(|theme| themes.contains(theme)))
    }

    fn freeze(
        &self,
        dataset: &PuzzleSelectionDataset,
        mut definitions: Vec<PuzzleDefinition>,
    ) -> Result<Vec<PuzzleDefinition>, PuzzleSelectionError> {
        dataset.validate_collection(definitions.iter().map(PuzzleDefinition::identity))?;
        definitions.retain(|definition| self.matches(definition));
        Ok(definitions)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleSelectionEnumStatus {
    Ready,
    Exhausted,
    NoMatches,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleSelectionRandomDraw {
    alternatives: NonZeroUsize,
    choice: usize,
}

impl PuzzleSelectionRandomDraw {
    pub fn new(alternatives: NonZeroUsize, choice: usize) -> Result<Self, PuzzleSelectionError> {
        if choice >= alternatives.get() {
            return Err(PuzzleSelectionError::InvalidRandomChoice {
                alternatives,
                choice,
            });
        }
        Ok(Self {
            alternatives,
            choice,
        })
    }

    pub const fn alternatives(self) -> NonZeroUsize {
        self.alternatives
    }

    pub const fn choice(self) -> usize {
        self.choice
    }
}

#[derive(Clone)]
pub struct PuzzleSelectionRandom<R> {
    provider: R,
    draws: Vec<PuzzleSelectionRandomDraw>,
    failure: Option<PuzzleSelectionError>,
}

impl<R: PuzzleRandomProvider> PuzzleSelectionRandom<R> {
    fn new(provider: R) -> Self {
        Self {
            provider,
            draws: Vec::new(),
            failure: None,
        }
    }

    fn restore(
        mut provider: R,
        draws: Vec<PuzzleSelectionRandomDraw>,
    ) -> Result<Self, PuzzleSelectionError> {
        for (draw, expected) in draws.iter().enumerate() {
            let actual = provider.index(expected.alternatives);
            if actual != expected.choice {
                return Err(PuzzleSelectionError::RandomReplayMismatch {
                    draw,
                    expected: expected.choice,
                    actual,
                });
            }
        }
        Ok(Self {
            provider,
            draws,
            failure: None,
        })
    }

    pub fn transaction<T, E>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E>
    where
        R: Clone,
    {
        let provider = self.provider.clone();
        let failure = self.failure.clone();
        let length = self.draws.len();
        let result = operation(self);
        if result.is_err() {
            self.provider = provider;
            self.failure = failure;
            self.draws.truncate(length);
        }
        result
    }

    fn check(&self) -> Result<(), PuzzleSelectionError> {
        match &self.failure {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    fn shuffle(&mut self, order: &mut [usize]) -> Result<(), PuzzleSelectionError> {
        self.check()?;
        for upper in (1..order.len()).rev() {
            let alternatives = NonZeroUsize::new(upper + 1)
                .ok_or(PuzzleSelectionError::InvalidShuffleRandomness)?;
            let choice = self.index(alternatives);
            self.check()?;
            order.swap(upper, choice);
        }
        Ok(())
    }
}

impl<R: PuzzleRandomProvider> PuzzleRandomProvider for PuzzleSelectionRandom<R> {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize {
        let choice = self.provider.index(alternatives);
        match PuzzleSelectionRandomDraw::new(alternatives, choice) {
            Ok(draw) => self.draws.push(draw),
            Err(error) => {
                if self.failure.is_none() {
                    self.failure = Some(error);
                }
            }
        }
        choice
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleSelectionSnapshotOptions {
    pub dataset: PuzzleSelectionDataset,
    pub filters: PuzzleSelectionFilter,
    pub collection: Vec<PuzzleDefinitionIdentity>,
    pub order: Vec<PuzzleDefinitionIdentity>,
    pub visited: Vec<PuzzleDefinitionIdentity>,
    pub cursor: usize,
    pub shuffle_start: usize,
    pub randomness: Vec<PuzzleSelectionRandomDraw>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleSelectionSnapshot {
    options: PuzzleSelectionSnapshotOptions,
}

impl PuzzleSelectionSnapshot {
    pub fn from_persisted(
        options: PuzzleSelectionSnapshotOptions,
    ) -> Result<Self, PuzzleSelectionError> {
        options.dataset.validate_collection(&options.collection)?;
        let collection: HashSet<_> = options.collection.iter().collect();
        let order: HashSet<_> = options.order.iter().collect();
        if options.order.len() != options.collection.len() || order != collection {
            return Err(PuzzleSelectionError::InvalidOrder);
        }
        let visited = options
            .order
            .get(..options.cursor)
            .ok_or(PuzzleSelectionError::InvalidCursor)?;
        if options.visited != visited {
            return Err(PuzzleSelectionError::InvalidVisited);
        }
        Self::validate_shuffle(&options)?;
        Ok(Self { options })
    }

    pub const fn persisted(&self) -> &PuzzleSelectionSnapshotOptions {
        &self.options
    }

    fn validate_shuffle(
        options: &PuzzleSelectionSnapshotOptions,
    ) -> Result<(), PuzzleSelectionError> {
        let length = options.collection.len();
        let end = options
            .shuffle_start
            .checked_add(length.saturating_sub(1))
            .ok_or(PuzzleSelectionError::InvalidShuffleRandomness)?;
        let draws = options
            .randomness
            .get(options.shuffle_start..end)
            .ok_or(PuzzleSelectionError::InvalidShuffleRandomness)?;
        let mut expected: Vec<_> = options.collection.iter().collect();
        for (upper, draw) in (1..length).rev().zip(draws) {
            if draw.alternatives.get() != upper + 1 {
                return Err(PuzzleSelectionError::InvalidShuffleRandomness);
            }
            expected.swap(upper, draw.choice);
        }
        if !expected.iter().copied().eq(&options.order) {
            return Err(PuzzleSelectionError::InvalidShuffleRandomness);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct PuzzleSelection<R> {
    dataset: PuzzleSelectionDataset,
    filters: PuzzleSelectionFilter,
    definitions: Vec<PuzzleDefinition>,
    order: Vec<usize>,
    cursor: usize,
    shuffle_start: usize,
    random: PuzzleSelectionRandom<R>,
}

impl<R: PuzzleRandomProvider> PuzzleSelection<R> {
    pub fn new(
        dataset: PuzzleSelectionDataset,
        definitions: Vec<PuzzleDefinition>,
        filters: PuzzleSelectionFilter,
        random: R,
    ) -> Result<Self, PuzzleSelectionError> {
        let definitions = filters.freeze(&dataset, definitions)?;
        let mut order: Vec<_> = (0..definitions.len()).collect();
        let mut random = PuzzleSelectionRandom::new(random);
        random.shuffle(&mut order)?;
        Ok(Self {
            dataset,
            filters,
            definitions,
            order,
            cursor: 0,
            shuffle_start: 0,
            random,
        })
    }

    pub fn restore(
        definitions: Vec<PuzzleDefinition>,
        snapshot: PuzzleSelectionSnapshot,
        random: R,
    ) -> Result<Self, PuzzleSelectionError> {
        let saved = snapshot.options;
        let definitions = saved.filters.freeze(&saved.dataset, definitions)?;
        let mut available: HashMap<_, _> = definitions
            .into_iter()
            .map(|definition| (definition.identity().clone(), definition))
            .collect();
        if available.len() != saved.collection.len() {
            return Err(PuzzleSelectionError::CollectionMismatch);
        }
        let definitions = saved
            .collection
            .iter()
            .map(|identity| {
                available
                    .remove(identity)
                    .ok_or(PuzzleSelectionError::CollectionMismatch)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let indices: HashMap<_, _> = saved
            .collection
            .iter()
            .enumerate()
            .map(|(index, identity)| (identity, index))
            .collect();
        let order = saved
            .order
            .iter()
            .map(|identity| {
                indices
                    .get(identity)
                    .copied()
                    .ok_or(PuzzleSelectionError::InvalidOrder)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            dataset: saved.dataset,
            filters: saved.filters,
            definitions,
            order,
            cursor: saved.cursor,
            shuffle_start: saved.shuffle_start,
            random: PuzzleSelectionRandom::restore(random, saved.randomness)?,
        })
    }

    pub const fn dataset(&self) -> &PuzzleSelectionDataset {
        &self.dataset
    }

    pub const fn filters(&self) -> &PuzzleSelectionFilter {
        &self.filters
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    pub fn status(&self) -> PuzzleSelectionEnumStatus {
        if self.is_empty() {
            PuzzleSelectionEnumStatus::NoMatches
        } else if self.cursor == self.order.len() {
            PuzzleSelectionEnumStatus::Exhausted
        } else {
            PuzzleSelectionEnumStatus::Ready
        }
    }

    pub fn current(&self) -> Option<&PuzzleDefinition> {
        self.order
            .get(self.cursor)
            .and_then(|index| self.definitions.get(*index))
    }

    pub fn advance(&mut self) -> PuzzleSelectionEnumStatus {
        if self.cursor < self.order.len() {
            self.cursor += 1;
        }
        self.status()
    }

    pub fn reshuffle_and_continue(
        &mut self,
    ) -> Result<PuzzleSelectionEnumStatus, PuzzleSelectionError>
    where
        R: Clone,
    {
        match self.status() {
            PuzzleSelectionEnumStatus::Ready => {
                return Err(PuzzleSelectionError::CollectionNotExhausted);
            }
            PuzzleSelectionEnumStatus::NoMatches => {
                return Err(PuzzleSelectionError::NoMatchingPuzzles);
            }
            PuzzleSelectionEnumStatus::Exhausted => {}
        }
        let shuffle_start = self.random.draws.len();
        let mut order: Vec<_> = (0..self.len()).collect();
        self.random
            .transaction(|random| random.shuffle(&mut order))?;
        self.order = order;
        self.cursor = 0;
        self.shuffle_start = shuffle_start;
        Ok(self.status())
    }

    pub fn random_mut(&mut self) -> &mut PuzzleSelectionRandom<R> {
        &mut self.random
    }

    pub fn snapshot(&self) -> Result<PuzzleSelectionSnapshot, PuzzleSelectionError> {
        self.random.check()?;
        let collection: Vec<_> = self
            .definitions
            .iter()
            .map(|definition| definition.identity().clone())
            .collect();
        let order: Vec<_> = self
            .order
            .iter()
            .map(|index| collection[*index].clone())
            .collect();
        Ok(PuzzleSelectionSnapshot {
            options: PuzzleSelectionSnapshotOptions {
                dataset: self.dataset.clone(),
                filters: self.filters.clone(),
                collection,
                visited: order[..self.cursor].to_vec(),
                order,
                cursor: self.cursor,
                shuffle_start: self.shuffle_start,
                randomness: self.random.draws.clone(),
            },
        })
    }
}
