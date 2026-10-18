pub mod linux;
pub mod macos;
pub mod service;
pub mod space;
pub mod windows;

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use semver::Version;
use thiserror::Error;

use crate::package::digest::{PackageDigest, PackageDigestError};
use crate::update::manifest::UpdateTarget;

#[derive(Debug, Error)]
pub enum DesktopNativeUpdateError {
    #[error("The installer I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The installer command {program} failed with status {status:?}")]
    Command {
        program: &'static str,
        status: Option<i32>,
    },
    #[error("The destination path {0:?} is not safe")]
    UnsafePath(PathBuf),
    #[error("The staged package contains no application bundle")]
    MissingBundle,
    #[error("The package bytes do not match the committed digest")]
    DigestMismatch,
    #[error(transparent)]
    Package(#[from] PackageDigestError),
    #[error("{operation}; removing the staged copy also failed: {cleanup}")]
    Cleanup {
        operation: Box<DesktopNativeUpdateError>,
        cleanup: std::io::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeUpdateEnumOutcome {
    Installed,
    UnobservableCompletion,
}

pub struct DesktopNativeUpdateStaging;

impl DesktopNativeUpdateStaging {
    const BUFFER_BYTES: usize = 64 * 1024;

    pub fn copy_verified(
        source: &Path,
        expected: PackageDigest,
        target: &Path,
    ) -> Result<(), DesktopNativeUpdateError> {
        let mut input = fs::File::open(source)?;
        if !input.metadata()?.is_file() {
            return Err(DesktopNativeUpdateError::UnsafePath(source.to_path_buf()));
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(target)?;
        let outcome = Self::stream(&mut input, &mut output, expected);
        drop(output);
        if let Err(operation) = outcome {
            return match fs::remove_file(target) {
                Ok(()) => Err(operation),
                Err(cleanup) => Err(DesktopNativeUpdateError::Cleanup {
                    operation: Box::new(operation),
                    cleanup,
                }),
            };
        }
        Ok(())
    }

    fn stream(
        input: &mut fs::File,
        output: &mut fs::File,
        expected: PackageDigest,
    ) -> Result<(), DesktopNativeUpdateError> {
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; Self::BUFFER_BYTES];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            output.write_all(&buffer[..count])?;
        }
        let actual: [u8; 32] = hasher.finalize().into();
        if &actual != expected.bytes() {
            return Err(DesktopNativeUpdateError::DigestMismatch);
        }
        output.sync_all()?;
        Ok(())
    }
}

pub struct DesktopNativeUpdate;

impl DesktopNativeUpdate {
    pub fn install(
        target: UpdateTarget,
        package: &Path,
        destination: &Path,
        version: &Version,
        digest: PackageDigest,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        match target {
            UpdateTarget::WindowsX86_64 => {
                windows::DesktopNativeUpdateWindows::install(package, destination, digest)
            }
            UpdateTarget::MacosArm64 => {
                macos::DesktopNativeUpdateMacos::install(package, destination, digest)
            }
            UpdateTarget::LinuxAppImage => {
                linux::DesktopNativeUpdateLinux::install_appimage(package, destination, digest)
            }
            UpdateTarget::LinuxDeb => {
                linux::DesktopNativeUpdateLinux::install_deb(package, version, digest)
            }
        }
    }
}
