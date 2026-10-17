use rusqlite::{Connection, TransactionBehavior};

use crate::storage::library_transaction::StorageLibraryTransaction;
use crate::storage::library_transaction::content::StorageLibraryTransactionContent;
use crate::storage::migration::StorageMigration;

type StorageLibraryImportJournalResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug)]
struct StorageLibraryImportJournalOpcode {
    name: String,
    first: i64,
    second: i64,
    third: i64,
}

struct StorageLibraryImportJournalProgram(Vec<StorageLibraryImportJournalOpcode>);

impl StorageLibraryImportJournalProgram {
    const ABORT_ACTION: i64 = 2;

    fn compile(connection: &Connection, sql: &str) -> StorageLibraryImportJournalResult<Self> {
        let mut statement = connection.prepare(&format!("EXPLAIN {sql}"))?;
        let mut rows = statement.raw_query();
        let mut opcodes = Vec::new();
        while let Some(row) = rows.next()? {
            opcodes.push(StorageLibraryImportJournalOpcode {
                name: row.get(1)?,
                first: row.get(2)?,
                second: row.get(3)?,
                third: row.get(4)?,
            });
        }
        assert!(!opcodes.is_empty());
        let program = Self(opcodes);
        eprintln!(
            "canonical import SQL: {sql}\noperations={}, direct_fk_halts={}, trigger={}, fk_counter={}, ephemeral={}",
            program.0.len(),
            program.direct_foreign_key_halts(),
            program.has("Program"),
            program.has("FkCounter"),
            program.has("OpenEphemeral"),
        );
        Ok(program)
    }

    fn has(&self, name: &str) -> bool {
        self.0.iter().any(|opcode| opcode.name == name)
    }

    fn direct_foreign_key_halts(&self) -> usize {
        self.0
            .iter()
            .filter(|opcode| {
                opcode.name == "Halt"
                    && opcode.first == i64::from(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY)
                    && opcode.second == Self::ABORT_ACTION
                    && opcode.third == 0
            })
            .count()
    }

    fn assert_single_row_shape(&self) {
        for name in [
            "Program",
            "RowSetAdd",
            "RowSetRead",
            "RowSetTest",
            "SorterOpen",
            "FkCounter",
        ] {
            assert!(!self.has(name), "unexpected {name}: {:?}", self.0);
        }
        assert!(self.has("Transaction"));
        assert!(self.has("Insert"));
    }
}

struct StorageLibraryImportJournalFixture {
    connection: Connection,
    directory: tempfile::TempDir,
}

impl StorageLibraryImportJournalFixture {
    fn new() -> StorageLibraryImportJournalResult<Self> {
        let directory = tempfile::tempdir()?;
        let mut connection = Connection::open(directory.path().join("journal-proof.sqlite"))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "defer_foreign_keys", "OFF")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        StorageMigration::apply(&mut connection, &StorageMigration::core())?;
        assert!(connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, bool>(0))?);
        assert!(
            !connection
                .pragma_query_value(None, "defer_foreign_keys", |row| row.get::<_, bool>(0))?
        );
        let triggers: i64 = connection.query_row(
            "SELECT count(*) FROM main.sqlite_schema WHERE type = 'trigger' AND tbl_name IN ('documents', 'library_entries', 'library_state')",
            [],
            |row| row.get(0),
        )?;
        let temporary_triggers: i64 = connection.query_row(
            "SELECT count(*) FROM temp.sqlite_schema WHERE type = 'trigger'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!((triggers, temporary_triggers), (0, 0));
        let source: (String, String) =
            connection.query_row("SELECT sqlite_version(), sqlite_source_id()", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        eprintln!("canonical import SQLite compiler: {source:?}");
        Ok(Self {
            connection,
            directory,
        })
    }

    fn close(self) -> StorageLibraryImportJournalResult {
        self.connection
            .close()
            .map_err(|(_connection, error)| error)?;
        let path = self.directory.path().to_owned();
        self.directory.close()?;
        assert!(!path.try_exists()?);
        Ok(())
    }
}

#[test]
fn canonical_document_insert_and_entry_upsert_compile_as_single_row_writes()
-> StorageLibraryImportJournalResult {
    let mut fixture = StorageLibraryImportJournalFixture::new()?;
    let transaction = fixture
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let document = StorageLibraryImportJournalProgram::compile(
        &transaction,
        StorageLibraryTransactionContent::INSERT_DOCUMENT_SQL,
    )?;
    document.assert_single_row_shape();
    let entry = StorageLibraryImportJournalProgram::compile(
        &transaction,
        StorageLibraryTransactionContent::PROJECT_SQL,
    )?;
    entry.assert_single_row_shape();
    assert!(entry.direct_foreign_key_halts() >= 2, "{:?}", entry.0);
    transaction.rollback()?;
    fixture.close()
}

#[test]
fn canonical_catalog_update_compiles_as_a_rowid_one_pass_write() -> StorageLibraryImportJournalResult
{
    let mut fixture = StorageLibraryImportJournalFixture::new()?;
    let transaction = fixture
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let catalog = StorageLibraryImportJournalProgram::compile(
        &transaction,
        StorageLibraryTransaction::BUMP_CATALOG_SQL,
    )?;
    catalog.assert_single_row_shape();
    assert!(catalog.has("SeekRowid"), "{:?}", catalog.0);
    assert!(!catalog.has("OpenEphemeral"), "{:?}", catalog.0);
    transaction.rollback()?;
    fixture.close()
}

#[test]
fn a_real_temporary_entry_fault_trigger_changes_the_canonical_upsert_program()
-> StorageLibraryImportJournalResult {
    let mut fixture = StorageLibraryImportJournalFixture::new()?;
    let before = StorageLibraryImportJournalProgram::compile(
        &fixture.connection,
        StorageLibraryTransactionContent::PROJECT_SQL,
    )?;
    before.assert_single_row_shape();
    fixture.connection.execute_batch(
        "CREATE TEMP TRIGGER import_entry_fault BEFORE INSERT ON library_entries BEGIN SELECT RAISE(ABORT, 'entry statement fault'); END",
    )?;
    let transaction = fixture
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let fault = StorageLibraryImportJournalProgram::compile(
        &transaction,
        StorageLibraryTransactionContent::PROJECT_SQL,
    )?;
    assert!(fault.has("Program"), "{:?}", fault.0);
    assert!(fault.has("FkCounter"), "{:?}", fault.0);
    assert_eq!(fault.direct_foreign_key_halts(), 0, "{:?}", fault.0);
    transaction.rollback()?;
    fixture.close()
}
