use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::str::FromStr;
use std::time::Duration;

use chess::desktop::native::update::{DesktopNativeUpdate, DesktopNativeUpdateEnumOutcome};
use chess::desktop::profile::installation::{
    DesktopProfileInstallationEnumOutcome, DesktopProfileInstallationEnumState,
    DesktopProfileInstallationIntent,
};
use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileId, DesktopProfileOsLock,
    DesktopProfileSecurity,
};
use chess::package::digest::PackageDigest;
use chess::update::manifest::UpdateTarget;
use semver::Version;
use thiserror::Error;

#[derive(Debug, Error)]
enum UpdateHelperError {
    #[error("Usage: chess-update-helper --data-dir <path> --package <path> --target <target> --version <version> --destination <path> --parent-pid <pid> --relaunch <path>")]
    Usage,
    #[error("The helper argument {0} is invalid")]
    Argument(&'static str),
    #[error("The committed intent does not match this handoff")]
    Intent,
    #[error("The package bytes no longer match the committed digest")]
    Package,
    #[error("The profile is owned by another process")]
    Busy,
    #[error("{0}")]
    Other(String),
}

struct UpdateHelperArguments {
    data_dir: PathBuf,
    package: PathBuf,
    target: UpdateTarget,
    version: Version,
    destination: PathBuf,
    parent_pid: u32,
    relaunch: PathBuf,
}

impl UpdateHelperArguments {
    fn parse(arguments: Vec<String>) -> Result<Self, UpdateHelperError> {
        let mut values = std::collections::HashMap::new();
        let mut iterator = arguments.into_iter();
        while let Some(flag) = iterator.next() {
            let value = iterator.next().ok_or(UpdateHelperError::Usage)?;
            if values.insert(flag, value).is_some() {
                return Err(UpdateHelperError::Usage);
            }
        }
        let mut take = |flag: &'static str| {
            values
                .remove(flag)
                .ok_or(UpdateHelperError::Argument(flag))
        };
        let parsed = Self {
            data_dir: PathBuf::from(take("--data-dir")?),
            package: PathBuf::from(take("--package")?),
            target: UpdateTarget::from_str(&take("--target")?)
                .map_err(|_error| UpdateHelperError::Argument("--target"))?,
            version: Version::parse(&take("--version")?)
                .map_err(|_error| UpdateHelperError::Argument("--version"))?,
            destination: PathBuf::from(take("--destination")?),
            parent_pid: take("--parent-pid")?
                .parse()
                .map_err(|_error| UpdateHelperError::Argument("--parent-pid"))?,
            relaunch: PathBuf::from(take("--relaunch")?),
        };
        if !values.is_empty() {
            return Err(UpdateHelperError::Usage);
        }
        Ok(parsed)
    }
}

fn wait_for_parent_exit(pid: u32) {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    loop {
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        if system.process(pid).is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn run(arguments: UpdateHelperArguments) -> Result<(), UpdateHelperError> {
    wait_for_parent_exit(arguments.parent_pid);
    let security = DesktopProfileSecurity::current()
        .map_err(|error| UpdateHelperError::Other(error.to_string()))?;
    let admission = DesktopProfileAdmission::new(security, DesktopProfileOsLock);
    let proposed = DesktopProfileId::from_uuid(uuid::Uuid::new_v4())
        .map_err(|error| UpdateHelperError::Other(error.to_string()))?;
    let lease = loop {
        match admission
            .acquire(&arguments.data_dir, proposed)
            .map_err(|error| UpdateHelperError::Other(error.to_string()))?
        {
            DesktopProfileEnumAdmission::Owned(lease) => break lease,
            DesktopProfileEnumAdmission::Busy => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    let intent = DesktopProfileInstallationIntent::read_current(&lease)
        .map_err(|error| UpdateHelperError::Other(error.to_string()))?
        .ok_or(UpdateHelperError::Intent)?;
    if intent.state() != DesktopProfileInstallationEnumState::Committed
        || *intent.package_version() != arguments.version
    {
        return Err(UpdateHelperError::Intent);
    }
    let file = std::fs::File::open(&arguments.package)
        .map_err(|error| UpdateHelperError::Other(error.to_string()))?;
    let digest = PackageDigest::read(file).map_err(|_error| UpdateHelperError::Package)?;
    if digest != intent.package_digest() {
        return Err(UpdateHelperError::Package);
    }
    let spec = intent.spec();
    let result = DesktopNativeUpdate::install(
        arguments.target,
        &arguments.package,
        &arguments.destination,
        &arguments.version,
    );
    let (outcome, relaunch) = match result {
        Ok(DesktopNativeUpdateEnumOutcome::Installed) => {
            (DesktopProfileInstallationEnumOutcome::Succeeded, true)
        }
        Ok(DesktopNativeUpdateEnumOutcome::UnobservableCompletion) => {
            (DesktopProfileInstallationEnumOutcome::Unknown, false)
        }
        Err(_error) => (DesktopProfileInstallationEnumOutcome::Failed, false),
    };
    DesktopProfileInstallationIntent::write(
        &lease,
        &spec,
        DesktopProfileInstallationEnumState::Outcome { result: outcome },
    )
    .map_err(|error| UpdateHelperError::Other(error.to_string()))?;
    drop(lease);
    if relaunch {
        Command::new(&arguments.relaunch)
            .spawn()
            .map_err(|error| UpdateHelperError::Other(error.to_string()))?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let arguments = match UpdateHelperArguments::parse(std::env::args().skip(1).collect()) {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match run(arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
