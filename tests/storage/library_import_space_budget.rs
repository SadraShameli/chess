use diesel::RunQueryDsl;
use diesel::SqliteConnection;
use diesel::sql_types::{BigInt, Text};

use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::storage::import_session::{
    StorageImportSessionCancellation, StorageImportSessionCommitFootprint,
};
use crate::storage::library_import::StorageLibraryImportPageLimit;
use crate::storage::library_import::space::StorageLibraryImportSpace;
use crate::storage::library_import::space::trees::planning_work_tests::{
    StorageLibraryImportPlanningWorkFixture, StorageLibraryImportPlanningWorkResult,
    StorageLibraryImportPlanningWorkRollback,
};
use crate::storage::library_repository::StorageLibraryError;

enum StorageLibraryImportSpaceBudgetEnumPragma {
    Ceiling,
    Pages,
}

#[derive(diesel::QueryableByName)]
struct StorageLibraryImportSpaceBudgetPragmaValue {
    #[diesel(sql_type = BigInt)]
    value: i64,
}

impl StorageLibraryImportSpaceBudgetEnumPragma {
    fn read(
        self,
        connection: &mut SqliteConnection,
    ) -> StorageLibraryImportPlanningWorkResult<u64> {
        let query = match self {
            Self::Ceiling => "SELECT max_page_count AS value FROM pragma_max_page_count",
            Self::Pages => "SELECT page_count AS value FROM pragma_page_count",
        };
        let row: StorageLibraryImportSpaceBudgetPragmaValue =
            diesel::sql_query(query).get_result(connection)?;
        Ok(u64::try_from(row.value)?)
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
            let previous =
                StorageLibraryImportSpaceBudgetEnumPragma::Ceiling.read(fixture.diesel.diesel())?;
            let mut scope = None;
            let mut primary = None;
            let (ceiling, installed) = StorageLibraryImportPlanningWorkRollback::run(
                fixture.diesel.diesel(),
                true,
                |transaction| {
                    let space = StorageLibraryImportSpace::plan(
                        transaction,
                        StorageLibraryImportPlanningWorkFixture::footprint()?,
                        &StorageImportSessionCancellation::new(),
                    )?;
                    let ceiling = space.page_ceiling();
                    StorageLibraryImportPageLimit::install(transaction, ceiling, &mut scope)?;
                    let installed = StorageLibraryImportSpaceBudgetEnumPragma::Ceiling.read(transaction)?;
                    diesel::sql_query("INSERT INTO documents(id, revision, body) VALUES (?, '1', '{}')")
                        .bind::<Text, _>(uuid::Uuid::from_u128(99_003).to_string())
                        .execute(transaction)?;
                    diesel::sql_query("UPDATE library_state SET revision = '999' WHERE singleton = 1")
                        .execute(transaction)?;
                    primary = Some(
                        diesel::sql_query(
                            "INSERT INTO documents(id, revision, body) VALUES (?, '1', json_object('padding', hex(zeroblob(?))))",
                        )
                        .bind::<Text, _>(uuid::Uuid::from_u128(99_002).to_string())
                        .bind::<BigInt, _>(i64::try_from(
                            ceiling
                                .checked_mul(StorageLibraryImportPlanningWorkFixture::PAGE_BYTES)
                                .ok_or("budget fixture size overflow")?,
                        )?)
                        .execute(transaction)
                        .map(drop)
                        .map_err(StorageLibraryError::from),
                    );
                    Ok((ceiling, installed))
                },
            )?;
            let primary = primary.ok_or("the oversized insert did not run")?;
            let pages =
                StorageLibraryImportSpaceBudgetEnumPragma::Pages.read(fixture.diesel.diesel())?;
            let outcome =
                StorageLibraryImportPageLimit::finish(fixture.diesel.diesel(), scope, primary);
            let restored =
                StorageLibraryImportSpaceBudgetEnumPragma::Ceiling.read(fixture.diesel.diesel())?;
            assert_eq!(fixture.checkpoint()?, (0, 0, 0));
            let after = fixture.snapshot()?;
            drop(fixture.connection);
            let path = fixture.directory.path().to_owned();
            fixture.directory.close()?;
            assert!(!path.try_exists()?);
            assert_eq!(installed, ceiling);
            assert!(pages <= ceiling);
            assert_eq!(restored, previous);
            assert!(
                matches!(&outcome, Err(StorageLibraryError::Diesel(error)) if error.to_string().contains("full")),
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
                diesel::sql_query(if foreign_keys {
                    "PRAGMA foreign_keys = ON"
                } else {
                    "PRAGMA foreign_keys = OFF"
                })
                .execute(fixture.diesel.diesel())?;
                let result = StorageLibraryImportPlanningWorkRollback::run(
                    fixture.diesel.diesel(),
                    true,
                    |transaction| {
                        diesel::sql_query(if deferred {
                            "PRAGMA defer_foreign_keys = ON"
                        } else {
                            "PRAGMA defer_foreign_keys = OFF"
                        })
                        .execute(transaction)?;
                        Ok(StorageLibraryImportSpace::plan(
                            transaction,
                            StorageLibraryImportPlanningWorkFixture::footprint()?,
                            &StorageImportSessionCancellation::new(),
                        )
                        .map(drop))
                    },
                )?;
                drop(fixture.connection);
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
