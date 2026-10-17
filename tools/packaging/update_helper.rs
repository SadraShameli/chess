use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::str::FromStr;
use std::time::Duration;

use chess::desktop::native::update::linux::{
    DesktopNativeUpdateLinux, DesktopNativeUpdateLinuxDebPlan,
};
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
    #[error("Usage: chess-update-helper --data-dir <path> --package <path> --target <target> --version <version> --destination <path> --parent-pid <pid> --relaunch <path> --generation <id>")]
    Usage,
    #[error("The helper argument {0} is invalid")]
    Argument(&'static str),
    #[error("The committed intent does not match this handoff")]
    Intent,
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
    generation: String,
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
            generation: take("--generation")?,
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
    {
        use std::io::Write;
        let mut output = std::io::stdout().lock();
        writeln!(output, "ready {}", arguments.generation)
            .and_then(|()| output.flush())
            .map_err(|error| UpdateHelperError::Other(error.to_string()))?;
    }
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
    let spec = intent.spec();
    let result = DesktopNativeUpdate::install(
        arguments.target,
        &arguments.package,
        &arguments.destination,
        &arguments.version,
        intent.package_digest(),
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

struct UpdateHelperPrivileged;

impl UpdateHelperPrivileged {
    fn run(arguments: Vec<String>) -> Result<(), UpdateHelperError> {
        let mut values = std::collections::HashMap::new();
        let mut iterator = arguments.into_iter();
        while let Some(flag) = iterator.next() {
            let value = iterator.next().ok_or(UpdateHelperError::Usage)?;
            if values.insert(flag, value).is_some() {
                return Err(UpdateHelperError::Usage);
            }
        }
        let package = values
            .remove(DesktopNativeUpdateLinuxDebPlan::PACKAGE_FLAG)
            .ok_or(UpdateHelperError::Argument("--package"))?;
        let digest = values
            .remove(DesktopNativeUpdateLinuxDebPlan::DIGEST_FLAG)
            .ok_or(UpdateHelperError::Argument("--sha256"))?;
        if !values.is_empty() {
            return Err(UpdateHelperError::Usage);
        }
        let digest = PackageDigest::from_str(&digest)
            .map_err(|_error| UpdateHelperError::Argument("--sha256"))?;
        DesktopNativeUpdateLinux::privileged_install_deb(&PathBuf::from(package), digest)
            .map_err(|error| UpdateHelperError::Other(error.to_string()))
    }
}

fn main() -> ExitCode {
    let mut command_line: Vec<String> = std::env::args().skip(1).collect();
    if command_line.first().map(String::as_str) == Some(DesktopNativeUpdateLinuxDebPlan::ACTION) {
        command_line.remove(0);
        return match UpdateHelperPrivileged::run(command_line) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(1)
            }
        };
    }
    let arguments = match UpdateHelperArguments::parse(command_line) {
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
