use std::fs::{DirBuilder, File};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags};
use rustix::io::Errno;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::desktop::profile::error::DesktopProfileError;
use crate::desktop::profile::filesystem::{
    DesktopProfileFilesystem, DesktopProfileFilesystemNative,
};
#[cfg(target_os = "macos")]
use crate::desktop::profile::native::macos_acl::{
    DesktopProfileNativeMacosAcl, DesktopProfileNativeMacosAclEnumScope,
};
use crate::desktop::profile::paths::DesktopProfilePaths;
use crate::desktop::profile::security::DesktopProfileSecurity;

enum DesktopProfileNativeUnixEnumDirectoryPrivacy {
    Data,
    Control,
}

impl DesktopProfileSecurity {
    const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
    const PRIVATE_FILE_MODE: u32 = 0o600;
    const PERMISSION_BITS: u32 = 0o7777;
    const OTHER_WRITE_BITS: u32 = 0o022;

    pub fn current() -> Result<Self, DesktopProfileError> {
        let pid =
            sysinfo::get_current_pid().map_err(|error| DesktopProfileError::CurrentUserQuery {
                source: io::Error::other(error),
            })?;
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_user(UpdateKind::Always),
        );
        let effective_uid = system
            .process(pid)
            .and_then(|process| process.effective_user_id())
            .map(|uid| **uid)
            .ok_or(DesktopProfileError::CurrentUserUnavailable)?;
        Ok(Self { effective_uid })
    }

    pub(in crate::desktop::profile) fn prepare_data_directory(
        &self,
        path: &Path,
    ) -> Result<PathBuf, DesktopProfileError> {
        DirBuilder::new()
            .recursive(true)
            .mode(Self::PRIVATE_DIRECTORY_MODE)
            .create(path)
            .map_err(|error| DesktopProfileError::io(path, error))?;
        path.canonicalize()
            .map_err(|error| DesktopProfileError::io(path, error))
    }

    pub(in crate::desktop::profile) fn open_data_directory(
        &self,
        path: &Path,
    ) -> Result<File, DesktopProfileError> {
        let fd = rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Self::open_error(path, error))?;
        let file = File::from(fd);
        self.validate_data_directory(&file, path)?;
        Ok(file)
    }

    pub(crate) fn open_private_directory(&self, path: &Path) -> Result<File, DesktopProfileError> {
        let fd = rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Self::open_error(path, error))?;
        let file = File::from(fd);
        self.validate_control_directory(&file, path)?;
        Ok(file)
    }

    pub(in crate::desktop::profile) fn open_control_directory(
        &self,
        data: &File,
        paths: &DesktopProfilePaths,
    ) -> Result<File, DesktopProfileError> {
        match rustix::fs::mkdirat(data, DesktopProfilePaths::CONTROL_NAME, Mode::RWXU) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(error) => return Err(DesktopProfileError::io(paths.control(), error.into())),
        }
        let fd = rustix::fs::openat(
            data,
            DesktopProfilePaths::CONTROL_NAME,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Self::open_error(paths.control(), error))?;
        let file = File::from(fd);
        self.validate_control_directory(&file, paths.control())?;
        Ok(file)
    }

    pub(in crate::desktop::profile) fn open_lock_file(
        &self,
        control: &File,
        paths: &DesktopProfilePaths,
    ) -> Result<File, DesktopProfileError> {
        let flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        let created = rustix::fs::openat(
            control,
            DesktopProfilePaths::LOCK_NAME,
            flags | OFlags::CREATE | OFlags::EXCL,
            Mode::RUSR | Mode::WUSR,
        );
        let fd = match created {
            Ok(fd) => Ok(fd),
            Err(Errno::EXIST) => rustix::fs::openat(
                control,
                DesktopProfilePaths::LOCK_NAME,
                flags,
                Mode::empty(),
            ),
            Err(error) => Err(error),
        }
        .map_err(|error| Self::open_error(paths.lock(), error))?;
        let file = File::from(fd);
        self.validate_control_file(&file, paths.lock())?;
        Ok(file)
    }

    pub(in crate::desktop::profile) fn open_existing_control_file(
        &self,
        control: &File,
        path: &Path,
    ) -> Result<File, DesktopProfileError> {
        let name = path
            .file_name()
            .ok_or_else(|| DesktopProfileError::UnsafeIdentity {
                path: path.to_path_buf(),
            })?;
        let fd = rustix::fs::openat(
            control,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|error| Self::open_error(path, error))?;
        let file = File::from(fd);
        self.validate_control_file(&file, path)?;
        Ok(file)
    }

    pub(in crate::desktop::profile) fn validate_data_directory(
        &self,
        file: &File,
        path: &Path,
    ) -> Result<(), DesktopProfileError> {
        self.validate_directory(
            file,
            path,
            DesktopProfileNativeUnixEnumDirectoryPrivacy::Data,
        )
    }

    pub(in crate::desktop::profile) fn validate_control_directory(
        &self,
        file: &File,
        path: &Path,
    ) -> Result<(), DesktopProfileError> {
        self.validate_directory(
            file,
            path,
            DesktopProfileNativeUnixEnumDirectoryPrivacy::Control,
        )
    }

    fn validate_directory(
        &self,
        file: &File,
        path: &Path,
        privacy: DesktopProfileNativeUnixEnumDirectoryPrivacy,
    ) -> Result<(), DesktopProfileError> {
        let metadata = file
            .metadata()
            .map_err(|error| DesktopProfileError::io(path, error))?;
        if !metadata.is_dir() {
            return Err(DesktopProfileError::UnsafeIdentity {
                path: path.to_path_buf(),
            });
        }
        let unsafe_mode = match privacy {
            DesktopProfileNativeUnixEnumDirectoryPrivacy::Data => {
                metadata.mode() & Self::OTHER_WRITE_BITS != 0
            }
            DesktopProfileNativeUnixEnumDirectoryPrivacy::Control => {
                metadata.mode() & Self::PERMISSION_BITS != Self::PRIVATE_DIRECTORY_MODE
            }
        };
        if metadata.uid() != self.effective_user_id() || unsafe_mode {
            return Err(DesktopProfileError::UnsafePermissions {
                path: path.to_path_buf(),
            });
        }
        DesktopProfileFilesystem::verify(&DesktopProfileFilesystemNative, file, path)?;
        #[cfg(target_os = "macos")]
        {
            let scope = match privacy {
                DesktopProfileNativeUnixEnumDirectoryPrivacy::Data => {
                    DesktopProfileNativeMacosAclEnumScope::DataDirectory
                }
                DesktopProfileNativeUnixEnumDirectoryPrivacy::Control => {
                    DesktopProfileNativeMacosAclEnumScope::PrivateControl
                }
            };
            DesktopProfileNativeMacosAcl::verify(file, path, scope)?;
        }
        Ok(())
    }

    pub(in crate::desktop::profile) fn validate_control_file(
        &self,
        file: &File,
        path: &Path,
    ) -> Result<(), DesktopProfileError> {
        let metadata = file
            .metadata()
            .map_err(|error| DesktopProfileError::io(path, error))?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(DesktopProfileError::UnsafeIdentity {
                path: path.to_path_buf(),
            });
        }
        if metadata.uid() != self.effective_user_id()
            || metadata.mode() & Self::PERMISSION_BITS != Self::PRIVATE_FILE_MODE
        {
            return Err(DesktopProfileError::UnsafePermissions {
                path: path.to_path_buf(),
            });
        }
        DesktopProfileFilesystem::verify(&DesktopProfileFilesystemNative, file, path)?;
        #[cfg(target_os = "macos")]
        DesktopProfileNativeMacosAcl::verify(
            file,
            path,
            DesktopProfileNativeMacosAclEnumScope::PrivateControl,
        )?;
        Ok(())
    }

    pub(in crate::desktop::profile) fn protect_created_file(
        &self,
        file: &mut File,
        path: &Path,
    ) -> Result<(), DesktopProfileError> {
        self.validate_control_file(file, path)
    }

    pub(in crate::desktop::profile) fn persist_control_file(
        &self,
        temporary: tempfile::NamedTempFile,
        control: &File,
        destination: &Path,
    ) -> Result<File, DesktopProfileError> {
        let file = temporary
            .persist_noclobber(destination)
            .map_err(|error| DesktopProfileError::io(destination, error.error))?;
        let directory =
            destination
                .parent()
                .ok_or_else(|| DesktopProfileError::UnsafeIdentity {
                    path: destination.to_path_buf(),
                })?;
        control
            .sync_all()
            .map_err(|error| DesktopProfileError::io(directory, error))?;
        Ok(file)
    }

    fn open_error(path: &Path, error: Errno) -> DesktopProfileError {
        if error == Errno::LOOP || error == Errno::NOTDIR {
            DesktopProfileError::UnsafeIdentity {
                path: path.to_path_buf(),
            }
        } else {
            DesktopProfileError::io(path, io::Error::from(error))
        }
    }
}
