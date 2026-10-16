use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use win_sd::{AccessMask, AceType, SddlParser, Sid};
use windows_permissions::constants::{SeObjectType, SecurityInformation};
use windows_permissions::{LocalBox, SecurityDescriptor, wrappers};
use winsafe::{HPROCESS, TokenInfo, co};

use crate::desktop::profile::error::DesktopProfileError;
use crate::desktop::profile::filesystem::{
    DesktopProfileFilesystem, DesktopProfileFilesystemEnumClass, DesktopProfileFilesystemNative,
    DesktopProfileFilesystemProvider,
};
use crate::desktop::profile::paths::DesktopProfilePaths;
use crate::desktop::profile::security::DesktopProfileSecurity;

#[derive(Clone, Copy)]
enum DesktopProfileNativeWindowsEnumObject {
    Directory,
    File,
}

impl DesktopProfileNativeWindowsEnumObject {
    fn inheritance(self) -> &'static str {
        match self {
            Self::Directory => "OICI",
            Self::File => "",
        }
    }
}

impl DesktopProfileFilesystemProvider for DesktopProfileFilesystemNative {
    fn classify(
        &self,
        file: &File,
        path: &Path,
    ) -> Result<DesktopProfileFilesystemEnumClass, DesktopProfileError> {
        let text = path
            .to_str()
            .ok_or_else(|| DesktopProfileError::UnsafeIdentity {
                path: path.to_path_buf(),
            })?;
        let root = winsafe::GetVolumePathName(text).map_err(|error| {
            DesktopProfileError::FilesystemQuery {
                path: path.to_path_buf(),
                source: io::Error::other(error),
            }
        })?;
        let drive = winsafe::GetDriveType(Some(&root));
        let drive_class = Self::classify_windows_drive(drive);
        if !matches!(drive_class, DesktopProfileFilesystemEnumClass::Local) {
            return Ok(drive_class);
        }
        let mut serial = 0;
        let mut flags = co::FILE_VOL::default();
        let mut filesystem = String::new();
        winsafe::GetVolumeInformation(
            Some(&root),
            None,
            Some(&mut serial),
            None,
            Some(&mut flags),
            Some(&mut filesystem),
        )
        .map_err(|error| DesktopProfileError::FilesystemQuery {
            path: path.to_path_buf(),
            source: io::Error::other(error),
        })?;
        let identity = winapi_util::file::information(file).map_err(|source| {
            DesktopProfileError::FilesystemQuery {
                path: path.to_path_buf(),
                source,
            }
        })?;
        if identity.volume_serial_number() != u64::from(serial) {
            return Err(DesktopProfileError::IdentityChanged {
                path: path.to_path_buf(),
            });
        }
        Ok(Self::classify_windows(drive, &filesystem, flags))
    }
}

impl DesktopProfileFilesystemNative {
    fn classify_windows_drive(drive: co::DRIVE) -> DesktopProfileFilesystemEnumClass {
        if drive == co::DRIVE::REMOTE {
            DesktopProfileFilesystemEnumClass::Nonlocal
        } else if matches!(drive, co::DRIVE::FIXED | co::DRIVE::REMOVABLE) {
            DesktopProfileFilesystemEnumClass::Local
        } else {
            DesktopProfileFilesystemEnumClass::Unsupported
        }
    }

    fn classify_windows(
        drive: co::DRIVE,
        filesystem: &str,
        flags: co::FILE_VOL,
    ) -> DesktopProfileFilesystemEnumClass {
        let drive_class = Self::classify_windows_drive(drive);
        if !matches!(drive_class, DesktopProfileFilesystemEnumClass::Local) {
            return drive_class;
        }
        if filesystem == "NTFS" && flags.has(co::FILE_VOL::PERSISTENT_ACLS) {
            DesktopProfileFilesystemEnumClass::Local
        } else {
            DesktopProfileFilesystemEnumClass::Unsupported
        }
    }
}

impl DesktopProfileSecurity {
    pub fn current() -> Result<Self, DesktopProfileError> {
        let token = HPROCESS::GetCurrentProcess()
            .OpenProcessToken(co::TOKEN::QUERY)
            .map_err(|error| DesktopProfileError::CurrentUserQuery {
                source: io::Error::other(error),
            })?;
        let TokenInfo::User(user) = token
            .GetTokenInformation(co::TOKEN_INFORMATION_CLASS::User)
            .map_err(|error| DesktopProfileError::CurrentUserQuery {
                source: io::Error::other(error),
            })?
        else {
            return Err(DesktopProfileError::CurrentUserUnavailable);
        };
        let sid = user
            .User
            .Sid()
            .ok_or(DesktopProfileError::CurrentUserUnavailable)?;
        let user_sid = winsafe::ConvertSidToStringSid(sid).map_err(|error| {
            DesktopProfileError::CurrentUserQuery {
                source: io::Error::other(error),
            }
        })?;
        Ok(Self { user_sid })
    }

    pub(in crate::desktop::profile) fn prepare_data_directory(
        &self,
        path: &Path,
    ) -> Result<PathBuf, DesktopProfileError> {
        fs::create_dir_all(path).map_err(|error| DesktopProfileError::io(path, error))?;
        path.canonicalize()
            .map_err(|error| DesktopProfileError::io(path, error))
    }

    pub(in crate::desktop::profile) fn open_data_directory(
        &self,
        path: &Path,
    ) -> Result<File, DesktopProfileError> {
        let file = Self::open_directory(path)?;
        self.validate_data_directory(&file, path)?;
        Ok(file)
    }

    pub(in crate::desktop::profile) fn open_control_directory(
        &self,
        _data: &File,
        paths: &DesktopProfilePaths,
    ) -> Result<File, DesktopProfileError> {
        match Self::open_directory(paths.control()) {
            Ok(file) => {
                self.validate_control_directory(&file, paths.control())?;
                return Ok(file);
            }
            Err(DesktopProfileError::Io { source, .. })
                if source.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let temporary = tempfile::Builder::new()
            .prefix(".control-")
            .tempdir_in(paths.data())
            .map_err(|error| DesktopProfileError::io(paths.data(), error))?;
        let mut prepared = Self::open_directory(temporary.path())?;
        self.validate_data_directory(&prepared, temporary.path())?;
        self.protect(
            &mut prepared,
            temporary.path(),
            DesktopProfileNativeWindowsEnumObject::Directory,
        )?;
        drop(prepared);
        Self::reconcile_publication(Self::move_without_replacement(
            temporary.path(),
            paths.control(),
        ))?;
        let file = Self::open_directory(paths.control())?;
        self.validate_control_directory(&file, paths.control())?;
        Ok(file)
    }

    pub(in crate::desktop::profile) fn open_lock_file(
        &self,
        control: &File,
        paths: &DesktopProfilePaths,
    ) -> Result<File, DesktopProfileError> {
        match Self::open_file(paths.lock(), true) {
            Ok(file) => {
                self.validate_control_file(&file, paths.lock())?;
                return Ok(file);
            }
            Err(DesktopProfileError::Io { source, .. })
                if source.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut temporary = NamedTempFile::new_in(paths.control())
            .map_err(|error| DesktopProfileError::io(paths.control(), error))?;
        let temporary_path = temporary.path().to_path_buf();
        self.protect_created_file(temporary.as_file_mut(), &temporary_path)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| DesktopProfileError::io(&temporary_path, error))?;
        let publication = self.persist_control_file(temporary, control, paths.lock());
        Self::reconcile_publication(publication.map(drop))?;
        let file = Self::open_file(paths.lock(), true)?;
        self.validate_control_file(&file, paths.lock())?;
        Ok(file)
    }

    pub(in crate::desktop::profile) fn open_existing_control_file(
        &self,
        _control: &File,
        path: &Path,
    ) -> Result<File, DesktopProfileError> {
        let file = Self::open_file(path, false)?;
        self.validate_control_file(&file, path)?;
        Ok(file)
    }

    pub(in crate::desktop::profile) fn validate_data_directory(
        &self,
        file: &File,
        path: &Path,
    ) -> Result<(), DesktopProfileError> {
        Self::validate_object_kind(file, path, DesktopProfileNativeWindowsEnumObject::Directory)?;
        let native = Self::descriptor(file, path)?;
        let sddl = Self::descriptor_sddl(&native, path)?;
        let text = sddl
            .to_str()
            .ok_or_else(|| DesktopProfileError::UnsafePermissions {
                path: path.to_path_buf(),
            })?;
        let descriptor = win_sd::parse::<SddlParser>(text).map_err(|error| {
            DesktopProfileError::io(path, io::Error::new(io::ErrorKind::InvalidData, error))
        })?;
        let current: Sid = self.user_sid().parse().map_err(|error| {
            DesktopProfileError::io(path, io::Error::new(io::ErrorKind::InvalidData, error))
        })?;
        if descriptor.owner() != Some(&current) {
            return Err(DesktopProfileError::UnsafePermissions {
                path: path.to_path_buf(),
            });
        }
        let dacl = descriptor
            .dacl()
            .ok_or_else(|| DesktopProfileError::UnsafePermissions {
                path: path.to_path_buf(),
            })?;
        let system = Sid::local_system();
        let administrators = Sid::administrators();
        let mutation = AccessMask::GENERIC_ALL
            | AccessMask::GENERIC_WRITE
            | AccessMask::WRITE_DAC
            | AccessMask::WRITE_OWNER
            | AccessMask::DELETE
            | AccessMask::FILE_WRITE_DATA
            | AccessMask::FILE_APPEND_DATA
            | AccessMask::FILE_WRITE_EA
            | AccessMask::FILE_DELETE_CHILD
            | AccessMask::FILE_WRITE_ATTRIBUTES;
        for ace in dacl.entries() {
            if ace.is_inherit_only() {
                continue;
            }
            if ace.ace_type() != AceType::AccessAllowed
                || (!(ace.mask() & mutation).is_empty()
                    && ace.sid() != &current
                    && ace.sid() != &system
                    && ace.sid() != &administrators)
            {
                return Err(DesktopProfileError::UnsafePermissions {
                    path: path.to_path_buf(),
                });
            }
        }
        Ok(())
    }

    pub(in crate::desktop::profile) fn validate_control_directory(
        &self,
        file: &File,
        path: &Path,
    ) -> Result<(), DesktopProfileError> {
        self.validate_private(file, path, DesktopProfileNativeWindowsEnumObject::Directory)
    }

    pub(in crate::desktop::profile) fn validate_control_file(
        &self,
        file: &File,
        path: &Path,
    ) -> Result<(), DesktopProfileError> {
        self.validate_private(file, path, DesktopProfileNativeWindowsEnumObject::File)
    }

    pub(in crate::desktop::profile) fn protect_created_file(
        &self,
        file: &mut File,
        path: &Path,
    ) -> Result<(), DesktopProfileError> {
        self.protect(file, path, DesktopProfileNativeWindowsEnumObject::File)
    }

    pub(in crate::desktop::profile) fn persist_control_file(
        &self,
        temporary: NamedTempFile,
        _control: &File,
        destination: &Path,
    ) -> Result<File, DesktopProfileError> {
        let (file, temporary_path) = temporary.into_parts();
        let temporary_text =
            temporary_path
                .to_str()
                .ok_or_else(|| DesktopProfileError::UnsafeIdentity {
                    path: temporary_path.to_path_buf(),
                })?;
        winsafe::SetFileAttributes(temporary_text, co::FILE_ATTRIBUTE::NORMAL)
            .map_err(|error| DesktopProfileError::io(&temporary_path, io::Error::other(error)))?;
        Self::move_without_replacement(&temporary_path, destination)?;
        Ok(file)
    }

    fn open_directory(path: &Path) -> Result<File, DesktopProfileError> {
        OpenOptions::new()
            .read(true)
            .access_mode(co::GENERIC::READ.raw() | co::ACCESS_RIGHTS::WRITE_DAC.raw())
            .custom_flags(
                (co::FILE_FLAG::BACKUP_SEMANTICS | co::FILE_FLAG::OPEN_REPARSE_POINT).raw(),
            )
            .share_mode((co::FILE_SHARE::READ | co::FILE_SHARE::WRITE).raw())
            .open(path)
            .map_err(|error| DesktopProfileError::io(path, error))
    }

    fn open_file(path: &Path, writable: bool) -> Result<File, DesktopProfileError> {
        OpenOptions::new()
            .read(true)
            .write(writable)
            .custom_flags(co::FILE_FLAG::OPEN_REPARSE_POINT.raw())
            .share_mode((co::FILE_SHARE::READ | co::FILE_SHARE::WRITE).raw())
            .open(path)
            .map_err(|error| DesktopProfileError::io(path, error))
    }

    fn validate_object_kind(
        file: &File,
        path: &Path,
        kind: DesktopProfileNativeWindowsEnumObject,
    ) -> Result<(), DesktopProfileError> {
        let metadata = file
            .metadata()
            .map_err(|error| DesktopProfileError::io(path, error))?;
        let valid_kind = match kind {
            DesktopProfileNativeWindowsEnumObject::Directory => metadata.is_dir(),
            DesktopProfileNativeWindowsEnumObject::File => {
                metadata.is_file()
                    && winapi_util::file::information(file)
                        .map_err(|error| DesktopProfileError::io(path, error))?
                        .number_of_links()
                        == 1
            }
        };
        if !valid_kind || metadata.file_attributes() & co::FILE_ATTRIBUTE::REPARSE_POINT.raw() != 0
        {
            return Err(DesktopProfileError::UnsafeIdentity {
                path: path.to_path_buf(),
            });
        }
        DesktopProfileFilesystem::verify(&DesktopProfileFilesystemNative, file, path)?;
        Ok(())
    }

    fn descriptor(
        file: &File,
        path: &Path,
    ) -> Result<LocalBox<SecurityDescriptor>, DesktopProfileError> {
        wrappers::GetSecurityInfo(
            file,
            SeObjectType::SE_FILE_OBJECT,
            SecurityInformation::Owner | SecurityInformation::Dacl,
        )
        .map_err(|error| DesktopProfileError::io(path, error))
    }

    fn descriptor_sddl(
        descriptor: &SecurityDescriptor,
        path: &Path,
    ) -> Result<OsString, DesktopProfileError> {
        wrappers::ConvertSecurityDescriptorToStringSecurityDescriptor(
            descriptor,
            SecurityInformation::Owner | SecurityInformation::Dacl,
        )
        .map_err(|error| DesktopProfileError::io(path, error))
    }

    fn private_descriptor(
        &self,
        kind: DesktopProfileNativeWindowsEnumObject,
        path: &Path,
    ) -> Result<LocalBox<SecurityDescriptor>, DesktopProfileError> {
        let inheritance = kind.inheritance();
        let sddl = format!(
            "O:{}D:P(A;{};FA;;;SY)(A;{};FA;;;{})",
            self.user_sid(),
            inheritance,
            inheritance,
            self.user_sid()
        );
        wrappers::ConvertStringSecurityDescriptorToSecurityDescriptor(&sddl)
            .map_err(|error| DesktopProfileError::io(path, error))
    }

    fn validate_private(
        &self,
        file: &File,
        path: &Path,
        kind: DesktopProfileNativeWindowsEnumObject,
    ) -> Result<(), DesktopProfileError> {
        Self::validate_object_kind(file, path, kind)?;
        let actual = Self::descriptor(file, path)?;
        let expected = self.private_descriptor(kind, path)?;
        if Self::descriptor_sddl(&actual, path)? != Self::descriptor_sddl(&expected, path)? {
            return Err(DesktopProfileError::UnsafePermissions {
                path: path.to_path_buf(),
            });
        }
        Ok(())
    }

    fn protect(
        &self,
        file: &mut File,
        path: &Path,
        kind: DesktopProfileNativeWindowsEnumObject,
    ) -> Result<(), DesktopProfileError> {
        let expected = self.private_descriptor(kind, path)?;
        let dacl = wrappers::GetSecurityDescriptorDacl(&expected)
            .map_err(|error| DesktopProfileError::io(path, error))?
            .ok_or_else(|| DesktopProfileError::UnsafePermissions {
                path: path.to_path_buf(),
            })?;
        wrappers::SetSecurityInfo(
            file,
            SeObjectType::SE_FILE_OBJECT,
            SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
            None,
            None,
            Some(dacl),
            None,
        )
        .map_err(|error| DesktopProfileError::io(path, error))?;
        self.validate_private(file, path, kind)
    }

    fn move_without_replacement(
        source: &Path,
        destination: &Path,
    ) -> Result<(), DesktopProfileError> {
        let source_text = source
            .to_str()
            .ok_or_else(|| DesktopProfileError::UnsafeIdentity {
                path: source.to_path_buf(),
            })?;
        let destination_text =
            destination
                .to_str()
                .ok_or_else(|| DesktopProfileError::UnsafeIdentity {
                    path: destination.to_path_buf(),
                })?;
        winsafe::MoveFileEx(
            source_text,
            Some(destination_text),
            co::MOVEFILE::WRITE_THROUGH,
        )
        .map_err(|error| {
            let source = match i32::try_from(error.raw()) {
                Ok(code) => io::Error::from_raw_os_error(code),
                Err(_) => io::Error::other(error),
            };
            DesktopProfileError::io(destination, source)
        })
    }

    fn reconcile_publication(
        result: Result<(), DesktopProfileError>,
    ) -> Result<(), DesktopProfileError> {
        match result {
            Ok(()) => Ok(()),
            Err(DesktopProfileError::Io { source, .. })
                if source.kind() == io::ErrorKind::AlreadyExists =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use winsafe::co;

    use crate::desktop::profile::filesystem::{
        DesktopProfileFilesystemEnumClass, DesktopProfileFilesystemNative,
    };

    #[test]
    fn a_network_drive_cannot_qualify_through_its_reported_filesystem_name() {
        assert!(matches!(
            DesktopProfileFilesystemNative::classify_windows(
                co::DRIVE::REMOTE,
                "NTFS",
                co::FILE_VOL::PERSISTENT_ACLS,
            ),
            DesktopProfileFilesystemEnumClass::Nonlocal
        ));
    }

    #[test]
    fn local_ownership_requires_ntfs_with_persistent_acl_support() {
        assert!(matches!(
            DesktopProfileFilesystemNative::classify_windows(
                co::DRIVE::FIXED,
                "NTFS",
                co::FILE_VOL::PERSISTENT_ACLS,
            ),
            DesktopProfileFilesystemEnumClass::Local
        ));
        assert!(matches!(
            DesktopProfileFilesystemNative::classify_windows(
                co::DRIVE::FIXED,
                "NTFS",
                co::FILE_VOL::default(),
            ),
            DesktopProfileFilesystemEnumClass::Unsupported
        ));
        assert!(matches!(
            DesktopProfileFilesystemNative::classify_windows(
                co::DRIVE::FIXED,
                "ReFS",
                co::FILE_VOL::PERSISTENT_ACLS,
            ),
            DesktopProfileFilesystemEnumClass::Unsupported
        ));
    }
}
