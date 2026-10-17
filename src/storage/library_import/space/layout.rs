use std::ffi::OsString;
use std::io::Read;
use std::path::Path;

use rusqlite::Transaction;

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

    pub(super) fn read(transaction: &Transaction<'_>) -> Result<Self, StorageLibraryError> {
        let page = u64::try_from(
            transaction.pragma_query_value(None, "page_size", |row| row.get::<_, i64>(0))?,
        )?;
        let pages = u64::try_from(
            transaction.pragma_query_value(None, "page_count", |row| row.get::<_, i64>(0))?,
        )?;
        let auto_vacuum: u32 =
            transaction.pragma_query_value(None, "auto_vacuum", |row| row.get(0))?;
        let journal: String =
            transaction.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        let encoding: String =
            transaction.pragma_query_value(None, "encoding", |row| row.get(0))?;
        let triggers: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_schema WHERE type = 'trigger' AND tbl_name IN ('documents', 'library_entries', 'library_state'))",
            [],
            |row| row.get(0),
        )?;
        if !(512..=65_536).contains(&page)
            || !page.is_power_of_two()
            || auto_vacuum != 0
            || journal != "wal"
            || encoding != "UTF-8"
            || triggers
        {
            return Err(StorageLibraryError::ImportLayout);
        }
        let path = transaction
            .path()
            .filter(|path| !path.is_empty())
            .ok_or(StorageLibraryError::ImportLayout)?;
        let mut file = std::fs::File::open(path).map_err(StorageImportSessionError::from)?;
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
        let mut wal_path = OsString::from(path);
        wal_path.push("-wal");
        let wal = Self::length(Path::new(&wal_path))?;
        let mut shm_path = OsString::from(path);
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
        let has_temporary_triggers: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM temp.sqlite_schema WHERE type = 'trigger')",
            [],
            |row| row.get(0),
        )?;
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
