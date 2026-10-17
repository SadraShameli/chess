use diesel::SqliteConnection;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text};

use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::storage::connection::StorageConnection;
use crate::storage::library_import::space::trees::planning_work_tests::StorageLibraryImportPlanningWorkFixture;
use crate::storage::migration::StorageMigrationRunner;

type StorageLibraryImportJournalResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, QueryableByName)]
struct StorageLibraryImportJournalOpcode {
    #[diesel(sql_type = Text, column_name = "opcode")]
    name: String,
    #[diesel(sql_type = BigInt, column_name = "p1")]
    first: i64,
    #[diesel(sql_type = BigInt, column_name = "p2")]
    second: i64,
    #[diesel(sql_type = BigInt, column_name = "p3")]
    third: i64,
}

#[derive(QueryableByName)]
struct StorageLibraryImportJournalFlag {
    #[diesel(sql_type = Integer)]
    value: i32,
}

#[derive(QueryableByName)]
struct StorageLibraryImportJournalVersion {
    #[diesel(sql_type = Text)]
    version: String,
    #[diesel(sql_type = Text)]
    source: String,
}

struct StorageLibraryImportJournalProgram(Vec<StorageLibraryImportJournalOpcode>);

impl StorageLibraryImportJournalProgram {
    const ABORT_ACTION: i64 = 2;

    fn compile(
        connection: &mut SqliteConnection,
        sql: &str,
    ) -> StorageLibraryImportJournalResult<Self> {
        let opcodes = diesel::sql_query(format!("EXPLAIN {sql}"))
            .load::<StorageLibraryImportJournalOpcode>(connection)?;
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
                    && opcode.first == i64::from(libsqlite3_sys::SQLITE_CONSTRAINT_FOREIGNKEY)
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
    connection: StorageConnection,
    directory: tempfile::TempDir,
}

impl StorageLibraryImportJournalFixture {
    fn new() -> StorageLibraryImportJournalResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("journal-proof.sqlite");
        StorageMigrationRunner::run_embedded(StorageConnection::open(&path)?.diesel())?;
        let mut connection = StorageConnection::open(&path)?;
        let pragma = |connection: &mut SqliteConnection, sql: &str| {
            diesel::sql_query(sql).get_result::<StorageLibraryImportJournalFlag>(connection)
        };
        assert_eq!(
            pragma(
                connection.diesel(),
                "SELECT foreign_keys AS value FROM pragma_foreign_keys"
            )?
            .value,
            1
        );
        assert_eq!(
            pragma(
                connection.diesel(),
                "SELECT defer_foreign_keys AS value FROM pragma_defer_foreign_keys"
            )?
            .value,
            0
        );
        let triggers: i64 = diesel::select(diesel::dsl::sql::<BigInt>(
            "(SELECT count(*) FROM main.sqlite_schema WHERE type = 'trigger' AND tbl_name IN ('documents', 'library_entries', 'library_state'))",
        ))
        .get_result(connection.diesel())?;
        let temporary_triggers: i64 = diesel::select(diesel::dsl::sql::<BigInt>(
            "(SELECT count(*) FROM temp.sqlite_schema WHERE type = 'trigger')",
        ))
        .get_result(connection.diesel())?;
        assert_eq!((triggers, temporary_triggers), (0, 0));
        let source =
            diesel::sql_query("SELECT sqlite_version() AS version, sqlite_source_id() AS source")
                .get_result::<StorageLibraryImportJournalVersion>(connection.diesel())?;
        let source = (source.version, source.source);
        eprintln!("canonical import SQLite compiler: {source:?}");
        Ok(Self {
            connection,
            directory,
        })
    }

    fn close(self) -> StorageLibraryImportJournalResult {
        drop(self.connection);
        let path = self.directory.path().to_owned();
        self.directory.close()?;
        assert!(!path.try_exists()?);
        Ok(())
    }
}

struct StorageLibraryImportJournalStatements {
    document: String,
    entry: String,
    catalog: String,
}

impl StorageLibraryImportJournalStatements {
    async fn capture() -> StorageLibraryImportJournalResult<Self> {
        let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
        let job = executor
            .submit_waiting(|token| {
                let fixture = StorageLibraryImportPlanningWorkFixture::with_records(
                    StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                    1,
                    token,
                )?;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(fixture.statements()?)
            })
            .await?;
        let result = job.join().await;
        executor.shutdown().await?;
        let statements = result?.map_err(|error| error.to_string())?;
        let find = |prefix: &str| -> StorageLibraryImportJournalResult<String> {
            statements
                .iter()
                .find(|sql| sql.starts_with(prefix))
                .cloned()
                .ok_or_else(|| format!("missing production statement: {prefix}").into())
        };
        Ok(Self {
            document: find("INSERT INTO `documents`")?,
            entry: find("INSERT INTO `library_entries`")?,
            catalog: find("UPDATE `library_state`")?,
        })
    }
}

#[tokio::test]
async fn canonical_document_insert_and_entry_upsert_compile_as_single_row_writes()
-> StorageLibraryImportJournalResult {
    let statements = StorageLibraryImportJournalStatements::capture().await?;
    let mut fixture = StorageLibraryImportJournalFixture::new()?;
    fixture
        .connection
        .diesel()
        .immediate_transaction(|transaction| {
            let document =
                StorageLibraryImportJournalProgram::compile(transaction, &statements.document)?;
            document.assert_single_row_shape();
            let entry =
                StorageLibraryImportJournalProgram::compile(transaction, &statements.entry)?;
            entry.assert_single_row_shape();
            assert!(entry.direct_foreign_key_halts() >= 2, "{:?}", entry.0);
            StorageLibraryImportJournalResult::Ok(())
        })?;
    fixture.close()
}

#[tokio::test]
async fn canonical_catalog_update_compiles_as_a_rowid_one_pass_write()
-> StorageLibraryImportJournalResult {
    let statements = StorageLibraryImportJournalStatements::capture().await?;
    let mut fixture = StorageLibraryImportJournalFixture::new()?;
    fixture
        .connection
        .diesel()
        .immediate_transaction(|transaction| {
            let catalog =
                StorageLibraryImportJournalProgram::compile(transaction, &statements.catalog)?;
            catalog.assert_single_row_shape();
            assert!(catalog.has("SeekRowid"), "{:?}", catalog.0);
            assert!(!catalog.has("OpenEphemeral"), "{:?}", catalog.0);
            StorageLibraryImportJournalResult::Ok(())
        })?;
    fixture.close()
}

#[tokio::test]
async fn a_real_temporary_entry_fault_trigger_changes_the_canonical_upsert_program()
-> StorageLibraryImportJournalResult {
    let statements = StorageLibraryImportJournalStatements::capture().await?;
    let mut fixture = StorageLibraryImportJournalFixture::new()?;
    let before = StorageLibraryImportJournalProgram::compile(
        fixture.connection.diesel(),
        &statements.entry,
    )?;
    before.assert_single_row_shape();
    fixture.connection.diesel().batch_execute(
        "CREATE TEMP TRIGGER import_entry_fault BEFORE INSERT ON library_entries BEGIN SELECT RAISE(ABORT, 'entry statement fault'); END",
    )?;
    fixture
        .connection
        .diesel()
        .immediate_transaction(|transaction| {
            let fault =
                StorageLibraryImportJournalProgram::compile(transaction, &statements.entry)?;
            assert!(fault.has("Program"), "{:?}", fault.0);
            assert!(fault.has("FkCounter"), "{:?}", fault.0);
            assert_eq!(fault.direct_foreign_key_halts(), 0, "{:?}", fault.0);
            StorageLibraryImportJournalResult::Ok(())
        })?;
    fixture.close()
}
