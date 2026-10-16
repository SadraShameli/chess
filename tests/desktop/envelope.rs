use std::str::FromStr;

use chess::clock::duration::ClockInstant;
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::request_cache::DesktopRequestCache;
use chess::desktop::stream::{DesktopEnumStreamClass, DesktopStream};

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
fn stream_saturation_preserves_control_events_through_resync()
-> Result<(), Box<dyn std::error::Error>> {
    let now = ClockInstant::from_millis(0)?;
    let mut stream = DesktopStream::new(DesktopRequestId::new(), now);
    for value in 0..DesktopStream::<u64>::MAX_UNACKNOWLEDGED {
        assert!(
            stream
                .publish(u64::try_from(value)?, DesktopEnumStreamClass::Control, now)?
                .is_some()
        );
    }
    assert!(
        stream
            .publish(100, DesktopEnumStreamClass::Snapshot, now)?
            .is_none()
    );
    assert!(
        stream
            .publish(101, DesktopEnumStreamClass::Control, now)
            .is_err()
    );
    assert!(stream.is_resync_required());
    Ok(())
}

#[test]
fn acknowledgement_deadlines_and_sequence_boundaries_are_checked()
-> Result<(), Box<dyn std::error::Error>> {
    let now = ClockInstant::from_millis(0)?;
    let mut stream = DesktopStream::new(DesktopRequestId::new(), now);
    let event = stream
        .publish(1_u64, DesktopEnumStreamClass::Control, now)?
        .ok_or("missing control event")?;
    assert!(stream.acknowledge(event.sequence().next()?, now).is_err());
    stream.acknowledge(event.sequence(), ClockInstant::from_millis(9_999)?)?;
    assert!(
        stream
            .publish(
                2,
                DesktopEnumStreamClass::Control,
                ClockInstant::from_millis(10_000)?
            )?
            .is_some()
    );
    assert!(
        stream
            .publish(
                3,
                DesktopEnumStreamClass::Control,
                ClockInstant::from_millis(20_000)?
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn deferred_snapshots_coalesce_and_resync_creates_a_monotonic_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let now = ClockInstant::from_nanos(0);
    let mut stream = DesktopStream::new(DesktopRequestId::new(), now);
    let mut latest = None;
    for _ in 0..DesktopStream::<u64>::MAX_UNACKNOWLEDGED {
        latest = stream.publish(1, DesktopEnumStreamClass::Control, now)?;
    }
    stream.publish(100, DesktopEnumStreamClass::Snapshot, now)?;
    stream.publish(200, DesktopEnumStreamClass::Snapshot, now)?;
    let sequence = latest.ok_or("missing latest event")?.sequence();
    stream.acknowledge(sequence, now)?;
    let flushed = stream.flush_pending(now)?;
    assert_eq!(flushed.len(), 1);
    let event = flushed.first().ok_or("missing coalesced snapshot")?;
    assert_eq!(*event.payload(), 200);
    assert_eq!(event.sequence(), sequence.next()?);
    stream.require_resync();
    assert!(
        stream
            .publish(300, DesktopEnumStreamClass::Control, now)
            .is_err()
    );
    let boundary = stream.resynchronize(400, now)?;
    assert!(boundary.sequence() > event.sequence());
    assert!(!stream.is_resync_required());
    Ok(())
}

#[test]
fn job_progress_is_limited_to_five_hertz_without_delaying_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let now = ClockInstant::from_nanos(0);
    let mut stream = DesktopStream::new(DesktopRequestId::new(), now);
    assert!(
        stream
            .publish(0, DesktopEnumStreamClass::JobProgress, now)?
            .is_some()
    );
    for millis in 1..200 {
        assert!(
            stream
                .publish(
                    millis,
                    DesktopEnumStreamClass::JobProgress,
                    ClockInstant::from_millis(millis)?
                )?
                .is_none()
        );
    }
    assert!(
        stream
            .flush_pending(ClockInstant::from_millis(199)?)?
            .is_empty()
    );
    let events = stream.flush_pending(ClockInstant::from_millis(200)?)?;
    assert_eq!(events.first().ok_or("missing progress")?.payload(), &199);
    assert!(
        stream
            .publish(
                1000,
                DesktopEnumStreamClass::Control,
                ClockInstant::from_millis(201)?
            )?
            .is_some()
    );
    assert!(
        stream
            .check_health(ClockInstant::from_millis(10_000)?)
            .is_err()
    );
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
