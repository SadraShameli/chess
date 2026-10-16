use std::ffi::OsString;

use chess::desktop::native::bootstrap::{DesktopNativeBootstrap, DesktopNativeBootstrapError};
use tempfile::TempDir;

#[test]
fn native_help_exits_before_creating_the_requested_profile_or_a_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let profile = directory.path().join("unused profile");
    assert!(tokio::runtime::Handle::try_current().is_err());
    DesktopNativeBootstrap::run_with_arguments([
        OsString::from("--profile-directory"),
        profile.as_os_str().to_os_string(),
        OsString::from("--help"),
    ])?;
    assert!(!profile.exists());
    assert!(tokio::runtime::Handle::try_current().is_err());
    Ok(())
}

#[test]
fn native_cli_rejects_unknown_arguments_before_starting_services() {
    for argument in ["--unknown", "", "position.fen"] {
        assert!(matches!(
            DesktopNativeBootstrap::run_with_arguments([OsString::from(argument)]),
            Err(DesktopNativeBootstrapError::Argument(value)) if value == argument
        ));
        assert!(tokio::runtime::Handle::try_current().is_err());
    }
}

#[test]
fn native_cli_rejects_missing_empty_or_duplicate_profile_paths() {
    let cases = [
        vec![OsString::from("--profile-directory")],
        vec![OsString::from("--profile-directory"), OsString::new()],
        vec![
            OsString::from("--profile-directory"),
            OsString::from("--help"),
        ],
        vec![
            OsString::from("--profile-directory"),
            OsString::from("first"),
            OsString::from("--profile-directory"),
            OsString::from("second"),
        ],
    ];
    for arguments in cases {
        assert!(matches!(
            DesktopNativeBootstrap::run_with_arguments(arguments),
            Err(DesktopNativeBootstrapError::ProfileArgument)
        ));
        assert!(tokio::runtime::Handle::try_current().is_err());
    }
}
