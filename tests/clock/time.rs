use std::sync::atomic::{AtomicU64, Ordering};

use chess::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;

struct ClockTimeTestProvider(AtomicU64);

impl ClockTimeProvider for ClockTimeTestProvider {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}

#[test]
fn deterministic_deadlines_keep_submillisecond_precision() -> Result<(), Box<dyn std::error::Error>>
{
    let provider = ClockTimeTestProvider(AtomicU64::new(100));
    let started = provider.now()?;
    let deadline = started.checked_add(ClockDuration::from_nanos(25))?;
    provider.0.store(124, Ordering::SeqCst);
    assert!(!provider.now()?.has_reached(deadline));
    provider.0.store(125, Ordering::SeqCst);
    assert!(provider.now()?.has_reached(deadline));
    assert_eq!(provider.now()?.elapsed_since(started)?.as_nanos(), 25);
    Ok(())
}

#[test]
fn rejects_overflows_and_backwards_time() -> Result<(), Box<dyn std::error::Error>> {
    assert!(ClockDuration::from_millis(u64::MAX).is_err());
    assert!(ClockInstant::from_millis(u64::MAX).is_err());
    assert!(
        ClockInstant::from_nanos(u64::MAX)
            .checked_add(ClockDuration::from_nanos(1))
            .is_err()
    );
    assert!(
        ClockInstant::from_nanos(1)
            .elapsed_since(ClockInstant::from_nanos(2))
            .is_err()
    );
    let duration = ClockDuration::from_millis(50)?;
    assert_eq!(duration.as_millis(), 50);
    assert_eq!(ClockDuration::try_from(duration.as_duration())?, duration);
    Ok(())
}
