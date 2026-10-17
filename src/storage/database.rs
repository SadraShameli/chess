pub(crate) mod enums;

use std::path::Path;
use std::time::Duration;

use diesel::connection::{AnsiTransactionManager, SimpleConnection, TransactionManager};
use diesel::dsl::sql;
use diesel::sql_types::{BigInt, Bool, Text};
use diesel::sqlite::SqliteConnection;
use diesel::{Connection, RunQueryDsl};
use thiserror::Error;
use url::Url;

use crate::storage::database::enums::{
    StorageDatabaseEnumIntegerProperty, StorageDatabaseEnumTextProperty,
};

#[derive(Debug, Error)]
pub enum StorageDatabaseError {
    #[error("The local database connection could not be opened: {0}")]
    Connection(#[from] diesel::ConnectionError),
    #[error("The local database configuration failed: {0}")]
    Query(#[from] diesel::result::Error),
    #[error("The local database path could not be resolved: {0}")]
    Path(#[from] std::io::Error),
    #[error("The local database path cannot be represented as a SQLite file URI")]
    Location,
    #[error("The local database path contains a null byte")]
    NullByte,
}

pub(crate) struct StorageDatabase;

impl StorageDatabase {
    const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

    pub(crate) fn open(path: &Path) -> Result<SqliteConnection, StorageDatabaseError> {
        if path.as_os_str().as_encoded_bytes().contains(&0) {
            return Err(StorageDatabaseError::NullByte);
        }
        let location = if path == Path::new(":memory:") || path.as_os_str().is_empty() {
            path.to_str()
                .ok_or(StorageDatabaseError::Location)?
                .to_owned()
        } else {
            Url::from_file_path(std::path::absolute(path)?)
                .map_err(|()| StorageDatabaseError::Location)?
                .to_string()
        };
        let mut connection = SqliteConnection::establish(&location)?;
        connection.batch_execute(&format!(
            "PRAGMA busy_timeout = {}; PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;",
            Self::BUSY_TIMEOUT.as_millis()
        ))?;
        Ok(connection)
    }

    pub(crate) fn is_resolved(connection: &mut SqliteConnection) -> bool {
        !AnsiTransactionManager::is_broken_transaction_manager(connection)
    }

    pub(crate) fn integer(
        connection: &mut SqliteConnection,
        property: StorageDatabaseEnumIntegerProperty,
    ) -> diesel::QueryResult<i64> {
        diesel::select(sql::<BigInt>(property.expression())).get_result(connection)
    }

    pub(crate) fn text(
        connection: &mut SqliteConnection,
        property: StorageDatabaseEnumTextProperty,
    ) -> diesel::QueryResult<String> {
        diesel::select(sql::<Text>(property.expression())).get_result(connection)
    }

    pub(crate) fn has_library(connection: &mut SqliteConnection) -> diesel::QueryResult<bool> {
        diesel::select(sql::<Bool>(
            "EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name IN ('library_entries', 'library_state'))",
        ))
        .get_result(connection)
    }

    pub(crate) fn set_max_page_count(
        connection: &mut SqliteConnection,
        requested: u64,
    ) -> diesel::QueryResult<u64> {
        connection.batch_execute(&format!("PRAGMA main.max_page_count = {requested}"))?;
        let actual = Self::integer(connection, StorageDatabaseEnumIntegerProperty::MaxPageCount)?;
        u64::try_from(actual)
            .map_err(|error| diesel::result::Error::DeserializationError(Box::new(error)))
    }
}
