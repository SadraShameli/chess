use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::game::session::{GameMetadataDate, GameMetadataText};
use crate::notation::game_identity::{NotationGameIdentity, NotationGameIdentityEnumResult};
use crate::opening::corpus::{
    OpeningCorpusBudget, OpeningCorpusEnumMetadata, OpeningCorpusError, OpeningCorpusLimits,
    OpeningCorpusMetadataSupport, OpeningCorpusRecord,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpeningQueryEnumRatingCriterion {
    #[default]
    BothPlayers,
    White,
    Black,
    SideToMove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningQueryRating {
    lower: u32,
    upper: u32,
    criterion: OpeningQueryEnumRatingCriterion,
}

impl OpeningQueryRating {
    pub fn new(
        lower: u32,
        upper: u32,
        criterion: OpeningQueryEnumRatingCriterion,
    ) -> Result<Self, OpeningCorpusError> {
        if lower > upper {
            return Err(OpeningCorpusError::RatingRange);
        }
        Ok(Self {
            lower,
            upper,
            criterion,
        })
    }

    fn matches(self, record: &OpeningCorpusRecord<'_>, turn: BoardEnumColor) -> bool {
        let known = record.metadata().known();
        let white = self.contains(known.white_elo);
        let black = self.contains(known.black_elo);
        match self.criterion {
            OpeningQueryEnumRatingCriterion::BothPlayers => white && black,
            OpeningQueryEnumRatingCriterion::White => white,
            OpeningQueryEnumRatingCriterion::Black => black,
            OpeningQueryEnumRatingCriterion::SideToMove => match turn {
                BoardEnumColor::White => white,
                BoardEnumColor::Black => black,
            },
        }
    }

    fn contains(self, rating: Option<u32>) -> bool {
        rating.is_some_and(|rating| (self.lower..=self.upper).contains(&rating))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningQueryEnumPlayer {
    White,
    Black,
    Either,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningQueryPlayer {
    criterion: OpeningQueryEnumPlayer,
    canonical: String,
}

impl OpeningQueryPlayer {
    pub fn new(
        criterion: OpeningQueryEnumPlayer,
        name: GameMetadataText,
        limits: OpeningCorpusLimits,
    ) -> Result<Self, OpeningCorpusError> {
        let mut budget = OpeningCorpusBudget::new(limits, || false);
        budget.charge(OpeningCorpusLimits::canonical_text_bytes(
            name.as_str().len(),
        )?)?;
        let canonical = NotationGameIdentity::canonical_player(Some(&name))
            .ok_or(OpeningCorpusError::PlayerFilter)?;
        Ok(Self {
            criterion,
            canonical,
        })
    }

    fn matches(&self, record: &OpeningCorpusRecord<'_>) -> bool {
        let identity = record.identity();
        let white = identity.white() == Some(self.canonical.as_str());
        let black = identity.black() == Some(self.canonical.as_str());
        match self.criterion {
            OpeningQueryEnumPlayer::White => white,
            OpeningQueryEnumPlayer::Black => black,
            OpeningQueryEnumPlayer::Either => white || black,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningQueryDateInterval {
    lower: (u16, u8, u8),
    upper: (u16, u8, u8),
}

impl OpeningQueryDateInterval {
    pub fn new(
        lower: GameMetadataDate,
        upper: GameMetadataDate,
    ) -> Result<Self, OpeningCorpusError> {
        let lower = Self::complete(lower).ok_or(OpeningCorpusError::DateInterval)?;
        let upper = Self::complete(upper).ok_or(OpeningCorpusError::DateInterval)?;
        if lower > upper {
            return Err(OpeningCorpusError::DateInterval);
        }
        Ok(Self { lower, upper })
    }

    fn complete(date: GameMetadataDate) -> Option<(u16, u8, u8)> {
        Some((date.year()?, date.month()?, date.day()?))
    }

    fn matches<F: FnMut() -> bool>(
        self,
        date: GameMetadataDate,
        budget: &mut OpeningCorpusBudget<F>,
    ) -> Result<bool, OpeningCorpusError> {
        if date.year().is_none() && date.month().is_none() && date.day().is_none() {
            return Ok(false);
        }
        let first_year = date.year().unwrap_or(self.lower.0).max(self.lower.0);
        let last_year = date.year().unwrap_or(self.upper.0).min(self.upper.0);
        for year in first_year..=last_year {
            for month in 1..=12 {
                budget.checkpoint()?;
                if date.month().is_some_and(|known| known != month) {
                    continue;
                }
                for day in 1..=31 {
                    budget.checkpoint()?;
                    if date.day().is_some_and(|known| known != day) {
                        continue;
                    }
                    let candidate = (year, month, day);
                    if candidate >= self.lower
                        && candidate <= self.upper
                        && GameMetadataDate::new(Some(year), Some(month), Some(day)).is_ok()
                    {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}

#[derive(Debug, Default)]
pub struct OpeningQueryFilters {
    pub date: Option<OpeningQueryDateInterval>,
    pub rating: Option<OpeningQueryRating>,
    pub player: Option<OpeningQueryPlayer>,
    pub time_control: Option<GameMetadataText>,
    pub result: Option<NotationGameIdentityEnumResult>,
}

pub struct OpeningQuery<'a> {
    position: &'a BoardPosition,
    filters: OpeningQueryFilters,
}

impl<'a> OpeningQuery<'a> {
    pub fn new(
        position: &'a BoardPosition,
        filters: OpeningQueryFilters,
    ) -> Result<Self, OpeningCorpusError> {
        if filters.time_control.as_ref().is_some_and(|value| {
            let text = value.as_str().trim();
            text.is_empty() || text == "?"
        }) {
            return Err(OpeningCorpusError::TimeControlFilter);
        }
        Ok(Self { position, filters })
    }

    pub const fn position(&self) -> &BoardPosition {
        self.position
    }

    pub(crate) fn matches<F: FnMut() -> bool>(
        &self,
        record: &OpeningCorpusRecord<'_>,
        supported: OpeningCorpusMetadataSupport,
        budget: &mut OpeningCorpusBudget<F>,
    ) -> Result<Option<bool>, OpeningCorpusError> {
        budget.checkpoint()?;
        if self.filters.rating.is_some_and(|filter| {
            !supported.supports(OpeningCorpusEnumMetadata::Ratings)
                || !filter.matches(record, self.position.turn())
        }) || self.filters.player.as_ref().is_some_and(|filter| {
            !supported.supports(OpeningCorpusEnumMetadata::Players) || !filter.matches(record)
        }) || self.filters.result.is_some_and(|filter| {
            !supported.supports(OpeningCorpusEnumMetadata::Result)
                || record.result() != Some(filter)
        }) || self.filters.time_control.as_ref().is_some_and(|filter| {
            !supported.supports(OpeningCorpusEnumMetadata::TimeControl)
                || record
                    .time_control()
                    .is_none_or(|value| value.as_str() != filter.as_str())
        }) {
            return Ok(None);
        }
        let mut partial = false;
        if let Some(interval) = self.filters.date {
            if !supported.supports(OpeningCorpusEnumMetadata::Date) {
                return Ok(None);
            }
            let Some(date) = record.identity().date() else {
                return Ok(None);
            };
            if !interval.matches(date, budget)? {
                return Ok(None);
            }
            partial = OpeningQueryDateInterval::complete(date).is_none();
        }
        Ok(Some(partial))
    }
}
