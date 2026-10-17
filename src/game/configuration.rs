use rand::Rng;

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::{BoardStart, BoardStartError, BoardStartIndex};
use crate::clock::duration::ClockDuration;
use crate::clock::time_control::{ClockTimeControl, ClockTimeControlError};
use crate::game::engine_provider::GameEngineSearchConfig;
use crate::game::enums::{GameEnumMode, GameEnumSeat};
use crate::game::session::{GameSeats, GameSessionError};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::Display,
    strum::EnumIter,
)]
pub enum GameConfigurationEnumPreset {
    #[strum(serialize = "Untimed")]
    Untimed,
    #[strum(serialize = "1+0")]
    OnePlusZero,
    #[strum(serialize = "2+1")]
    TwoPlusOne,
    #[strum(serialize = "3+0")]
    ThreePlusZero,
    #[strum(serialize = "3+2")]
    ThreePlusTwo,
    #[strum(serialize = "5+0")]
    FivePlusZero,
    #[strum(serialize = "5+3")]
    FivePlusThree,
    #[strum(serialize = "10+0")]
    TenPlusZero,
    #[strum(serialize = "10+5")]
    TenPlusFive,
    #[strum(serialize = "15+10")]
    FifteenPlusTen,
    #[strum(serialize = "30+0")]
    ThirtyPlusZero,
}

impl GameConfigurationEnumPreset {
    pub const ALL: [Self; 11] = [
        Self::Untimed,
        Self::OnePlusZero,
        Self::TwoPlusOne,
        Self::ThreePlusZero,
        Self::ThreePlusTwo,
        Self::FivePlusZero,
        Self::FivePlusThree,
        Self::TenPlusZero,
        Self::TenPlusFive,
        Self::FifteenPlusTen,
        Self::ThirtyPlusZero,
    ];

    pub fn label(self) -> &'static str {
        self.into()
    }

    pub fn time_control(self) -> Result<ClockTimeControl, ClockTimeControlError> {
        let (base_millis, increment_millis) = match self {
            Self::Untimed => return Ok(ClockTimeControl::untimed()),
            Self::TenPlusFive => return Ok(ClockTimeControl::initial_default()),
            Self::OnePlusZero => (60_000, 0),
            Self::TwoPlusOne => (120_000, 1000),
            Self::ThreePlusZero => (180_000, 0),
            Self::ThreePlusTwo => (180_000, 2000),
            Self::FivePlusZero => (300_000, 0),
            Self::FivePlusThree => (300_000, 3000),
            Self::TenPlusZero => (600_000, 0),
            Self::FifteenPlusTen => (900_000, 10_000),
            Self::ThirtyPlusZero => (1_800_000, 0),
        };
        let base = ClockDuration::from_millis(base_millis)?;
        if increment_millis == 0 {
            ClockTimeControl::sudden_death(base)
        } else {
            ClockTimeControl::fischer(base, ClockDuration::from_millis(increment_millis)?)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameConfigurationEnumOpponent {
    Reckless { human: BoardEnumColor },
    LocalTwoPlayer,
}

impl GameConfigurationEnumOpponent {
    pub fn seats(self) -> Result<GameSeats, GameSessionError> {
        let (mode, white, black) = match self {
            Self::Reckless {
                human: BoardEnumColor::White,
            } => (
                GameEnumMode::HumanVersusEngine,
                GameEnumSeat::Human,
                GameEnumSeat::Reckless,
            ),
            Self::Reckless {
                human: BoardEnumColor::Black,
            } => (
                GameEnumMode::HumanVersusEngine,
                GameEnumSeat::Reckless,
                GameEnumSeat::Human,
            ),
            Self::LocalTwoPlayer => (
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            ),
        };
        GameSeats::new(mode, white, black)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameConfigurationEnumIndex {
    Random,
    Numbered(BoardStartIndex),
}

impl GameConfigurationEnumIndex {
    pub fn resolve<R: Rng + ?Sized>(self, random: &mut R) -> BoardStartIndex {
        match self {
            Self::Random => BoardStartIndex::random(random),
            Self::Numbered(index) => index,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameConfigurationEnumStart {
    Standard,
    Chess960 {
        index: GameConfigurationEnumIndex,
    },
    Dfrc {
        white: GameConfigurationEnumIndex,
        black: GameConfigurationEnumIndex,
    },
    Custom(BoardPosition),
}

impl GameConfigurationEnumStart {
    pub fn resolve<R: Rng + ?Sized>(&self, random: &mut R) -> Result<BoardStart, BoardStartError> {
        match self {
            Self::Standard => Ok(BoardStart::standard()),
            Self::Chess960 { index } => BoardStart::chess960(index.resolve(random)),
            Self::Dfrc { white, black } => {
                BoardStart::dfrc(white.resolve(random), black.resolve(random))
            }
            Self::Custom(position) => Ok(BoardStart::custom(position.clone())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameConfigurationOptions {
    engine_search: GameEngineSearchConfig,
    requires_local_takeback_consent: bool,
    is_premove_enabled: bool,
    is_auto_flip_enabled: bool,
}

impl GameConfigurationOptions {
    pub const fn new(
        engine_search: GameEngineSearchConfig,
        requires_local_takeback_consent: bool,
        is_premove_enabled: bool,
        is_auto_flip_enabled: bool,
    ) -> Self {
        Self {
            engine_search,
            requires_local_takeback_consent,
            is_premove_enabled,
            is_auto_flip_enabled,
        }
    }

    pub const fn initial_default() -> Self {
        Self::new(
            GameEngineSearchConfig::initial_default(),
            true,
            false,
            false,
        )
    }

    pub const fn engine_search(self) -> GameEngineSearchConfig {
        self.engine_search
    }
    pub const fn requires_local_takeback_consent(self) -> bool {
        self.requires_local_takeback_consent
    }
    pub const fn is_premove_enabled(self) -> bool {
        self.is_premove_enabled
    }
    pub const fn is_auto_flip_enabled(self) -> bool {
        self.is_auto_flip_enabled
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameConfiguration {
    start: GameConfigurationEnumStart,
    opponent: GameConfigurationEnumOpponent,
    time_control: ClockTimeControl,
    options: GameConfigurationOptions,
}

impl GameConfiguration {
    pub const fn new(
        start: GameConfigurationEnumStart,
        opponent: GameConfigurationEnumOpponent,
        time_control: ClockTimeControl,
        options: GameConfigurationOptions,
    ) -> Self {
        Self {
            start,
            opponent,
            time_control,
            options,
        }
    }

    pub fn initial_default() -> Self {
        Self::new(
            GameConfigurationEnumStart::Standard,
            GameConfigurationEnumOpponent::Reckless {
                human: BoardEnumColor::White,
            },
            ClockTimeControl::initial_default(),
            GameConfigurationOptions::initial_default(),
        )
    }

    pub fn start(&self) -> &GameConfigurationEnumStart {
        &self.start
    }
    pub const fn opponent(&self) -> GameConfigurationEnumOpponent {
        self.opponent
    }
    pub fn time_control(&self) -> &ClockTimeControl {
        &self.time_control
    }
    pub const fn options(&self) -> GameConfigurationOptions {
        self.options
    }
    pub fn seats(&self) -> Result<GameSeats, GameSessionError> {
        self.opponent.seats()
    }
}
