use std::cell::RefCell;
use std::num::NonZeroU32;

use semver::Version;

use chess::desktop::identity::DesktopRequestId;
use chess::desktop::profile::installation::{
    DesktopProfileInstallationEnumOutcome, DesktopProfileInstallationEnumState,
    DesktopProfileInstallationError, DesktopProfileInstallationSpec,
};
use chess::update::install::{
    UpdateHelperEnumReadiness, UpdateInstall, UpdateInstallConfirmation, UpdateInstallEnvironment,
    UpdateInstallError, UpdateInstallRequest, UpdateIntentWriter,
};
use chess::update::manifest::{UpdateManifest, UpdateTarget};
use chess::update::verifier::UpdateVerifier;

use crate::update_fixture::{MACOS_PACKAGE, MANIFEST, PUBLIC_KEY, SIGNATURE};

#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateFixtureHelper {
    Ready,
    WrongGeneration,
    Lost,
}

struct UpdateFixtureEnvironment<'a> {
    calls: &'a RefCell<Vec<&'static str>>,
    helper: UpdateFixtureHelper,
    fail_save: bool,
    stopped: bool,
}

impl UpdateInstallEnvironment for UpdateFixtureEnvironment<'_> {
    fn resolve_unsaved_edits(&mut self) -> Result<(), String> {
        self.calls.borrow_mut().push("edits");
        Ok(())
    }

    fn pause_and_save_games(&mut self) -> Result<(), String> {
        self.calls.borrow_mut().push("save");
        if self.fail_save {
            Err("a game could not be saved".to_owned())
        } else {
            Ok(())
        }
    }

    fn safety_backup(&mut self) -> Result<DesktopRequestId, String> {
        self.calls.borrow_mut().push("backup");
        Ok(DesktopRequestId::new())
    }

    fn start_helper(&mut self, spec: &DesktopProfileInstallationSpec) -> UpdateHelperEnumReadiness {
        self.calls.borrow_mut().push("helper");
        match self.helper {
            UpdateFixtureHelper::Ready => UpdateHelperEnumReadiness::Ready(spec.helper_generation),
            UpdateFixtureHelper::WrongGeneration => {
                UpdateHelperEnumReadiness::Ready(DesktopRequestId::new())
            }
            UpdateFixtureHelper::Lost => UpdateHelperEnumReadiness::Lost,
        }
    }

    fn confirm_helper_stopped(&mut self, _spec: &DesktopProfileInstallationSpec) -> bool {
        self.stopped
    }
}

#[derive(Default)]
struct UpdateFixtureWriter {
    states: RefCell<Vec<DesktopProfileInstallationEnumState>>,
}

impl UpdateIntentWriter for UpdateFixtureWriter {
    fn write(
        &self,
        _spec: &DesktopProfileInstallationSpec,
        state: DesktopProfileInstallationEnumState,
    ) -> Result<(), DesktopProfileInstallationError> {
        self.states.borrow_mut().push(state);
        Ok(())
    }
}

fn request_parts() -> Result<
    (
        chess::update::manifest::UpdateManifestAsset,
        tempfile::TempDir,
    ),
    Box<dyn std::error::Error>,
> {
    let manifest =
        UpdateManifest::verify_and_parse(&UpdateVerifier::new(PUBLIC_KEY)?, MANIFEST, SIGNATURE)?;
    let asset = manifest
        .asset_for(UpdateTarget::MacosArm64)
        .ok_or("asset")?
        .clone();
    Ok((asset, tempfile::tempdir()?))
}

type UpdateInstallRun = (
    Result<DesktopProfileInstallationSpec, UpdateInstallError>,
    Vec<&'static str>,
    Vec<DesktopProfileInstallationEnumState>,
);

fn run(
    helper: UpdateFixtureHelper,
    fail_save: bool,
    bytes: &[u8],
) -> Result<UpdateInstallRun, Box<dyn std::error::Error>> {
    let (asset, directory) = request_parts()?;
    let package = directory.path().join("package.zip");
    std::fs::write(&package, bytes)?;
    let calls = RefCell::new(Vec::new());
    let mut environment = UpdateFixtureEnvironment {
        calls: &calls,
        helper,
        fail_save,
        stopped: true,
    };
    let writer = UpdateFixtureWriter::default();
    let outcome = UpdateInstall::run(
        UpdateInstallConfirmation::explicit(),
        &UpdateInstallRequest {
            asset: &asset,
            package: &package,
            version: Version::new(1, 2, 0),
            data_schema: NonZeroU32::MIN,
        },
        &mut environment,
        &writer,
    );
    let recorded = calls.borrow().clone();
    let states = writer.states.borrow().clone();
    Ok((outcome, recorded, states))
}

#[test]
fn prerequisites_run_in_order_and_commit_follows_the_prepared_intent()
-> Result<(), Box<dyn std::error::Error>> {
    let (outcome, calls, states) = run(UpdateFixtureHelper::Ready, false, MACOS_PACKAGE)?;
    assert!(outcome.is_ok());
    assert_eq!(calls, ["edits", "save", "backup", "helper"]);
    assert_eq!(
        states,
        [
            DesktopProfileInstallationEnumState::Prepared,
            DesktopProfileInstallationEnumState::Committed
        ]
    );
    Ok(())
}

#[test]
fn a_failed_save_or_tampered_package_writes_no_intent() -> Result<(), Box<dyn std::error::Error>> {
    let (outcome, calls, states) = run(UpdateFixtureHelper::Ready, true, MACOS_PACKAGE)?;
    assert!(matches!(outcome, Err(UpdateInstallError::Prerequisite(_))));
    assert_eq!(calls, ["edits", "save"]);
    assert!(states.is_empty());
    let (tampered, _calls, states) = run(UpdateFixtureHelper::Ready, false, b"hello w0rld")?;
    assert!(matches!(tampered, Err(UpdateInstallError::Package(_))));
    assert!(states.is_empty());
    Ok(())
}

#[test]
fn a_lost_or_mismatched_helper_acknowledgment_never_commits()
-> Result<(), Box<dyn std::error::Error>> {
    for helper in [
        UpdateFixtureHelper::Lost,
        UpdateFixtureHelper::WrongGeneration,
    ] {
        let (outcome, _calls, states) = run(helper, false, MACOS_PACKAGE)?;
        assert!(matches!(outcome, Err(UpdateInstallError::HelperNotReady)));
        assert_eq!(states, [DesktopProfileInstallationEnumState::Prepared]);
    }
    Ok(())
}

#[test]
fn precommit_cancel_needs_confirmed_helper_quiescence() -> Result<(), Box<dyn std::error::Error>> {
    let (outcome, _calls, _states) = run(UpdateFixtureHelper::Ready, false, MACOS_PACKAGE)?;
    let spec = outcome?;
    let calls = RefCell::new(Vec::new());
    let writer = UpdateFixtureWriter::default();
    let mut running = UpdateFixtureEnvironment {
        calls: &calls,
        helper: UpdateFixtureHelper::Ready,
        fail_save: false,
        stopped: false,
    };
    assert!(matches!(
        UpdateInstall::cancel_precommit(&spec, &mut running, &writer),
        Err(UpdateInstallError::HelperStillRunning)
    ));
    assert!(writer.states.borrow().is_empty());
    let mut stopped = UpdateFixtureEnvironment {
        calls: &calls,
        helper: UpdateFixtureHelper::Ready,
        fail_save: false,
        stopped: true,
    };
    UpdateInstall::cancel_precommit(&spec, &mut stopped, &writer)?;
    assert_eq!(
        writer.states.borrow().as_slice(),
        [DesktopProfileInstallationEnumState::Outcome {
            result: DesktopProfileInstallationEnumOutcome::Failed
        }]
    );
    Ok(())
}
