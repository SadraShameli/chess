use std::str::FromStr;

use chess::clock::duration::ClockInstant;
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::request_cache::DesktopRequestCache;

#[test]
fn idempotency_has_bounded_retention_and_refuses_payload_reuse()
-> Result<(), Box<dyn std::error::Error>> {
    let mut cache = DesktopRequestCache::new();
    let id = DesktopRequestId::from_str("123e4567-e89b-42d3-a456-426614174000")?;
    let now = ClockInstant::from_millis(0)?;
    cache.remember(id, [1; 32], 17_u64, now)?;
    assert_eq!(cache.lookup(id, &[1; 32], now)?, Some(&17));
    assert!(cache.lookup(id, &[2; 32], now).is_err());
    assert_eq!(
        cache.lookup(id, &[1; 32], ClockInstant::from_millis(300_000)?)?,
        None
    );
    for _ in 0..DesktopRequestCache::<u64>::CAPACITY + 1 {
        cache.remember(
            DesktopRequestId::new(),
            [3; 32],
            20,
            ClockInstant::from_millis(300_000)?,
        )?;
    }
    assert_eq!(cache.len(), DesktopRequestCache::<u64>::CAPACITY);
    Ok(())
}

#[test]
fn duplicate_results_never_extend_retention_or_replace_the_original_result()
-> Result<(), Box<dyn std::error::Error>> {
    let id = DesktopRequestId::new();
    let mut cache = DesktopRequestCache::new();
    cache.remember(id, [1; 32], 1, ClockInstant::from_nanos(0))?;
    cache.remember(id, [1; 32], 2, ClockInstant::from_millis(299_999)?)?;
    assert_eq!(
        cache.lookup(id, &[1; 32], ClockInstant::from_millis(299_999)?)?,
        Some(&1)
    );
    assert!(
        cache
            .lookup(id, &[1; 32], ClockInstant::from_millis(299_998)?)
            .is_err()
    );
    assert_eq!(
        cache.lookup(id, &[1; 32], ClockInstant::from_millis(300_000)?)?,
        None
    );
    Ok(())
}
