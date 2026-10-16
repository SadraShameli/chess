use rusqlite::Connection;

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
        ]
    }

    pub(super) fn apply(
        connection: &mut Connection,
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
        let current: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let latest = u32::try_from(migrations.len())?;
        if current > latest {
            return Err(StorageDocumentError::FutureSchema {
                found: current,
                supported: latest,
            });
        }
        let transaction = connection.transaction()?;
        for migration in migrations
            .iter()
            .filter(|migration| migration.version > current)
        {
            transaction.execute_batch(&migration.sql)?;
            transaction.pragma_update(None, "user_version", migration.version)?;
        }
        transaction.commit()?;
        Ok(())
    }
}
