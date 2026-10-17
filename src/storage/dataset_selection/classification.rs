use serde::{Deserialize, Serialize};

use crate::board::enums::BoardEnumColor;
use crate::storage::dataset_selection::{StorageDatasetSelection, StorageDatasetSelectionError};

enum StorageDatasetSelectionEnumUnsignedReason {
    Malformed,
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumRatingReason {
    Missing,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumRating {
    Known(u32),
    Unknown(StorageDatasetSelectionEnumRatingReason),
}

impl StorageDatasetSelectionEnumRating {
    fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw.map(str::trim_ascii).filter(|value| !value.is_empty()) else {
            return Self::Unknown(StorageDatasetSelectionEnumRatingReason::Missing);
        };
        match StorageDatasetSelection::parse_unsigned(raw) {
            Ok(rating) => Self::Known(rating),
            Err(
                StorageDatasetSelectionEnumUnsignedReason::Malformed
                | StorageDatasetSelectionEnumUnsignedReason::Overflow,
            ) => Self::Unknown(StorageDatasetSelectionEnumRatingReason::Invalid),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumGameRatingBand {
    Below1200,
    From1200,
    From1600,
    From2000,
    From2400,
    Unknown,
}

impl StorageDatasetSelectionEnumGameRatingBand {
    const SUM_THRESHOLDS: [u64; 4] = [2400, 3200, 4000, 4800];

    fn classify(
        white: StorageDatasetSelectionEnumRating,
        black: StorageDatasetSelectionEnumRating,
    ) -> Result<Self, StorageDatasetSelectionError> {
        let (
            StorageDatasetSelectionEnumRating::Known(white),
            StorageDatasetSelectionEnumRating::Known(black),
        ) = (white, black)
        else {
            return Ok(Self::Unknown);
        };
        let sum = u64::from(white)
            .checked_add(u64::from(black))
            .ok_or(StorageDatasetSelectionError::ArithmeticOverflow)?;
        let [from1200, from1600, from2000, from2400] = Self::SUM_THRESHOLDS;
        Ok(if sum < from1200 {
            Self::Below1200
        } else if sum < from1600 {
            Self::From1200
        } else if sum < from2000 {
            Self::From1600
        } else if sum < from2400 {
            Self::From2000
        } else {
            Self::From2400
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumTimeClass {
    Bullet,
    Blitz,
    Rapid,
    Classical,
    Correspondence,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumTimeReason {
    Unavailable,
    Malformed,
    Overflow,
    MultiStage,
    Hourglass,
    Slash,
}

impl From<StorageDatasetSelectionEnumUnsignedReason> for StorageDatasetSelectionEnumTimeReason {
    fn from(reason: StorageDatasetSelectionEnumUnsignedReason) -> Self {
        match reason {
            StorageDatasetSelectionEnumUnsignedReason::Malformed => Self::Malformed,
            StorageDatasetSelectionEnumUnsignedReason::Overflow => Self::Overflow,
        }
    }
}

impl StorageDatasetSelectionEnumTimeClass {
    const ESTIMATE_MOVES: u64 = 40;
    const CLASS_THRESHOLDS: [u64; 3] = [180, 480, 1500];
    const SINGLE_DAY: &str = "1 day per move";
    const MULTIPLE_DAYS_SUFFIX: &str = " days per move";

    fn parse(raw: Option<&str>) -> Result<Self, StorageDatasetSelectionEnumTimeReason> {
        let raw = raw
            .map(str::trim_ascii)
            .filter(|raw| !raw.is_empty() && *raw != "?" && *raw != "-")
            .ok_or(StorageDatasetSelectionEnumTimeReason::Unavailable)?;
        if raw == Self::SINGLE_DAY {
            return Ok(Self::Correspondence);
        }
        if let Some(days) = raw.strip_suffix(Self::MULTIPLE_DAYS_SUFFIX) {
            return if StorageDatasetSelection::parse_unsigned(days)? >= 2 {
                Ok(Self::Correspondence)
            } else {
                Err(StorageDatasetSelectionEnumTimeReason::Malformed)
            };
        }
        if raw.contains(':') {
            return Err(StorageDatasetSelectionEnumTimeReason::MultiStage);
        }
        if raw.contains('/') {
            return Err(StorageDatasetSelectionEnumTimeReason::Slash);
        }
        if raw.contains('*') {
            return Err(StorageDatasetSelectionEnumTimeReason::Hourglass);
        }
        let (base, increment) = match raw.split_once('+') {
            Some((base, increment)) => (
                StorageDatasetSelection::parse_unsigned(base)?,
                StorageDatasetSelection::parse_unsigned(increment)?,
            ),
            None => (StorageDatasetSelection::parse_unsigned(raw)?, 0),
        };
        if base == 0 && increment == 0 {
            return Err(StorageDatasetSelectionEnumTimeReason::Malformed);
        }
        let estimate = u64::from(increment)
            .checked_mul(Self::ESTIMATE_MOVES)
            .and_then(|increment| u64::from(base).checked_add(increment))
            .ok_or(StorageDatasetSelectionEnumTimeReason::Overflow)?;
        let [blitz, rapid, classical] = Self::CLASS_THRESHOLDS;
        Ok(if estimate < blitz {
            Self::Bullet
        } else if estimate < rapid {
            Self::Blitz
        } else if estimate < classical {
            Self::Rapid
        } else {
            Self::Classical
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDatasetSelectionGameStratum {
    rating_band: StorageDatasetSelectionEnumGameRatingBand,
    time_class: StorageDatasetSelectionEnumTimeClass,
}

impl StorageDatasetSelectionGameStratum {
    pub const fn rating_band(&self) -> StorageDatasetSelectionEnumGameRatingBand {
        self.rating_band
    }

    pub const fn time_class(&self) -> StorageDatasetSelectionEnumTimeClass {
        self.time_class
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StorageDatasetSelectionGameClassification {
    stratum: StorageDatasetSelectionGameStratum,
    white_rating: StorageDatasetSelectionEnumRating,
    black_rating: StorageDatasetSelectionEnumRating,
    time_reason: Option<StorageDatasetSelectionEnumTimeReason>,
}

impl StorageDatasetSelectionGameClassification {
    pub const fn stratum(&self) -> &StorageDatasetSelectionGameStratum {
        &self.stratum
    }

    pub const fn white_rating(&self) -> StorageDatasetSelectionEnumRating {
        self.white_rating
    }

    pub const fn black_rating(&self) -> StorageDatasetSelectionEnumRating {
        self.black_rating
    }

    pub const fn time_reason(&self) -> Option<StorageDatasetSelectionEnumTimeReason> {
        self.time_reason
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u32")]
pub struct StorageDatasetSelectionPuzzleRatingBand(u32);

impl TryFrom<u32> for StorageDatasetSelectionPuzzleRatingBand {
    type Error = StorageDatasetSelectionError;

    fn try_from(index: u32) -> Result<Self, Self::Error> {
        if index > u32::MAX / Self::WIDTH {
            return Err(StorageDatasetSelectionError::InvalidPuzzleBand(index));
        }
        Ok(Self(index))
    }
}

impl StorageDatasetSelectionPuzzleRatingBand {
    const WIDTH: u32 = 250;

    pub const fn index(self) -> u32 {
        self.0
    }

    pub fn endpoints(self) -> Result<(u32, u32), StorageDatasetSelectionError> {
        let lower = u64::from(self.0)
            .checked_mul(u64::from(Self::WIDTH))
            .ok_or(StorageDatasetSelectionError::ArithmeticOverflow)?;
        let upper = lower
            .checked_add(u64::from(Self::WIDTH) - 1)
            .ok_or(StorageDatasetSelectionError::ArithmeticOverflow)?
            .min(u64::from(u32::MAX));
        Ok((
            u32::try_from(lower)
                .map_err(|_error| StorageDatasetSelectionError::ArithmeticOverflow)?,
            u32::try_from(upper)
                .map_err(|_error| StorageDatasetSelectionError::ArithmeticOverflow)?,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumPuzzleRatingBand {
    Known(StorageDatasetSelectionPuzzleRatingBand),
    Unknown,
}

impl StorageDatasetSelectionEnumPuzzleRatingBand {
    fn classify(rating: StorageDatasetSelectionEnumRating) -> Self {
        match rating {
            StorageDatasetSelectionEnumRating::Known(rating) => {
                Self::Known(StorageDatasetSelectionPuzzleRatingBand(
                    rating / StorageDatasetSelectionPuzzleRatingBand::WIDTH,
                ))
            }
            StorageDatasetSelectionEnumRating::Unknown(
                StorageDatasetSelectionEnumRatingReason::Missing
                | StorageDatasetSelectionEnumRatingReason::Invalid,
            ) => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct StorageDatasetSelectionTheme(String);

impl TryFrom<String> for StorageDatasetSelectionTheme {
    type Error = StorageDatasetSelectionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(StorageDatasetSelectionError::InvalidTheme);
        }
        Ok(Self(value))
    }
}

impl StorageDatasetSelectionTheme {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumPrimaryTheme {
    Known(StorageDatasetSelectionTheme),
    Unknown,
}

impl StorageDatasetSelectionEnumPrimaryTheme {
    fn classify(raw: Option<&str>) -> Self {
        match raw.and_then(|themes| {
            themes
                .split_ascii_whitespace()
                .min_by(|left, right| left.as_bytes().cmp(right.as_bytes()))
        }) {
            Some(theme) => Self::Known(StorageDatasetSelectionTheme(theme.to_owned())),
            None => Self::Unknown,
        }
    }

    pub fn token(&self) -> Option<&str> {
        match self {
            Self::Known(theme) => Some(theme.as_str()),
            Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDatasetSelectionPuzzleStratum {
    rating_band: StorageDatasetSelectionEnumPuzzleRatingBand,
    primary_theme: StorageDatasetSelectionEnumPrimaryTheme,
}

impl StorageDatasetSelectionPuzzleStratum {
    pub const fn rating_band(&self) -> StorageDatasetSelectionEnumPuzzleRatingBand {
        self.rating_band
    }

    pub const fn primary_theme(&self) -> &StorageDatasetSelectionEnumPrimaryTheme {
        &self.primary_theme
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StorageDatasetSelectionPuzzleClassification {
    stratum: StorageDatasetSelectionPuzzleStratum,
    rating: StorageDatasetSelectionEnumRating,
    theme_reason: Option<StorageDatasetSelectionEnumThemeReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumThemeReason {
    Missing,
    Empty,
}

impl StorageDatasetSelectionPuzzleClassification {
    pub const fn stratum(&self) -> &StorageDatasetSelectionPuzzleStratum {
        &self.stratum
    }

    pub const fn rating(&self) -> StorageDatasetSelectionEnumRating {
        self.rating
    }

    pub const fn theme_reason(&self) -> Option<StorageDatasetSelectionEnumThemeReason> {
        self.theme_reason
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumSource {
    Standard,
    Broadcast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumMasterEligibility {
    Eligible,
    Broadcast,
    MissingTitle(BoardEnumColor),
    UnsupportedTitle(BoardEnumColor),
}

impl StorageDatasetSelection {
    fn parse_unsigned(raw: &str) -> Result<u32, StorageDatasetSelectionEnumUnsignedReason> {
        if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(StorageDatasetSelectionEnumUnsignedReason::Malformed);
        }
        raw.parse()
            .map_err(|_error| StorageDatasetSelectionEnumUnsignedReason::Overflow)
    }

    pub fn classify_game(
        white: Option<&str>,
        black: Option<&str>,
        time: Option<&str>,
    ) -> Result<StorageDatasetSelectionGameClassification, StorageDatasetSelectionError> {
        let white_rating = StorageDatasetSelectionEnumRating::parse(white);
        let black_rating = StorageDatasetSelectionEnumRating::parse(black);
        let rating_band =
            StorageDatasetSelectionEnumGameRatingBand::classify(white_rating, black_rating)?;
        let (time_class, time_reason) = match StorageDatasetSelectionEnumTimeClass::parse(time) {
            Ok(class) => (class, None),
            Err(reason) => (StorageDatasetSelectionEnumTimeClass::Unknown, Some(reason)),
        };
        Ok(StorageDatasetSelectionGameClassification {
            stratum: StorageDatasetSelectionGameStratum {
                rating_band,
                time_class,
            },
            white_rating,
            black_rating,
            time_reason,
        })
    }

    pub fn classify_puzzle(
        rating: Option<&str>,
        themes: Option<&str>,
    ) -> Result<StorageDatasetSelectionPuzzleClassification, StorageDatasetSelectionError> {
        let rating = StorageDatasetSelectionEnumRating::parse(rating);
        let rating_band = StorageDatasetSelectionEnumPuzzleRatingBand::classify(rating);
        let primary_theme = StorageDatasetSelectionEnumPrimaryTheme::classify(themes);
        let theme_reason = match &primary_theme {
            StorageDatasetSelectionEnumPrimaryTheme::Known(_) => None,
            StorageDatasetSelectionEnumPrimaryTheme::Unknown => Some(if themes.is_some() {
                StorageDatasetSelectionEnumThemeReason::Empty
            } else {
                StorageDatasetSelectionEnumThemeReason::Missing
            }),
        };
        if let StorageDatasetSelectionEnumPuzzleRatingBand::Known(band) = rating_band {
            band.endpoints()?;
        }
        Ok(StorageDatasetSelectionPuzzleClassification {
            stratum: StorageDatasetSelectionPuzzleStratum {
                rating_band,
                primary_theme,
            },
            rating,
            theme_reason,
        })
    }

    pub fn master_eligibility(
        source: StorageDatasetSelectionEnumSource,
        white: Option<&str>,
        black: Option<&str>,
    ) -> StorageDatasetSelectionEnumMasterEligibility {
        const TITLES: [&str; 6] = ["GM", "IM", "FM", "WGM", "WIM", "WFM"];
        match source {
            StorageDatasetSelectionEnumSource::Broadcast => {
                return StorageDatasetSelectionEnumMasterEligibility::Broadcast;
            }
            StorageDatasetSelectionEnumSource::Standard => {}
        }
        for (color, title) in [
            (BoardEnumColor::White, white),
            (BoardEnumColor::Black, black),
        ] {
            let Some(title) = title else {
                return StorageDatasetSelectionEnumMasterEligibility::MissingTitle(color);
            };
            if !TITLES.contains(&title) {
                return StorageDatasetSelectionEnumMasterEligibility::UnsupportedTitle(color);
            }
        }
        StorageDatasetSelectionEnumMasterEligibility::Eligible
    }
}
