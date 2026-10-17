use rusqlite::{Connection, TransactionBehavior};

use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::storage::import_session::{
    StorageImportSessionCancellation, StorageImportSessionCommitFootprint,
};
use crate::storage::library_import::StorageLibraryImportPageLimit;
use crate::storage::library_import::space::StorageLibraryImportSpace;
use crate::storage::library_import::space::trees::planning_work_tests::{
    StorageLibraryImportPlanningWorkFixture, StorageLibraryImportPlanningWorkResult,
};
use crate::storage::library_repository::StorageLibraryError;

enum StorageLibraryImportSpaceBudgetEnumPragma {
    Ceiling,
    Pages,
}

impl StorageLibraryImportSpaceBudgetEnumPragma {
    fn read(self, connection: &Connection) -> StorageLibraryImportPlanningWorkResult<u64> {
        let name = match self {
            Self::Ceiling => "max_page_count",
            Self::Pages => "page_count",
        };
        Ok(u64::try_from(connection.pragma_query_value(
            None,
            name,
            |row| row.get::<_, i64>(0),
        )?)?)
    }
}

#[tokio::test]
async fn incoming_page_budget_has_no_exact_cardinality_cliff()
-> StorageLibraryImportPlanningWorkResult {
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let job = executor
        .submit_waiting(|token| {
            let footprint = StorageImportSessionCommitFootprint {
                count: 2,
                bodies: 2048,
                projections: 2048,
                ..StorageLibraryImportPlanningWorkFixture::footprint()?
            };
            let mut observations = Vec::new();
            for records in [1, 2, 3] {
                observations.push(
                    StorageLibraryImportPlanningWorkFixture::with_records(
                        StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                        records,
                        token,
                    )?
                    .observe(footprint)?,
                );
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(observations)
        })
        .await?;
    let result = job.join().await;
    executor.shutdown().await?;
    let observations = result??;
    for observation in &observations {
        observation.assert_controls();
    }
    for pair in observations.windows(2) {
        assert_eq!(
            pair[0].deferred.new_page_budget,
            pair[1].deferred.new_page_budget,
        );
        assert!(pair[0].deferred.old_tree_pages < pair[1].deferred.old_tree_pages);
        assert_eq!(
            pair[0].deferred.allocator_operations,
            pair[1].deferred.allocator_operations,
        );
    }
    assert_eq!(
        observations[1].deferred.old_tree_pages - observations[0].deferred.old_tree_pages,
        observations[2].deferred.old_tree_pages - observations[1].deferred.old_tree_pages,
    );
    Ok(())
}

#[tokio::test]
async fn page_budget_refuses_excess_allocation_and_restores_the_ceiling_after_rollback()
-> StorageLibraryImportPlanningWorkResult {
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let job = executor
        .submit_waiting(|token| {
            let mut fixture = StorageLibraryImportPlanningWorkFixture::new(
                StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                token,
            )?;
            assert_eq!(fixture.checkpoint()?, (0, 0, 0));
            let before = fixture.snapshot()?;
            let previous = StorageLibraryImportSpaceBudgetEnumPragma::Ceiling.read(&fixture.connection)?;
            let transaction = fixture
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let space = StorageLibraryImportSpace::plan(
                &transaction,
                StorageLibraryImportPlanningWorkFixture::footprint()?,
                &StorageImportSessionCancellation::new(),
            )?;
            let ceiling = space.page_ceiling();
            let mut scope = None;
            StorageLibraryImportPageLimit::install(&transaction, ceiling, &mut scope)?;
            let installed = StorageLibraryImportSpaceBudgetEnumPragma::Ceiling.read(&transaction)?;
            transaction.execute(
                "INSERT INTO documents(id, revision, body) VALUES (?1, '1', '{}')",
                [uuid::Uuid::from_u128(99_003).to_string()],
            )?;
            transaction.execute("UPDATE library_state SET revision = '999' WHERE singleton = 1", [])?;
            let primary = transaction
                .execute(
                    "INSERT INTO documents(id, revision, body) VALUES (?1, '1', json_object('padding', hex(zeroblob(?2))))",
                    rusqlite::params![
                        uuid::Uuid::from_u128(99_002).to_string(),
                        i64::try_from(
                            ceiling
                                .checked_mul(StorageLibraryImportPlanningWorkFixture::PAGE_BYTES)
                                .ok_or("budget fixture size overflow")?,
                        )?,
                    ],
                )
                .map_err(StorageLibraryError::from);
            transaction.finish()?;
            let pages = StorageLibraryImportSpaceBudgetEnumPragma::Pages.read(&fixture.connection)?;
            let outcome = StorageLibraryImportPageLimit::finish(&fixture.connection, scope, primary);
            let restored = StorageLibraryImportSpaceBudgetEnumPragma::Ceiling.read(&fixture.connection)?;
            assert_eq!(fixture.checkpoint()?, (0, 0, 0));
            let after = fixture.snapshot()?;
            fixture
                .connection
                .close()
                .map_err(|(_connection, error)| error)?;
            let path = fixture.directory.path().to_owned();
            fixture.directory.close()?;
            assert!(!path.try_exists()?);
            assert_eq!(installed, ceiling);
            assert!(pages <= ceiling);
            assert_eq!(restored, previous);
            assert!(
                matches!(outcome, Err(StorageLibraryError::Sqlite(rusqlite::Error::SqliteFailure(error, _))) if error.code == rusqlite::ErrorCode::DiskFull),
            );
            assert_eq!(after.documents, before.documents);
            assert_eq!(after.metadata, before.metadata);
            assert_eq!(after.schema, before.schema);
            assert_eq!(after.catalog, before.catalog);
            assert_eq!(after.nonoverflow, before.nonoverflow);
            assert_eq!(after.document_overflow_pages, before.document_overflow_pages);
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await?;
    let result = job.join().await;
    executor.shutdown().await?;
    result??;
    Ok(())
}

#[tokio::test]
async fn journal_proof_requires_enabled_immediate_foreign_keys()
-> StorageLibraryImportPlanningWorkResult {
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let job = executor
        .submit_waiting(|token| {
            let mut outcomes = Vec::new();
            for (foreign_keys, deferred) in [(true, false), (false, false), (true, true)] {
                let mut fixture = StorageLibraryImportPlanningWorkFixture::with_records(
                    StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                    0,
                    token,
                )?;
                fixture
                    .connection
                    .pragma_update(None, "foreign_keys", foreign_keys)?;
                let transaction = fixture
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                transaction.pragma_update(None, "defer_foreign_keys", deferred)?;
                let result = StorageLibraryImportSpace::plan(
                    &transaction,
                    StorageLibraryImportPlanningWorkFixture::footprint()?,
                    &StorageImportSessionCancellation::new(),
                );
                transaction.rollback()?;
                fixture
                    .connection
                    .close()
                    .map_err(|(_connection, error)| error)?;
                let path = fixture.directory.path().to_owned();
                fixture.directory.close()?;
                assert!(!path.try_exists()?);
                outcomes.push((foreign_keys, deferred, result));
            }
            for (foreign_keys, deferred, result) in outcomes {
                if foreign_keys && !deferred {
                    assert!(result.is_ok());
                } else {
                    assert!(matches!(result, Err(StorageLibraryError::ImportLayout)));
                }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await?;
    let result = job.join().await;
    executor.shutdown().await?;
    result??;
    Ok(())
}
