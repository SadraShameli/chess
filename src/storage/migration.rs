use diesel::Connection;
use diesel::connection::SimpleConnection;
use diesel::sqlite::SqliteConnection;

use crate::storage::database::StorageDatabase;
use crate::storage::database::enums::StorageDatabaseEnumIntegerProperty;
use crate::storage::document_store::StorageDocumentError;

pub struct StorageMigration {
    version: u32,
    sql: String,
}

impl StorageMigration {
    pub fn new(version: u32, sql: String) -> Self {
        Self { version, sql }
    }

    pub fn core() -> Vec<Self> {
        vec![
            Self::new(1, include_str!("migrations/0001_core.sql").into()),
            Self::new(2, include_str!("migrations/0002_puzzles.sql").into()),
            Self::new(3, include_str!("migrations/0003_library.sql").into()),
        ]
    }

    pub(super) fn apply(
        connection: &mut SqliteConnection,
        migrations: &[Self],
    ) -> Result<(), StorageDocumentError> {
        if migrations.is_empty() {
            return Err(StorageDocumentError::InvalidMigrations);
        }
        for (index, migration) in migrations.iter().enumerate() {
            if usize::try_from(migration.version)? != index + 1 {
                return Err(StorageDocumentError::InvalidMigrations);
            }
        }
        let current = u32::try_from(StorageDatabase::integer(
            connection,
            StorageDatabaseEnumIntegerProperty::UserVersion,
        )?)?;
        let latest = u32::try_from(migrations.len())?;
        if current > latest {
            return Err(StorageDocumentError::FutureSchema {
                found: current,
                supported: latest,
            });
        }
        connection.transaction(|connection| {
            for migration in migrations
                .iter()
                .filter(|migration| migration.version > current)
            {
                connection.batch_execute(&migration.sql)?;
                connection
                    .batch_execute(&format!("PRAGMA user_version = {}", migration.version))?;
            }
            Ok(())
        })
    }
}
