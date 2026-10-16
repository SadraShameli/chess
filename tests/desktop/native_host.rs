use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};

use chess::capacity::budget::CapacityBudget;
use chess::capacity::phase::CapacityPhase;
use chess::desktop::application::DesktopApplicationWakeProvider;
use chess::desktop::native::event_loop::DesktopNativeEventLoopWake;
use chess::desktop::native::recovery::{
    DesktopNativeRecovery, DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryEnumStage,
    DesktopNativeRecoveryError, DesktopNativeRecoveryFailure, DesktopNativeRecoveryProvider,
};

struct DesktopNativeRecoveryFixture {
    calls: AtomicUsize,
    action: DesktopNativeRecoveryEnumAction,
}

impl DesktopNativeRecoveryProvider for DesktopNativeRecoveryFixture {
    fn choose(
        &self,
        _failure: &DesktopNativeRecoveryFailure,
    ) -> Result<DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.action)
    }
}

#[test]
fn native_wakes_coalesce_before_event_loop_attachment() -> Result<(), Box<dyn std::error::Error>> {
    let wake = DesktopNativeEventLoopWake::new();
    assert!(!wake.is_pending());
    wake.wake()?;
    wake.wake()?;
    assert!(wake.is_pending());
    wake.acknowledge();
    assert!(!wake.is_pending());
    wake.wake()?;
    assert!(wake.is_pending());
    Ok(())
}

#[test]
fn native_recovery_waits_for_interruption_and_does_not_repeat_a_dismissed_dialog()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::new(4).ok_or("zero capacity")?)?;
    let phase = CapacityPhase::new(budget)?;
    let generation = phase.generation().ok_or("missing preparation generation")?;
    let provider = DesktopNativeRecoveryFixture {
        calls: AtomicUsize::new(0),
        action: DesktopNativeRecoveryEnumAction::KeepPaused,
    };
    let mut recovery = DesktopNativeRecovery::new(
        DesktopNativeRecoveryFailure::new(
            DesktopNativeRecoveryEnumStage::Device,
            "The graphics device was removed".to_owned(),
        ),
        generation,
    );
    assert!(matches!(
        recovery.choose(&provider),
        Err(DesktopNativeRecoveryError::InterruptionUnacknowledged)
    ));
    assert_eq!(provider.calls.load(Ordering::Relaxed), 0);
    recovery.acknowledge(generation)?;
    assert_eq!(
        recovery.choose(&provider)?,
        DesktopNativeRecoveryEnumAction::KeepPaused
    );
    assert_eq!(
        recovery.choose(&provider)?,
        DesktopNativeRecoveryEnumAction::KeepPaused
    );
    assert_eq!(provider.calls.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn stale_recovery_acknowledgments_cannot_open_the_dialog() -> Result<(), Box<dyn std::error::Error>>
{
    let budget = CapacityBudget::new(NonZeroUsize::new(4).ok_or("zero capacity")?)?;
    let mut phase = CapacityPhase::new(budget)?;
    let first = phase.generation().ok_or("missing preparation generation")?;
    phase.acknowledge_searches_stopped(first)?;
    phase.try_enter_preparation(first)?;
    phase.complete_preparation(first)?;
    let next = phase.request_preparation()?;
    let mut recovery = DesktopNativeRecovery::new(
        DesktopNativeRecoveryFailure::new(
            DesktopNativeRecoveryEnumStage::Surface,
            "The window surface is unavailable".to_owned(),
        ),
        next,
    );
    assert!(matches!(
        recovery.acknowledge(first),
        Err(DesktopNativeRecoveryError::StaleGeneration)
    ));
    Ok(())
}
