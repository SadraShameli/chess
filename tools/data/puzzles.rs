use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;

use chess::package::digest::PackageDigest;
use chess::puzzle::csv_import::{PuzzleCsvImport, PuzzleCsvImportEnumContinuation};
use chess::puzzle::identity::{PuzzleDefinitionIdentity, PuzzleDefinitionIdentityError};
use chess::puzzle::selection::PuzzleSelectionDataset;
use chess::storage::dataset_preparation::{
    StorageDatasetPreparationPuzzle, StorageDatasetPreparationPuzzles,
};
use chess::storage::dataset_selection::{
    StorageDatasetSelectionError, StorageDatasetSelectionIdentity,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ToolsDataPuzzlesError {
    #[error("Data file access failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Selection(#[from] StorageDatasetSelectionError),
    #[error(transparent)]
    Identity(#[from] PuzzleDefinitionIdentityError),
}

#[derive(Debug)]
pub(crate) struct ToolsDataPuzzlesSummary {
    pub(crate) accepted: u64,
    pub(crate) rejected: u64,
    pub(crate) unique: usize,
    pub(crate) identical_copies: u64,
    pub(crate) selected: usize,
    pub(crate) digest: PackageDigest,
}

pub(crate) struct ToolsDataPuzzles;

impl ToolsDataPuzzles {
    pub(crate) fn run(
        csv: &Path,
        output: &Path,
        source: &str,
        version: &str,
        prefix: usize,
    ) -> Result<ToolsDataPuzzlesSummary, ToolsDataPuzzlesError> {
        let identity = PuzzleDefinitionIdentity::new(
            source.to_owned(),
            version.to_owned(),
            "dataset".to_owned(),
        )?;
        let mut import = PuzzleCsvImport::new(
            BufReader::new(File::open(csv)?),
            PuzzleSelectionDataset::from_identity(&identity),
        );
        let mut puzzles = StorageDatasetPreparationPuzzles::new();
        let mut accepted: u64 = 0;
        let mut rejected: u64 = 0;
        loop {
            match import.next_row(|| false) {
                Ok(Some(row)) => {
                    accepted += 1;
                    let definition = row.definition();
                    let metadata = definition.metadata();
                    let rating = metadata
                        .difficulty()
                        .map(|rating| rating.rating().to_string());
                    let themes = metadata.themes().map(|themes| {
                        themes
                            .iter()
                            .map(|theme| theme.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    });
                    let handle = definition.identity().puzzle_id().to_owned();
                    puzzles.admit(
                        handle,
                        StorageDatasetPreparationPuzzle {
                            record: format!("{definition:?}"),
                            identity: StorageDatasetSelectionIdentity::puzzle(
                                definition.identity(),
                            ),
                            rating,
                            themes,
                        },
                    )?;
                }
                Ok(None) => break,
                Err(error) => {
                    rejected += 1;
                    if error.continuation() == PuzzleCsvImportEnumContinuation::Terminated {
                        break;
                    }
                }
            }
        }
        let unique = puzzles.len();
        let identical_copies = puzzles.identical_copies();
        let sequence = puzzles.finish()?;
        let selected = sequence.prefix(prefix)?;
        let mut file = BufWriter::new(File::create(output.join("puzzles.txt"))?);
        for candidate in selected.candidates() {
            writeln!(file, "{}", candidate.handle())?;
        }
        writeln!(file, "# sequence-sha256 {}", selected.digest())?;
        file.flush()?;
        Ok(ToolsDataPuzzlesSummary {
            accepted,
            rejected,
            unique,
            identical_copies,
            selected: selected.candidates().len(),
            digest: selected.digest(),
        })
    }
}
