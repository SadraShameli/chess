use std::path::Path;

use chess::capacity::budget::CapacityBudget;
use chess::storage::connection::StorageConnection;
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use chess::storage::migration::StorageMigrationRunner;
use diesel::RunQueryDsl;
use diesel_migrations::FileBasedMigrations;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

type StorageMigrationContinuityResult = Result<(), Box<dyn std::error::Error>>;

const BASELINE_VERSIONS: [&str; 1] = ["0001"];

struct StorageMigrationContinuityFixture;

impl StorageMigrationContinuityFixture {
    fn open(path: &Path) -> Result<Connection, rusqlite::Error> {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
    }

    fn user_version(connection: &Connection) -> Result<u32, rusqlite::Error> {
        connection.pragma_query_value(None, "user_version", |row| row.get(0))
    }

    fn applied(connection: &Connection) -> Result<Vec<String>, rusqlite::Error> {
        let mut statement = connection
            .prepare("SELECT version FROM __diesel_schema_migrations ORDER BY version")?;
        let versions = statement.query_map([], |row| row.get(0))?;
        versions.collect()
    }

    fn has_migration_table(connection: &Connection) -> Result<bool, rusqlite::Error> {
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = '__diesel_schema_migrations')",
            [],
            |row| row.get(0),
        )
    }

    fn user_tables(connection: &Connection) -> Result<Vec<String>, rusqlite::Error> {
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '__diesel_schema_migrations' ORDER BY name",
        )?;
        let names = statement.query_map([], |row| row.get(0))?;
        names.collect()
    }

    fn rows(connection: &Connection, table: &str) -> Result<Vec<String>, rusqlite::Error> {
        let mut statement = connection.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
        let columns = statement.column_count();
        let mut rows = statement.query([])?;
        let mut dump = Vec::new();
        while let Some(row) = rows.next()? {
            let mut cells = Vec::with_capacity(columns);
            for column in 0..columns {
                cells.push(match row.get_ref(column)? {
                    ValueRef::Null => "NULL".to_owned(),
                    ValueRef::Integer(value) => value.to_string(),
                    ValueRef::Real(value) => value.to_string(),
                    ValueRef::Text(value) => format!("t:{}", String::from_utf8_lossy(value)),
                    ValueRef::Blob(value) => format!("b:{value:?}"),
                });
            }
            dump.push(cells.join("|"));
        }
        Ok(dump)
    }

    fn data(connection: &Connection) -> Result<Vec<(String, Vec<String>)>, rusqlite::Error> {
        Self::user_tables(connection)?
            .into_iter()
            .map(|table| Self::rows(connection, &table).map(|rows| (table, rows)))
            .collect()
    }

    fn schema(connection: &Connection) -> Result<Vec<String>, rusqlite::Error> {
        let mut statement = connection.prepare(
            "SELECT type || ':' || name || ':' || coalesce(sql, '') FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )?;
        let entries = statement.query_map([], |row| row.get(0))?;
        entries.collect()
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
    let connection = StorageMigrationContinuityFixture::open(&path)?;
    assert_eq!(
        StorageMigrationContinuityFixture::applied(&connection)?,
        BASELINE_VERSIONS,
    );
    let tables = StorageMigrationContinuityFixture::user_tables(&connection)?;
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
    let before =
        StorageMigrationContinuityFixture::data(&StorageMigrationContinuityFixture::open(&path)?)?;
    StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?)
        .await?
        .close()
        .await?;
    let connection = StorageMigrationContinuityFixture::open(&path)?;
    assert_eq!(
        StorageMigrationContinuityFixture::applied(&connection)?,
        BASELINE_VERSIONS,
    );
    assert_eq!(
        StorageMigrationContinuityFixture::data(&connection)?,
        before
    );
    Ok(())
}

#[tokio::test]
async fn a_pre_diesel_database_is_refused_unchanged() -> StorageMigrationContinuityResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("legacy.sqlite3");
    let connection = Connection::open(&path)?;
    connection.execute_batch(
        "CREATE TABLE documents (id TEXT PRIMARY KEY, revision TEXT, body TEXT); INSERT INTO documents VALUES ('a', '1', '{}'); PRAGMA user_version = 4;",
    )?;
    drop(connection);
    let before = StorageMigrationContinuityFixture::open(&path)?;
    let schema = StorageMigrationContinuityFixture::schema(&before)?;
    let data = StorageMigrationContinuityFixture::data(&before)?;
    drop(before);
    let refused = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await;
    assert!(matches!(
        refused,
        Err(StorageDocumentError::UnsupportedLegacyDatabase)
    ));
    let after = StorageMigrationContinuityFixture::open(&path)?;
    assert!(!StorageMigrationContinuityFixture::has_migration_table(
        &after
    )?);
    assert_eq!(StorageMigrationContinuityFixture::user_version(&after)?, 4);
    assert_eq!(StorageMigrationContinuityFixture::schema(&after)?, schema);
    assert_eq!(StorageMigrationContinuityFixture::data(&after)?, data);
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
    Connection::open(&path)?.execute(
        "INSERT INTO __diesel_schema_migrations (version) VALUES ('0099')",
        [],
    )?;
    let before = StorageMigrationContinuityFixture::open(&path)?;
    let schema = StorageMigrationContinuityFixture::schema(&before)?;
    let data = StorageMigrationContinuityFixture::data(&before)?;
    let applied = StorageMigrationContinuityFixture::applied(&before)?;
    drop(before);
    let refused = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await;
    assert!(matches!(
        refused,
        Err(StorageDocumentError::FutureMigration { .. })
    ));
    let after = StorageMigrationContinuityFixture::open(&path)?;
    assert_eq!(StorageMigrationContinuityFixture::applied(&after)?, applied);
    assert_eq!(StorageMigrationContinuityFixture::schema(&after)?, schema);
    assert_eq!(StorageMigrationContinuityFixture::data(&after)?, data);
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
    let after = StorageMigrationContinuityFixture::open(&path)?;
    assert_eq!(
        StorageMigrationContinuityFixture::applied(&after)?,
        ["0001"]
    );
    assert_eq!(
        StorageMigrationContinuityFixture::user_tables(&after)?,
        ["kept"]
    );
    Ok(())
}
