use semver::Version;

use chess::desktop::profile::installation::{
    DesktopProfileInstallationEnumOutcome, DesktopProfileInstallationEnumState,
};
use chess::update::recovery::{
    UpdateRecovery, UpdateRecoveryEnumBlock, UpdateRecoveryEnumDecision, UpdateRecoveryIntent,
    UpdateRecoveryObservation,
};

fn intent(state: DesktopProfileInstallationEnumState) -> UpdateRecoveryIntent {
    UpdateRecoveryIntent::new(state, Version::new(1, 2, 0))
}

fn observation(
    installed: Version,
    quiescent: bool,
    migration: Option<bool>,
) -> UpdateRecoveryObservation {
    UpdateRecoveryObservation {
        installed_version: installed,
        helper_quiescent: quiescent,
        migration_valid: migration,
    }
}

#[test]
fn no_intent_permits_ordinary_startup() {
    assert_eq!(
        UpdateRecovery::decide(None, &observation(Version::new(1, 1, 0), true, None)),
        UpdateRecoveryEnumDecision::Proceed
    );
}

#[test]
fn prepared_never_authorizes_anything_and_a_running_helper_blocks_retry() {
    let prepared = intent(DesktopProfileInstallationEnumState::Prepared);
    let running = observation(Version::new(1, 1, 0), false, None);
    assert_eq!(
        UpdateRecovery::decide(Some(&prepared), &running),
        UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::HelperRunning)
    );
    assert!(!UpdateRecovery::retry_allowed(&running));
    let stopped = observation(Version::new(1, 1, 0), true, None);
    assert_eq!(
        UpdateRecovery::decide(Some(&prepared), &stopped),
        UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::PreparedUncommitted)
    );
    assert!(UpdateRecovery::retry_allowed(&stopped));
}

#[test]
fn committed_clears_only_after_identity_and_migration_validate() {
    let committed = intent(DesktopProfileInstallationEnumState::Committed);
    assert_eq!(
        UpdateRecovery::decide(
            Some(&committed),
            &observation(Version::new(1, 2, 0), true, Some(true))
        ),
        UpdateRecoveryEnumDecision::ClearSuccess
    );
    assert_eq!(
        UpdateRecovery::decide(
            Some(&committed),
            &observation(Version::new(1, 2, 0), true, Some(false))
        ),
        UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::MigrationFailed)
    );
    assert_eq!(
        UpdateRecovery::decide(
            Some(&committed),
            &observation(Version::new(1, 2, 0), true, None)
        ),
        UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::CommittedPending)
    );
    assert_eq!(
        UpdateRecovery::decide(
            Some(&committed),
            &observation(Version::new(1, 1, 0), false, None)
        ),
        UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::HelperRunning)
    );
    assert_eq!(
        UpdateRecovery::decide(
            Some(&committed),
            &observation(Version::new(1, 1, 0), true, None)
        ),
        UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::CommittedPending)
    );
}

#[test]
fn ambiguous_and_failed_outcomes_keep_the_barrier() {
    let unknown = intent(DesktopProfileInstallationEnumState::Outcome {
        result: DesktopProfileInstallationEnumOutcome::Unknown,
    });
    assert_eq!(
        UpdateRecovery::decide(
            Some(&unknown),
            &observation(Version::new(1, 2, 0), true, Some(true))
        ),
        UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::UnknownOutcome)
    );
    let failed = intent(DesktopProfileInstallationEnumState::Outcome {
        result: DesktopProfileInstallationEnumOutcome::Failed,
    });
    assert_eq!(
        UpdateRecovery::decide(
            Some(&failed),
            &observation(Version::new(1, 1, 0), true, None)
        ),
        UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::FailedOutcome)
    );
    let succeeded = intent(DesktopProfileInstallationEnumState::Outcome {
        result: DesktopProfileInstallationEnumOutcome::Succeeded,
    });
    assert_eq!(
        UpdateRecovery::decide(
            Some(&succeeded),
            &observation(Version::new(1, 2, 0), true, Some(true))
        ),
        UpdateRecoveryEnumDecision::ClearSuccess
    );
}

#[test]
fn installer_adapters_refuse_unsafe_destinations() -> Result<(), Box<dyn std::error::Error>> {
    use chess::desktop::native::update::linux::DesktopNativeUpdateLinux;
    use chess::desktop::native::update::windows::DesktopNativeUpdateWindows;

    assert!(
        DesktopNativeUpdateWindows::validate_destination(std::path::Path::new("relative")).is_err()
    );
    assert!(
        DesktopNativeUpdateWindows::validate_destination(std::path::Path::new("/a/../b")).is_err()
    );
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let real = root.join("Chess.AppImage");
    std::fs::write(&real, b"image")?;
    assert!(DesktopNativeUpdateLinux::validate_appimage(&real).is_ok());
    let wrong = root.join("Chess.bin");
    std::fs::write(&wrong, b"image")?;
    assert!(DesktopNativeUpdateLinux::validate_appimage(&wrong).is_err());
    #[cfg(unix)]
    {
        let link = root.join("Link.AppImage");
        std::os::unix::fs::symlink(&real, &link)?;
        assert!(DesktopNativeUpdateLinux::validate_appimage(&link).is_err());
    }
    Ok(())
}
