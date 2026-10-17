use std::path::Path;
use std::time::Duration;

use diesel::connection::SimpleConnection;
use diesel::{Connection, SqliteConnection};

use crate::storage::document_store::StorageDocumentError;

pub struct StorageConnection {
    diesel: SqliteConnection,
}

impl StorageConnection {
    pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

    pub fn open(path: &Path) -> Result<Self, StorageDocumentError> {
        let mut diesel = SqliteConnection::establish(&path.to_string_lossy())?;
        diesel.batch_execute(&Self::policy_script())?;
        Ok(Self { diesel })
    }

    pub fn diesel(&mut self) -> &mut SqliteConnection {
        &mut self.diesel
    }

    fn policy_script() -> String {
        format!(
            "PRAGMA busy_timeout = {}; PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;",
            Self::BUSY_TIMEOUT.as_millis()
        )
    }
}
