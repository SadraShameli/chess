use std::str::FromStr;

use chess::desktop::identity::{DesktopRequestId, DesktopRevision};

#[test]
fn round_trips_canonical_unsigned_counter_across_the_full_native_range()
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
fn identifiers_keep_canonical_external_encodings() -> Result<(), Box<dyn std::error::Error>> {
    let canonical = "123e4567-e89b-42d3-a456-426614174000";
    let id = DesktopRequestId::from_str(canonical)?;
    let encoded = serde_json::to_string(&id)?;
    assert_eq!(encoded, format!("\"{canonical}\""));
    assert_eq!(serde_json::from_str::<DesktopRequestId>(&encoded)?, id);
    for invalid in [
        "123E4567-E89B-42D3-A456-426614174000",
        "123e4567e89b42d3a456426614174000",
        "00000000-0000-0000-0000-000000000000",
        "not-an-id",
    ] {
        assert!(DesktopRequestId::from_str(invalid).is_err(), "{invalid}");
        assert!(
            serde_json::from_str::<DesktopRequestId>(&serde_json::to_string(invalid)?).is_err(),
            "{invalid}"
        );
    }
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
