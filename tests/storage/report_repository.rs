use std::num::NonZeroUsize;

use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::report_repository::{
    StorageReportEnumFreshness, StorageReportEnumState, StorageReportError, StorageReportJobRecord,
    StorageReportRepository, StorageReportResultRecord,
};
use uuid::Uuid;

type StorageReportTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageReportFixture {
    _directory: tempfile::TempDir,
    store: StorageDocumentStore,
    repository: StorageReportRepository,
}

impl StorageReportFixture {
    async fn open(directory: tempfile::TempDir) -> StorageReportTestResult<Self> {
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let store =
            StorageDocumentStore::open(directory.path().join("reports.sqlite"), budget).await?;
        let repository = StorageReportRepository::new(store.client());
        Ok(Self {
            _directory: directory,
            store,
            repository,
        })
    }

    async fn new() -> StorageReportTestResult<Self> {
        Self::open(tempfile::tempdir()?).await
    }

    fn record(
        job: u128,
        game: u128,
        digest: u8,
        count: u32,
    ) -> StorageReportTestResult<StorageReportJobRecord> {
        Ok(StorageReportJobRecord::new(
            Uuid::from_u128(job),
            Uuid::from_u128(game),
            1,
            [digest; 32],
            count,
            "{\"engine\":\"fixture\"}".to_owned(),
            "{\"line\":[]}".to_owned(),
        )?)
    }

    fn result(index: u32) -> StorageReportTestResult<StorageReportResultRecord> {
        Ok(StorageReportResultRecord::new(
            index,
            format!("{{\"position\":{index}}}"),
        )?)
    }
}

#[tokio::test]
async fn jobs_queue_in_insertion_order_and_reject_duplicates() -> StorageReportTestResult {
    let fixture = StorageReportFixture::new().await?;
    assert_eq!(
        fixture
            .repository
            .insert(StorageReportFixture::record(1, 10, 1, 3)?)
            .await?,
        0
    );
    assert_eq!(
        fixture
            .repository
            .insert(StorageReportFixture::record(2, 11, 1, 2)?)
            .await?,
        1
    );
    assert!(matches!(
        fixture
            .repository
            .insert(StorageReportFixture::record(1, 10, 1, 3)?)
            .await,
        Err(StorageReportError::ExistingJob(_))
    ));
    let page = fixture.repository.list(None).await?;
    assert_eq!(
        page.iter().map(|summary| summary.job()).collect::<Vec<_>>(),
        [Uuid::from_u128(1), Uuid::from_u128(2)]
    );
    assert_eq!(fixture.repository.list(Some(0)).await?.len(), 1);
    assert!(
        StorageReportJobRecord::new(
            Uuid::nil(),
            Uuid::nil(),
            1,
            [0; 32],
            0,
            "{}".to_owned(),
            "{}".to_owned()
        )
        .is_err()
    );
    fixture.store.close().await?;
    Ok(())
}

#[tokio::test]
async fn completed_positions_survive_pause_cancel_and_relaunch() -> StorageReportTestResult {
    let directory = tempfile::tempdir()?;
    let fixture = StorageReportFixture::open(directory).await?;
    let job = Uuid::from_u128(5);
    fixture
        .repository
        .insert(StorageReportFixture::record(5, 10, 1, 3)?)
        .await?;
    fixture
        .repository
        .save_result(job, StorageReportFixture::result(0)?)
        .await?;
    fixture
        .repository
        .save_result(job, StorageReportFixture::result(0)?)
        .await?;
    fixture
        .repository
        .set_state(job, StorageReportEnumState::Paused)
        .await?;
    fixture
        .repository
        .save_result(job, StorageReportFixture::result(2)?)
        .await?;
    fixture
        .repository
        .set_state(job, StorageReportEnumState::Queued)
        .await?;
    assert_eq!(fixture.repository.recover_after_relaunch().await?, 1);
    let snapshot = fixture.repository.load(job).await?.ok_or("missing job")?;
    assert_eq!(snapshot.state(), StorageReportEnumState::Paused);
    assert_eq!(
        snapshot
            .results()
            .iter()
            .map(|result| result.index())
            .collect::<Vec<_>>(),
        [0, 2]
    );
    fixture
        .repository
        .set_state(job, StorageReportEnumState::Cancelled)
        .await?;
    let cancelled = fixture.repository.load(job).await?.ok_or("missing job")?;
    assert_eq!(cancelled.results().len(), 2);
    assert!(matches!(
        fixture
            .repository
            .save_result(job, StorageReportFixture::result(1)?)
            .await,
        Err(StorageReportError::Terminal(
            StorageReportEnumState::Cancelled
        ))
    ));
    assert!(matches!(
        fixture
            .repository
            .set_state(job, StorageReportEnumState::Queued)
            .await,
        Err(StorageReportError::Terminal(_))
    ));
    fixture.store.close().await?;
    Ok(())
}

#[tokio::test]
async fn results_are_immutable_and_completion_needs_every_position() -> StorageReportTestResult {
    let fixture = StorageReportFixture::new().await?;
    let job = Uuid::from_u128(7);
    fixture
        .repository
        .insert(StorageReportFixture::record(7, 10, 1, 2)?)
        .await?;
    fixture
        .repository
        .save_result(job, StorageReportFixture::result(0)?)
        .await?;
    assert!(matches!(
        fixture
            .repository
            .save_result(
                job,
                StorageReportResultRecord::new(0, "{\"other\":1}".to_owned())?
            )
            .await,
        Err(StorageReportError::ResultConflict(0))
    ));
    assert!(matches!(
        fixture
            .repository
            .save_result(job, StorageReportFixture::result(2)?)
            .await,
        Err(StorageReportError::PositionIndex { index: 2, count: 2 })
    ));
    assert!(matches!(
        fixture
            .repository
            .set_state(job, StorageReportEnumState::Completed)
            .await,
        Err(StorageReportError::Incomplete {
            recorded: 1,
            count: 2
        })
    ));
    fixture
        .repository
        .save_result(job, StorageReportFixture::result(1)?)
        .await?;
    fixture
        .repository
        .set_state(job, StorageReportEnumState::Completed)
        .await?;
    assert_eq!(
        fixture
            .repository
            .load(job)
            .await?
            .ok_or("missing job")?
            .state(),
        StorageReportEnumState::Completed
    );
    fixture.store.close().await?;
    Ok(())
}

#[tokio::test]
async fn a_changed_line_outdates_old_reports_and_keeps_their_results() -> StorageReportTestResult {
    let fixture = StorageReportFixture::new().await?;
    let old = Uuid::from_u128(1);
    let other = Uuid::from_u128(2);
    fixture
        .repository
        .insert(StorageReportFixture::record(1, 10, 1, 2)?)
        .await?;
    fixture
        .repository
        .insert(StorageReportFixture::record(2, 11, 1, 2)?)
        .await?;
    fixture
        .repository
        .save_result(old, StorageReportFixture::result(0)?)
        .await?;
    assert_eq!(
        fixture
            .repository
            .mark_outdated(Uuid::from_u128(10), [2; 32])
            .await?,
        1
    );
    assert_eq!(
        fixture
            .repository
            .mark_outdated(Uuid::from_u128(10), [2; 32])
            .await?,
        0
    );
    let outdated = fixture.repository.load(old).await?.ok_or("missing job")?;
    assert_eq!(outdated.freshness(), StorageReportEnumFreshness::Outdated);
    assert_eq!(outdated.state(), StorageReportEnumState::Paused);
    assert_eq!(outdated.results().len(), 1);
    assert!(matches!(
        fixture
            .repository
            .set_state(old, StorageReportEnumState::Queued)
            .await,
        Err(StorageReportError::Transition { .. })
    ));
    let untouched = fixture.repository.load(other).await?.ok_or("missing job")?;
    assert_eq!(untouched.freshness(), StorageReportEnumFreshness::Current);
    fixture
        .repository
        .insert(StorageReportFixture::record(3, 10, 2, 2)?)
        .await?;
    assert!(fixture.repository.delete(old).await?);
    assert!(fixture.repository.load(old).await?.is_none());
    assert!(!fixture.repository.delete(old).await?);
    fixture.store.close().await?;
    Ok(())
}

#[tokio::test]
async fn reports_persist_across_reopening_the_database() -> StorageReportTestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().to_owned();
    let first = StorageReportFixture::open(directory).await?;
    let job = Uuid::from_u128(9);
    first
        .repository
        .insert(StorageReportFixture::record(9, 10, 1, 2)?)
        .await?;
    first
        .repository
        .save_result(job, StorageReportFixture::result(1)?)
        .await?;
    first.store.close().await?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store = StorageDocumentStore::open(path.join("reports.sqlite"), budget).await?;
    let repository = StorageReportRepository::new(store.client());
    let snapshot = repository.load(job).await?.ok_or("missing job")?;
    assert_eq!(snapshot.record().line_digest(), [1; 32]);
    assert_eq!(snapshot.results().len(), 1);
    store.close().await?;
    drop(first._directory);
    Ok(())
}
