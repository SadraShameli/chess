use std::ffi::OsString;
use std::io::Read;
use std::path::Path;

use diesel::SqliteConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};

mod main_schema {
    diesel::table! {
        main.sqlite_schema (name) {
            #[sql_name = "type"]
            kind -> Text,
            name -> Text,
            tbl_name -> Text,
        }
    }
}

mod temp_schema {
    diesel::table! {
        temp.sqlite_schema (name) {
            #[sql_name = "type"]
            kind -> Text,
            name -> Text,
        }
    }
}

#[derive(QueryableByName)]
struct StorageLibraryImportSpacePragmaInteger {
    #[diesel(sql_type = BigInt)]
    value: i64,
}

#[derive(QueryableByName)]
struct StorageLibraryImportSpacePragmaText {
    #[diesel(sql_type = Text)]
    value: String,
}

use crate::storage::import_session::{StorageImportSessionCancellation, StorageImportSessionError};
use crate::storage::library_import::space::StorageLibraryImportSpace;
use crate::storage::library_repository::StorageLibraryError;

#[derive(Clone, Copy)]
enum StorageLibraryImportSpaceEnumPragma {
    PageSize,
    PageCount,
    AutoVacuum,
    JournalMode,
    Encoding,
    ForeignKeys,
    DeferForeignKeys,
}

impl StorageLibraryImportSpaceEnumPragma {
    const fn query(self) -> &'static str {
        match self {
            Self::PageSize => "SELECT page_size AS value FROM pragma_page_size",
            Self::PageCount => "SELECT page_count AS value FROM pragma_page_count",
            Self::AutoVacuum => "SELECT auto_vacuum AS value FROM pragma_auto_vacuum",
            Self::JournalMode => "SELECT journal_mode AS value FROM pragma_journal_mode",
            Self::Encoding => "SELECT encoding AS value FROM pragma_encoding",
            Self::ForeignKeys => "SELECT foreign_keys AS value FROM pragma_foreign_keys",
            Self::DeferForeignKeys => {
                "SELECT defer_foreign_keys AS value FROM pragma_defer_foreign_keys"
            }
        }
    }
}

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

    fn integer(
        connection: &mut SqliteConnection,
        pragma: StorageLibraryImportSpaceEnumPragma,
    ) -> Result<i64, StorageLibraryError> {
        let row: StorageLibraryImportSpacePragmaInteger =
            diesel::sql_query(pragma.query()).get_result(connection)?;
        Ok(row.value)
    }

    fn text(
        connection: &mut SqliteConnection,
        pragma: StorageLibraryImportSpaceEnumPragma,
    ) -> Result<String, StorageLibraryError> {
        let row: StorageLibraryImportSpacePragmaText =
            diesel::sql_query(pragma.query()).get_result(connection)?;
        Ok(row.value)
    }

    pub(super) fn read(
        connection: &mut SqliteConnection,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<Self, StorageLibraryError> {
        cancellation.check()?;
        let page = u64::try_from(Self::integer(
            connection,
            StorageLibraryImportSpaceEnumPragma::PageSize,
        )?)?;
        cancellation.check()?;
        let pages = u64::try_from(Self::integer(
            connection,
            StorageLibraryImportSpaceEnumPragma::PageCount,
        )?)?;
        cancellation.check()?;
        let auto_vacuum =
            Self::integer(connection, StorageLibraryImportSpaceEnumPragma::AutoVacuum)?;
        cancellation.check()?;
        let journal = Self::text(connection, StorageLibraryImportSpaceEnumPragma::JournalMode)?;
        cancellation.check()?;
        let encoding = Self::text(connection, StorageLibraryImportSpaceEnumPragma::Encoding)?;
        cancellation.check()?;
        let foreign_keys =
            Self::integer(connection, StorageLibraryImportSpaceEnumPragma::ForeignKeys)? != 0;
        cancellation.check()?;
        let deferred_foreign_keys = Self::integer(
            connection,
            StorageLibraryImportSpaceEnumPragma::DeferForeignKeys,
        )? != 0;
        cancellation.check()?;
        let triggers: bool = diesel::select(diesel::dsl::exists(
            main_schema::sqlite_schema::table
                .filter(main_schema::sqlite_schema::kind.eq("trigger"))
                .filter(main_schema::sqlite_schema::tbl_name.eq_any([
                    "documents",
                    "library_entries",
                    "library_state",
                ])),
        ))
        .get_result(connection)?;
        if !(512..=65_536).contains(&page)
            || !page.is_power_of_two()
            || auto_vacuum != 0
            || journal != "wal"
            || encoding != "UTF-8"
            || !foreign_keys
            || deferred_foreign_keys
            || triggers
        {
            return Err(StorageLibraryError::ImportLayout);
        }
        cancellation.check()?;
        let path: String =
            diesel::sql_query("SELECT file AS value FROM pragma_database_list WHERE name = 'main'")
                .get_result::<StorageLibraryImportSpacePragmaText>(connection)?
                .value;
        if path.is_empty() {
            return Err(StorageLibraryError::ImportLayout);
        }
        let path = path.as_str();
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
        cancellation.check()?;
        let mut wal_path = OsString::from(path);
        wal_path.push("-wal");
        let wal = Self::length(Path::new(&wal_path))?;
        cancellation.check()?;
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
        cancellation.check()?;
        let has_temporary_triggers: bool = diesel::select(diesel::dsl::exists(
            temp_schema::sqlite_schema::table
                .filter(temp_schema::sqlite_schema::kind.eq("trigger")),
        ))
        .get_result(connection)?;
        cancellation.check()?;
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
