use semver::Version;

use chess::presentation::settings::update::{
    PresentationSettingsUpdate, PresentationSettingsUpdateEnumAction,
};
use chess::presentation::settings::update_install::PresentationSettingsUpdateInstall;
use chess::update::credentials::UpdateCredentialEnumStatus;
use chess::update::manifest::UpdateTarget;
use chess::update::network_cost::UpdateNetworkEnumCost;
use chess::update::recovery::UpdateRecoveryEnumBlock;
use chess::update::state::{UpdateReleaseIdentity, UpdateState};

type Action = PresentationSettingsUpdateEnumAction;

fn available() -> Result<UpdateState, Box<dyn std::error::Error>> {
    let mut state = UpdateState::default();
    state.begin_check()?;
    state.check_found(
        UpdateReleaseIdentity {
            release_id: 1,
            asset_id: 2,
            version: Version::new(1, 2, 0),
            target: UpdateTarget::MacosArm64,
            asset_name: "chess.zip".to_owned(),
        },
        [0; 32],
    )?;
    Ok(state)
}

#[test]
fn missing_credentials_offer_entry_or_session_use_without_blocking_the_app() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = UpdateState::default();
    state.begin_check()?;
    state.credentials_missing()?;
    let actions = PresentationSettingsUpdate::actions(&state, UpdateCredentialEnumStatus::Missing);
    assert_eq!(actions, [Action::EnterCredential, Action::UseSessionCredential]);
    assert!(
        PresentationSettingsUpdate::status(&state, UpdateCredentialEnumStatus::Missing)
            .contains("Offline chess is unaffected")
    );
    Ok(())
}

#[test]
fn no_action_ever_removes_a_download_or_retries_in_the_background() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = available()?;
    state.request_download(UpdateNetworkEnumCost::Metered)?;
    let waiting = PresentationSettingsUpdate::actions(&state, UpdateCredentialEnumStatus::Persistent);
    assert_eq!(waiting, [Action::Approve, Action::Cancel]);
    state.cancel()?;
    let cancelled = PresentationSettingsUpdate::actions(&state, UpdateCredentialEnumStatus::Persistent);
    assert_eq!(cancelled, [Action::Retry]);
    for action in waiting.into_iter().chain(cancelled) {
        assert!(!action.label().to_lowercase().contains("remove"));
    }
    Ok(())
}

#[test]
fn install_requires_a_downloaded_package_and_explains_recovery() -> Result<(), Box<dyn std::error::Error>> {
    let state = available()?;
    assert_eq!(
        PresentationSettingsUpdate::actions(&state, UpdateCredentialEnumStatus::Persistent),
        [Action::Download]
    );
    assert!(PresentationSettingsUpdateInstall::CONFIRMATION.contains("paused"));
    assert!(
        PresentationSettingsUpdateInstall::recovery_guidance(UpdateRecoveryEnumBlock::MigrationFailed)
            .contains("safety backup")
    );
    Ok(())
}
