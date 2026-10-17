use std::num::NonZeroU64;

use crate::game::pause::GamePauseEnumCause;
use crate::game::recovery::{GameRecovery, GameRecoveryEnumTrigger};
use crate::puzzle::timer::PuzzleTimerEnumPauseReason;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopNativeLifecyclePuzzleWindowId(NonZeroU64);

impl DesktopNativeLifecyclePuzzleWindowId {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeLifecycleEnumWindowEvent {
    FocusLost,
    FocusGained,
    Hidden,
    Visible,
    Minimized,
    Restored,
    Occluded,
    Exposed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeLifecycleEnumSignal {
    Window {
        window: DesktopNativeLifecyclePuzzleWindowId,
        event: DesktopNativeLifecycleEnumWindowEvent,
    },
    Suspend,
    Resumed,
}

pub struct DesktopNativeLifecycle {
    puzzle_window: DesktopNativeLifecyclePuzzleWindowId,
}

impl DesktopNativeLifecycle {
    pub const fn new(puzzle_window: DesktopNativeLifecyclePuzzleWindowId) -> Self {
        Self { puzzle_window }
    }

    pub const fn game_trigger(
        signal: DesktopNativeLifecycleEnumSignal,
    ) -> Option<GameRecoveryEnumTrigger> {
        match signal {
            DesktopNativeLifecycleEnumSignal::Window { event, .. } => match event {
                DesktopNativeLifecycleEnumWindowEvent::FocusLost => {
                    Some(GameRecoveryEnumTrigger::FocusLost)
                }
                DesktopNativeLifecycleEnumWindowEvent::FocusGained => {
                    Some(GameRecoveryEnumTrigger::FocusGained)
                }
                DesktopNativeLifecycleEnumWindowEvent::Hidden
                | DesktopNativeLifecycleEnumWindowEvent::Visible
                | DesktopNativeLifecycleEnumWindowEvent::Minimized
                | DesktopNativeLifecycleEnumWindowEvent::Restored
                | DesktopNativeLifecycleEnumWindowEvent::Occluded
                | DesktopNativeLifecycleEnumWindowEvent::Exposed => None,
            },
            DesktopNativeLifecycleEnumSignal::Suspend => {
                Some(GameRecoveryEnumTrigger::ComputerSleep)
            }
            DesktopNativeLifecycleEnumSignal::Resumed => {
                Some(GameRecoveryEnumTrigger::WakeFromSleep)
            }
        }
    }

    pub const fn game_interruption(
        signal: DesktopNativeLifecycleEnumSignal,
    ) -> Option<GamePauseEnumCause> {
        match Self::game_trigger(signal) {
            Some(trigger) => GameRecovery::pause_cause(trigger),
            None => None,
        }
    }

    pub fn puzzle_interruption(
        &self,
        signal: DesktopNativeLifecycleEnumSignal,
    ) -> Option<PuzzleTimerEnumPauseReason> {
        match signal {
            DesktopNativeLifecycleEnumSignal::Window { window, event }
                if window == self.puzzle_window =>
            {
                match event {
                    DesktopNativeLifecycleEnumWindowEvent::FocusLost => {
                        Some(PuzzleTimerEnumPauseReason::FocusLost)
                    }
                    DesktopNativeLifecycleEnumWindowEvent::Hidden
                    | DesktopNativeLifecycleEnumWindowEvent::Minimized
                    | DesktopNativeLifecycleEnumWindowEvent::Occluded => {
                        Some(PuzzleTimerEnumPauseReason::WindowHidden)
                    }
                    DesktopNativeLifecycleEnumWindowEvent::FocusGained
                    | DesktopNativeLifecycleEnumWindowEvent::Visible
                    | DesktopNativeLifecycleEnumWindowEvent::Restored
                    | DesktopNativeLifecycleEnumWindowEvent::Exposed => None,
                }
            }
            DesktopNativeLifecycleEnumSignal::Suspend => Some(PuzzleTimerEnumPauseReason::Sleep),
            DesktopNativeLifecycleEnumSignal::Window { .. }
            | DesktopNativeLifecycleEnumSignal::Resumed => None,
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/desktop/native/puzzle_lifecycle.rs"]
mod tests;
