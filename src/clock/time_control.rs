use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError};
use crate::clock::stage::{ClockStage, ClockStageEnumBonus, ClockStageEnumQuota, ClockStageError};

#[derive(Debug, Error)]
pub enum ClockTimeControlError {
    #[error("A timed control requires at least one stage")]
    EmptyStages,
    #[error("Only the final stage can be unlimited or cyclic")]
    NonfinalQuota,
    #[error("The final stage must be unlimited or cyclic")]
    FinalQuota,
    #[error(transparent)]
    Stage(#[from] ClockStageError),
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockTimeControl {
    stages: Vec<ClockStage>,
}

impl ClockTimeControl {
    pub const fn untimed() -> Self {
        Self { stages: Vec::new() }
    }

    pub fn initial_default() -> Self {
        Self {
            stages: vec![ClockStage::INITIAL_DEFAULT],
        }
    }

    pub fn staged(stages: Vec<ClockStage>) -> Result<Self, ClockTimeControlError> {
        let Some((last, preceding)) = stages.split_last() else {
            return Err(ClockTimeControlError::EmptyStages);
        };
        if !matches!(
            last.quota(),
            ClockStageEnumQuota::Unlimited | ClockStageEnumQuota::Cyclic(_)
        ) {
            return Err(ClockTimeControlError::FinalQuota);
        }
        if preceding
            .iter()
            .any(|stage| !matches!(stage.quota(), ClockStageEnumQuota::Moves(_)))
        {
            return Err(ClockTimeControlError::NonfinalQuota);
        }
        let mut accumulated = ClockDuration::ZERO;
        for stage in &stages {
            accumulated = accumulated.checked_add(stage.base_time())?;
            accumulated.checked_add(stage.bonus().delay())?;
            let moves = match stage.quota() {
                ClockStageEnumQuota::Moves(quota) | ClockStageEnumQuota::Cyclic(quota) => {
                    u64::from(quota.get())
                }
                ClockStageEnumQuota::Unlimited => 1,
            };
            let increment = stage
                .bonus()
                .increment()
                .as_nanos()
                .checked_mul(moves)
                .ok_or(ClockDurationError::Overflow)?;
            accumulated = accumulated.checked_add(ClockDuration::from_nanos(increment))?;
        }
        Ok(Self { stages })
    }

    pub fn sudden_death(base_time: ClockDuration) -> Result<Self, ClockTimeControlError> {
        Self::single(base_time, ClockStageEnumBonus::None)
    }

    pub fn fischer(
        base_time: ClockDuration,
        increment: ClockDuration,
    ) -> Result<Self, ClockTimeControlError> {
        Self::single(base_time, ClockStageEnumBonus::Fischer(increment))
    }

    pub fn simple_delay(
        base_time: ClockDuration,
        delay: ClockDuration,
    ) -> Result<Self, ClockTimeControlError> {
        Self::single(base_time, ClockStageEnumBonus::SimpleDelay(delay))
    }

    pub fn bronstein_delay(
        base_time: ClockDuration,
        delay: ClockDuration,
    ) -> Result<Self, ClockTimeControlError> {
        Self::single(base_time, ClockStageEnumBonus::BronsteinDelay(delay))
    }

    pub fn is_timed(&self) -> bool {
        !self.stages.is_empty()
    }

    pub fn stages(&self) -> &[ClockStage] {
        &self.stages
    }

    fn single(
        base_time: ClockDuration,
        bonus: ClockStageEnumBonus,
    ) -> Result<Self, ClockTimeControlError> {
        Self::staged(vec![ClockStage::new(
            ClockStageEnumQuota::Unlimited,
            base_time,
            bonus,
        )?])
    }
}
