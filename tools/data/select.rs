use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;

use chess::board::r#move::BoardMove;
use chess::notation::game::NotationGame;
use chess::notation::game_identity::NotationGameIdentity;
use chess::notation::pgn_import::{NotationPgnImport, NotationPgnImportEnumRecovery};
use chess::package::digest::PackageDigest;
use chess::storage::dataset_preparation::{
    StorageDatasetPreparationGame, StorageDatasetPreparationGames,
};
use chess::storage::dataset_selection::{
    StorageDatasetSelectionEnumSource, StorageDatasetSelectionError,
    StorageDatasetSelectionIdentity,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ToolsDataSelectError {
    #[error("Data file access failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Selection(#[from] StorageDatasetSelectionError),
    #[error("A source game could not be normalized: {0}")]
    Game(String),
}

#[derive(Debug)]
pub(crate) struct ToolsDataSelectSummary {
    pub(crate) parsed: u64,
    pub(crate) rejected: u64,
    pub(crate) unique: usize,
    pub(crate) master: usize,
    pub(crate) broad: usize,
    pub(crate) master_digest: PackageDigest,
    pub(crate) broad_digest: PackageDigest,
}

pub(crate) struct ToolsDataSelect;

impl ToolsDataSelect {
    pub(crate) fn run(
        pgn: &Path,
        output: &Path,
        broadcast: bool,
        master_prefix: usize,
        broad_prefix: usize,
    ) -> Result<ToolsDataSelectSummary, ToolsDataSelectError> {
        let mut node: u128 = 0;
        let mut parser = NotationPgnImport::new(BufReader::new(File::open(pgn)?), move || {
            node += 1;
            uuid::Uuid::from_u128(node).to_string().parse()
        });
        let source = if broadcast {
            StorageDatasetSelectionEnumSource::Broadcast
        } else {
            StorageDatasetSelectionEnumSource::Standard
        };
        let mut games = StorageDatasetPreparationGames::new();
        let mut parsed: u64 = 0;
        let mut rejected: u64 = 0;
        loop {
            match parser.next_game(&|| false) {
                Ok(Some(game)) => {
                    parsed += 1;
                    let admitted = Self::admit(&game, source)
                        .map_err(|error| ToolsDataSelectError::Game(error.to_string()))?;
                    games.admit(parsed, admitted)?;
                }
                Ok(None) => break,
                Err(error) => {
                    rejected += 1;
                    if error.recovery() == NotationPgnImportEnumRecovery::Terminated {
                        break;
                    }
                }
            }
        }
        let unique = games.len();
        let sequences = games.finish()?;
        let master = sequences.master.prefix(master_prefix)?;
        let broad = sequences.broad.prefix(broad_prefix)?;
        Self::write(
            &output.join("master.txt"),
            master.candidates().iter().map(|c| *c.handle()),
            master.digest(),
        )?;
        Self::write(
            &output.join("broad.txt"),
            broad.candidates().iter().map(|c| *c.handle()),
            broad.digest(),
        )?;
        Ok(ToolsDataSelectSummary {
            parsed,
            rejected,
            unique,
            master: master.candidates().len(),
            broad: broad.candidates().len(),
            master_digest: master.digest(),
            broad_digest: broad.digest(),
        })
    }

    fn admit(
        game: &NotationGame,
        source: StorageDatasetSelectionEnumSource,
    ) -> Result<StorageDatasetPreparationGame, Box<dyn std::error::Error>> {
        let line: Vec<BoardMove> = game
            .tree()
            .path_to(game.mainline_tip())?
            .into_iter()
            .filter_map(|id| game.tree().node(id).map(|node| node.movement()).transpose())
            .collect::<Result<_, _>>()?;
        let metadata = game.metadata();
        let identity = NotationGameIdentity::new(
            game.start().position(),
            &line,
            metadata,
            Some(game.result()),
        )?;
        let extension = |name: &str| {
            metadata
                .extensions()
                .iter()
                .find(|extension| extension.name() == name)
                .map(|extension| extension.value().as_str().to_owned())
        };
        let known = metadata.known();
        let record = format!("{metadata:?}|{}", line.len());
        Ok(StorageDatasetPreparationGame {
            record,
            identity: StorageDatasetSelectionIdentity::game(&identity)?,
            source,
            white_title: extension("WhiteTitle"),
            black_title: extension("BlackTitle"),
            white_elo: known.white_elo.map(|rating| rating.to_string()),
            black_elo: known.black_elo.map(|rating| rating.to_string()),
            time_control: game.time_control().map(|text| text.as_str().to_owned()),
        })
    }

    fn write(
        path: &Path,
        handles: impl Iterator<Item = u64>,
        digest: PackageDigest,
    ) -> io::Result<()> {
        let mut file = BufWriter::new(File::create(path)?);
        for handle in handles {
            writeln!(file, "{handle}")?;
        }
        writeln!(file, "# sequence-sha256 {digest}")?;
        file.flush()
    }
}
