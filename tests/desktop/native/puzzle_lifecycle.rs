use std::num::NonZeroU64;

use crate::desktop::native::lifecycle::{
    DesktopNativeLifecycle, DesktopNativeLifecycleEnumSignal,
    DesktopNativeLifecycleEnumWindowEvent, DesktopNativeLifecyclePuzzleWindowId,
};
use crate::puzzle::timer::PuzzleTimerEnumPauseReason;

#[test]
fn hiding_puzzle_window_pauses_even_with_another_app_window_focused()
-> Result<(), Box<dyn std::error::Error>> {
    let puzzle = DesktopNativeLifecyclePuzzleWindowId::new(NonZeroU64::MIN);
    let other = DesktopNativeLifecyclePuzzleWindowId::new(
        NonZeroU64::new(2).ok_or("Missing other window identity")?,
    );
    let lifecycle = DesktopNativeLifecycle::new(puzzle);
    assert_eq!(
        lifecycle.puzzle_interruption(DesktopNativeLifecycleEnumSignal::Window {
            window: other,
            event: DesktopNativeLifecycleEnumWindowEvent::FocusGained,
        }),
        None
    );
    for event in [
        DesktopNativeLifecycleEnumWindowEvent::Hidden,
        DesktopNativeLifecycleEnumWindowEvent::Minimized,
        DesktopNativeLifecycleEnumWindowEvent::Occluded,
    ] {
        assert_eq!(
            lifecycle.puzzle_interruption(DesktopNativeLifecycleEnumSignal::Window {
                window: other,
                event,
            }),
            None
        );
        assert_eq!(
            lifecycle.puzzle_interruption(DesktopNativeLifecycleEnumSignal::Window {
                window: puzzle,
                event,
            }),
            Some(PuzzleTimerEnumPauseReason::WindowHidden)
        );
    }
    Ok(())
}

#[test]
fn focus_restoration_and_resume_signals_never_request_countdown_resume() {
    let window = DesktopNativeLifecyclePuzzleWindowId::new(NonZeroU64::MIN);
    let lifecycle = DesktopNativeLifecycle::new(window);
    assert_eq!(
        lifecycle.puzzle_interruption(DesktopNativeLifecycleEnumSignal::Window {
            window,
            event: DesktopNativeLifecycleEnumWindowEvent::FocusLost,
        }),
        Some(PuzzleTimerEnumPauseReason::FocusLost)
    );
    for event in [
        DesktopNativeLifecycleEnumWindowEvent::FocusGained,
        DesktopNativeLifecycleEnumWindowEvent::Visible,
        DesktopNativeLifecycleEnumWindowEvent::Restored,
        DesktopNativeLifecycleEnumWindowEvent::Exposed,
    ] {
        assert_eq!(
            lifecycle
                .puzzle_interruption(DesktopNativeLifecycleEnumSignal::Window { window, event }),
            None
        );
    }
    assert_eq!(
        lifecycle.puzzle_interruption(DesktopNativeLifecycleEnumSignal::Suspend),
        Some(PuzzleTimerEnumPauseReason::Sleep)
    );
    assert_eq!(
        lifecycle.puzzle_interruption(DesktopNativeLifecycleEnumSignal::Resumed),
        None
    );
}
