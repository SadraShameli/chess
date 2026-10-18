use std::fs::File;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use exacl::{AclEntry, AclEntryKind, AclOption, Flag, Perm};
use rustix::fs::{Mode, OFlags};

use crate::desktop::profile::error::DesktopProfileError;

pub(in crate::desktop::profile) enum DesktopProfileNativeMacosAclEnumScope {
    DataDirectory,
    PrivateControl,
}

trait DesktopProfileNativeMacosAclProvider {
    fn entries(&self, path: &Path) -> io::Result<Vec<AclEntry>>;
}

struct DesktopProfileNativeMacosAclOs;

impl DesktopProfileNativeMacosAclProvider for DesktopProfileNativeMacosAclOs {
    fn entries(&self, path: &Path) -> io::Result<Vec<AclEntry>> {
        exacl::getfacl(path, AclOption::SYMLINK_ACL)
    }
}

pub(in crate::desktop::profile) struct DesktopProfileNativeMacosAcl;

impl DesktopProfileNativeMacosAcl {
    pub(in crate::desktop::profile) fn verify(
        file: &File,
        path: &Path,
        scope: DesktopProfileNativeMacosAclEnumScope,
    ) -> Result<(), DesktopProfileError> {
        Self::verify_with(&DesktopProfileNativeMacosAclOs, file, path, scope)
    }

    fn verify_with<P: DesktopProfileNativeMacosAclProvider>(
        provider: &P,
        file: &File,
        path: &Path,
        scope: DesktopProfileNativeMacosAclEnumScope,
    ) -> Result<(), DesktopProfileError> {
        Self::verify_path_identity(file, path)?;
        let entries = provider
            .entries(path)
            .map_err(|source| DesktopProfileError::AclQuery {
                path: path.to_path_buf(),
                source,
            })?;
        Self::verify_path_identity(file, path)?;
        let mutation = Perm::WRITE
            | Perm::APPEND
            | Perm::DELETE
            | Perm::DELETE_CHILD
            | Perm::WRITEATTR
            | Perm::WRITEEXTATTR
            | Perm::WRITESECURITY
            | Perm::CHOWN;
        for entry in entries {
            let invalid = entry.kind == AclEntryKind::Unknown
                || !entry.perms.difference(Perm::all()).is_empty()
                || !entry.flags.difference(Flag::all()).is_empty();
            let grant = entry.allow
                && match scope {
                    DesktopProfileNativeMacosAclEnumScope::DataDirectory => {
                        entry.perms.intersects(mutation)
                    }
                    DesktopProfileNativeMacosAclEnumScope::PrivateControl => {
                        !entry.perms.is_empty()
                    }
                };
            if invalid || grant {
                return Err(DesktopProfileError::UnsafePermissions {
                    path: path.to_path_buf(),
                });
            }
        }
        Ok(())
    }

    fn verify_path_identity(file: &File, path: &Path) -> Result<(), DesktopProfileError> {
        let descriptor = rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|error| DesktopProfileError::AclQuery {
            path: path.to_path_buf(),
            source: error.into(),
        })?;
        let current = File::from(descriptor)
            .metadata()
            .map_err(|error| DesktopProfileError::io(path, error))?;
        let held = file
            .metadata()
            .map_err(|error| DesktopProfileError::io(path, error))?;
        if (current.dev(), current.ino()) != (held.dev(), held.ino()) {
            return Err(DesktopProfileError::IdentityChanged {
                path: path.to_path_buf(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;

    use exacl::{AclEntry, Perm};

    use crate::desktop::profile::error::DesktopProfileError;
    use crate::desktop::profile::native::macos_acl::{
        DesktopProfileNativeMacosAcl, DesktopProfileNativeMacosAclEnumScope,
        DesktopProfileNativeMacosAclProvider,
    };

    enum DesktopProfileNativeMacosAclTestEnumProbe {
        Unavailable,
        ReadGrant,
        WriteGrant,
        Deny,
        ReplacedPath,
    }

    impl DesktopProfileNativeMacosAclProvider for DesktopProfileNativeMacosAclTestEnumProbe {
        fn entries(&self, path: &Path) -> io::Result<Vec<AclEntry>> {
            match self {
                Self::Unavailable => Err(io::ErrorKind::PermissionDenied.into()),
                Self::ReadGrant => Ok(vec![AclEntry::allow_group("everyone", Perm::READ, None)]),
                Self::WriteGrant => Ok(vec![AclEntry::allow_group("everyone", Perm::WRITE, None)]),
                Self::Deny => Ok(vec![AclEntry::deny_group("everyone", Perm::DELETE, None)]),
                Self::ReplacedPath => {
                    let previous = path.with_extension("previous");
                    std::fs::rename(path, &previous)?;
                    std::os::unix::fs::symlink(previous, path)?;
                    Ok(Vec::new())
                }
            }
        }
    }

    #[test]
    fn an_acl_read_failure_retains_its_cause_instead_of_admitting_the_profile()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = tempfile::NamedTempFile::new()?;
        assert!(matches!(
            DesktopProfileNativeMacosAcl::verify_with(
                &DesktopProfileNativeMacosAclTestEnumProbe::Unavailable,
                file.as_file(),
                file.path(),
                DesktopProfileNativeMacosAclEnumScope::PrivateControl,
            ),
            Err(DesktopProfileError::AclQuery { source, .. })
                if source.kind() == io::ErrorKind::PermissionDenied
        ));
        Ok(())
    }

    #[test]
    fn mutation_grants_are_refused_despite_restrictive_mode_bits()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = tempfile::NamedTempFile::new()?;
        assert!(matches!(
            DesktopProfileNativeMacosAcl::verify_with(
                &DesktopProfileNativeMacosAclTestEnumProbe::WriteGrant,
                file.as_file(),
                file.path(),
                DesktopProfileNativeMacosAclEnumScope::DataDirectory,
            ),
            Err(DesktopProfileError::UnsafePermissions { .. })
        ));
        Ok(())
    }

    #[test]
    fn private_control_rejects_read_grants_but_accepts_harmless_denials()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = tempfile::NamedTempFile::new()?;
        assert!(matches!(
            DesktopProfileNativeMacosAcl::verify_with(
                &DesktopProfileNativeMacosAclTestEnumProbe::ReadGrant,
                file.as_file(),
                file.path(),
                DesktopProfileNativeMacosAclEnumScope::PrivateControl,
            ),
            Err(DesktopProfileError::UnsafePermissions { .. })
        ));
        DesktopProfileNativeMacosAcl::verify_with(
            &DesktopProfileNativeMacosAclTestEnumProbe::Deny,
            file.as_file(),
            file.path(),
            DesktopProfileNativeMacosAclEnumScope::PrivateControl,
        )?;
        Ok(())
    }

    #[test]
    fn a_path_replaced_during_acl_read_cannot_reuse_an_empty_result()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::TempDir::new()?;
        let path = directory.path().join("control");
        std::fs::write(&path, b"identity")?;
        let file = std::fs::File::open(&path)?;
        assert!(matches!(
            DesktopProfileNativeMacosAcl::verify_with(
                &DesktopProfileNativeMacosAclTestEnumProbe::ReplacedPath,
                &file,
                &path,
                DesktopProfileNativeMacosAclEnumScope::PrivateControl,
            ),
            Err(DesktopProfileError::AclQuery { .. })
        ));
        Ok(())
    }
}
