use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::MetadataExt;

use crate::storage::import_session::filesystem::{
    StorageImportSessionFilesystem, StorageImportSessionFilesystemOrphan,
    StorageImportSessionUnixSpace,
};
use crate::storage::import_session::{
    StorageImportSessionError, StorageImportSessionFilesystemProvider,
};

struct StorageImportSessionFilesystemReviewFixture {
    directory: tempfile::TempDir,
    filesystem: StorageImportSessionFilesystem<StorageImportSessionUnixSpace>,
    original: File,
}

impl StorageImportSessionFilesystemReviewFixture {
    const RETAINED: &'static str = "retained-source";
    const RENAMED: &'static str = "renamed-source";
    const ORIGINAL: &'static [u8] = b"owned";
    const REPLACEMENT: &'static [u8] = b"other";

    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let anchor = File::open(directory.path())?;
        let filesystem = StorageImportSessionFilesystem::new(
            anchor.try_clone()?,
            anchor,
            StorageImportSessionUnixSpace,
        );
        let mut original = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(directory.path().join(Self::RETAINED))?;
        original.write_all(Self::ORIGINAL)?;
        let retained = original.try_clone()?;
        filesystem
            .orphan
            .lock()
            .map_err(|_error| StorageImportSessionError::Poisoned)?
            .push(StorageImportSessionFilesystemOrphan {
                file: retained,
                name: Self::RETAINED.to_owned(),
            });
        Ok(Self {
            directory,
            filesystem,
            original,
        })
    }

    fn replace_basename(&self) -> Result<(u64, u64), Box<dyn std::error::Error>> {
        std::fs::rename(
            self.directory.path().join(Self::RETAINED),
            self.directory.path().join(Self::RENAMED),
        )?;
        let mut replacement = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(self.directory.path().join(Self::RETAINED))?;
        replacement.write_all(Self::REPLACEMENT)?;
        let metadata = replacement.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    fn identity(&self, name: &str) -> std::io::Result<(u64, u64)> {
        let metadata = std::fs::metadata(self.directory.path().join(name))?;
        Ok((metadata.dev(), metadata.ino()))
    }

    fn close(self) -> Result<(), Box<dyn std::error::Error>> {
        let Self {
            directory,
            filesystem,
            original,
        } = self;
        let pending = {
            let mut pending = filesystem
                .orphan
                .lock()
                .map_err(|_error| StorageImportSessionError::Poisoned)?;
            std::mem::take(&mut *pending)
        };
        drop(pending);
        drop(filesystem);
        drop(original);
        directory.close()?;
        Ok(())
    }
}

#[test]
fn review_unix_orphan_cleanup_preserves_replacement_at_reused_basename()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionFilesystemReviewFixture::new()?;
    let original = fixture.original.metadata()?;
    let original_identity = (original.dev(), original.ino());
    let replacement_identity = fixture.replace_basename()?;
    let release = fixture.filesystem.cleanup_retained();
    let replacement_bytes = std::fs::read(
        fixture
            .directory
            .path()
            .join(StorageImportSessionFilesystemReviewFixture::RETAINED),
    );
    let replacement_after = fixture.identity(StorageImportSessionFilesystemReviewFixture::RETAINED);
    let original_bytes = std::fs::read(
        fixture
            .directory
            .path()
            .join(StorageImportSessionFilesystemReviewFixture::RENAMED),
    );
    let original_after = fixture.identity(StorageImportSessionFilesystemReviewFixture::RENAMED);
    fixture.close()?;
    assert_ne!(original_identity, replacement_identity);
    assert!(
        replacement_bytes
            .as_deref()
            .is_ok_and(|bytes| bytes == StorageImportSessionFilesystemReviewFixture::REPLACEMENT),
        "{release:?}; {replacement_bytes:?}"
    );
    assert!(
        replacement_after
            .as_ref()
            .is_ok_and(|identity| *identity == replacement_identity),
        "{replacement_after:?}"
    );
    assert!(
        original_bytes
            .as_deref()
            .is_ok_and(|bytes| bytes == StorageImportSessionFilesystemReviewFixture::ORIGINAL),
        "{original_bytes:?}"
    );
    assert!(
        original_after
            .as_ref()
            .is_ok_and(|identity| *identity == original_identity),
        "{original_after:?}"
    );
    Ok(())
}

#[test]
fn review_unix_orphan_cleanup_releases_its_original_entry() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = StorageImportSessionFilesystemReviewFixture::new()?;
    let release = fixture.filesystem.cleanup_retained();
    let path = std::fs::symlink_metadata(
        fixture
            .directory
            .path()
            .join(StorageImportSessionFilesystemReviewFixture::RETAINED),
    );
    let is_empty = fixture
        .filesystem
        .orphan
        .lock()
        .map_err(|_error| StorageImportSessionError::Poisoned)?
        .is_empty();
    fixture.close()?;
    release?;
    assert!(matches!(path, Err(error) if error.kind() == std::io::ErrorKind::NotFound));
    assert!(is_empty);
    Ok(())
}
