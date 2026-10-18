use std::path::Path;

use crate::storage_sqlite_inspector::{StorageSqliteInspector, StorageSqliteInspectorError};
use chess::capacity::budget::CapacityBudget;
use chess::storage::connection::StorageConnection;
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use chess::storage::migration::StorageMigrationRunner;
use diesel::RunQueryDsl;
use diesel_migrations::FileBasedMigrations;

type StorageMigrationContinuityResult = Result<(), Box<dyn std::error::Error>>;

struct StorageMigrationContinuityFixture;

impl StorageMigrationContinuityFixture {
    fn open(path: &Path) -> Result<StorageSqliteInspector, StorageSqliteInspectorError> {
        StorageSqliteInspector::open(path)
    }

    fn user_version(
        connection: &mut StorageSqliteInspector,
    ) -> Result<i64, StorageSqliteInspectorError> {
        connection.integer("SELECT user_version AS value FROM pragma_user_version")
    }

    fn applied(
        connection: &mut StorageSqliteInspector,
    ) -> Result<Vec<String>, StorageSqliteInspectorError> {
        connection.texts("SELECT version AS value FROM __diesel_schema_migrations ORDER BY version")
    }

    fn has_migration_table(
        connection: &mut StorageSqliteInspector,
    ) -> Result<bool, StorageSqliteInspectorError> {
        Ok(connection.integer(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = '__diesel_schema_migrations') AS value",
        )? == 1)
    }

    fn user_tables(
        connection: &mut StorageSqliteInspector,
    ) -> Result<Vec<String>, StorageSqliteInspectorError> {
        connection.texts(
            "SELECT name AS value FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '__diesel_schema_migrations' ORDER BY name",
        )
    }

    fn rows(
        connection: &mut StorageSqliteInspector,
        table: &str,
    ) -> Result<Vec<String>, StorageSqliteInspectorError> {
        let columns = connection.texts(&format!(
            "SELECT name AS value FROM pragma_table_info('{table}') ORDER BY cid"
        ))?;
        let cells = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        connection.texts(&format!(
            "SELECT {cells} AS value FROM {table} ORDER BY value"
        ))
    }

    fn data(
        connection: &mut StorageSqliteInspector,
    ) -> Result<Vec<(String, Vec<String>)>, StorageSqliteInspectorError> {
        Self::user_tables(connection)?
            .into_iter()
            .map(|table| Self::rows(connection, &table).map(|rows| (table, rows)))
            .collect()
    }

    fn schema(
        connection: &mut StorageSqliteInspector,
    ) -> Result<Vec<String>, StorageSqliteInspectorError> {
        connection.texts(
            "SELECT type || ':' || name || ':' || coalesce(sql, '') AS value FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
    }
}

#[tokio::test]
async fn a_fresh_database_applies_the_baseline() -> StorageMigrationContinuityResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fresh.sqlite3");
    StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?)
        .await?
        .close()
        .await?;
    let mut connection = StorageMigrationContinuityFixture::open(&path)?;
    assert_eq!(
        StorageMigrationContinuityFixture::applied(&mut connection)?,
        StorageMigrationRunner::embedded_versions()?,
    );
    let tables = StorageMigrationContinuityFixture::user_tables(&mut connection)?;
    for expected in [
        "documents",
        "puzzle_runs",
        "library_entries",
        "editor_drafts",
    ] {
        assert!(tables.iter().any(|name| name == expected), "{expected}");
    }
    Ok(())
}

#[tokio::test]
async fn a_reopened_database_keeps_its_rows() -> StorageMigrationContinuityResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("reopen.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    store
        .client()
        .save(uuid::Uuid::from_u128(1), 0, "{}".into())
        .await?;
    store.close().await?;
    let before = StorageMigrationContinuityFixture::data(
        &mut StorageMigrationContinuityFixture::open(&path)?,
    )?;
    StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?)
        .await?
        .close()
        .await?;
    let mut connection = StorageMigrationContinuityFixture::open(&path)?;
    assert_eq!(
        StorageMigrationContinuityFixture::applied(&mut connection)?,
        StorageMigrationRunner::embedded_versions()?,
    );
    assert_eq!(
        StorageMigrationContinuityFixture::data(&mut connection)?,
        before
    );
    Ok(())
}

#[tokio::test]
async fn a_pre_diesel_database_is_refused_unchanged() -> StorageMigrationContinuityResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("unmanaged.sqlite3");
    let mut connection = StorageSqliteInspector::open(&path)?;
    connection.batch(
        "CREATE TABLE documents (id TEXT PRIMARY KEY, revision TEXT, body TEXT); INSERT INTO documents VALUES ('a', '1', '{}'); PRAGMA user_version = 4;",
    )?;
    drop(connection);
    let mut before = StorageMigrationContinuityFixture::open(&path)?;
    let schema = StorageMigrationContinuityFixture::schema(&mut before)?;
    let data = StorageMigrationContinuityFixture::data(&mut before)?;
    drop(before);
    let refused = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await;
    assert!(matches!(
        refused,
        Err(StorageDocumentError::UnmanagedDatabase)
    ));
    let mut after = StorageMigrationContinuityFixture::open(&path)?;
    assert!(!StorageMigrationContinuityFixture::has_migration_table(
        &mut after
    )?);
    assert_eq!(
        StorageMigrationContinuityFixture::user_version(&mut after)?,
        4
    );
    assert_eq!(
        StorageMigrationContinuityFixture::schema(&mut after)?,
        schema
    );
    assert_eq!(StorageMigrationContinuityFixture::data(&mut after)?, data);
    Ok(())
}

#[tokio::test]
async fn an_applied_diesel_version_outside_the_embedded_set_is_rejected_unchanged()
-> StorageMigrationContinuityResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("future.sqlite3");
    StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?)
        .await?
        .close()
        .await?;
    StorageSqliteInspector::open(&path)?
        .execute("INSERT INTO __diesel_schema_migrations (version) VALUES ('0099')")?;
    let mut before = StorageMigrationContinuityFixture::open(&path)?;
    let schema = StorageMigrationContinuityFixture::schema(&mut before)?;
    let data = StorageMigrationContinuityFixture::data(&mut before)?;
    let applied = StorageMigrationContinuityFixture::applied(&mut before)?;
    drop(before);
    let refused = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await;
    assert!(matches!(
        refused,
        Err(StorageDocumentError::FutureMigration { .. })
    ));
    let mut after = StorageMigrationContinuityFixture::open(&path)?;
    assert_eq!(
        StorageMigrationContinuityFixture::applied(&mut after)?,
        applied
    );
    assert_eq!(
        StorageMigrationContinuityFixture::schema(&mut after)?,
        schema
    );
    assert_eq!(StorageMigrationContinuityFixture::data(&mut after)?, data);
    Ok(())
}

#[test]
fn a_failing_later_migration_leaves_the_prior_version_intact() -> StorageMigrationContinuityResult {
    let directory = tempfile::tempdir()?;
    let migrations = directory.path().join("migrations");
    std::fs::create_dir_all(migrations.join("0001_first"))?;
    std::fs::write(
        migrations.join("0001_first/up.sql"),
        "CREATE TABLE kept (id INTEGER PRIMARY KEY); INSERT INTO kept VALUES (1);",
    )?;
    let path = directory.path().join("rollback.sqlite3");
    let mut connection = StorageConnection::open(&path)?;
    let source = FileBasedMigrations::from_path(&migrations)?;
    StorageMigrationRunner::run(connection.diesel(), &source)?;
    std::fs::create_dir_all(migrations.join("0002_second"))?;
    std::fs::write(
        migrations.join("0002_second/up.sql"),
        "CREATE TABLE doomed (id INTEGER); THIS IS INVALID SQL;",
    )?;
    let source = FileBasedMigrations::from_path(&migrations)?;
    assert!(StorageMigrationRunner::run(connection.diesel(), &source).is_err());
    diesel::sql_query("SELECT 1").execute(connection.diesel())?;
    drop(connection);
    let mut after = StorageMigrationContinuityFixture::open(&path)?;
    assert_eq!(
        StorageMigrationContinuityFixture::applied(&mut after)?,
        ["0001"]
    );
    assert_eq!(
        StorageMigrationContinuityFixture::user_tables(&mut after)?,
        ["kept"]
    );
    Ok(())
}
