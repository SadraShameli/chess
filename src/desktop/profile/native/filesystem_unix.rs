use std::fs::File;
use std::path::Path;

use crate::desktop::profile::error::DesktopProfileError;
use crate::desktop::profile::filesystem::{
    DesktopProfileFilesystemEnumClass, DesktopProfileFilesystemNative,
    DesktopProfileFilesystemProvider,
};

impl DesktopProfileFilesystemProvider for DesktopProfileFilesystemNative {
    fn classify(
        &self,
        file: &File,
        path: &Path,
    ) -> Result<DesktopProfileFilesystemEnumClass, DesktopProfileError> {
        let observation =
            rustix::fs::fstatfs(file).map_err(|error| DesktopProfileError::FilesystemQuery {
                path: path.to_path_buf(),
                source: error.into(),
            })?;
        #[cfg(target_os = "linux")]
        {
            Ok(Self::classify_linux_magic(observation.f_type))
        }
        #[cfg(target_os = "macos")]
        {
            let name = observation
                .f_fstypename
                .into_iter()
                .take_while(|byte| *byte != 0)
                .map(u8::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| DesktopProfileError::FilesystemQuery {
                    path: path.to_path_buf(),
                    source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                })?;
            Ok(Self::classify_macos(&name, observation.f_flags))
        }
    }
}

#[cfg(target_os = "linux")]
impl DesktopProfileFilesystemNative {
    const LINUX_EXT_MAGIC: rustix::fs::FsWord = 0xEF53;
    const LINUX_XFS_MAGIC: rustix::fs::FsWord = 0x5846_5342;
    const LINUX_BTRFS_MAGIC: rustix::fs::FsWord = 0x9123_683E;
    const LINUX_F2FS_MAGIC: rustix::fs::FsWord = 0xF2F5_2010;
    const LINUX_TMPFS_MAGIC: rustix::fs::FsWord = 0x0102_1994;

    fn classify_linux_magic(magic: rustix::fs::FsWord) -> DesktopProfileFilesystemEnumClass {
        match magic {
            Self::LINUX_EXT_MAGIC
            | Self::LINUX_XFS_MAGIC
            | Self::LINUX_BTRFS_MAGIC
            | Self::LINUX_F2FS_MAGIC
            | Self::LINUX_TMPFS_MAGIC => DesktopProfileFilesystemEnumClass::Local,
            rustix::fs::NFS_SUPER_MAGIC => DesktopProfileFilesystemEnumClass::Nonlocal,
            _ => DesktopProfileFilesystemEnumClass::Unsupported,
        }
    }
}

#[cfg(target_os = "macos")]
impl DesktopProfileFilesystemNative {
    const MACOS_LOCAL_MOUNT_FLAG: u32 = 0x0000_1000;

    fn classify_macos(name: &[u8], flags: u32) -> DesktopProfileFilesystemEnumClass {
        if flags & Self::MACOS_LOCAL_MOUNT_FLAG == 0 {
            return DesktopProfileFilesystemEnumClass::Nonlocal;
        }
        match name {
            b"apfs" | b"hfs" => DesktopProfileFilesystemEnumClass::Local,
            _ => DesktopProfileFilesystemEnumClass::Unsupported,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::desktop::profile::filesystem::{
        DesktopProfileFilesystemEnumClass, DesktopProfileFilesystemNative,
    };

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_rejects_network_and_unrecognized_filesystems() {
        assert!(matches!(
            DesktopProfileFilesystemNative::classify_linux_magic(rustix::fs::NFS_SUPER_MAGIC),
            DesktopProfileFilesystemEnumClass::Nonlocal
        ));
        assert!(matches!(
            DesktopProfileFilesystemNative::classify_linux_magic(0),
            DesktopProfileFilesystemEnumClass::Unsupported
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_requires_local_mount_evidence_even_for_a_known_format() {
        assert!(matches!(
            DesktopProfileFilesystemNative::classify_macos(b"apfs", 0),
            DesktopProfileFilesystemEnumClass::Nonlocal
        ));
        assert!(matches!(
            DesktopProfileFilesystemNative::classify_macos(
                b"apfs",
                DesktopProfileFilesystemNative::MACOS_LOCAL_MOUNT_FLAG,
            ),
            DesktopProfileFilesystemEnumClass::Local
        ));
        assert!(matches!(
            DesktopProfileFilesystemNative::classify_macos(
                b"unknown",
                DesktopProfileFilesystemNative::MACOS_LOCAL_MOUNT_FLAG,
            ),
            DesktopProfileFilesystemEnumClass::Unsupported
        ));
    }
}
