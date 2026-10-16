use thiserror::Error;

use crate::engine::package::manifest::{EnginePackageEnumTarget, EnginePackageManifestError};

#[derive(Debug, Error)]
pub enum EnginePackageHostError {
    #[error(transparent)]
    Target(#[from] EnginePackageManifestError),
    #[error("Cannot determine the operating system version")]
    UnknownVersion,
    #[error("This engine requires macOS 14+, Windows 11+, or Ubuntu 22.04+")]
    UnsupportedVersion,
    #[error("The processor does not support this package's baseline instructions")]
    UnsupportedCpu,
}

#[derive(Debug, Clone, Copy)]
pub struct EnginePackageHost {
    target: EnginePackageEnumTarget,
}

impl EnginePackageHost {
    pub fn detect() -> Result<Self, EnginePackageHostError> {
        let target = EnginePackageEnumTarget::current()?;
        let version =
            sysinfo::System::os_version().ok_or(EnginePackageHostError::UnknownVersion)?;
        Self::from_observation(
            target,
            &version,
            &sysinfo::System::distribution_id(),
            Self::baseline_cpu(),
        )
    }

    pub fn from_observation(
        target: EnginePackageEnumTarget,
        version: &str,
        distribution: &str,
        baseline_cpu: bool,
    ) -> Result<Self, EnginePackageHostError> {
        if !baseline_cpu {
            return Err(EnginePackageHostError::UnsupportedCpu);
        }
        let mut parts = version.split(['.', ' ']);
        let major = parts
            .next()
            .ok_or(EnginePackageHostError::UnknownVersion)?
            .parse::<u16>()
            .map_err(|_error| EnginePackageHostError::UnknownVersion)?;
        let supported = match target {
            EnginePackageEnumTarget::MacosArm => major >= 14,
            EnginePackageEnumTarget::WindowsX64 => major >= 11,
            EnginePackageEnumTarget::LinuxX64 => {
                let minor = parts
                    .next()
                    .ok_or(EnginePackageHostError::UnknownVersion)?
                    .parse::<u16>()
                    .map_err(|_error| EnginePackageHostError::UnknownVersion)?;
                distribution == "ubuntu" && (major, minor) >= (22, 4)
            }
        };
        if !supported {
            return Err(EnginePackageHostError::UnsupportedVersion);
        }
        Ok(Self { target })
    }

    pub fn target(self) -> EnginePackageEnumTarget {
        self.target
    }

    fn baseline_cpu() -> bool {
        #[cfg(target_arch = "aarch64")]
        {
            std::arch::is_aarch64_feature_detected!("neon")
        }
        #[cfg(target_arch = "x86_64")]
        {
            std::arch::is_x86_feature_detected!("sse2")
        }
        #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
        {
            false
        }
    }
}
