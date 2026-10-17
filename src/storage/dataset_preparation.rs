use std::collections::HashMap;

use thiserror::Error;

use crate::storage::dataset_selection::{
    StorageDatasetSelection, StorageDatasetSelectionCandidate,
    StorageDatasetSelectionEnumDuplicate, StorageDatasetSelectionEnumMasterEligibility,
    StorageDatasetSelectionEnumSource, StorageDatasetSelectionEnumStratum,
    StorageDatasetSelectionEnumSubset, StorageDatasetSelectionEnumTransition,
    StorageDatasetSelectionError, StorageDatasetSelectionFootprint,
    StorageDatasetSelectionPrefixes, StorageDatasetSelectionReduction,
    StorageDatasetSelectionGameClassification, StorageDatasetSelectionIdentity,
    StorageDatasetSelectionPuzzleClassification, StorageDatasetSelectionSequence,
};

pub struct StorageDatasetPreparationGame {
    pub record: String,
    pub identity: StorageDatasetSelectionIdentity,
    pub source: StorageDatasetSelectionEnumSource,
    pub white_title: Option<String>,
    pub black_title: Option<String>,
    pub white_elo: Option<String>,
    pub black_elo: Option<String>,
    pub time_control: Option<String>,
}

struct StorageDatasetPreparationGameEntry<H> {
    handle: H,
    record: String,
    identity: StorageDatasetSelectionIdentity,
    eligibility: StorageDatasetSelectionEnumMasterEligibility,
    classification: StorageDatasetSelectionGameClassification,
    copies: Vec<H>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageDatasetPreparationEnumAdmission {
    New,
    Duplicate,
    Replaced,
}

pub struct StorageDatasetPreparationGameRecord<H> {
    pub handle: H,
    pub eligibility: StorageDatasetSelectionEnumMasterEligibility,
    pub classification: StorageDatasetSelectionGameClassification,
    pub copies: Vec<H>,
}

pub struct StorageDatasetPreparationGameSequences<H> {
    pub master: StorageDatasetSelectionSequence<H>,
    pub broad: StorageDatasetSelectionSequence<H>,
    pub records: Vec<StorageDatasetPreparationGameRecord<H>>,
}

pub struct StorageDatasetPreparationGames<H> {
    entries: Vec<StorageDatasetPreparationGameEntry<H>>,
    index: HashMap<Vec<u8>, usize>,
}

impl<H> Default for StorageDatasetPreparationGames<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H> StorageDatasetPreparationGames<H> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn admit(
        &mut self,
        handle: H,
        game: StorageDatasetPreparationGame,
    ) -> Result<StorageDatasetPreparationEnumAdmission, StorageDatasetSelectionError> {
        let eligibility = StorageDatasetSelection::master_eligibility(
            game.source,
            game.white_title.as_deref(),
            game.black_title.as_deref(),
        );
        let classification = StorageDatasetSelection::classify_game(
            game.white_elo.as_deref(),
            game.black_elo.as_deref(),
            game.time_control.as_deref(),
        )?;
        let key = game.identity.bytes().to_vec();
        let Some(&position) = self.index.get(&key) else {
            self.index.insert(key, self.entries.len());
            self.entries.push(StorageDatasetPreparationGameEntry {
                handle,
                record: game.record,
                identity: game.identity,
                eligibility,
                classification,
                copies: Vec::new(),
            });
            return Ok(StorageDatasetPreparationEnumAdmission::New);
        };
        let entry = &mut self.entries[position];
        match StorageDatasetSelection::compare_duplicate(
            &entry.identity,
            &entry.record,
            &game.identity,
            &game.record,
        )? {
            StorageDatasetSelectionEnumDuplicate::Replace => {
                let previous = std::mem::replace(&mut entry.handle, handle);
                entry.copies.push(previous);
                entry.record = game.record;
                entry.eligibility = eligibility;
                entry.classification = classification;
                Ok(StorageDatasetPreparationEnumAdmission::Replaced)
            }
            StorageDatasetSelectionEnumDuplicate::Identical
            | StorageDatasetSelectionEnumDuplicate::Keep
            | StorageDatasetSelectionEnumDuplicate::Distinct => {
                entry.copies.push(handle);
                Ok(StorageDatasetPreparationEnumAdmission::Duplicate)
            }
        }
    }
}

impl<H: Clone> StorageDatasetPreparationGames<H> {
    pub fn finish(
        self,
    ) -> Result<StorageDatasetPreparationGameSequences<H>, StorageDatasetSelectionError> {
        let mut master = Vec::new();
        let mut broad = Vec::new();
        let mut records = Vec::with_capacity(self.entries.len());
        for entry in self.entries {
            if entry.eligibility == StorageDatasetSelectionEnumMasterEligibility::Eligible {
                master.push(StorageDatasetSelectionCandidate::new(
                    entry.handle.clone(),
                    entry.identity.clone(),
                    StorageDatasetSelectionEnumStratum::Master,
                )?);
            }
            broad.push(StorageDatasetSelectionCandidate::new(
                entry.handle.clone(),
                entry.identity,
                StorageDatasetSelectionEnumStratum::Game(entry.classification.stratum().clone()),
            )?);
            records.push(StorageDatasetPreparationGameRecord {
                handle: entry.handle,
                eligibility: entry.eligibility,
                classification: entry.classification,
                copies: entry.copies,
            });
        }
        Ok(StorageDatasetPreparationGameSequences {
            master: StorageDatasetSelectionSequence::new(
                StorageDatasetSelectionEnumSubset::Master,
                master,
            )?,
            broad: StorageDatasetSelectionSequence::new(
                StorageDatasetSelectionEnumSubset::Broad,
                broad,
            )?,
            records,
        })
    }
}

pub struct StorageDatasetPreparationPuzzle {
    pub record: String,
    pub identity: StorageDatasetSelectionIdentity,
    pub rating: Option<String>,
    pub themes: Option<String>,
}

struct StorageDatasetPreparationPuzzleEntry<H> {
    handle: H,
    record: String,
    identity: StorageDatasetSelectionIdentity,
    classification: StorageDatasetSelectionPuzzleClassification,
    copies: u64,
}

pub struct StorageDatasetPreparationPuzzles<H> {
    entries: Vec<StorageDatasetPreparationPuzzleEntry<H>>,
    index: HashMap<Vec<u8>, usize>,
}

impl<H> Default for StorageDatasetPreparationPuzzles<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H> StorageDatasetPreparationPuzzles<H> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn identical_copies(&self) -> u64 {
        self.entries.iter().map(|entry| entry.copies).sum()
    }

    pub fn admit(
        &mut self,
        handle: H,
        puzzle: StorageDatasetPreparationPuzzle,
    ) -> Result<StorageDatasetPreparationEnumAdmission, StorageDatasetSelectionError> {
        let key = puzzle.identity.bytes().to_vec();
        if let Some(&position) = self.index.get(&key) {
            let entry = &mut self.entries[position];
            StorageDatasetSelection::compare_duplicate(
                &entry.identity,
                &entry.record,
                &puzzle.identity,
                &puzzle.record,
            )?;
            entry.copies = entry.copies.saturating_add(1);
            return Ok(StorageDatasetPreparationEnumAdmission::Duplicate);
        }
        let classification = StorageDatasetSelection::classify_puzzle(
            puzzle.rating.as_deref(),
            puzzle.themes.as_deref(),
        )?;
        self.index.insert(key, self.entries.len());
        self.entries.push(StorageDatasetPreparationPuzzleEntry {
            handle,
            record: puzzle.record,
            identity: puzzle.identity,
            classification,
            copies: 0,
        });
        Ok(StorageDatasetPreparationEnumAdmission::New)
    }

    pub fn finish(
        self,
    ) -> Result<StorageDatasetSelectionSequence<H>, StorageDatasetSelectionError> {
        let candidates = self
            .entries
            .into_iter()
            .map(|entry| {
                StorageDatasetSelectionCandidate::new(
                    entry.handle,
                    entry.identity,
                    StorageDatasetSelectionEnumStratum::Puzzle(
                        entry.classification.stratum().clone(),
                    ),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        StorageDatasetSelectionSequence::new(StorageDatasetSelectionEnumSubset::Puzzles, candidates)
    }
}

#[derive(Debug, Error)]
pub enum StorageDatasetPreparationBuildError<E: std::error::Error + 'static> {
    #[error(transparent)]
    Selection(#[from] StorageDatasetSelectionError),
    #[error("The dataset builder failed: {0}")]
    Builder(#[source] E),
    #[error("The installed-byte cap loop did not converge within {0} reductions")]
    Diverged(usize),
}

pub trait StorageDatasetPreparationBuilder {
    type Error: std::error::Error + 'static;

    fn build(
        &mut self,
        prefixes: StorageDatasetSelectionPrefixes,
    ) -> Result<StorageDatasetSelectionFootprint, Self::Error>;
}

#[derive(Debug)]
pub struct StorageDatasetPreparationBuildOutcome {
    pub prefixes: StorageDatasetSelectionPrefixes,
    pub footprint: StorageDatasetSelectionFootprint,
    pub reductions: Vec<StorageDatasetSelectionReduction>,
}

pub struct StorageDatasetPreparationBuild;

impl StorageDatasetPreparationBuild {
    pub const MAX_REDUCTIONS: usize = 512;

    pub fn run<B: StorageDatasetPreparationBuilder>(
        builder: &mut B,
        initial: StorageDatasetSelectionPrefixes,
    ) -> Result<
        StorageDatasetPreparationBuildOutcome,
        StorageDatasetPreparationBuildError<B::Error>,
    > {
        let mut prefixes = initial;
        let mut reductions = Vec::new();
        loop {
            let footprint = builder
                .build(prefixes)
                .map_err(StorageDatasetPreparationBuildError::Builder)?;
            match footprint.transition(prefixes)? {
                StorageDatasetSelectionEnumTransition::Accept => {
                    return Ok(StorageDatasetPreparationBuildOutcome {
                        prefixes,
                        footprint,
                        reductions,
                    });
                }
                StorageDatasetSelectionEnumTransition::Reduce(reduction) => {
                    if reductions.len() >= Self::MAX_REDUCTIONS {
                        return Err(StorageDatasetPreparationBuildError::Diverged(
                            Self::MAX_REDUCTIONS,
                        ));
                    }
                    prefixes = reduction.after();
                    reductions.push(reduction);
                }
            }
        }
    }
}
