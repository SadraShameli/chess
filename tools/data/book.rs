use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use chess::board::r#move::BoardMove;
use chess::board::start::BoardStart;
use chess::notation::game::NotationGame;
use chess::notation::pgn_import::{NotationPgnImport, NotationPgnImportEnumRecovery};
use chess::opening::book::{OpeningBookGenerationError, OpeningBookGenerator};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ToolsDataBookError {
    #[error("Data file access failed: {0}")]
    Io(#[from] io::Error),
    #[error("The master selection line {0:?} is not a game handle")]
    Handle(String),
    #[error("A selected master game could not be replayed: {0}")]
    Game(String),
    #[error(transparent)]
    Generation(#[from] OpeningBookGenerationError),
}

#[derive(Debug)]
pub(crate) struct ToolsDataBookSummary {
    pub(crate) selected: usize,
    pub(crate) standard: usize,
    pub(crate) moves: usize,
    pub(crate) bytes: usize,
}

pub(crate) struct ToolsDataBook;

impl ToolsDataBook {
    pub(crate) const OUTPUT: &str = "book.bin";
    const SELECTION: &str = "master.txt";

    pub(crate) fn run(
        pgn: &Path,
        selection: &Path,
    ) -> Result<ToolsDataBookSummary, ToolsDataBookError> {
        let handles = Self::handles(&selection.join(Self::SELECTION))?;
        let lines = Self::lines(pgn, &handles)?;
        let generated = OpeningBookGenerator::generate(lines.iter().map(Vec::as_slice), || false)?;
        std::fs::write(selection.join(Self::OUTPUT), generated.bytes())?;
        Ok(ToolsDataBookSummary {
            selected: handles.len(),
            standard: lines.len(),
            moves: generated.moves().len(),
            bytes: generated.bytes().len(),
        })
    }

    fn handles(path: &Path) -> Result<BTreeSet<u64>, ToolsDataBookError> {
        let mut handles = BTreeSet::new();
        for line in BufReader::new(File::open(path)?).lines() {
            let line = line?;
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let handle = line
                .trim()
                .parse()
                .map_err(|_error| ToolsDataBookError::Handle(line.clone()))?;
            handles.insert(handle);
        }
        Ok(handles)
    }

    fn lines(
        pgn: &Path,
        handles: &BTreeSet<u64>,
    ) -> Result<Vec<Vec<BoardMove>>, ToolsDataBookError> {
        let mut node: u128 = 0;
        let mut parser = NotationPgnImport::new(BufReader::new(File::open(pgn)?), move || {
            node += 1;
            uuid::Uuid::from_u128(node).to_string().parse()
        });
        let standard = BoardStart::standard();
        let mut parsed: u64 = 0;
        let mut lines = Vec::new();
        loop {
            match parser.next_game(&|| false) {
                Ok(Some(game)) => {
                    parsed += 1;
                    if handles.contains(&parsed) && game.start() == &standard {
                        lines.push(Self::mainline(&game)?);
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    if error.recovery() == NotationPgnImportEnumRecovery::Terminated {
                        break;
                    }
                }
            }
        }
        Ok(lines)
    }

    fn mainline(game: &NotationGame) -> Result<Vec<BoardMove>, ToolsDataBookError> {
        let path = game
            .tree()
            .path_to(game.mainline_tip())
            .map_err(|error| ToolsDataBookError::Game(error.to_string()))?;
        path.into_iter()
            .filter_map(|id| game.tree().node(id).map(|node| node.movement()).transpose())
            .collect::<Result<_, _>>()
            .map_err(|error| ToolsDataBookError::Game(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::ToolsDataBook;

    type ToolsDataBookTestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn only_selected_standard_games_reach_the_generated_book() -> ToolsDataBookTestResult {
        let directory = tempfile::tempdir()?;
        let pgn = directory.path().join("master.pgn");
        std::fs::write(
            &pgn,
            "[Event \"a\"]\n\n1. e4 e5 2. Nf3 1-0\n\n[Event \"b\"]\n\n1. d4 d5 0-1\n\n[Event \"c\"]\n\n1. c4 1/2-1/2\n",
        )?;
        std::fs::write(
            directory.path().join("master.txt"),
            "1\n3\n# sequence-sha256 0\n",
        )?;
        let summary = ToolsDataBook::run(&pgn, directory.path())?;
        assert_eq!(summary.selected, 2);
        assert_eq!(summary.standard, 2);
        assert_eq!(summary.moves, 4);
        assert_eq!(summary.bytes, summary.moves * 16);
        assert_eq!(
            std::fs::metadata(directory.path().join(ToolsDataBook::OUTPUT))?.len(),
            u64::try_from(summary.bytes)?
        );
        Ok(())
    }
}
