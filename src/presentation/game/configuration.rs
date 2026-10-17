use std::num::NonZeroU32;

use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::BoardStartIndexError;
use crate::clock::duration::{ClockDuration, ClockDurationError};
use crate::clock::stage::{ClockStage, ClockStageEnumBonus, ClockStageEnumQuota, ClockStageError};
use crate::clock::time_control::{ClockTimeControl, ClockTimeControlError};
use crate::game::configuration::{
    GameConfiguration, GameConfigurationEnumOpponent, GameConfigurationEnumPreset,
    GameConfigurationEnumStart, GameConfigurationOptions,
};
use crate::game::engine_provider::{GameEngineRequestError, GameEngineSearchConfig};
use crate::game::enums::GameEnumMode;

#[derive(Debug, Error)]
pub enum PresentationGameConfigurationError {
    #[error("Add at least one clock stage")]
    EmptyStages,
    #[error("A repeating or intermediate stage needs at least one move")]
    EmptyQuota,
    #[error("This clock format requires exactly one stage")]
    MultipleStages,
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
    #[error(transparent)]
    Stage(#[from] ClockStageError),
    #[error(transparent)]
    TimeControl(#[from] ClockTimeControlError),
    #[error(transparent)]
    Engine(#[from] GameEngineRequestError),
    #[error(transparent)]
    StartIndex(#[from] BoardStartIndexError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::Display, strum::EnumIter)]
pub enum PresentationGameConfigurationEnumClockFormat {
    #[strum(serialize = "Sudden death")]
    SuddenDeath,
    #[strum(serialize = "Fischer increment")]
    Fischer,
    #[strum(serialize = "Staged")]
    Staged,
    #[strum(serialize = "Cyclic")]
    Cyclic,
    #[strum(serialize = "Simple delay")]
    SimpleDelay,
    #[strum(serialize = "Bronstein delay")]
    Bronstein,
}

impl PresentationGameConfigurationEnumClockFormat {
    pub fn label(self) -> &'static str {
        self.into()
    }

    pub(super) const fn bonus(
        self,
        staged: PresentationGameConfigurationEnumBonus,
    ) -> PresentationGameConfigurationEnumBonus {
        match self {
            Self::SuddenDeath => PresentationGameConfigurationEnumBonus::None,
            Self::Fischer => PresentationGameConfigurationEnumBonus::Fischer,
            Self::SimpleDelay => PresentationGameConfigurationEnumBonus::SimpleDelay,
            Self::Bronstein => PresentationGameConfigurationEnumBonus::Bronstein,
            Self::Staged | Self::Cyclic => staged,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::Display, strum::EnumIter)]
pub enum PresentationGameConfigurationEnumBonus {
    #[strum(serialize = "No increment or delay")]
    None,
    #[strum(serialize = "Fischer increment")]
    Fischer,
    #[strum(serialize = "Simple delay")]
    SimpleDelay,
    #[strum(serialize = "Bronstein delay")]
    Bronstein,
}

impl PresentationGameConfigurationEnumBonus {
    pub fn label(self) -> &'static str {
        self.into()
    }

    fn resolve(self, millis: u64) -> Result<ClockStageEnumBonus, ClockDurationError> {
        Ok(match self {
            Self::None => ClockStageEnumBonus::None,
            Self::Fischer => ClockStageEnumBonus::Fischer(ClockDuration::from_millis(millis)?),
            Self::SimpleDelay => {
                ClockStageEnumBonus::SimpleDelay(ClockDuration::from_millis(millis)?)
            }
            Self::Bronstein => {
                ClockStageEnumBonus::BronsteinDelay(ClockDuration::from_millis(millis)?)
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PresentationGameConfigurationEnumControl {
    Preset(GameConfigurationEnumPreset),
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationGameConfigurationEnumFinalQuota {
    Unlimited,
    Cyclic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationGameConfigurationStage {
    pub(super) base_millis: u64,
    pub(super) bonus_millis: u64,
    pub(super) moves: u32,
    pub(super) bonus: PresentationGameConfigurationEnumBonus,
}

impl PresentationGameConfigurationStage {
    const INITIAL_MOVES: u32 = 40;

    pub const fn new(
        base_millis: u64,
        bonus_millis: u64,
        moves: u32,
        bonus: PresentationGameConfigurationEnumBonus,
    ) -> Self {
        Self {
            base_millis,
            bonus_millis,
            moves,
            bonus,
        }
    }

    fn from_stage(stage: ClockStage) -> Self {
        let (bonus, bonus_millis) = match stage.bonus() {
            ClockStageEnumBonus::None => (PresentationGameConfigurationEnumBonus::None, 0),
            ClockStageEnumBonus::Fischer(duration) => (
                PresentationGameConfigurationEnumBonus::Fischer,
                duration.as_millis(),
            ),
            ClockStageEnumBonus::SimpleDelay(duration) => (
                PresentationGameConfigurationEnumBonus::SimpleDelay,
                duration.as_millis(),
            ),
            ClockStageEnumBonus::BronsteinDelay(duration) => (
                PresentationGameConfigurationEnumBonus::Bronstein,
                duration.as_millis(),
            ),
        };
        let moves = match stage.quota() {
            ClockStageEnumQuota::Moves(moves) | ClockStageEnumQuota::Cyclic(moves) => moves.get(),
            ClockStageEnumQuota::Unlimited => Self::INITIAL_MOVES,
        };
        Self::new(stage.base_time().as_millis(), bonus_millis, moves, bonus)
    }

    fn quota(self) -> Result<NonZeroU32, PresentationGameConfigurationError> {
        NonZeroU32::new(self.moves).ok_or(PresentationGameConfigurationError::EmptyQuota)
    }

    fn resolve(
        self,
        quota: ClockStageEnumQuota,
        bonus: PresentationGameConfigurationEnumBonus,
    ) -> Result<ClockStage, PresentationGameConfigurationError> {
        Ok(ClockStage::new(
            quota,
            ClockDuration::from_millis(self.base_millis)?,
            bonus.resolve(self.bonus_millis)?,
        )?)
    }
}

#[derive(Debug, Clone)]
pub struct PresentationGameConfiguration {
    pub(super) mode: GameEnumMode,
    pub(super) human: BoardEnumColor,
    pub(super) start: GameConfigurationEnumStart,
    pub(super) custom_position: Option<BoardPosition>,
    pub(super) control: PresentationGameConfigurationEnumControl,
    pub(super) clock_format: PresentationGameConfigurationEnumClockFormat,
    pub(super) final_quota: PresentationGameConfigurationEnumFinalQuota,
    pub(super) stages: Vec<PresentationGameConfigurationStage>,
    pub(super) untimed_move_millis: u64,
    pub(super) requires_local_takeback_consent: bool,
    pub(super) is_premove_enabled: bool,
    pub(super) is_auto_flip_enabled: bool,
    move_overhead: ClockDuration,
}

impl PresentationGameConfiguration {
    pub fn new() -> Self {
        let default = GameConfiguration::initial_default();
        let options = default.options();
        Self {
            mode: GameEnumMode::HumanVersusEngine,
            human: BoardEnumColor::White,
            start: default.start().clone(),
            custom_position: None,
            control: PresentationGameConfigurationEnumControl::Preset(
                GameConfigurationEnumPreset::TenPlusFive,
            ),
            clock_format: PresentationGameConfigurationEnumClockFormat::Fischer,
            final_quota: PresentationGameConfigurationEnumFinalQuota::Unlimited,
            stages: default
                .time_control()
                .stages()
                .iter()
                .copied()
                .map(PresentationGameConfigurationStage::from_stage)
                .collect(),
            untimed_move_millis: options.engine_search().untimed_move_time().as_millis(),
            move_overhead: options.engine_search().move_overhead(),
            requires_local_takeback_consent: options.requires_local_takeback_consent(),
            is_premove_enabled: options.is_premove_enabled(),
            is_auto_flip_enabled: options.is_auto_flip_enabled(),
        }
    }

    pub fn validate(&self) -> Result<GameConfiguration, PresentationGameConfigurationError> {
        let opponent = match self.mode {
            GameEnumMode::HumanVersusEngine => {
                GameConfigurationEnumOpponent::Reckless { human: self.human }
            }
            GameEnumMode::LocalTwoPlayer => GameConfigurationEnumOpponent::LocalTwoPlayer,
        };
        let engine = GameEngineSearchConfig::new(
            ClockDuration::from_millis(self.untimed_move_millis)?,
            self.move_overhead,
        )?;
        Ok(GameConfiguration::new(
            self.start.clone(),
            opponent,
            self.time_control()?,
            GameConfigurationOptions::new(
                engine,
                self.requires_local_takeback_consent,
                self.is_premove_enabled,
                self.is_auto_flip_enabled,
            ),
        ))
    }

    pub fn set_opponent(&mut self, opponent: GameConfigurationEnumOpponent) {
        match opponent {
            GameConfigurationEnumOpponent::Reckless { human } => {
                self.mode = GameEnumMode::HumanVersusEngine;
                self.human = human;
            }
            GameConfigurationEnumOpponent::LocalTwoPlayer => {
                self.mode = GameEnumMode::LocalTwoPlayer
            }
        }
    }

    pub fn set_start(&mut self, start: GameConfigurationEnumStart) {
        if let GameConfigurationEnumStart::Custom(position) = &start {
            self.custom_position = Some(position.clone());
        }
        self.start = start;
    }

    pub fn select_preset(&mut self, preset: GameConfigurationEnumPreset) {
        self.control = PresentationGameConfigurationEnumControl::Preset(preset);
    }

    pub fn set_untimed_move_millis(&mut self, millis: u64) {
        self.untimed_move_millis = millis;
    }

    pub fn set_custom_clock(
        &mut self,
        format: PresentationGameConfigurationEnumClockFormat,
        stages: Vec<PresentationGameConfigurationStage>,
    ) {
        self.control = PresentationGameConfigurationEnumControl::Custom;
        self.clock_format = format;
        self.stages = stages;
    }

    pub fn set_final_quota(&mut self, quota: PresentationGameConfigurationEnumFinalQuota) {
        self.final_quota = quota;
    }

    pub fn set_options(&mut self, options: GameConfigurationOptions) {
        self.untimed_move_millis = options.engine_search().untimed_move_time().as_millis();
        self.move_overhead = options.engine_search().move_overhead();
        self.requires_local_takeback_consent = options.requires_local_takeback_consent();
        self.is_premove_enabled = options.is_premove_enabled();
        self.is_auto_flip_enabled = options.is_auto_flip_enabled();
    }

    pub(super) fn add_stage(&mut self) {
        if let Some(stage) = self.stages.last().copied() {
            self.stages.push(stage);
        } else {
            self.stages.extend(
                ClockTimeControl::initial_default()
                    .stages()
                    .iter()
                    .copied()
                    .map(PresentationGameConfigurationStage::from_stage),
            );
        }
    }

    fn time_control(&self) -> Result<ClockTimeControl, PresentationGameConfigurationError> {
        if let PresentationGameConfigurationEnumControl::Preset(preset) = self.control {
            return Ok(preset.time_control()?);
        }
        let (last, preceding) = self
            .stages
            .split_last()
            .ok_or(PresentationGameConfigurationError::EmptyStages)?;
        let stages = match self.clock_format {
            PresentationGameConfigurationEnumClockFormat::Staged => {
                let mut stages = Vec::with_capacity(self.stages.len());
                for stage in preceding {
                    stages.push(
                        stage.resolve(ClockStageEnumQuota::Moves(stage.quota()?), stage.bonus)?,
                    );
                }
                let quota = match self.final_quota {
                    PresentationGameConfigurationEnumFinalQuota::Unlimited => {
                        ClockStageEnumQuota::Unlimited
                    }
                    PresentationGameConfigurationEnumFinalQuota::Cyclic => {
                        ClockStageEnumQuota::Cyclic(last.quota()?)
                    }
                };
                stages.push(last.resolve(quota, last.bonus)?);
                stages
            }
            PresentationGameConfigurationEnumClockFormat::Cyclic => {
                if !preceding.is_empty() {
                    return Err(PresentationGameConfigurationError::MultipleStages);
                }
                vec![last.resolve(ClockStageEnumQuota::Cyclic(last.quota()?), last.bonus)?]
            }
            format @ (PresentationGameConfigurationEnumClockFormat::SuddenDeath
            | PresentationGameConfigurationEnumClockFormat::Fischer
            | PresentationGameConfigurationEnumClockFormat::SimpleDelay
            | PresentationGameConfigurationEnumClockFormat::Bronstein) => {
                if !preceding.is_empty() {
                    return Err(PresentationGameConfigurationError::MultipleStages);
                }
                let bonus = format.bonus(last.bonus);
                vec![last.resolve(ClockStageEnumQuota::Unlimited, bonus)?]
            }
        };
        Ok(ClockTimeControl::staged(stages)?)
    }
}

impl Default for PresentationGameConfiguration {
    fn default() -> Self {
        Self::new()
    }
}
