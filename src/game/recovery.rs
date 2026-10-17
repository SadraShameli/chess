use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRecoveryEnumTrigger {
    FocusLost,
    FocusGained,
    ComputerSleep,
    WakeFromSleep,
    TabClosed,
    ApplicationQuit,
    ProcessCrash,
    Fault(GamePauseEnumFault),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRecoveryEnumAction {
    KeepRunning,
    Pause(GamePauseEnumCause),
    RestorePausedFromLastDurable,
}

pub struct GameRecovery;

impl GameRecovery {
    pub const fn decide(trigger: GameRecoveryEnumTrigger) -> GameRecoveryEnumAction {
        match trigger {
            GameRecoveryEnumTrigger::FocusLost
            | GameRecoveryEnumTrigger::FocusGained
            | GameRecoveryEnumTrigger::WakeFromSleep => GameRecoveryEnumAction::KeepRunning,
            GameRecoveryEnumTrigger::ComputerSleep => {
                GameRecoveryEnumAction::Pause(GamePauseEnumCause::ComputerSleep)
            }
            GameRecoveryEnumTrigger::TabClosed => {
                GameRecoveryEnumAction::Pause(GamePauseEnumCause::TabClosed)
            }
            GameRecoveryEnumTrigger::ApplicationQuit => {
                GameRecoveryEnumAction::Pause(GamePauseEnumCause::ApplicationQuit)
            }
            GameRecoveryEnumTrigger::ProcessCrash => {
                GameRecoveryEnumAction::RestorePausedFromLastDurable
            }
            GameRecoveryEnumTrigger::Fault(fault) => {
                GameRecoveryEnumAction::Pause(GamePauseEnumCause::Fault(fault))
            }
        }
    }

    pub const fn pause_cause(trigger: GameRecoveryEnumTrigger) -> Option<GamePauseEnumCause> {
        match Self::decide(trigger) {
            GameRecoveryEnumAction::Pause(cause) => Some(cause),
            GameRecoveryEnumAction::RestorePausedFromLastDurable => {
                Some(GamePauseEnumCause::Relaunch)
            }
            GameRecoveryEnumAction::KeepRunning => None,
        }
    }

    pub const fn requires_retry_before_resume(
        cause: GamePauseEnumCause,
    ) -> Option<GamePauseEnumFault> {
        match cause {
            GamePauseEnumCause::Fault(fault) => Some(fault),
            GamePauseEnumCause::AutosaveFailure => Some(GamePauseEnumFault::Save),
            GamePauseEnumCause::Manual
            | GamePauseEnumCause::ComputerSleep
            | GamePauseEnumCause::Relaunch
            | GamePauseEnumCause::TabClosed
            | GamePauseEnumCause::ApplicationQuit
            | GamePauseEnumCause::UpdateInstall => None,
        }
    }

    pub const fn resumes_automatically(_cause: GamePauseEnumCause) -> bool {
        false
    }
}
