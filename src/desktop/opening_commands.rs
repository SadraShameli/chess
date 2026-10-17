use thiserror::Error;

use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::opening::book::{OpeningBook, OpeningBookError, OpeningBookStudy};
use crate::opening::corpus::OpeningCorpusError;
use crate::opening::index::{OpeningIndexError, OpeningIndexKey};
use crate::storage::opening_repository::{StorageOpeningError, StorageOpeningRepository};

#[derive(Debug, Error)]
pub enum DesktopOpeningCommandsError {
    #[error(transparent)]
    Corpus(#[from] OpeningCorpusError),
    #[error(transparent)]
    Book(#[from] OpeningBookError),
    #[error(transparent)]
    Index(#[from] OpeningIndexError),
    #[error(transparent)]
    Storage(#[from] StorageOpeningError),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopOpeningOutcomes {
    pub white: u64,
    pub draws: u64,
    pub black: u64,
    pub unknown: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopOpeningRatio {
    pub numerator: u64,
    pub denominator: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopOpeningRating {
    pub sum: u64,
    pub samples: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopOpeningContinuation {
    pub movement: BoardMove,
    pub uci: String,
    pub games: u64,
    pub frequency: DesktopOpeningRatio,
    pub outcomes: DesktopOpeningOutcomes,
    pub white_rating: Option<DesktopOpeningRating>,
    pub black_rating: Option<DesktopOpeningRating>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopOpeningSample {
    pub game: u64,
    pub sources: usize,
    pub white: Option<String>,
    pub black: Option<String>,
    pub white_rating: Option<u32>,
    pub black_rating: Option<u32>,
    pub partial_date: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopOpeningExplorerView {
    pub games: u64,
    pub next_move_games: u64,
    pub ending_games: u64,
    pub partial_date_matches: u64,
    pub potential_duplicate_pairs: u64,
    pub outcomes: DesktopOpeningOutcomes,
    pub white_rating: Option<DesktopOpeningRating>,
    pub black_rating: Option<DesktopOpeningRating>,
    pub continuations: Vec<DesktopOpeningContinuation>,
    pub samples: Vec<DesktopOpeningSample>,
    pub next_sample_offset: Option<usize>,
}

pub struct DesktopOpeningCommands;

impl DesktopOpeningCommands {
    pub const SAMPLE_PAGE: usize = 20;

    pub async fn explore_indexed(
        repository: &StorageOpeningRepository,
        position: &BoardPosition,
        sample_offset: usize,
    ) -> Result<DesktopOpeningExplorerView, DesktopOpeningCommandsError> {
        let key = OpeningIndexKey::of(position)?;
        let stored = repository.continuations(key.clone()).await?;
        let wanted = sample_offset.saturating_add(Self::SAMPLE_PAGE + 1);
        let mut hits = Vec::new();
        let mut cursor = None;
        while hits.len() < wanted {
            let page = repository.hits(key.clone(), cursor).await?;
            let Some(last) = page.last() else {
                break;
            };
            cursor = Some(last.cursor());
            hits.extend(page);
        }
        let more = hits.len() > sample_offset.saturating_add(Self::SAMPLE_PAGE);
        let hits: Vec<_> = hits
            .into_iter()
            .skip(sample_offset)
            .take(Self::SAMPLE_PAGE)
            .collect();
        let mut continuations = Vec::new();
        let mut moving: u64 = 0;
        let mut ending: u64 = 0;
        for continuation in &stored {
            if continuation.next_move().is_some() {
                moving = moving.saturating_add(continuation.games());
            } else {
                ending = ending.saturating_add(continuation.games());
            }
        }
        for continuation in &stored {
            let Some(next) = continuation.next_move() else {
                continue;
            };
            continuations.push(DesktopOpeningContinuation {
                movement: position.parse_uci(next)?,
                uci: next.to_owned(),
                games: continuation.games(),
                frequency: DesktopOpeningRatio {
                    numerator: continuation.games(),
                    denominator: moving.max(1),
                },
                outcomes: DesktopOpeningOutcomes {
                    white: 0,
                    draws: 0,
                    black: 0,
                    unknown: continuation.games(),
                },
                white_rating: None,
                black_rating: None,
            });
        }
        let games = moving.saturating_add(ending);
        let samples = hits
            .iter()
            .map(|hit| DesktopOpeningSample {
                game: hit.game(),
                sources: 1,
                white: None,
                black: None,
                white_rating: None,
                black_rating: None,
                partial_date: false,
            })
            .collect();
        Ok(DesktopOpeningExplorerView {
            games,
            next_move_games: moving,
            ending_games: ending,
            partial_date_matches: 0,
            potential_duplicate_pairs: 0,
            outcomes: DesktopOpeningOutcomes {
                white: 0,
                draws: 0,
                black: 0,
                unknown: games,
            },
            white_rating: None,
            black_rating: None,
            continuations,
            samples,
            next_sample_offset: more.then_some(sample_offset.saturating_add(Self::SAMPLE_PAGE)),
        })
    }

    pub fn study_book(
        book: &OpeningBook<'_>,
        position: &BoardPosition,
        cancellation: impl Fn() -> bool,
    ) -> Result<OpeningBookStudy, DesktopOpeningCommandsError> {
        Ok(book.study(position, cancellation)?)
    }
}
