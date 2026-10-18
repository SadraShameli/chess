use std::fs;

use chess::desktop::profile::installation::{
    DesktopProfileInstallationEnumOutcome, DesktopProfileInstallationEnumStartup,
    DesktopProfileInstallationEnumState, DesktopProfileInstallationIntent,
};

use crate::desktop_owner_record::DesktopProfileOwnerRecordFixture;

struct DesktopProfileInstallationFixture;

impl DesktopProfileInstallationFixture {
    const LOCAL_PROFILE: &'static str = "d92f840f-e575-4654-92bd-79b3ba8b8e55";
    const FOREIGN_PROFILE: &'static str = "b726b8e1-813c-4190-8be1-b17c07b0aa16";

    fn bytes(profile: &str, state: DesktopProfileInstallationEnumState) -> Vec<u8> {
        let state = match state {
            DesktopProfileInstallationEnumState::Prepared => r#"{"phase":"prepared"}"#,
            DesktopProfileInstallationEnumState::Committed => r#"{"phase":"committed"}"#,
            DesktopProfileInstallationEnumState::Outcome { result } => match result {
                DesktopProfileInstallationEnumOutcome::Succeeded => {
                    r#"{"phase":"outcome","result":"succeeded"}"#
                }
                DesktopProfileInstallationEnumOutcome::Failed => {
                    r#"{"phase":"outcome","result":"failed"}"#
                }
                DesktopProfileInstallationEnumOutcome::Unknown => {
                    r#"{"phase":"outcome","result":"unknown"}"#
                }
            },
        };
        format!(
            r#"{{"version":1,"record":{{"profile_id":"{profile}","owner_generation":"191b8766-233c-4a15-820a-65f3e042b730","helper_generation":"34fa257f-deec-4988-a5e8-3af16487d77d","package_digest":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","package_version":"1.2.3","safety_backup_id":"4420fb16-b3ed-491e-ab96-80b34890b62e","data_schema":1,"state":{state}}}}}"#
        )
        .into_bytes()
    }
}

#[test]
fn only_an_absent_intent_permits_normal_initialization() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    assert!(matches!(
        DesktopProfileInstallationIntent::inspect(&fixture.lease)?,
        DesktopProfileInstallationEnumStartup::Normal
    ));
    assert!(!fixture.record_path("installation.json").exists());
    Ok(())
}

#[test]
fn prepared_committed_and_successful_intents_all_preserve_the_startup_barrier()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    let cases = [
        DesktopProfileInstallationEnumState::Prepared,
        DesktopProfileInstallationEnumState::Committed,
        DesktopProfileInstallationEnumState::Outcome {
            result: DesktopProfileInstallationEnumOutcome::Succeeded,
        },
        DesktopProfileInstallationEnumState::Outcome {
            result: DesktopProfileInstallationEnumOutcome::Failed,
        },
        DesktopProfileInstallationEnumState::Outcome {
            result: DesktopProfileInstallationEnumOutcome::Unknown,
        },
    ];
    for expected in cases {
        let bytes = DesktopProfileInstallationFixture::bytes(
            DesktopProfileInstallationFixture::LOCAL_PROFILE,
            expected,
        );
        fixture.write_private_record("installation.json", &bytes)?;
        let DesktopProfileInstallationEnumStartup::Blocked(intent) =
            DesktopProfileInstallationIntent::inspect(&fixture.lease)?
        else {
            return Err(std::io::Error::other("Existing intent must block normal startup").into());
        };
        assert_eq!(intent.state(), expected);
        assert_eq!(intent.profile_id(), fixture.lease.identity().profile_id());
        assert_eq!(intent.package_version().to_string(), "1.2.3");
        assert_eq!(intent.data_schema().get(), 1);
        assert_ne!(intent.owner_generation(), intent.helper_generation());
        assert_eq!(fs::read(fixture.record_path("installation.json"))?, bytes);
    }
    Ok(())
}

#[test]
fn malformed_future_oversized_and_foreign_intents_fail_without_clearing_the_barrier()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    let foreign = DesktopProfileInstallationFixture::bytes(
        DesktopProfileInstallationFixture::FOREIGN_PROFILE,
        DesktopProfileInstallationEnumState::Committed,
    );
    let cases: [&[u8]; 5] = [
        b"",
        br#"{"version":1,"record":{"phase":"prepared"}}"#,
        br#"{"version":2,"record":{}}"#,
        &[b'x'; 8193],
        &foreign,
    ];
    for bytes in cases {
        fixture.write_private_record("installation.json", bytes)?;
        assert!(DesktopProfileInstallationIntent::inspect(&fixture.lease).is_err());
        assert_eq!(fs::read(fixture.record_path("installation.json"))?, bytes);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn installation_inspection_refuses_a_symlink_without_following_or_removing_it()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    let committed = DesktopProfileInstallationFixture::bytes(
        DesktopProfileInstallationFixture::LOCAL_PROFILE,
        DesktopProfileInstallationEnumState::Committed,
    );
    let outside = fixture
        .lease
        .identity()
        .data_directory()
        .join("outside-intent");
    fs::write(&outside, &committed)?;
    symlink(&outside, fixture.record_path("installation.json"))?;
    assert!(DesktopProfileInstallationIntent::inspect(&fixture.lease).is_err());
    assert_eq!(fs::read(outside)?, committed);
    assert!(fs::symlink_metadata(fixture.record_path("installation.json"))?.is_symlink());
    Ok(())
}
