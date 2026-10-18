use std::fs::File;
use std::path::Path;

use crate::desktop::profile::error::DesktopProfileError;

pub(super) enum DesktopProfileFilesystemEnumClass {
    Local,
    Nonlocal,
    Unsupported,
}

pub(super) trait DesktopProfileFilesystemProvider {
    fn classify(
        &self,
        file: &File,
        path: &Path,
    ) -> Result<DesktopProfileFilesystemEnumClass, DesktopProfileError>;
}

pub(super) struct DesktopProfileFilesystemNative;

pub(super) struct DesktopProfileFilesystem;

impl DesktopProfileFilesystem {
    pub(super) fn verify<P: DesktopProfileFilesystemProvider>(
        provider: &P,
        file: &File,
        path: &Path,
    ) -> Result<(), DesktopProfileError> {
        match provider.classify(file, path)? {
            DesktopProfileFilesystemEnumClass::Local => Ok(()),
            DesktopProfileFilesystemEnumClass::Nonlocal => {
                Err(DesktopProfileError::NonlocalFilesystem {
                    path: path.to_path_buf(),
                })
            }
            DesktopProfileFilesystemEnumClass::Unsupported => {
                Err(DesktopProfileError::UnsupportedFilesystem {
                    path: path.to_path_buf(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    use crate::desktop::profile::error::DesktopProfileError;
    use crate::desktop::profile::filesystem::{
        DesktopProfileFilesystem, DesktopProfileFilesystemEnumClass,
        DesktopProfileFilesystemProvider,
    };

    enum DesktopProfileFilesystemTestEnumProbe {
        Nonlocal,
        Unsupported,
        Unavailable,
    }

    impl DesktopProfileFilesystemProvider for DesktopProfileFilesystemTestEnumProbe {
        fn classify(
            &self,
            _file: &File,
            path: &Path,
        ) -> Result<DesktopProfileFilesystemEnumClass, DesktopProfileError> {
            match self {
                Self::Nonlocal => Ok(DesktopProfileFilesystemEnumClass::Nonlocal),
                Self::Unsupported => Ok(DesktopProfileFilesystemEnumClass::Unsupported),
                Self::Unavailable => Err(DesktopProfileError::FilesystemQuery {
                    path: path.to_path_buf(),
                    source: io::ErrorKind::PermissionDenied.into(),
                }),
            }
        }
    }

    #[test]
    fn nonlocal_and_unknown_filesystems_never_admit_ownership()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = tempfile::NamedTempFile::new()?;
        assert!(matches!(
            DesktopProfileFilesystem::verify(
                &DesktopProfileFilesystemTestEnumProbe::Nonlocal,
                file.as_file(),
                file.path(),
            ),
            Err(DesktopProfileError::NonlocalFilesystem { .. })
        ));
        assert!(matches!(
            DesktopProfileFilesystem::verify(
                &DesktopProfileFilesystemTestEnumProbe::Unsupported,
                file.as_file(),
                file.path(),
            ),
            Err(DesktopProfileError::UnsupportedFilesystem { .. })
        ));
        Ok(())
    }

    #[test]
    fn classification_failure_preserves_its_os_cause() -> Result<(), Box<dyn std::error::Error>> {
        let file = tempfile::NamedTempFile::new()?;
        let result = DesktopProfileFilesystem::verify(
            &DesktopProfileFilesystemTestEnumProbe::Unavailable,
            file.as_file(),
            file.path(),
        );
        assert!(matches!(
            result,
            Err(DesktopProfileError::FilesystemQuery { source, .. })
                if source.kind() == io::ErrorKind::PermissionDenied
        ));
        Ok(())
    }
}
