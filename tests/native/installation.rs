use std::path::{Path, PathBuf};

use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::manifest::EnginePackageEnumTarget;
use chess::engine::package::selector::EnginePackageSelector;
use chess::game::enums::{GameId, GameRevision};
use chess::game::pause::GamePauseEnumCause;
use chess::game::recovery::{GameRecovery, GameRecoveryEnumAction, GameRecoveryEnumTrigger};
use chess::game::save::GameSaveEnumStatus;
use chess::game::save_coordinator::{GameSaveCoordinator, GameSaveCoordinatorEnumBarrier};
use chess::desktop::native::resources::{
    DesktopNativeResources, DesktopNativeResourcesEnumDependency, DesktopNativeResourcesEnumFailure,
    DesktopNativeResourcesError,
};

pub struct NativeInstallationHarness;

impl NativeInstallationHarness {
    pub fn run(resources: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/platform-dependencies.json"))?;
        Self::check_fixture(&fixture)?;
        Self::failure_classes_match_the_fixture(&fixture)?;
        Self::installed_resources_verify(resources)?;
        Self::missing_optional_is_feature_local(resources)?;
        Self::missing_engine_manifest_is_a_required_failure(resources)?;
        Self::corrupt_pieces_are_a_required_failure(resources)?;
        Self::empty_installation_is_a_required_failure()?;
        Self::engine_failure_is_a_required_interruption(resources)?;
        Self::save_failure_keeps_unsaved_state_and_blocks_every_barrier()?;
        Self::sleep_pauses_games_without_an_automatic_resume()?;
        println!("installation scenarios passed on the main thread");
        Ok(())
    }

    fn check_fixture(fixture: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        let required = Self::paths(fixture, "required")?;
        for expected in DesktopNativeResources::REQUIRED {
            if !required.iter().any(|path| path == expected) {
                return Err(format!("the fixture does not list required {expected}").into());
            }
        }
        let optional = Self::paths(fixture, "optional")?;
        for expected in DesktopNativeResources::OPTIONAL {
            if !optional.iter().any(|path| path == expected) {
                return Err(format!("the fixture does not list optional {expected}").into());
            }
        }
        Ok(())
    }

    fn failure_classes_match_the_fixture(
        fixture: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let classes = fixture
            .get("dependencies")
            .and_then(serde_json::Value::as_array)
            .ok_or("the fixture lists dependency failure classes")?;
        for dependency in DesktopNativeResourcesEnumDependency::ALL {
            let entry = classes
                .iter()
                .find(|entry| {
                    entry.get("name").and_then(serde_json::Value::as_str) == Some(dependency.name())
                })
                .ok_or_else(|| format!("{} is missing from the fixture", dependency.name()))?;
            let expected = match entry.get("class").and_then(serde_json::Value::as_str) {
                Some("interruption") => DesktopNativeResourcesEnumFailure::Interruption,
                Some("feature-local") => DesktopNativeResourcesEnumFailure::FeatureLocal,
                Some("dataset-local") => DesktopNativeResourcesEnumFailure::DatasetLocal,
                other => return Err(format!("unknown failure class {other:?}").into()),
            };
            if dependency.failure() != expected {
                return Err(format!(
                    "{} is classified {:?}, the fixture expects {expected:?}",
                    dependency.name(),
                    dependency.failure()
                )
                .into());
            }
        }
        Ok(())
    }

    fn installed_resources_verify(resources: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let installed = DesktopNativeResources::from_directory(resources)?;
        let report = installed.inspect()?;
        println!(
            "installed resources verified; optional absent: {:?}",
            report.missing_optional()
        );
        Ok(())
    }

    fn missing_optional_is_feature_local(
        resources: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let staged = Self::stage(resources, true, true)?;
        let installed = DesktopNativeResources::from_directory(staged.path())?;
        let report = installed.inspect()?;
        for optional in DesktopNativeResources::OPTIONAL {
            if !report.missing_optional().contains(&optional) {
                return Err(format!("{optional} should be reported as unavailable").into());
            }
        }
        Ok(())
    }

    fn missing_engine_manifest_is_a_required_failure(
        resources: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let staged = Self::stage(resources, true, false)?;
        let installed = DesktopNativeResources::from_directory(staged.path())?;
        match installed.inspect() {
            Err(DesktopNativeResourcesError::MissingRequired(name))
                if name == DesktopNativeResources::ENGINE_MANIFEST =>
            {
                Ok(())
            }
            other => Err(format!("a missing engine manifest must be required: {other:?}").into()),
        }
    }

    fn corrupt_pieces_are_a_required_failure(
        resources: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let staged = Self::stage(resources, true, true)?;
        std::fs::write(staged.path().join(DesktopNativeResources::PIECES_ASSET), b"corrupt")?;
        match DesktopNativeResources::from_directory(staged.path()) {
            Err(DesktopNativeResourcesError::Integrity(_)) => Ok(()),
            other => Err(format!("corrupt pieces must fail integrity: {other:?}").into()),
        }
    }

    fn empty_installation_is_a_required_failure() -> Result<(), Box<dyn std::error::Error>> {
        let broken = tempfile::tempdir()?;
        match DesktopNativeResources::from_directory(broken.path()) {
            Err(
                DesktopNativeResourcesError::Io { .. } | DesktopNativeResourcesError::InvalidFile(_),
            ) => Ok(()),
            other => {
                Err(format!("an empty directory must be a required failure: {other:?}").into())
            }
        }
    }

    fn engine_failure_is_a_required_interruption(
        resources: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let target = EnginePackageEnumTarget::current()?;
        let (version, distribution) = match target {
            EnginePackageEnumTarget::MacosArm => ("14.0", ""),
            EnginePackageEnumTarget::WindowsX64 => ("11", ""),
            EnginePackageEnumTarget::LinuxX64 => ("22.04", "ubuntu"),
        };
        let host = EnginePackageHost::from_observation(target, version, distribution, true)?;
        let staged = tempfile::tempdir()?;
        std::fs::copy(
            resources.join(DesktopNativeResources::ENGINE_MANIFEST),
            staged.path().join("manifest.json"),
        )?;
        if EnginePackageSelector::load(staged.path(), host).is_ok() {
            return Err("an engine package without its binary must not load".into());
        }
        if DesktopNativeResourcesEnumDependency::Engine.failure()
            != DesktopNativeResourcesEnumFailure::Interruption
        {
            return Err("an engine failure must interrupt the application".into());
        }
        Ok(())
    }

    fn save_failure_keeps_unsaved_state_and_blocks_every_barrier(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let game: GameId = "00000000-0000-4000-8000-0000000000a1".parse()?;
        let revision = GameRevision::ZERO.next()?.next()?;
        let mut coordinator = GameSaveCoordinator::new(game, revision);
        coordinator.record_failure("disk full".to_owned());
        if coordinator.status() != GameSaveEnumStatus::Required
            || coordinator.memory_revision() != revision
        {
            return Err("a failed save must retain the unsaved memory state".into());
        }
        for barrier in [
            GameSaveCoordinatorEnumBarrier::CloseGame,
            GameSaveCoordinatorEnumBarrier::QuitApplication,
            GameSaveCoordinatorEnumBarrier::MoveToTrash,
        ] {
            if coordinator.require_saved(barrier).is_ok() {
                return Err(format!("{barrier:?} must wait for the save").into());
            }
        }
        coordinator.acknowledge(game, revision)?;
        coordinator.require_saved(GameSaveCoordinatorEnumBarrier::QuitApplication)?;
        Ok(())
    }

    fn sleep_pauses_games_without_an_automatic_resume() -> Result<(), Box<dyn std::error::Error>> {
        if GameRecovery::decide(GameRecoveryEnumTrigger::ComputerSleep)
            != GameRecoveryEnumAction::Pause(GamePauseEnumCause::ComputerSleep)
        {
            return Err("computer sleep must pause live games".into());
        }
        if GameRecovery::decide(GameRecoveryEnumTrigger::WakeFromSleep)
            != GameRecoveryEnumAction::KeepRunning
            || GameRecovery::resumes_automatically(GamePauseEnumCause::ComputerSleep)
        {
            return Err("waking must not resume a paused game".into());
        }
        Ok(())
    }

    fn stage(
        resources: &Path,
        pieces: bool,
        engine_manifest: bool,
    ) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut copies: Vec<&str> = Vec::new();
        if pieces {
            copies.push(DesktopNativeResources::PIECES_ASSET);
        }
        if engine_manifest {
            copies.push(DesktopNativeResources::ENGINE_MANIFEST);
        }
        for relative in copies {
            let destination: PathBuf = directory.path().join(relative);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(resources.join(relative), destination)?;
        }
        Ok(directory)
    }

    fn paths(
        fixture: &serde_json::Value,
        section: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        Ok(fixture
            .get(section)
            .and_then(serde_json::Value::as_array)
            .ok_or("the fixture section is missing")?
            .iter()
            .filter_map(|entry| entry.get("path").and_then(serde_json::Value::as_str))
            .map(str::to_owned)
            .collect())
    }
}
