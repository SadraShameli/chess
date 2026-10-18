use thiserror::Error;

use crate::capacity::phase::CapacityPhaseGeneration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeRecoveryEnumAction {
    Retry,
    SafeQuit,
    KeepPaused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeRecoveryEnumStage {
    Adapter,
    Device,
    Surface,
    Renderer,
    Assets,
}

#[derive(Debug, Error)]
pub enum DesktopNativeRecoveryError {
    #[error("The native recovery dialog failed: {source}")]
    Dialog {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("The native recovery choice does not belong to the interrupted generation")]
    StaleGeneration,
    #[error("The native recovery dialog requires acknowledged application interruption")]
    InterruptionUnacknowledged,
}

#[derive(Debug, Clone)]
pub struct DesktopNativeRecoveryFailure {
    stage: DesktopNativeRecoveryEnumStage,
    message: String,
}

impl DesktopNativeRecoveryFailure {
    pub fn new(stage: DesktopNativeRecoveryEnumStage, message: String) -> Self {
        Self { stage, message }
    }

    pub const fn stage(&self) -> DesktopNativeRecoveryEnumStage {
        self.stage
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub trait DesktopNativeRecoveryProvider: Send + Sync {
    fn choose(
        &self,
        failure: &DesktopNativeRecoveryFailure,
    ) -> Result<DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryError>;
}

pub struct DesktopNativeRecovery {
    failure: DesktopNativeRecoveryFailure,
    generation: CapacityPhaseGeneration,
    acknowledged: bool,
    choice: Option<DesktopNativeRecoveryEnumAction>,
}

impl DesktopNativeRecovery {
    pub fn new(failure: DesktopNativeRecoveryFailure, generation: CapacityPhaseGeneration) -> Self {
        Self {
            failure,
            generation,
            acknowledged: false,
            choice: None,
        }
    }

    pub fn acknowledge(
        &mut self,
        generation: CapacityPhaseGeneration,
    ) -> Result<(), DesktopNativeRecoveryError> {
        if generation != self.generation {
            return Err(DesktopNativeRecoveryError::StaleGeneration);
        }
        self.acknowledged = true;
        Ok(())
    }

    pub fn choose(
        &mut self,
        provider: &dyn DesktopNativeRecoveryProvider,
    ) -> Result<DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryError> {
        if !self.acknowledged {
            return Err(DesktopNativeRecoveryError::InterruptionUnacknowledged);
        }
        if let Some(choice) = self.choice {
            return Ok(choice);
        }
        let choice = provider.choose(&self.failure)?;
        self.choice = Some(choice);
        Ok(choice)
    }

    pub const fn generation(&self) -> CapacityPhaseGeneration {
        self.generation
    }

    pub fn failure(&self) -> &DesktopNativeRecoveryFailure {
        &self.failure
    }
}
