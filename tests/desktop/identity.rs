use std::str::FromStr;

use chess::desktop::envelope::DesktopEnvelope;
use chess::desktop::identity::{DesktopRequestId, DesktopRevision};
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
