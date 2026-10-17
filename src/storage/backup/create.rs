use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use diesel::sql_types::Text;
use diesel::{QueryableByName, RunQueryDsl, SqliteConnection};
use uuid::Uuid;

use crate::package::digest::PackageDigest;
use crate::storage::backup::archive::BackupArchiveFormat;
use crate::storage::backup::error::BackupError;
use crate::storage::backup::manifest::{
    BackupDependency, BackupEnumCategory, BackupEnumPlatform, BackupManifest, BackupManifestParts,
};
use crate::storage::connection::StorageConnection;

pub trait BackupSpaceProvider {
    fn available_bytes(&self, directory: &Path) -> Result<Option<u64>, std::io::Error>;
}

pub struct BackupSpaceNative;

impl BackupSpaceProvider for BackupSpaceNative {
    #[cfg(unix)]
    fn available_bytes(&self, directory: &Path) -> Result<Option<u64>, std::io::Error> {
        let stat = rustix::fs::statvfs(directory)?;
        Ok(Some(stat.f_bavail.saturating_mul(stat.f_frsize)))
    }

    #[cfg(not(unix))]
    fn available_bytes(&self, _directory: &Path) -> Result<Option<u64>, std::io::Error> {
        Ok(None)
    }
}

#[derive(QueryableByName)]
struct BackupCreateRevision {
    #[diesel(sql_type = Text)]
    value: String,
}

pub struct BackupRequest<'a> {
    pub database: &'a Path,
    pub directory: &'a Path,
    pub category: BackupEnumCategory,
    pub id: Uuid,
    pub created_ms: u64,
    pub dependencies: Vec<BackupDependency>,
    pub portable_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupArchive {
    path: PathBuf,
    manifest: BackupManifest,
}

impl BackupArchive {
    pub const fn new(path: PathBuf, manifest: BackupManifest) -> Self {
        Self { path, manifest }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }
}

pub struct BackupCreator;

impl BackupCreator {
    pub const SNAPSHOT_ATTEMPTS: usize = 3;
    const SPACE_MARGIN_BYTES: u64 = 1024 * 1024;

    pub fn file_name(manifest: &BackupManifest) -> String {
        format!(
            "chess-{}-{:020}-{}.{}",
            manifest.category().slug(),
            manifest.created_ms(),
            manifest.id(),
            BackupArchiveFormat::EXTENSION
        )
    }

    pub fn create_blocking(
        request: BackupRequest<'_>,
        space: &impl BackupSpaceProvider,
    ) -> Result<BackupArchive, BackupError> {
        fs::create_dir_all(request.directory)?;
        if !request.directory.is_dir() {
            return Err(BackupError::Destination(request.directory.to_path_buf()));
        }
        Self::check_space(&request, space)?;
        let staging = request.directory.join(format!("{}.staging", request.id));
        let mut connection = StorageConnection::open(request.database)?;
        let revision = Self::snapshot(connection.diesel(), &staging)?;
        let outcome = Self::publish(connection.diesel(), &request, &staging, revision);
        if staging.exists() {
            fs::remove_file(&staging)?;
        }
        outcome
    }

    fn check_space(
        request: &BackupRequest<'_>,
        space: &impl BackupSpaceProvider,
    ) -> Result<(), BackupError> {
        let mut size = fs::metadata(request.database)?.len();
        let wal = PathBuf::from(format!("{}-wal", request.database.display()));
        if let Ok(metadata) = fs::metadata(&wal) {
            size = size.saturating_add(metadata.len());
        }
        let required = size
            .saturating_mul(2)
            .saturating_add(Self::SPACE_MARGIN_BYTES);
        if let Some(available) = space.available_bytes(request.directory)?
            && available < required
        {
            return Err(BackupError::InsufficientSpace {
                required,
                available,
            });
        }
        Ok(())
    }

    fn snapshot(connection: &mut SqliteConnection, staging: &Path) -> Result<String, BackupError> {
        let target = staging
            .to_str()
            .ok_or(BackupError::InvalidArchive("staging path"))?;
        for _attempt in 0..Self::SNAPSHOT_ATTEMPTS {
            if staging.exists() {
                fs::remove_file(staging)?;
            }
            let before = Self::catalog_revision(connection)?;
            diesel::sql_query("VACUUM INTO ?1")
                .bind::<Text, _>(target)
                .execute(connection)?;
            if before == Self::catalog_revision(connection)? {
                return Ok(before);
            }
        }
        Err(BackupError::ConcurrentWrite)
    }

    pub fn catalog_revision(connection: &mut SqliteConnection) -> Result<String, BackupError> {
        Ok(
            diesel::sql_query("SELECT revision AS value FROM library_state WHERE singleton = 1")
                .get_result::<BackupCreateRevision>(connection)?
                .value,
        )
    }

    pub fn schema_versions(connection: &mut SqliteConnection) -> Result<Vec<String>, BackupError> {
        let rows = diesel::sql_query(
            "SELECT version AS value FROM __diesel_schema_migrations ORDER BY version",
        )
        .load::<BackupCreateRevision>(connection)?;
        Ok(rows.into_iter().map(|row| row.value).collect())
    }

    fn publish(
        connection: &mut SqliteConnection,
        request: &BackupRequest<'_>,
        staging: &Path,
        revision: String,
    ) -> Result<BackupArchive, BackupError> {
        let payload_bytes = fs::metadata(staging)?.len();
        let payload_sha256 = PackageDigest::read(File::open(staging)?)?;
        let manifest = BackupManifest::new(BackupManifestParts {
            id: request.id,
            category: request.category,
            created_ms: request.created_ms,
            platform: BackupEnumPlatform::current(),
            schema_versions: Self::schema_versions(connection)?,
            catalog_revision: revision,
            payload_bytes,
            payload_sha256,
            dependencies: request.dependencies.clone(),
            portable_paths: request.portable_paths.clone(),
        })?;
        let final_path = request.directory.join(Self::file_name(&manifest));
        let temporary = request.directory.join(format!("{}.partial", request.id));
        let written = Self::write_archive(&temporary, staging, &manifest);
        if let Err(error) = written {
            if temporary.exists() {
                fs::remove_file(&temporary)?;
            }
            return Err(error);
        }
        fs::rename(&temporary, &final_path)?;
        Self::sync_directory(request.directory)?;
        Ok(BackupArchive::new(final_path, manifest))
    }

    fn write_archive(
        temporary: &Path,
        staging: &Path,
        manifest: &BackupManifest,
    ) -> Result<(), BackupError> {
        let file = File::create(temporary)?;
        let mut writer = BufWriter::new(file);
        BackupArchiveFormat::write_header(&mut writer, manifest)?;
        let mut source = File::open(staging)?;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            writer.write_all(&buffer[..count])?;
        }
        writer.flush()?;
        writer
            .into_inner()
            .map_err(|error| BackupError::Io(error.into_error()))?
            .sync_all()?;
        Ok(())
    }

    #[cfg(unix)]
    fn sync_directory(directory: &Path) -> Result<(), BackupError> {
        File::open(directory)?.sync_all()?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn sync_directory(_directory: &Path) -> Result<(), BackupError> {
        Ok(())
    }
}
