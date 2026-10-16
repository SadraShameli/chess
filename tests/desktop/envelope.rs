use std::str::FromStr;

use chess::clock::duration::ClockInstant;
use chess::desktop::envelope::DesktopEnvelope;
use chess::desktop::identity::{DesktopRequestId, DesktopRevision};
use chess::desktop::request_cache::DesktopRequestCache;
use chess::desktop::stream::{DesktopEnumStreamClass, DesktopStream};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct DesktopEnvelopeFixturePayload {
    label: String,
}

#[test]
fn round_trips_maximum_unsigned_counter_without_javascript_precision_loss()
-> Result<(), Box<dyn std::error::Error>> {
    let revision = DesktopRevision::from_str("18446744073709551615")?;
    assert_eq!(
        serde_json::to_string(&revision)?,
        "\"18446744073709551615\""
    );
    assert_eq!(
        serde_json::from_str::<DesktopRevision>("\"18446744073709551615\"")?,
        revision
    );
    for invalid in [
        "\"01\"",
        "\"-1\"",
        "\"18446744073709551616\"",
        "42",
        "\"+1\"",
        "\" 1\"",
    ] {
        assert!(
            serde_json::from_str::<DesktopRevision>(invalid).is_err(),
            "{invalid}"
        );
    }
    Ok(())
}

#[test]
fn rejects_future_protocols_and_noncanonical_request_ids() -> Result<(), Box<dyn std::error::Error>>
{
    let valid = r#"{"protocol_version":1,"request_id":"123e4567-e89b-42d3-a456-426614174000","expected_revision":"0","payload":{"label":"bootstrap"}}"#;
    let request: DesktopEnvelope<DesktopEnvelopeFixturePayload> = serde_json::from_str(valid)?;
    assert_eq!(request.expected_revision(), Some(DesktopRevision::ZERO));
    assert!(
        serde_json::from_str::<DesktopEnvelope<DesktopEnvelopeFixturePayload>>(
            &valid.replace("\"protocol_version\":1", "\"protocol_version\":2")
        )
        .is_err()
    );
    assert!(DesktopRequestId::from_str("123E4567-E89B-42D3-A456-426614174000").is_err());
    assert!(DesktopRequestId::from_str("not-an-id").is_err());
    Ok(())
}

#[test]
fn stale_revisions_are_not_accepted() -> Result<(), Box<dyn std::error::Error>> {
    let current = DesktopRevision::from_str("9007199254740993")?;
    assert!(
        current
            .check_expected(DesktopRevision::from_str("9007199254740992")?)
            .is_err()
    );
    assert!(current.check_expected(current).is_ok());
    assert!(
        DesktopRevision::from_str("18446744073709551615")?
            .next()
            .is_err()
    );
    Ok(())
}

#[test]
fn idempotency_has_bounded_retention_and_refuses_payload_reuse()
-> Result<(), Box<dyn std::error::Error>> {
    let mut cache = DesktopRequestCache::new();
    let id = DesktopRequestId::from_str("123e4567-e89b-42d3-a456-426614174000")?;
    let now = ClockInstant::from_millis(0);
    cache.remember(id, [1; 32], 17_u64, now)?;
    assert_eq!(cache.lookup(id, &[1; 32], now)?, Some(&17));
    assert!(cache.lookup(id, &[2; 32], now).is_err());
    assert_eq!(
        cache.lookup(id, &[1; 32], ClockInstant::from_millis(300_000))?,
        None
    );
    for _ in 0..DesktopRequestCache::<u64>::CAPACITY + 1 {
        cache.remember(
            DesktopRequestId::new(),
            [3; 32],
            20,
            ClockInstant::from_millis(300_000),
        )?;
    }
    assert_eq!(cache.len(), DesktopRequestCache::<u64>::CAPACITY);
    Ok(())
}

#[test]
fn stream_saturation_preserves_control_events_through_resync()
-> Result<(), Box<dyn std::error::Error>> {
    let now = ClockInstant::from_millis(0);
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
    let now = ClockInstant::from_millis(0);
    let mut stream = DesktopStream::new(DesktopRequestId::new(), now);
    let event = stream
        .publish(1_u64, DesktopEnumStreamClass::Control, now)?
        .ok_or("missing control event")?;
    assert!(stream.acknowledge(event.sequence().next()?, now).is_err());
    stream.acknowledge(event.sequence(), ClockInstant::from_millis(9_999))?;
    assert!(
        stream
            .publish(
                2,
                DesktopEnumStreamClass::Control,
                ClockInstant::from_millis(10_000)
            )?
            .is_some()
    );
    assert!(
        stream
            .publish(
                3,
                DesktopEnumStreamClass::Control,
                ClockInstant::from_millis(20_000)
            )
            .is_err()
    );
    Ok(())
}
