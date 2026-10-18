use std::num::NonZeroU32;
use strum::IntoEnumIterator;

use chess::clock::duration::ClockDuration;
use chess::engine::capabilities::EngineIdentity;
use chess::engine::capabilities::persistence::EngineIdentityRecord;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::identity::EnginePackageIdentity;
use chess::engine::package::manifest::{
    EnginePackageEnumProfile, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::engine::search::limit::persistence::EngineSearchLimitRecord;
use chess::engine::search::limit::{EngineSearchClock, EngineSearchLimit};
use chess::package::digest::PackageDigest;

use crate::engine_report_persistence_codecs::{
    EngineReportPersistenceFixture, EngineReportPersistenceTestResult,
};

#[test]
fn historical_package_identity_roundtrips_without_an_executable()
-> EngineReportPersistenceTestResult {
    let identity = EnginePackageIdentity::new(
        EnginePackageEnumTarget::WindowsX64,
        EnginePackageEnumProfile::Portable,
        PackageDigest::hash_slice(b"historical unavailable package"),
    );
    let restored = EngineReportPersistenceFixture::roundtrip(&identity)?;
    assert_eq!(restored, identity);
    assert_eq!(restored.target(), EnginePackageEnumTarget::WindowsX64);
    assert_eq!(restored.profile(), EnginePackageEnumProfile::Portable);
    assert_eq!(restored.digest(), identity.digest());
    let json = serde_json::to_string(&identity)?;
    assert!(!json.contains("executable"));
    assert!(!json.contains("path"));
    for malformed in [
        json.replace("\"version\":1", "\"version\":2"),
        json.replace("\"data\":{", "\"data\":{\"executable\":\"/tmp/reckless\","),
        json.replace(&identity.digest().to_string(), "BAD"),
        json.replace("portable", "native"),
    ] {
        assert!(serde_json::from_str::<EnginePackageIdentity>(&malformed).is_err());
    }
    assert!(
        serde_json::from_str::<EnginePackageIdentity>(
            &" ".repeat(EnginePackageIdentity::MAX_BYTES + 1)
        )
        .is_err()
    );
    assert!(
        EnginePackageIdentity::parse(
            format!("{}{}", " ".repeat(EnginePackageIdentity::MAX_BYTES), json).as_bytes()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn identity_restores_the_full_registry_and_the_historical_threads_maximum()
-> EngineReportPersistenceTestResult {
    let identity = EngineIdentity::new(
        format!("Reckless {}", EnginePackageManifest::VERSION),
        EngineIdentity::AUTHOR.to_owned(),
        EngineReportPersistenceFixture::capabilities(2048)?,
    )?;
    let record = EngineIdentityRecord::capture(&identity)?;
    let restored = EngineReportPersistenceFixture::roundtrip(&record)?.restore()?;
    assert_eq!(restored.name(), identity.name());
    assert_eq!(restored.author(), identity.author());
    assert_eq!(
        restored.capabilities().version(),
        identity.capabilities().version()
    );
    assert!(restored.capabilities().options().next().is_some());
    for option in EngineEnumOption::iter() {
        assert_eq!(
            restored.capabilities().descriptor(option),
            identity.capabilities().descriptor(option)
        );
    }
    let json = serde_json::to_string(&record)?;
    assert!(
        EngineIdentityRecord::parse(
            format!("{}{}", " ".repeat(EngineIdentityRecord::MAX_BYTES), json).as_bytes()
        )
        .is_err()
    );
    for malformed in [
        json.replace("\"version\":1", "\"version\":2"),
        json.replace("\"registry_version\":1", "\"registry_version\":2"),
        json.replace("max 2048", "max 513"),
        json.replace("\"data\":{", "\"data\":{\"unknown\":true,"),
        json.replace("Reckless 0.9.0", "Unknown 0.9.0"),
    ] {
        assert!(serde_json::from_str::<EngineIdentityRecord>(&malformed).is_err());
    }
    Ok(())
}

#[test]
fn identity_rejects_duplicate_missing_and_excessive_descriptors()
-> EngineReportPersistenceTestResult {
    let identity = EngineIdentity::new(
        format!("Reckless {}", EnginePackageManifest::VERSION),
        EngineIdentity::AUTHOR.to_owned(),
        EngineReportPersistenceFixture::capabilities(1024)?,
    )?;
    let json = serde_json::to_string(&EngineIdentityRecord::capture(&identity)?)?;
    let descriptor = "option name Hash type spin default 16 min 1 max 262144";
    let encoded = serde_json::to_string(descriptor)?;
    assert!(
        serde_json::from_str::<EngineIdentityRecord>(&json.replace(&format!("{encoded},"), ""))
            .is_err()
    );
    assert!(
        serde_json::from_str::<EngineIdentityRecord>(
            &json.replace(&encoded, &format!("{encoded},{encoded}"))
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<EngineIdentityRecord>(&json.replace(descriptor, &"x".repeat(1024)))
            .is_err()
    );
    Ok(())
}

#[test]
fn limit_roundtrips_preserve_finite_infinite_and_clock_distinctions()
-> EngineReportPersistenceTestResult {
    let maximum = ClockDuration::from_millis(EngineSearchLimit::MAX_NUMBER)?;
    for limit in [
        EngineSearchLimit::infinite(),
        EngineSearchLimit::depth(1)?,
        EngineSearchLimit::depth(EngineSearchLimit::MAX_DEPTH)?,
        EngineSearchLimit::nodes(EngineSearchLimit::MAX_NUMBER)?,
        EngineSearchLimit::move_time(maximum)?,
        EngineSearchLimit::clock(EngineSearchClock::new(
            ClockDuration::ZERO,
            maximum,
            ClockDuration::from_millis(17)?,
            ClockDuration::ZERO,
            NonZeroU32::new(23),
        )?),
        EngineSearchLimit::clock(EngineSearchClock::new(
            maximum, maximum, maximum, maximum, None,
        )?),
    ] {
        let record = EngineSearchLimitRecord::capture(limit);
        let restored = EngineReportPersistenceFixture::roundtrip(&record)?.restore()?;
        assert_eq!(restored, limit);
        assert_eq!(restored.command(), limit.command());
    }
    for malformed in [
        r#"{"version":1,"data":{"kind":"depth","value":0}}"#,
        r#"{"version":1,"data":{"kind":"depth","value":128}}"#,
        r#"{"version":1,"data":{"kind":"nodes","value":0}}"#,
        r#"{"version":1,"data":{"kind":"nodes","value":2147483648}}"#,
        r#"{"version":1,"data":{"kind":"move_time","value":1.5}}"#,
        r#"{"version":2,"data":{"kind":"infinite"}}"#,
        r#"{"version":1,"data":{"kind":"infinite","extra":0}}"#,
        r#"{"version":1,"data":{"kind":"clock","value":{"white_time":1,"black_time":1,"white_increment":0,"black_increment":0,"moves_to_go":0}}}"#,
    ] {
        assert!(
            serde_json::from_str::<EngineSearchLimitRecord>(malformed).is_err(),
            "accepted {malformed}"
        );
    }
    let clock = r#"{"version":1,"data":{"kind":"clock","value":{"white_time":1,"black_time":2,"white_increment":3,"black_increment":4,"moves_to_go":5}}}"#;
    for field in [
        "\"white_time\":1",
        "\"black_time\":2",
        "\"white_increment\":3",
        "\"black_increment\":4",
        "\"moves_to_go\":5",
    ] {
        let (name, _value) = field
            .split_once(':')
            .ok_or("missing clock field separator")?;
        assert!(
            serde_json::from_str::<EngineSearchLimitRecord>(
                &clock.replace(field, &format!("{name}:2147483648"))
            )
            .is_err()
        );
    }
    assert!(
        EngineSearchLimitRecord::parse(&vec![b' '; EngineSearchLimitRecord::MAX_BYTES + 1])
            .is_err()
    );
    Ok(())
}
