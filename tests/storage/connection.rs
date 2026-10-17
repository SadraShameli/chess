use std::path::Path;

use chess::capacity::budget::CapacityBudget;
use chess::storage::connection::StorageConnection;
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use diesel::sql_types::{Integer, Text};
use diesel::{QueryableByName, RunQueryDsl};
use uuid::Uuid;

type StorageConnectionTestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(QueryableByName)]
struct StorageConnectionText {
    #[diesel(sql_type = Text)]
    value: String,
}

#[derive(QueryableByName)]
struct StorageConnectionNumber {
    #[diesel(sql_type = Integer)]
    value: i32,
}

struct StorageConnectionFixture;

impl StorageConnectionFixture {
    const JOURNAL_MODE: &'static str = "wal";
    const SYNCHRONOUS_FULL: i32 = 2;
    const BUSY_TIMEOUT_MILLISECONDS: i32 = 5000;

    fn path(directory: &tempfile::TempDir) -> std::path::PathBuf {
        directory.path().join("policy.sqlite3")
    }

    fn diesel_text(
        connection: &mut StorageConnection,
        pragma: &str,
    ) -> Result<String, diesel::result::Error> {
        diesel::sql_query(format!("SELECT {pragma} AS value FROM pragma_{pragma}"))
            .get_result::<StorageConnectionText>(connection.diesel())
            .map(|row| row.value)
    }

    fn diesel_number(
        connection: &mut StorageConnection,
        pragma: &str,
        column: &str,
    ) -> Result<i32, diesel::result::Error> {
        diesel::sql_query(format!("SELECT {column} AS value FROM pragma_{pragma}"))
            .get_result::<StorageConnectionNumber>(connection.diesel())
            .map(|row| row.value)
    }

    fn revision(path: &Path, id: Uuid) -> Result<Option<String>, rusqlite::Error> {
        let connection = rusqlite::Connection::open(path)?;
        match connection.query_row(
            "SELECT revision FROM documents WHERE id = ?1",
            [id.to_string()],
            |row| row.get(0),
        ) {
            Ok(revision) => Ok(Some(revision)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[test]
fn diesel_connections_apply_the_storage_policy() -> StorageConnectionTestResult {
    let directory = tempfile::tempdir()?;
    let mut connection = StorageConnection::open(&StorageConnectionFixture::path(&directory))?;
    assert_eq!(
        StorageConnectionFixture::diesel_text(&mut connection, "journal_mode")?,
        StorageConnectionFixture::JOURNAL_MODE
    );
    assert_eq!(
        StorageConnectionFixture::diesel_number(&mut connection, "foreign_keys", "foreign_keys")?,
        1
    );
    assert_eq!(
        StorageConnectionFixture::diesel_number(&mut connection, "synchronous", "synchronous")?,
        StorageConnectionFixture::SYNCHRONOUS_FULL
    );
    assert_eq!(
        StorageConnectionFixture::diesel_number(&mut connection, "busy_timeout", "timeout")?,
        StorageConnectionFixture::BUSY_TIMEOUT_MILLISECONDS
    );
    assert_eq!(
        StorageConnection::BUSY_TIMEOUT.as_millis(),
        u128::try_from(StorageConnectionFixture::BUSY_TIMEOUT_MILLISECONDS)?
    );
    Ok(())
}

#[test]
fn repository_connections_apply_the_same_storage_policy() -> StorageConnectionTestResult {
    let directory = tempfile::tempdir()?;
    let connection = StorageConnection::open_legacy(&StorageConnectionFixture::path(&directory))?;
    let journal: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    let foreign_keys: i32 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    let synchronous: i32 = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    let busy: i32 = connection.pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
    assert_eq!(journal, StorageConnectionFixture::JOURNAL_MODE);
    assert_eq!(foreign_keys, 1);
    assert_eq!(synchronous, StorageConnectionFixture::SYNCHRONOUS_FULL);
    assert_eq!(busy, StorageConnectionFixture::BUSY_TIMEOUT_MILLISECONDS);
    Ok(())
}

#[tokio::test]
async fn the_storage_actor_leaves_a_write_ahead_log_database() -> StorageConnectionTestResult {
    let directory = tempfile::tempdir()?;
    let path = StorageConnectionFixture::path(&directory);
    StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?)
        .await?
        .close()
        .await?;
    let connection = rusqlite::Connection::open(path)?;
    let journal: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    assert_eq!(journal, StorageConnectionFixture::JOURNAL_MODE);
    Ok(())
}

#[tokio::test]
async fn a_save_receipt_is_visible_to_another_connection() -> StorageConnectionTestResult {
    let directory = tempfile::tempdir()?;
    let path = StorageConnectionFixture::path(&directory);
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let id = Uuid::from_u128(7);
    assert_eq!(StorageConnectionFixture::revision(&path, id)?, None);
    let receipt = store.client().save(id, 0, "{}".into()).await?;
    assert_eq!(receipt.revision(), 1);
    assert_eq!(
        StorageConnectionFixture::revision(&path, id)?,
        Some("1".to_owned())
    );
    let conflict = store.client().save(id, 0, "{}".into()).await;
    assert!(matches!(
        conflict,
        Err(StorageDocumentError::Conflict {
            expected: 0,
            actual: 1
        })
    ));
    assert_eq!(
        StorageConnectionFixture::revision(&path, id)?,
        Some("1".to_owned())
    );
    store.close().await?;
    Ok(())
}

#[test]
fn a_diesel_transaction_is_invisible_to_readers_until_it_commits() -> StorageConnectionTestResult {
    let directory = tempfile::tempdir()?;
    let path = StorageConnectionFixture::path(&directory);
    let mut writer = StorageConnection::open(&path)?;
    diesel::sql_query("CREATE TABLE receipts (id INTEGER PRIMARY KEY)").execute(writer.diesel())?;
    let count = |path: &Path| -> Result<i64, rusqlite::Error> {
        rusqlite::Connection::open(path)?
            .query_row("SELECT count(*) FROM receipts", [], |row| row.get(0))
    };
    let during = diesel::Connection::transaction::<i64, diesel::result::Error, _>(
        writer.diesel(),
        |transaction| {
            diesel::sql_query("INSERT INTO receipts VALUES (1)").execute(transaction)?;
            count(&path).map_err(|_error| diesel::result::Error::RollbackTransaction)
        },
    )?;
    assert_eq!(during, 0);
    assert_eq!(count(&path)?, 1);
    Ok(())
}
