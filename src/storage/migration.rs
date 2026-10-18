use diesel::migration::{Migration, MigrationSource};
use diesel::sql_types::Bool;
use diesel::sqlite::Sqlite;
use diesel::{QueryableByName, RunQueryDsl, SqliteConnection};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

use crate::storage::document_store::StorageDocumentError;

#[derive(QueryableByName)]
struct StorageMigrationTablePresence {
    #[diesel(sql_type = Bool)]
    present: bool,
}

pub struct StorageMigrationRunner;

impl StorageMigrationRunner {
    const EMBEDDED: EmbeddedMigrations = embed_migrations!("src/storage/migrations");

    pub fn run_embedded(connection: &mut SqliteConnection) -> Result<(), StorageDocumentError> {
        Self::run(connection, &Self::EMBEDDED)
    }

    pub fn embedded_versions() -> Result<Vec<String>, StorageDocumentError> {
        let mut versions: Vec<String> = MigrationSource::<Sqlite>::migrations(&Self::EMBEDDED)
            .map_err(StorageDocumentError::Migration)?
            .iter()
            .map(|migration| migration.name().version().to_string())
            .collect();
        versions.sort();
        Ok(versions)
    }

    pub fn run<S: MigrationSource<Sqlite>>(
        connection: &mut SqliteConnection,
        source: &S,
    ) -> Result<(), StorageDocumentError> {
        connection.immediate_transaction(|transaction| {
            let migrations = source
                .migrations()
                .map_err(StorageDocumentError::Migration)?;
            if !Self::has_migration_table(transaction)? && Self::has_user_tables(transaction)? {
                return Err(StorageDocumentError::UnmanagedDatabase);
            }
            let applied = transaction
                .applied_migrations()
                .map_err(StorageDocumentError::Migration)?;
            for version in &applied {
                if !migrations
                    .iter()
                    .any(|migration| migration.name().version() == *version)
                {
                    return Err(StorageDocumentError::FutureMigration {
                        version: version.to_string(),
                    });
                }
            }
            let pending: Vec<Box<dyn Migration<Sqlite>>> = migrations
                .into_iter()
                .filter(|migration| !applied.contains(&migration.name().version()))
                .collect();
            transaction
                .run_migrations(&pending)
                .map_err(StorageDocumentError::Migration)?;
            Ok(())
        })
    }

    fn has_user_tables(connection: &mut SqliteConnection) -> Result<bool, StorageDocumentError> {
        let presence = diesel::sql_query(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%') AS present",
        )
        .get_result::<StorageMigrationTablePresence>(connection)?;
        Ok(presence.present)
    }

    fn has_migration_table(
        connection: &mut SqliteConnection,
    ) -> Result<bool, StorageDocumentError> {
        let presence = diesel::sql_query(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = '__diesel_schema_migrations') AS present",
        )
        .get_result::<StorageMigrationTablePresence>(connection)?;
        Ok(presence.present)
    }
}
