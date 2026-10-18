use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
use std::num::NonZeroU64;

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::board::r#move::BoardMove;
use crate::board::position::BoardPositionIdentity;
use crate::game::enums::GameEnumResult;
use crate::notation::game_identity::{
    NotationGameIdentity, NotationGameIdentityEnumMatch, NotationGameIdentityEnumResult,
};
use crate::opening::corpus::{
    OpeningCorpus, OpeningCorpusBudget, OpeningCorpusEnumMetadata, OpeningCorpusError,
    OpeningCorpusGameId, OpeningCorpusLimits, OpeningCorpusMetadataSupport, OpeningCorpusRecord,
};
use crate::opening::query::OpeningQuery;
use crate::storage::dataset_package::StorageDatasetPackageSourceIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningStatisticsRatio {
    numerator: u64,
    denominator: NonZeroU64,
}

impl OpeningStatisticsRatio {
    fn new(numerator: u64, denominator: u64) -> Option<Self> {
        NonZeroU64::new(denominator).map(|denominator| Self {
            numerator,
            denominator,
        })
    }
    pub const fn numerator(self) -> u64 {
        self.numerator
    }
    pub const fn denominator(self) -> NonZeroU64 {
        self.denominator
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpeningStatisticsOutcomes {
    white: u64,
    draws: u64,
    black: u64,
    unknown: u64,
}

impl OpeningStatisticsOutcomes {
    fn add(
        &mut self,
        result: Option<NotationGameIdentityEnumResult>,
    ) -> Result<(), OpeningCorpusError> {
        let count = match result {
            Some(NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::White,
            ))) => &mut self.white,
            Some(NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw)) => {
                &mut self.draws
            }
            Some(NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::Black,
            ))) => &mut self.black,
            Some(NotationGameIdentityEnumResult::Unfinished) | None => &mut self.unknown,
        };
        *count = count.checked_add(1).ok_or(OpeningCorpusError::Overflow)?;
        Ok(())
    }
    pub const fn white(self) -> u64 {
        self.white
    }
    pub const fn draws(self) -> u64 {
        self.draws
    }
    pub const fn black(self) -> u64 {
        self.black
    }
    pub const fn unknown(self) -> u64 {
        self.unknown
    }
    pub const fn known(self) -> u64 {
        self.white + self.draws + self.black
    }
    pub fn white_share(self) -> Option<OpeningStatisticsRatio> {
        OpeningStatisticsRatio::new(self.white, self.known())
    }
    pub fn draw_share(self) -> Option<OpeningStatisticsRatio> {
        OpeningStatisticsRatio::new(self.draws, self.known())
    }
    pub fn black_share(self) -> Option<OpeningStatisticsRatio> {
        OpeningStatisticsRatio::new(self.black, self.known())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningStatisticsAverage {
    sum: u64,
    samples: NonZeroU64,
}

impl OpeningStatisticsAverage {
    pub const fn sum(self) -> u64 {
        self.sum
    }
    pub const fn samples(self) -> NonZeroU64 {
        self.samples
    }
}

#[derive(Debug, Default)]
struct OpeningStatisticsRating {
    sum: u64,
    samples: u64,
}

impl OpeningStatisticsRating {
    fn add(&mut self, value: Option<u32>) -> Result<(), OpeningCorpusError> {
        if let Some(value) = value {
            self.sum = self
                .sum
                .checked_add(u64::from(value))
                .ok_or(OpeningCorpusError::Overflow)?;
            self.samples = self
                .samples
                .checked_add(1)
                .ok_or(OpeningCorpusError::Overflow)?;
        }
        Ok(())
    }
    fn average(&self) -> Option<OpeningStatisticsAverage> {
        NonZeroU64::new(self.samples).map(|samples| OpeningStatisticsAverage {
            sum: self.sum,
            samples,
        })
    }
}

#[derive(Debug, Default)]
struct OpeningStatisticsTotals {
    games: u64,
    outcomes: OpeningStatisticsOutcomes,
    white_rating: OpeningStatisticsRating,
    black_rating: OpeningStatisticsRating,
}

impl OpeningStatisticsTotals {
    fn add(&mut self, game: &OpeningStatisticsGame<'_>) -> Result<(), OpeningCorpusError> {
        self.games = self
            .games
            .checked_add(1)
            .ok_or(OpeningCorpusError::Overflow)?;
        self.outcomes.add(game.outcome)?;
        self.white_rating.add(game.white_rating)?;
        self.black_rating.add(game.black_rating)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OpeningStatisticsProvenance<'a> {
    source: &'a StorageDatasetPackageSourceIdentity,
    record: &'a OpeningCorpusRecord<'a>,
}

impl<'a> OpeningStatisticsProvenance<'a> {
    pub const fn source(&self) -> &StorageDatasetPackageSourceIdentity {
        self.source
    }
    pub const fn game(&self) -> OpeningCorpusGameId {
        self.record.id()
    }
    pub const fn record(&self) -> &OpeningCorpusRecord<'a> {
        self.record
    }
}

#[derive(Clone, Copy)]
struct OpeningStatisticsInput<'a> {
    source: &'a StorageDatasetPackageSourceIdentity,
    record: &'a OpeningCorpusRecord<'a>,
    supported: OpeningCorpusMetadataSupport,
}

enum OpeningStatisticsEnumOccurrence {
    Absent,
    Ending,
    Continuation(BoardMove),
}

impl OpeningStatisticsEnumOccurrence {
    fn first<F: FnMut() -> bool>(
        record: &OpeningCorpusRecord<'_>,
        selected: &BoardPositionIdentity,
        budget: &mut OpeningCorpusBudget<F>,
    ) -> Result<Self, OpeningCorpusError> {
        let mut board = record.initial().clone();
        for ply in 0..=record.mainline().len() {
            budget.checkpoint()?;
            if &board.identity() == selected {
                return Ok(match record.mainline().get(ply) {
                    Some(&movement) => Self::Continuation(movement),
                    None => Self::Ending,
                });
            }
            if let Some(&movement) = record.mainline().get(ply) {
                board = board.play(movement)?;
            }
        }
        Ok(Self::Absent)
    }
}

#[derive(Debug)]
pub struct OpeningStatisticsGame<'a> {
    record: &'a OpeningCorpusRecord<'a>,
    supported: OpeningCorpusMetadataSupport,
    provenance: Vec<OpeningStatisticsProvenance<'a>>,
    movement: Option<BoardMove>,
    partial_date: Option<bool>,
    next_bucket: Option<usize>,
    outcome: Option<NotationGameIdentityEnumResult>,
    white_rating: Option<u32>,
    black_rating: Option<u32>,
}

impl<'a> OpeningStatisticsGame<'a> {
    pub const fn record(&self) -> &OpeningCorpusRecord<'a> {
        self.record
    }
    pub fn provenance(&self) -> &[OpeningStatisticsProvenance<'a>] {
        &self.provenance
    }
    pub const fn is_partial_date_match(&self) -> bool {
        matches!(self.partial_date, Some(true))
    }

    pub const fn rating(&self, side: BoardEnumColor) -> Option<u32> {
        match side {
            BoardEnumColor::White => self.white_rating,
            BoardEnumColor::Black => self.black_rating,
        }
    }

    fn include<F: FnMut() -> bool>(
        &mut self,
        input: OpeningStatisticsInput<'a>,
        partial_date: Option<bool>,
        budget: &mut OpeningCorpusBudget<F>,
    ) -> Result<(), OpeningCorpusError> {
        budget.reserve(&mut self.provenance, 1)?;
        self.provenance.push(OpeningStatisticsProvenance {
            source: input.source,
            record: input.record,
        });
        if partial_date.is_some() {
            if self.partial_date.is_none() {
                self.record = input.record;
                self.supported = input.supported;
                self.partial_date = partial_date;
            }
            self.supported = self.supported.union(input.supported);
            if input.supported.supports(OpeningCorpusEnumMetadata::Result) {
                self.outcome = self.outcome.or(input.record.result());
            }
            if input.supported.supports(OpeningCorpusEnumMetadata::Ratings) {
                self.white_rating = self
                    .white_rating
                    .or(input.record.metadata().known().white_elo);
                self.black_rating = self
                    .black_rating
                    .or(input.record.metadata().known().black_elo);
            }
        }
        Ok(())
    }

    fn compare_identity(left: &NotationGameIdentity, right: &NotationGameIdentity) -> Ordering {
        let left_key = left.structural_key();
        let right_key = right.structural_key();
        Self::compare_variant(left_key.variant(), right_key.variant())
            .then_with(|| left_key.initial_fen().cmp(right_key.initial_fen()))
            .then_with(|| left_key.mainline_uci().cmp(right_key.mainline_uci()))
            .then_with(|| left.white().cmp(&right.white()))
            .then_with(|| left.black().cmp(&right.black()))
            .then_with(|| Self::date_key(left).cmp(&Self::date_key(right)))
            .then_with(|| {
                left.result()
                    .map(NotationGameIdentityEnumResult::pgn_token)
                    .cmp(
                        &right
                            .result()
                            .map(NotationGameIdentityEnumResult::pgn_token),
                    )
            })
    }

    fn compare_variant(left: BoardEnumVariant, right: BoardEnumVariant) -> Ordering {
        match (left, right) {
            (BoardEnumVariant::Standard, BoardEnumVariant::Standard)
            | (BoardEnumVariant::Chess960, BoardEnumVariant::Chess960) => Ordering::Equal,
            (BoardEnumVariant::Standard, BoardEnumVariant::Chess960) => Ordering::Less,
            (BoardEnumVariant::Chess960, BoardEnumVariant::Standard) => Ordering::Greater,
        }
    }

    fn date_key(identity: &NotationGameIdentity) -> (Option<u16>, Option<u8>, Option<u8>) {
        let date = identity.date();
        (
            date.and_then(|date| date.year()),
            date.and_then(|date| date.month()),
            date.and_then(|date| date.day()),
        )
    }

    fn compare_sample(&self, other: &Self) -> Ordering {
        other
            .sample_date_key()
            .cmp(&self.sample_date_key())
            .then_with(|| Self::compare_identity(self.record.identity(), other.record.identity()))
            .then_with(|| self.provenance[0].source.cmp(other.provenance[0].source))
            .then_with(|| self.provenance[0].game().cmp(&other.provenance[0].game()))
    }

    fn sample_date_key(&self) -> (Option<u16>, Option<u8>, Option<u8>) {
        if self.supported.supports(OpeningCorpusEnumMetadata::Date) {
            Self::date_key(self.record.identity())
        } else {
            (None, None, None)
        }
    }
}

#[derive(Debug)]
pub struct OpeningStatisticsContinuation {
    movement: BoardMove,
    totals: OpeningStatisticsTotals,
    frequency: OpeningStatisticsRatio,
}

impl OpeningStatisticsContinuation {
    pub const fn movement(&self) -> BoardMove {
        self.movement
    }
    pub const fn game_count(&self) -> u64 {
        self.totals.games
    }
    pub const fn outcomes(&self) -> OpeningStatisticsOutcomes {
        self.totals.outcomes
    }
    pub const fn frequency(&self) -> OpeningStatisticsRatio {
        self.frequency
    }
    pub fn white_rating(&self) -> Option<OpeningStatisticsAverage> {
        self.totals.white_rating.average()
    }
    pub fn black_rating(&self) -> Option<OpeningStatisticsAverage> {
        self.totals.black_rating.average()
    }
}

pub struct OpeningStatisticsResult<'a> {
    sources: Vec<&'a StorageDatasetPackageSourceIdentity>,
    games: Vec<OpeningStatisticsGame<'a>>,
    sample_order: Vec<usize>,
    continuations: Vec<OpeningStatisticsContinuation>,
    totals: OpeningStatisticsTotals,
    next_move_games: u64,
    ending_games: u64,
    partial_date_matches: u64,
    potential_duplicate_pairs: u64,
}

impl<'a> OpeningStatisticsResult<'a> {
    pub const SAMPLE_PAGE_SIZE: usize = 50;

    pub const fn game_count(&self) -> u64 {
        self.totals.games
    }
    pub fn sources(&self) -> &[&'a StorageDatasetPackageSourceIdentity] {
        &self.sources
    }
    pub fn games(&self) -> &[OpeningStatisticsGame<'a>] {
        &self.games
    }
    pub fn continuations(&self) -> &[OpeningStatisticsContinuation] {
        &self.continuations
    }
    pub const fn outcomes(&self) -> OpeningStatisticsOutcomes {
        self.totals.outcomes
    }
    pub const fn next_move_games(&self) -> u64 {
        self.next_move_games
    }
    pub const fn ending_games(&self) -> u64 {
        self.ending_games
    }
    pub const fn partial_date_matches(&self) -> u64 {
        self.partial_date_matches
    }
    pub const fn potential_duplicate_pairs(&self) -> u64 {
        self.potential_duplicate_pairs
    }
    pub fn white_rating(&self) -> Option<OpeningStatisticsAverage> {
        self.totals.white_rating.average()
    }
    pub fn black_rating(&self) -> Option<OpeningStatisticsAverage> {
        self.totals.black_rating.average()
    }

    pub fn sample_page(
        &self,
        offset: usize,
    ) -> Result<OpeningStatisticsSamplePage<'_, 'a>, OpeningCorpusError> {
        if offset > self.sample_order.len() {
            return Err(OpeningCorpusError::SampleOffset);
        }
        let end = offset
            .saturating_add(Self::SAMPLE_PAGE_SIZE)
            .min(self.sample_order.len());
        Ok(OpeningStatisticsSamplePage {
            result: self,
            order: &self.sample_order[offset..end],
            next: (end < self.sample_order.len()).then_some(end),
        })
    }
}

pub struct OpeningStatisticsSamplePage<'r, 'a> {
    result: &'r OpeningStatisticsResult<'a>,
    order: &'r [usize],
    next: Option<usize>,
}

impl<'r, 'a> OpeningStatisticsSamplePage<'r, 'a> {
    pub fn len(&self) -> usize {
        self.order.len()
    }
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
    pub const fn next_offset(&self) -> Option<usize> {
        self.next
    }
    pub fn games(&self) -> impl ExactSizeIterator<Item = &'r OpeningStatisticsGame<'a>> + '_ {
        let games: &'r [OpeningStatisticsGame<'a>] = &self.result.games;
        self.order.iter().map(move |&index| &games[index])
    }
}

struct OpeningStatisticsMove {
    movement: BoardMove,
    uci: String,
    totals: OpeningStatisticsTotals,
}

pub struct OpeningStatistics;

impl OpeningStatistics {
    pub fn query<'a>(
        corpora: &[&'a OpeningCorpus<'a>],
        query: &OpeningQuery<'_>,
        limits: OpeningCorpusLimits,
        cancelled: impl FnMut() -> bool,
    ) -> Result<OpeningStatisticsResult<'a>, OpeningCorpusError> {
        Self::query_with_hasher(
            corpora,
            query,
            limits,
            cancelled,
            BuildHasherDefault::<DefaultHasher>::default(),
        )
    }

    pub fn query_with_hasher<'a>(
        corpora: &[&'a OpeningCorpus<'a>],
        query: &OpeningQuery<'_>,
        limits: OpeningCorpusLimits,
        cancelled: impl FnMut() -> bool,
        hasher: impl BuildHasher,
    ) -> Result<OpeningStatisticsResult<'a>, OpeningCorpusError> {
        let mut budget = OpeningCorpusBudget::new(limits, cancelled);
        budget.checkpoint()?;
        let inputs = Self::inputs(corpora, limits, &mut budget)?;
        let mut sources = Vec::new();
        budget.reserve(&mut sources, corpora.len())?;
        for corpus in corpora {
            budget.checkpoint()?;
            sources.push(corpus.source());
        }
        budget.sort(&mut sources, Ord::cmp)?;
        let mut games: Vec<OpeningStatisticsGame<'a>> = Vec::new();
        let mut buckets = HashMap::with_hasher(hasher);
        budget.reserve(&mut games, inputs.len())?;
        budget.reserve_map(&mut buckets, inputs.len())?;
        let position = query.position().identity();
        for input in inputs {
            budget.checkpoint()?;
            let movement =
                match OpeningStatisticsEnumOccurrence::first(input.record, &position, &mut budget)?
                {
                    OpeningStatisticsEnumOccurrence::Absent => continue,
                    OpeningStatisticsEnumOccurrence::Ending => None,
                    OpeningStatisticsEnumOccurrence::Continuation(movement) => Some(movement),
                };
            let partial_date = query.matches(input.record, input.supported, &mut budget)?;
            let key = input.record.identity().structural_key();
            let bucket: Option<usize> = buckets.get(key).copied();
            let mut candidate = bucket;
            let mut exact = None;
            while let Some(index) = candidate {
                budget.checkpoint()?;
                let game = &games[index];
                if game.record.identity().compare(input.record.identity())
                    == NotationGameIdentityEnumMatch::Exact
                {
                    exact = Some(index);
                    break;
                }
                candidate = game.next_bucket;
            }
            if let Some(index) = exact {
                games[index].include(input, partial_date, &mut budget)?;
            } else {
                let index = games.len();
                let mut game = OpeningStatisticsGame {
                    record: input.record,
                    supported: input.supported,
                    provenance: Vec::new(),
                    movement,
                    partial_date: None,
                    next_bucket: bucket,
                    outcome: None,
                    white_rating: None,
                    black_rating: None,
                };
                game.include(input, partial_date, &mut budget)?;
                games.push(game);
                let _previous = buckets.insert(key, index);
            }
        }
        let mut potential_duplicate_pairs = 0_u64;
        for bucket in buckets.into_values() {
            let mut first = Some(bucket);
            while let Some(left) = first {
                budget.checkpoint()?;
                let mut second = games[left].next_bucket;
                while let Some(right) = second {
                    budget.checkpoint()?;
                    if games[left].partial_date.is_some()
                        && games[right].partial_date.is_some()
                        && games[left]
                            .record
                            .identity()
                            .compare(games[right].record.identity())
                            == NotationGameIdentityEnumMatch::Potential
                    {
                        potential_duplicate_pairs = potential_duplicate_pairs
                            .checked_add(1)
                            .ok_or(OpeningCorpusError::Overflow)?;
                    }
                    second = games[right].next_bucket;
                }
                first = games[left].next_bucket;
            }
        }
        for game in &mut games {
            budget.checkpoint()?;
            game.next_bucket = None;
        }
        games.retain(|game| game.partial_date.is_some());
        Self::finish(
            games,
            sources,
            potential_duplicate_pairs,
            query,
            &mut budget,
        )
    }

    fn inputs<'a, F: FnMut() -> bool>(
        corpora: &[&'a OpeningCorpus<'a>],
        limits: OpeningCorpusLimits,
        budget: &mut OpeningCorpusBudget<F>,
    ) -> Result<Vec<OpeningStatisticsInput<'a>>, OpeningCorpusError> {
        limits.validate_records(corpora.len())?;
        let mut count = 0_usize;
        for (index, corpus) in corpora.iter().enumerate() {
            budget.checkpoint()?;
            for prior in &corpora[..index] {
                budget.checkpoint()?;
                if prior.source().source() == corpus.source().source()
                    && prior.source().version() == corpus.source().version()
                {
                    return Err(OpeningCorpusError::SourceBinding);
                }
            }
            count = count
                .checked_add(corpus.records().len())
                .ok_or(OpeningCorpusError::Overflow)?;
            limits.validate_records(count)?;
        }
        let mut inputs = Vec::new();
        budget.reserve(&mut inputs, count)?;
        for corpus in corpora {
            for record in corpus.records() {
                budget.checkpoint()?;
                limits.validate_plies(record.mainline().len())?;
                inputs.push(OpeningStatisticsInput {
                    source: corpus.source(),
                    record,
                    supported: corpus.supported_metadata(),
                });
            }
        }
        budget.sort(&mut inputs, |left, right| {
            left.source
                .cmp(right.source)
                .then_with(|| left.record.id().cmp(&right.record.id()))
        })?;
        Ok(inputs)
    }

    fn finish<'a, F: FnMut() -> bool>(
        games: Vec<OpeningStatisticsGame<'a>>,
        sources: Vec<&'a StorageDatasetPackageSourceIdentity>,
        potential_duplicate_pairs: u64,
        query: &OpeningQuery<'_>,
        budget: &mut OpeningCorpusBudget<F>,
    ) -> Result<OpeningStatisticsResult<'a>, OpeningCorpusError> {
        let mut result = OpeningStatisticsResult {
            sources,
            games,
            sample_order: Vec::new(),
            continuations: Vec::new(),
            totals: OpeningStatisticsTotals::default(),
            next_move_games: 0,
            ending_games: 0,
            partial_date_matches: 0,
            potential_duplicate_pairs,
        };
        budget.reserve(&mut result.sample_order, result.games.len())?;
        let mut movements: Vec<OpeningStatisticsMove> = Vec::new();
        for (index, game) in result.games.iter().enumerate() {
            budget.checkpoint()?;
            result.sample_order.push(index);
            result.totals.add(game)?;
            if game.is_partial_date_match() {
                result.partial_date_matches = result
                    .partial_date_matches
                    .checked_add(1)
                    .ok_or(OpeningCorpusError::Overflow)?;
            }
            if let Some(movement) = game.movement {
                result.next_move_games = result
                    .next_move_games
                    .checked_add(1)
                    .ok_or(OpeningCorpusError::Overflow)?;
                let mut found = None;
                for (index, candidate) in movements.iter().enumerate() {
                    budget.checkpoint()?;
                    if candidate.movement == movement {
                        found = Some(index);
                        break;
                    }
                }
                let index = match found {
                    Some(index) => index,
                    None => {
                        budget.reserve(&mut movements, 1)?;
                        budget.charge(OpeningCorpusLimits::CANONICAL_MOVE_RESERVATION_BYTES)?;
                        movements.push(OpeningStatisticsMove {
                            movement,
                            uci: movement.uci(query.position().variant()),
                            totals: OpeningStatisticsTotals::default(),
                        });
                        movements.len() - 1
                    }
                };
                movements[index].totals.add(game)?;
            } else {
                result.ending_games = result
                    .ending_games
                    .checked_add(1)
                    .ok_or(OpeningCorpusError::Overflow)?;
            }
        }
        budget.sort(&mut result.sample_order, |&left, &right| {
            result.games[left].compare_sample(&result.games[right])
        })?;
        let mut move_order: Vec<usize> = Vec::new();
        budget.reserve(&mut move_order, movements.len())?;
        for index in 0..movements.len() {
            budget.checkpoint()?;
            move_order.push(index);
        }
        budget.sort(&mut move_order, |&left, &right| {
            movements[right]
                .totals
                .games
                .cmp(&movements[left].totals.games)
                .then_with(|| movements[left].uci.cmp(&movements[right].uci))
        })?;
        budget.reserve(&mut result.continuations, movements.len())?;
        for index in move_order {
            budget.checkpoint()?;
            let movement = &mut movements[index];
            let frequency =
                OpeningStatisticsRatio::new(movement.totals.games, result.next_move_games)
                    .ok_or(OpeningCorpusError::Overflow)?;
            result.continuations.push(OpeningStatisticsContinuation {
                movement: movement.movement,
                totals: std::mem::take(&mut movement.totals),
                frequency,
            });
        }
        budget.checkpoint()?;
        Ok(result)
    }
}
