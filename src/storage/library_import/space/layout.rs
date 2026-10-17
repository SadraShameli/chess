use std::ffi::OsString;
use std::io::Read;
use std::path::Path;

use diesel::RunQueryDsl;
use diesel::dsl::sql;
use diesel::sql_types::Bool;
use diesel::sqlite::SqliteConnection;

use crate::storage::database::StorageDatabase;
use crate::storage::database::enums::{
    StorageDatabaseEnumIntegerProperty, StorageDatabaseEnumTextProperty,
};

use crate::storage::import_session::StorageImportSessionError;
use crate::storage::library_import::space::StorageLibraryImportSpace;
use crate::storage::library_repository::StorageLibraryError;

pub(super) struct StorageLibraryImportSpaceLayout {
    pub(super) page: u64,
    pub(super) pages: u64,
    pub(super) main: u64,
    pub(super) wal: u64,
    pub(super) shm: u64,
    pub(super) wal_frames: u64,
    pub(super) has_temporary_triggers: bool,
}

impl StorageLibraryImportSpaceLayout {
    fn length(path: &Path) -> Result<u64, StorageLibraryError> {
        match std::fs::metadata(path) {
            Ok(metadata) => Ok(metadata.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(StorageImportSessionError::from(error).into()),
        }
    }

    pub(super) fn read(transaction: &mut SqliteConnection) -> Result<Self, StorageLibraryError> {
        let page = u64::try_from(StorageDatabase::integer(
            transaction,
            StorageDatabaseEnumIntegerProperty::PageSize,
        )?)?;
        let pages = u64::try_from(StorageDatabase::integer(
            transaction,
            StorageDatabaseEnumIntegerProperty::PageCount,
        )?)?;
        let auto_vacuum =
            StorageDatabase::integer(transaction, StorageDatabaseEnumIntegerProperty::AutoVacuum)?;
        let journal =
            StorageDatabase::text(transaction, StorageDatabaseEnumTextProperty::JournalMode)?;
        let encoding =
            StorageDatabase::text(transaction, StorageDatabaseEnumTextProperty::Encoding)?;
        let triggers: bool = diesel::select(sql::<Bool>("EXISTS(SELECT 1 FROM main.sqlite_schema WHERE type = 'trigger' AND tbl_name IN ('documents', 'library_entries', 'library_state'))")).get_result(transaction)?;
        if !(512..=65_536).contains(&page)
            || !page.is_power_of_two()
            || auto_vacuum != 0
            || journal != "wal"
            || encoding != "UTF-8"
            || triggers
        {
            return Err(StorageLibraryError::ImportLayout);
        }
        let path = StorageDatabase::text(transaction, StorageDatabaseEnumTextProperty::MainPath)?;
        if path.is_empty() {
            return Err(StorageLibraryError::ImportLayout);
        }
        let mut file = std::fs::File::open(&path).map_err(StorageImportSessionError::from)?;
        let main = file
            .metadata()
            .map_err(StorageImportSessionError::from)?
            .len();
        let mut header = [0; 100];
        file.read_exact(&mut header)
            .map_err(StorageImportSessionError::from)?;
        if header[20] != 0 {
            return Err(StorageLibraryError::ImportLayout);
        }
        let mut wal_path = OsString::from(&path);
        wal_path.push("-wal");
        let wal = Self::length(Path::new(&wal_path))?;
        let mut shm_path = OsString::from(&path);
        shm_path.push("-shm");
        let shm = Self::length(Path::new(&shm_path))?;
        if wal != 0 {
            let mut file =
                std::fs::File::open(&wal_path).map_err(StorageImportSessionError::from)?;
            let mut header = [0; 32];
            file.read_exact(&mut header)
                .map_err(StorageImportSessionError::from)?;
            let magic = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
            let wal_page = u64::from(u32::from_be_bytes([
                header[8], header[9], header[10], header[11],
            ]));
            if !matches!(magic, 0x377f_0682 | 0x377f_0683) || wal_page != page {
                return Err(StorageLibraryError::ImportLayout);
            }
        }
        let wal_frames = StorageLibraryImportSpace::ceil(
            wal.saturating_sub(StorageLibraryImportSpace::WAL_HEADER_BYTES),
            StorageLibraryImportSpace::add(
                page,
                StorageLibraryImportSpace::WAL_FRAME_HEADER_BYTES,
            )?,
        )?;
        let has_temporary_triggers: bool = diesel::select(sql::<Bool>(
            "EXISTS(SELECT 1 FROM temp.sqlite_schema WHERE type = 'trigger')",
        ))
        .get_result(transaction)?;
        Ok(Self {
            page,
            pages,
            main,
            wal,
            shm,
            wal_frames,
            has_temporary_triggers,
        })
    }
}
