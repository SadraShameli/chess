use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use chess::storage::dataset_preparation::{
    StorageDatasetPreparationBuild, StorageDatasetPreparationBuildError,
    StorageDatasetPreparationBuilder,
};
use chess::storage::dataset_selection::{
    StorageDatasetSelectionEnumComponent, StorageDatasetSelectionError,
    StorageDatasetSelectionFootprint, StorageDatasetSelectionInstalledArtifact,
    StorageDatasetSelectionPrefixes, StorageDatasetSelectionReduction,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ToolsDataBuildError {
    #[error("Data file access failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Build(#[from] StorageDatasetPreparationBuildError<ToolsDataBuildBuilderError>),
}

#[derive(Debug, Error)]
pub(crate) enum ToolsDataBuildBuilderError {
    #[error("Data file access failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Selection(#[from] StorageDatasetSelectionError),
}

#[derive(Debug)]
pub(crate) struct ToolsDataBuildSummary {
    pub(crate) prefixes: StorageDatasetSelectionPrefixes,
    pub(crate) total_bytes: u64,
    pub(crate) reductions: Vec<StorageDatasetSelectionReduction>,
}

struct ToolsDataBuildBuilder {
    input: PathBuf,
    output: PathBuf,
}

impl ToolsDataBuildBuilder {
    fn write_prefix(
        &self,
        name: &str,
        prefix: u64,
    ) -> Result<u64, ToolsDataBuildBuilderError> {
        let limit = usize::try_from(prefix).unwrap_or(usize::MAX);
        let source = BufReader::new(File::open(self.input.join(name))?);
        let path = self.output.join(name);
        let mut file = BufWriter::new(File::create(&path)?);
        let mut written = 0_usize;
        for line in source.lines() {
            let line = line?;
            if line.starts_with('#') {
                continue;
            }
            if written == limit {
                break;
            }
            writeln!(file, "{line}")?;
            written += 1;
        }
        file.flush()?;
        Ok(std::fs::metadata(path)?.len())
    }

    fn copy_fixed(&self, name: &str) -> Result<u64, ToolsDataBuildBuilderError> {
        let path = self.input.join(name);
        if !path.is_file() {
            return Ok(0);
        }
        Ok(std::fs::copy(path, self.output.join(name))?)
    }
}

impl StorageDatasetPreparationBuilder for ToolsDataBuildBuilder {
    type Error = ToolsDataBuildBuilderError;

    fn build(
        &mut self,
        prefixes: StorageDatasetSelectionPrefixes,
    ) -> Result<StorageDatasetSelectionFootprint, Self::Error> {
        let master = self.write_prefix("master.txt", prefixes.master())?;
        let broad = self.write_prefix("broad.txt", prefixes.broad())?;
        let puzzles = self.write_prefix("puzzles.txt", prefixes.puzzles())?;
        let book = self.copy_fixed("book.bin")?;
        let labels = self.copy_fixed("labels.json")?;
        let measured = [master, broad, puzzles, book, labels];
        let artifacts = StorageDatasetSelectionEnumComponent::ORDER
            .into_iter()
            .zip(measured)
            .map(|(component, bytes)| {
                StorageDatasetSelectionInstalledArtifact::new(
                    format!("{component:?}"),
                    Some(component),
                    bytes,
                    0,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StorageDatasetSelectionFootprint::from_artifacts(artifacts)?)
    }
}

pub(crate) struct ToolsDataBuild;

impl ToolsDataBuild {
    pub(crate) fn run(
        input: &Path,
        output: &Path,
        initial: StorageDatasetSelectionPrefixes,
    ) -> Result<ToolsDataBuildSummary, ToolsDataBuildError> {
        std::fs::create_dir_all(output)?;
        let mut builder = ToolsDataBuildBuilder {
            input: input.to_path_buf(),
            output: output.to_path_buf(),
        };
        let outcome = StorageDatasetPreparationBuild::run(&mut builder, initial)?;
        Ok(ToolsDataBuildSummary {
            prefixes: outcome.prefixes,
            total_bytes: outcome.footprint.total_bytes(),
            reductions: outcome.reductions,
        })
    }
}
