use thiserror::Error;

use crate::board::position::BoardPosition;
use crate::board::r#move::BoardMove;
use crate::opening::book::{OpeningBook, OpeningBookError, OpeningBookStudy};
use crate::opening::corpus::{OpeningCorpus, OpeningCorpusError, OpeningCorpusLimits};
use crate::opening::query::{OpeningQuery, OpeningQueryFilters};
use crate::opening::statistics::{
    OpeningStatistics, OpeningStatisticsAverage, OpeningStatisticsOutcomes, OpeningStatisticsRatio,
    OpeningStatisticsResult,
};
use crate::board::enums::BoardEnumColor;

#[derive(Debug, Error)]
pub enum DesktopOpeningCommandsError {
    #[error(transparent)]
    Corpus(#[from] OpeningCorpusError),
    #[error(transparent)]
    Book(#[from] OpeningBookError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopOpeningOutcomes {
    pub white: u64,
    pub draws: u64,
    pub black: u64,
    pub unknown: u64,
}

impl DesktopOpeningOutcomes {
    fn capture(outcomes: OpeningStatisticsOutcomes) -> Self {
        Self {
            white: outcomes.white(),
            draws: outcomes.draws(),
            black: outcomes.black(),
            unknown: outcomes.unknown(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopOpeningRatio {
    pub numerator: u64,
    pub denominator: u64,
}

impl DesktopOpeningRatio {
    fn capture(ratio: OpeningStatisticsRatio) -> Self {
        Self {
            numerator: ratio.numerator(),
            denominator: ratio.denominator().get(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopOpeningRating {
    pub sum: u64,
    pub samples: u64,
}

impl DesktopOpeningRating {
    fn capture(average: Option<OpeningStatisticsAverage>) -> Option<Self> {
        average.map(|average| Self {
            sum: average.sum(),
            samples: average.samples().get(),
        })
    }
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
    pub fn explore<'a>(
        corpora: &[&'a OpeningCorpus<'a>],
        position: &BoardPosition,
        filters: OpeningQueryFilters,
        limits: OpeningCorpusLimits,
        sample_offset: usize,
        cancelled: impl FnMut() -> bool,
    ) -> Result<DesktopOpeningExplorerView, DesktopOpeningCommandsError> {
        let query = OpeningQuery::new(position, filters)?;
        let result = OpeningStatistics::query(corpora, &query, limits, cancelled)?;
        Ok(Self::view(&result, position, sample_offset)?)
    }

    pub fn study_book(
        book: &OpeningBook<'_>,
        position: &BoardPosition,
        cancellation: impl Fn() -> bool,
    ) -> Result<OpeningBookStudy, DesktopOpeningCommandsError> {
        Ok(book.study(position, cancellation)?)
    }

    fn view(
        result: &OpeningStatisticsResult<'_>,
        position: &BoardPosition,
        sample_offset: usize,
    ) -> Result<DesktopOpeningExplorerView, OpeningCorpusError> {
        let page = result.sample_page(sample_offset)?;
        let samples = page
            .games()
            .map(|game| {
                let known = game.record().metadata().known();
                DesktopOpeningSample {
                    game: game.record().id().value(),
                    sources: game.provenance().len(),
                    white: known.white.as_ref().map(|name| name.as_str().to_owned()),
                    black: known.black.as_ref().map(|name| name.as_str().to_owned()),
                    white_rating: game.rating(BoardEnumColor::White),
                    black_rating: game.rating(BoardEnumColor::Black),
                    partial_date: game.is_partial_date_match(),
                }
            })
            .collect();
        Ok(DesktopOpeningExplorerView {
            games: result.game_count(),
            next_move_games: result.next_move_games(),
            ending_games: result.ending_games(),
            partial_date_matches: result.partial_date_matches(),
            potential_duplicate_pairs: result.potential_duplicate_pairs(),
            outcomes: DesktopOpeningOutcomes::capture(result.outcomes()),
            white_rating: DesktopOpeningRating::capture(result.white_rating()),
            black_rating: DesktopOpeningRating::capture(result.black_rating()),
            continuations: result
                .continuations()
                .iter()
                .map(|continuation| DesktopOpeningContinuation {
                    movement: continuation.movement(),
                    uci: continuation.movement().uci(position.variant()),
                    games: continuation.game_count(),
                    frequency: DesktopOpeningRatio::capture(continuation.frequency()),
                    outcomes: DesktopOpeningOutcomes::capture(continuation.outcomes()),
                    white_rating: DesktopOpeningRating::capture(continuation.white_rating()),
                    black_rating: DesktopOpeningRating::capture(continuation.black_rating()),
                })
                .collect(),
            samples,
            next_sample_offset: page.next_offset(),
        })
    }
}
