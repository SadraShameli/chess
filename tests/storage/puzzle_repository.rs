use std::future::{Future, poll_fn};
use std::num::{NonZeroU64, NonZeroUsize};
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockInstant;
use chess::puzzle::checkpoint::{
    PuzzleCheckpoint, PuzzleCheckpointEnumAdvance, PuzzleCheckpointEnumFreezeCause,
    PuzzleCheckpointEnumTrigger, PuzzleCheckpointObservation, PuzzleCheckpointSnapshot,
};
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::enums::PuzzleEnumCountdownMode;
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::metadata::PuzzleDefinitionMetadata;
use chess::puzzle::persistence::{
    PuzzlePersistenceCodec, PuzzlePersistenceEnumCommand, PuzzlePersistenceReceipt,
    PuzzlePersistenceRun, PuzzlePersistenceSaveRequest, PuzzlePersistenceSnapshot,
    PuzzlePersistenceStorageRevision,
};
use chess::puzzle::random_provider::PuzzleRandomProviderSeeded;
use chess::puzzle::run::{PuzzleRunEnumContent, PuzzleRunEnumStatus};
use chess::puzzle::selection::PuzzleSelectionFilter;
use chess::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use chess::puzzle::storage_repository::PuzzleStorageDriver;
use chess::puzzle::timer::PuzzleTimerEnumReadiness;
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use chess::storage::puzzle_repository::{StoragePuzzleError, StoragePuzzleRepository};
use uuid::Uuid;

struct StoragePuzzleFixture;

impl StoragePuzzleFixture {
    const INVALID_DECIMALS: &[&str] = &[
        "",
        "0",
        "01",
        "+1",
        "-1",
        "1.0",
        " 1",
        "1 ",
        "18446744073709551616",
        "111111111111111111111",
        "９",
        "\0",
    ];

    fn owner() -> Result<PuzzlePersistenceRun, Box<dyn std::error::Error>> {
        Self::owner_with_id(PuzzleSessionId::new(NonZeroU64::MIN))
    }

    fn id(value: u64) -> Result<PuzzleSessionId, Box<dyn std::error::Error>> {
        Ok(PuzzleSessionId::new(
            NonZeroU64::new(value).ok_or("zero fixture identity")?,
        ))
    }

    fn owner_with_id(
        id: PuzzleSessionId,
    ) -> Result<PuzzlePersistenceRun, Box<dyn std::error::Error>> {
        let initial = BoardPosition::standard();
        let movement = initial.parse_uci("e2e4")?;
        let definition = PuzzleDefinition::new(
            PuzzleDefinitionIdentity::new("fixture".into(), "v1".into(), "first".into())?,
            initial,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default(),
        )?;
        Ok(PuzzlePersistenceRun::new(
            id,
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![definition],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(100),
        )?)
    }

    async fn save_initial(
        repository: &StoragePuzzleRepository,
        id: PuzzleSessionId,
    ) -> Result<PuzzlePersistenceReceipt, Box<dyn std::error::Error>> {
        let mut owner = Self::owner_with_id(id)?;
        Ok(repository
            .begin_save(Self::request(
                &mut owner,
                ClockInstant::from_nanos(0),
                PuzzlePersistenceStorageRevision::INITIAL,
                0,
            )?)?
            .wait()
            .await?)
    }

    async fn assert_pending<F: Future>(mut future: Pin<&mut F>) {
        poll_fn(|context| {
            assert!(future.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }

    fn snapshot(
        owner: &mut PuzzlePersistenceRun,
        now: ClockInstant,
        base: usize,
    ) -> Result<PuzzleCheckpointSnapshot<PuzzlePersistenceSnapshot>, Box<dyn std::error::Error>>
    {
        let payload = owner.capture(now, base)?;
        Ok(PuzzleCheckpointSnapshot::new(
            PuzzleCheckpointObservation::from_run(owner.run(), now)?,
            payload,
        ))
    }

    fn request(
        owner: &mut PuzzlePersistenceRun,
        now: ClockInstant,
        revision: PuzzlePersistenceStorageRevision,
        base: usize,
    ) -> Result<PuzzlePersistenceSaveRequest, Box<dyn std::error::Error>> {
        let mut checkpoint = PuzzleCheckpoint::new(Self::snapshot(owner, now, base)?)?;
        let token = checkpoint.begin_write(now)?;
        let payload = checkpoint
            .pending()
            .ok_or("missing checkpoint")?
            .payload()
            .clone();
        Ok(PuzzlePersistenceSaveRequest::new(payload, revision, token)?)
    }
}

#[tokio::test]
async fn transactional_receipt_survives_reopen_and_puzzle_rows_do_not_enter_game_inventory()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let mut owner = StoragePuzzleFixture::owner()?;
    let request = StoragePuzzleFixture::request(
        &mut owner,
        ClockInstant::from_nanos(0),
        PuzzlePersistenceStorageRevision::INITIAL,
        0,
    )?;
    let mut write = repository.begin_save(request.clone())?;
    let receipt = write.wait().await?;
    assert_eq!(receipt.storage_revision().value(), 1);
    assert!(store.client().inventory(None).await?.is_empty());
    let mut stale = repository.begin_save(request)?;
    assert!(matches!(
        stale.wait().await,
        Err(StoragePuzzleError::Conflict {
            expected: 0,
            actual: 1
        })
    ));
    store.close().await?;
    let reopened = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let saved = StoragePuzzleRepository::new(reopened.client())
        .load(owner.run().context().session_id())
        .await?
        .ok_or("missing puzzle run")?;
    assert_eq!(saved.storage_revision().value(), 1);
    assert!(matches!(
        saved
            .owner()
            .run()
            .view(ClockInstant::from_nanos(0))?
            .status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    assert_eq!(
        saved
            .owner()
            .run()
            .view(ClockInstant::from_nanos(0))?
            .content(),
        PuzzleRunEnumContent::Hidden
    );
    reopened.close().await?;
    let connection = rusqlite::Connection::open(path)?;
    let mut statement =
        connection.prepare("SELECT version FROM __diesel_schema_migrations ORDER BY version")?;
    let versions = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(versions, ["0001"]);
    Ok(())
}

#[tokio::test]
async fn cancelled_wait_retains_exact_single_write_and_real_commit_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store =
        StorageDocumentStore::open(directory.path().join("puzzles.sqlite3"), budget.clone())
            .await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let mut owner = StoragePuzzleFixture::owner()?;
    let request = StoragePuzzleFixture::request(
        &mut owner,
        ClockInstant::from_nanos(0),
        PuzzlePersistenceStorageRevision::INITIAL,
        0,
    )?;
    let held = budget.try_lease(1)?;
    let mut write = repository.begin_save(request)?;
    let mut waiting = Box::pin(write.wait());
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    drop(held);
    assert_eq!(write.wait().await?.storage_revision().value(), 1);
    assert!(matches!(
        write.wait().await,
        Err(StoragePuzzleError::CompletionConsumed)
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn failed_transaction_rolls_back_collection_run_events_attempts_and_checkpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch("CREATE TRIGGER reject_puzzle_checkpoint BEFORE INSERT ON puzzle_checkpoints BEGIN SELECT RAISE(ABORT, 'injected failure'); END;")?;
    let mut owner = StoragePuzzleFixture::owner()?;
    let mut write =
        StoragePuzzleRepository::new(store.client()).begin_save(StoragePuzzleFixture::request(
            &mut owner,
            ClockInstant::from_nanos(0),
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?)?;
    assert!(write.wait().await.is_err());
    for query in [
        "SELECT count(*) FROM puzzle_collections",
        "SELECT count(*) FROM puzzle_runs",
        "SELECT count(*) FROM puzzle_events",
        "SELECT count(*) FROM puzzle_attempts",
        "SELECT count(*) FROM puzzle_checkpoints",
    ] {
        let count: i64 = connection.query_row(query, [], |row| row.get(0))?;
        assert_eq!(count, 0);
    }
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn driver_coalesces_latest_state_and_acknowledges_only_after_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store =
        StorageDocumentStore::open(directory.path().join("puzzles.sqlite3"), budget.clone())
            .await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let mut owner = StoragePuzzleFixture::owner()?;
    let mut driver = PuzzleStorageDriver::new(
        repository.clone(),
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(0), 0)?,
        PuzzlePersistenceStorageRevision::INITIAL,
        0,
    )?;
    driver.begin_write(ClockInstant::from_nanos(0))?;
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(0))?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    driver.record(
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(0), 0)?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    let held = budget.try_lease(1)?;
    driver.begin_write(ClockInstant::from_nanos(0))?;
    for nanos in [250_000_000, 500_000_000, 750_000_000] {
        let now = ClockInstant::from_nanos(nanos);
        driver.record(
            StoragePuzzleFixture::snapshot(&mut owner, now, driver.persisted_event_count())?,
            PuzzleCheckpointEnumTrigger::Countdown,
        )?;
    }
    assert_eq!(driver.storage_revision().value(), 1);
    assert_eq!(
        driver
            .checkpoint()
            .coalesced()
            .ok_or("missing latest snapshot")?
            .payload()
            .elapsed_nanos(),
        750_000_000
    );
    let mut waiting = Box::pin(driver.wait());
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    drop(held);
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(750_000_000))?;
    assert_eq!(driver.persisted_event_count(), 1);
    driver.begin_write(ClockInstant::from_nanos(750_000_000))?;
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(750_000_000))?;
    assert_eq!(driver.storage_revision().value(), 3);
    let recovered = repository
        .load(owner.run().context().session_id())
        .await?
        .ok_or("missing checkpoint")?;
    assert_eq!(
        recovered
            .owner()
            .run()
            .view(ClockInstant::from_nanos(0))?
            .timer()
            .elapsed()
            .as_nanos(),
        750_000_000
    );
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn retry_keeps_failed_snapshot_and_original_revision_then_requires_explicit_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let connection = rusqlite::Connection::open(path)?;
    connection.execute_batch("CREATE TRIGGER reject_puzzle_checkpoint BEFORE INSERT ON puzzle_checkpoints BEGIN SELECT RAISE(ABORT, 'injected failure'); END;")?;
    let mut owner = StoragePuzzleFixture::owner()?;
    let mut driver = PuzzleStorageDriver::new(
        StoragePuzzleRepository::new(store.client()),
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(0), 0)?,
        PuzzlePersistenceStorageRevision::INITIAL,
        0,
    )?;
    driver.begin_write(ClockInstant::from_nanos(100))?;
    driver.wait().await?;
    assert!(driver.settle(ClockInstant::from_nanos(0)).is_err());
    assert!(matches!(
        driver.failure(),
        Some(StoragePuzzleError::Diesel(_))
    ));
    assert!(driver.pending_request().is_some());
    driver.wait().await?;
    assert!(driver.settle(ClockInstant::from_nanos(100)).is_err());
    owner.execute(
        PuzzlePersistenceEnumCommand::Pause(
            chess::puzzle::timer::PuzzleTimerEnumPauseReason::Manual,
        ),
        ClockInstant::from_nanos(100),
    )?;
    driver.record(
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(100), 0)?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    connection.execute_batch("DROP TRIGGER reject_puzzle_checkpoint")?;
    driver.retry_save(ClockInstant::from_nanos(100))?;
    let request = driver.pending_request().ok_or("missing original request")?;
    assert_eq!(request.expected_storage_revision().value(), 0);
    assert_eq!(request.snapshot().context().revision().value(), 0);
    assert_eq!(request.snapshot().event_count(), 0);
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(100))?;
    assert!(driver.resume(ClockInstant::from_nanos(100)).is_err());
    driver.begin_write(ClockInstant::from_nanos(100))?;
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(100))?;
    driver.resume(ClockInstant::from_nanos(100))?;
    assert!(matches!(
        owner.run().view(ClockInstant::from_nanos(100))?.status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    assert_eq!(driver.storage_revision().value(), 2);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn committed_receipt_is_retained_when_checkpoint_acknowledgement_rejects_time()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let mut owner = StoragePuzzleFixture::owner()?;
    let mut driver = PuzzleStorageDriver::new(
        StoragePuzzleRepository::new(store.client()),
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(0), 0)?,
        PuzzlePersistenceStorageRevision::INITIAL,
        0,
    )?;
    driver.begin_write(ClockInstant::from_nanos(100))?;
    driver.wait().await?;
    assert!(driver.settle(ClockInstant::from_nanos(0)).is_err());
    assert_eq!(driver.storage_revision().value(), 0);
    assert_eq!(
        driver
            .pending_request()
            .ok_or("lost committed request")?
            .expected_storage_revision()
            .value(),
        0
    );
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(100))?;
    assert_eq!(driver.storage_revision().value(), 1);
    store.close().await?;
    let connection = rusqlite::Connection::open(path)?;
    let revision: String =
        connection.query_row("SELECT storage_revision FROM puzzle_runs", [], |row| {
            row.get(0)
        })?;
    assert_eq!(revision, "1");
    Ok(())
}

#[tokio::test]
async fn corrupted_checkpoint_and_attempt_rows_are_refused_on_read()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let mut owner = StoragePuzzleFixture::owner()?;
    repository
        .begin_save(StoragePuzzleFixture::request(
            &mut owner,
            ClockInstant::from_nanos(0),
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?)?
        .wait()
        .await?;
    let connection = rusqlite::Connection::open(path)?;
    let checkpoint: String =
        connection.query_row("SELECT body FROM puzzle_checkpoints", [], |row| row.get(0))?;
    connection.execute("UPDATE puzzle_checkpoints SET body = '{}'", [])?;
    assert!(
        repository
            .load(owner.run().context().session_id())
            .await
            .is_err()
    );
    connection.execute("UPDATE puzzle_checkpoints SET body = ?1", [&checkpoint])?;
    connection.execute("UPDATE puzzle_attempts SET body = '{}'", [])?;
    assert!(matches!(
        repository.load(owner.run().context().session_id()).await,
        Err(StoragePuzzleError::Corrupt)
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn current_revision_and_equal_event_count_do_not_authorize_a_divergent_prefix()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("puzzles.sqlite3"),
        CapacityBudget::discover()?,
    )
    .await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let mut original = StoragePuzzleFixture::owner()?;
    let mut divergent = StoragePuzzleFixture::owner()?;
    let initial_receipt = repository
        .begin_save(StoragePuzzleFixture::request(
            &mut original,
            ClockInstant::from_nanos(0),
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?)?
        .wait()
        .await?;
    let mut checkpoint = PuzzleCheckpoint::new(StoragePuzzleFixture::snapshot(
        &mut original,
        ClockInstant::from_nanos(0),
        initial_receipt.event_count(),
    )?)?;
    checkpoint.begin_write(ClockInstant::from_nanos(0))?;
    initial_receipt.acknowledge(&mut checkpoint, ClockInstant::from_nanos(0))?;
    for owner in [&mut original, &mut divergent] {
        owner.execute(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
            ClockInstant::from_nanos(0),
        )?;
    }
    let ready_snapshot = StoragePuzzleFixture::snapshot(
        &mut original,
        ClockInstant::from_nanos(0),
        initial_receipt.event_count(),
    )?;
    let ready_payload = ready_snapshot.payload().clone();
    checkpoint.record(
        ready_snapshot,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    let token = checkpoint.begin_write(ClockInstant::from_nanos(0))?;
    let ready_receipt = repository
        .begin_save(PuzzlePersistenceSaveRequest::new(
            ready_payload,
            initial_receipt.storage_revision(),
            token,
        )?)?
        .wait()
        .await?;
    ready_receipt.acknowledge(&mut checkpoint, ClockInstant::from_nanos(0))?;
    original.execute(
        PuzzlePersistenceEnumCommand::Hint,
        ClockInstant::from_nanos(100),
    )?;
    divergent.execute(
        PuzzlePersistenceEnumCommand::Move(BoardPosition::standard().parse_uci("e2e4")?),
        ClockInstant::from_nanos(100),
    )?;
    assert_eq!(original.event_count(), divergent.event_count());
    let original_snapshot = StoragePuzzleFixture::snapshot(
        &mut original,
        ClockInstant::from_nanos(100),
        ready_receipt.event_count(),
    )?;
    let original_payload = original_snapshot.payload().clone();
    checkpoint.record(
        original_snapshot,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    let token = checkpoint.begin_write(ClockInstant::from_nanos(100))?;
    let receipt = repository
        .begin_save(PuzzlePersistenceSaveRequest::new(
            original_payload,
            ready_receipt.storage_revision(),
            token,
        )?)?
        .wait()
        .await?;
    let request = StoragePuzzleFixture::request(
        &mut divergent,
        ClockInstant::from_nanos(100),
        receipt.storage_revision(),
        receipt.event_count(),
    )?;
    assert!(matches!(
        repository.begin_save(request)?.wait().await,
        Err(StoragePuzzleError::CollectionConflict)
    ));
    let recovered = repository
        .load(original.run().context().session_id())
        .await?
        .ok_or("missing original checkpoint")?;
    assert_eq!(recovered.storage_revision(), receipt.storage_revision());
    assert_eq!(
        recovered
            .owner()
            .run()
            .summary(ClockInstant::from_nanos(0))?
            .hints(),
        1
    );
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn delayed_completion_requires_current_time_settlement_before_durability_advances()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store =
        StorageDocumentStore::open(directory.path().join("puzzles.sqlite3"), budget.clone())
            .await?;
    let mut owner = StoragePuzzleFixture::owner()?;
    let mut driver = PuzzleStorageDriver::new(
        StoragePuzzleRepository::new(store.client()),
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(0), 0)?,
        PuzzlePersistenceStorageRevision::INITIAL,
        0,
    )?;
    driver.begin_write(ClockInstant::from_nanos(0))?;
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(0))?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    driver.record(
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(0), 0)?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    driver.begin_write(ClockInstant::from_nanos(0))?;
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(0))?;
    let captured_at = ClockInstant::from_nanos(250_000_000);
    driver.record(
        StoragePuzzleFixture::snapshot(&mut owner, captured_at, driver.persisted_event_count())?,
        PuzzleCheckpointEnumTrigger::Countdown,
    )?;
    let held = budget.try_lease(1)?;
    driver.begin_write(captured_at)?;
    let mut waiting = Box::pin(driver.wait());
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(held);
    waiting.await?;
    let completed_at = ClockInstant::from_nanos(1_100_000_000);
    assert_eq!(driver.storage_revision().value(), 2);
    assert_eq!(
        driver
            .checkpoint()
            .durable()
            .ok_or("missing durable checkpoint")?
            .elapsed_active()
            .as_nanos(),
        0
    );
    let PuzzleCheckpointEnumAdvance::FreezeRequired(freeze) = driver.settle(completed_at)? else {
        return Err("delayed completion missed the original durability boundary".into());
    };
    assert_eq!(freeze.cause(), PuzzleCheckpointEnumFreezeCause::DurableLag);
    assert_eq!(freeze.pause_at(), ClockInstant::from_nanos(1_000_000_000));
    assert_eq!(driver.storage_revision().value(), 3);
    assert_eq!(
        driver
            .checkpoint()
            .durable()
            .ok_or("missing settled checkpoint")?
            .elapsed_active()
            .as_nanos(),
        250_000_000
    );
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn coalesced_events_rebase_to_the_receipts_original_journal_prefix()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store =
        StorageDocumentStore::open(directory.path().join("puzzles.sqlite3"), budget.clone())
            .await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let mut owner = StoragePuzzleFixture::owner()?;
    let mut driver = PuzzleStorageDriver::new(
        repository.clone(),
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(0), 0)?,
        PuzzlePersistenceStorageRevision::INITIAL,
        0,
    )?;
    driver.begin_write(ClockInstant::from_nanos(0))?;
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(0))?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    driver.record(
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(0), 0)?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    let held = budget.try_lease(1)?;
    driver.begin_write(ClockInstant::from_nanos(0))?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Hint,
        ClockInstant::from_nanos(100),
    )?;
    driver.record(
        StoragePuzzleFixture::snapshot(&mut owner, ClockInstant::from_nanos(100), 0)?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    drop(held);
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(100))?;
    assert_eq!(driver.persisted_event_count(), 1);
    driver.begin_write(ClockInstant::from_nanos(100))?;
    assert_eq!(
        driver
            .pending_request()
            .ok_or("missing rebased request")?
            .snapshot()
            .base_event_count(),
        1
    );
    driver.wait().await?;
    driver.settle(ClockInstant::from_nanos(100))?;
    let recovered = repository
        .load(owner.run().context().session_id())
        .await?
        .ok_or("missing coalesced checkpoint")?;
    assert_eq!(recovered.persisted_event_count(), 2);
    assert_eq!(
        recovered
            .owner()
            .run()
            .summary(ClockInstant::from_nanos(0))?
            .hints(),
        1
    );
    let revision = recovered.storage_revision();
    let base = recovered.persisted_event_count();
    let mut restored = recovered.into_owner();
    repository
        .begin_save(StoragePuzzleFixture::request(
            &mut restored,
            ClockInstant::from_nanos(0),
            revision,
            base,
        )?)?
        .wait()
        .await?;
    assert!(
        repository
            .load(owner.run().context().session_id())
            .await?
            .is_some()
    );
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn invalid_journal_proof_metadata_is_rejected_on_read_and_write()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let mut owner = StoragePuzzleFixture::owner()?;
    let receipt = repository
        .begin_save(StoragePuzzleFixture::request(
            &mut owner,
            ClockInstant::from_nanos(0),
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?)?
        .wait()
        .await?;
    let request = StoragePuzzleFixture::request(
        &mut owner,
        ClockInstant::from_nanos(0),
        receipt.storage_revision(),
        receipt.event_count(),
    )?;
    let connection = rusqlite::Connection::open(path)?;
    let original: Vec<u8> =
        connection.query_row("SELECT journal_digest FROM puzzle_runs", [], |row| {
            row.get(0)
        })?;
    for update in [
        "UPDATE puzzle_runs SET journal_version = 999",
        "UPDATE puzzle_runs SET journal_digest = x'00'",
    ] {
        assert!(matches!(
            connection.execute(update, []),
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation
        ));
    }
    connection.execute("UPDATE puzzle_runs SET journal_digest = zeroblob(32)", [])?;
    assert!(matches!(
        repository.load(owner.run().context().session_id()).await,
        Err(StoragePuzzleError::Corrupt)
    ));
    assert!(matches!(
        repository.begin_save(request)?.wait().await,
        Err(StoragePuzzleError::Corrupt)
    ));
    connection.execute("UPDATE puzzle_runs SET journal_digest = ?1", [&original])?;
    assert!(
        repository
            .load(owner.run().context().session_id())
            .await?
            .is_some()
    );
    owner.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Move(BoardPosition::standard().parse_uci("e2e4")?),
        ClockInstant::from_nanos(100),
    )?;
    let receipt = repository
        .begin_save(StoragePuzzleFixture::request(
            &mut owner,
            ClockInstant::from_nanos(100),
            receipt.storage_revision(),
            receipt.event_count(),
        )?)?
        .wait()
        .await?;
    let request = StoragePuzzleFixture::request(
        &mut owner,
        ClockInstant::from_nanos(100),
        receipt.storage_revision(),
        receipt.event_count(),
    )?;
    connection.execute("UPDATE puzzle_runs SET journal_digest = zeroblob(32)", [])?;
    assert!(matches!(
        repository.load(owner.run().context().session_id()).await,
        Err(StoragePuzzleError::Corrupt)
    ));
    assert!(matches!(
        repository.begin_save(request)?.wait().await,
        Err(StoragePuzzleError::CollectionConflict)
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_is_durable_and_isolated_from_game_documents_and_detached_collections()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let empty = repository.inventory(None).await?;
    assert!(empty.entries().is_empty());
    assert_eq!(empty.next_cursor(), None);
    let game = Uuid::from_u128(1);
    store.client().save(game, 0, "{}".into()).await?;
    assert!(repository.inventory(None).await?.entries().is_empty());
    let id = StoragePuzzleFixture::id(1)?;
    let receipt = StoragePuzzleFixture::save_initial(&repository, id).await?;
    let mut owner = StoragePuzzleFixture::owner_with_id(id)?;
    repository
        .begin_save(StoragePuzzleFixture::request(
            &mut owner,
            ClockInstant::from_nanos(0),
            receipt.storage_revision(),
            receipt.event_count(),
        )?)?
        .wait()
        .await?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute("INSERT INTO puzzle_collections (collection_id, schema_version, digest, body) SELECT 'detached', schema_version, digest, body FROM puzzle_collections WHERE collection_id = '1'", [])?;
    let page = repository.inventory(None).await?;
    assert_eq!(page.entries().len(), 1);
    let entry = page.entries()[0];
    assert_eq!(entry.id(), id);
    assert_eq!(entry.storage_revision().value(), 2);
    assert_eq!(entry.persisted_event_count(), 0);
    assert_eq!(page.next_cursor(), None);
    assert!(repository.inventory(Some(id)).await?.entries().is_empty());
    let games = store.client().inventory(None).await?;
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].id(), game);
    store.close().await?;
    assert!(matches!(
        repository.inventory(None).await,
        Err(StoragePuzzleError::Document(StorageDocumentError::Closed))
    ));
    let reopened = StorageDocumentStore::open(path, CapacityBudget::discover()?).await?;
    let restored = StoragePuzzleRepository::new(reopened.client())
        .inventory(None)
        .await?;
    assert_eq!(restored.entries(), page.entries());
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_has_bounded_numeric_pages_and_stable_exclusive_cursors()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("puzzles.sqlite3"),
        CapacityBudget::discover()?,
    )
    .await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let total = StoragePuzzleRepository::INVENTORY_PAGE_SIZE * 2 + 1;
    for index in (1..=total).rev() {
        StoragePuzzleFixture::save_initial(
            &repository,
            StoragePuzzleFixture::id(u64::try_from(index * 2)?)?,
        )
        .await?;
    }
    let first = repository.inventory(None).await?;
    assert_eq!(
        first.entries().len(),
        StoragePuzzleRepository::INVENTORY_PAGE_SIZE
    );
    for (index, entry) in first.entries().iter().enumerate() {
        assert_eq!(entry.id().value().get(), u64::try_from((index + 1) * 2)?);
    }
    assert_eq!(repository.inventory(None).await?.entries(), first.entries());
    let cursor = first.next_cursor().ok_or("missing full-page cursor")?;
    StoragePuzzleFixture::save_initial(&repository, StoragePuzzleFixture::id(1)?).await?;
    let second = repository.inventory(Some(cursor)).await?;
    assert_eq!(
        second.entries().len(),
        StoragePuzzleRepository::INVENTORY_PAGE_SIZE
    );
    for (index, entry) in second.entries().iter().enumerate() {
        assert_eq!(
            entry.id().value().get(),
            cursor.value().get() + u64::try_from((index + 1) * 2)?
        );
    }
    let third = repository.inventory(second.next_cursor()).await?;
    assert_eq!(third.entries().len(), 1);
    assert_eq!(
        third.entries()[0].id().value().get(),
        u64::try_from(total * 2)?
    );
    assert_eq!(third.next_cursor(), None);
    let absent_cursor = repository
        .inventory(Some(StoragePuzzleFixture::id(5)?))
        .await?;
    assert_eq!(
        absent_cursor.entries()[0].id(),
        StoragePuzzleFixture::id(6)?
    );
    assert!(
        repository
            .inventory(Some(third.entries()[0].id()))
            .await?
            .entries()
            .is_empty()
    );
    let last_full = repository
        .inventory(Some(StoragePuzzleFixture::id(u64::try_from(
            (total - StoragePuzzleRepository::INVENTORY_PAGE_SIZE) * 2,
        )?)?))
        .await?;
    assert_eq!(
        last_full.entries().len(),
        StoragePuzzleRepository::INVENTORY_PAGE_SIZE
    );
    let final_cursor = last_full
        .next_cursor()
        .ok_or("missing final full-page cursor")?;
    assert_eq!(final_cursor, third.entries()[0].id());
    let exhausted = repository.inventory(Some(final_cursor)).await?;
    assert!(exhausted.entries().is_empty());
    assert_eq!(exhausted.next_cursor(), None);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_orders_unsigned_identities_and_preserves_maximum_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let signed_maximum = u64::try_from(i64::MAX)?;
    let ids = [
        1,
        9,
        10,
        signed_maximum,
        signed_maximum + 1,
        signed_maximum + 2,
        u64::MAX - 1,
        u64::MAX,
    ];
    for value in ids.into_iter().rev() {
        StoragePuzzleFixture::save_initial(&repository, StoragePuzzleFixture::id(value)?).await?;
    }
    let connection = rusqlite::Connection::open(path)?;
    connection.execute(
        "UPDATE puzzle_runs SET storage_revision = ?1 WHERE run_id = ?1",
        [u64::MAX.to_string()],
    )?;
    let page = repository.inventory(None).await?;
    let actual: Vec<u64> = page
        .entries()
        .iter()
        .map(|entry| entry.id().value().get())
        .collect();
    assert_eq!(actual, ids);
    assert_eq!(
        page.entries()
            .last()
            .ok_or("missing maximum identity")?
            .storage_revision()
            .value(),
        u64::MAX
    );
    let after = repository
        .inventory(Some(StoragePuzzleFixture::id(signed_maximum + 1)?))
        .await?;
    assert_eq!(
        after
            .entries()
            .iter()
            .map(|entry| entry.id().value().get())
            .collect::<Vec<_>>(),
        ids[5..]
    );
    assert!(
        repository
            .inventory(Some(StoragePuzzleFixture::id(u64::MAX)?))
            .await?
            .entries()
            .is_empty()
    );
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_refuses_noncanonical_identities()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    StoragePuzzleFixture::save_initial(&repository, StoragePuzzleFixture::id(1)?).await?;
    let connection = rusqlite::Connection::open(path)?;
    for invalid in StoragePuzzleFixture::INVALID_DECIMALS {
        connection.execute("INSERT INTO puzzle_runs (run_id, collection_id, storage_revision, event_count, journal_version, journal_digest) SELECT ?1, collection_id, storage_revision, event_count, journal_version, journal_digest FROM puzzle_runs WHERE run_id = '1'", [invalid])?;
        connection.execute("INSERT INTO puzzle_checkpoints (run_id, body) SELECT ?1, body FROM puzzle_checkpoints WHERE run_id = '1'", [invalid])?;
        assert!(
            matches!(
                repository.inventory(None).await,
                Err(StoragePuzzleError::Corrupt)
            ),
            "accepted identity {invalid:?}"
        );
        connection.execute(
            "DELETE FROM puzzle_checkpoints WHERE run_id = ?1",
            [invalid],
        )?;
        connection.execute("DELETE FROM puzzle_runs WHERE run_id = ?1", [invalid])?;
    }
    assert_eq!(repository.inventory(None).await?.entries().len(), 1);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_refuses_malformed_revisions_without_partial_success()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    for value in [1, 2] {
        StoragePuzzleFixture::save_initial(&repository, StoragePuzzleFixture::id(value)?).await?;
    }
    let connection = rusqlite::Connection::open(path)?;
    for invalid in StoragePuzzleFixture::INVALID_DECIMALS {
        connection.execute(
            "UPDATE puzzle_runs SET storage_revision = ?1 WHERE run_id = '2'",
            [invalid],
        )?;
        assert!(
            matches!(
                repository.inventory(None).await,
                Err(StoragePuzzleError::Corrupt)
            ),
            "accepted revision {invalid:?}"
        );
    }
    connection.execute(
        "UPDATE puzzle_runs SET storage_revision = '1' WHERE run_id = '2'",
        [],
    )?;
    assert_eq!(repository.inventory(None).await?.entries().len(), 2);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_exposes_bounded_event_metadata_without_proving_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let id = StoragePuzzleFixture::id(1)?;
    StoragePuzzleFixture::save_initial(&repository, id).await?;
    let connection = rusqlite::Connection::open(path)?;
    for count in [-1, i64::try_from(PuzzlePersistenceCodec::MAX_EVENTS)? + 1] {
        assert!(matches!(
            connection.execute("UPDATE puzzle_runs SET event_count = ?1", [count]),
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation
        ));
    }
    connection.execute(
        "UPDATE puzzle_runs SET event_count = ?1",
        [i64::try_from(PuzzlePersistenceCodec::MAX_EVENTS)?],
    )?;
    assert_eq!(
        repository.inventory(None).await?.entries()[0].persisted_event_count(),
        PuzzlePersistenceCodec::MAX_EVENTS
    );
    assert!(matches!(
        repository.load(id).await,
        Err(StoragePuzzleError::Corrupt)
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_refuses_missing_collection_and_checkpoint_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    StoragePuzzleFixture::save_initial(&repository, StoragePuzzleFixture::id(1)?).await?;
    let connection = rusqlite::Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", "OFF")?;
    connection.execute("UPDATE puzzle_runs SET collection_id = 'missing'", [])?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    assert!(connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, bool>(0))?);
    assert!(matches!(
        repository.inventory(None).await,
        Err(StoragePuzzleError::Corrupt)
    ));
    connection.execute("UPDATE puzzle_runs SET collection_id = '1'", [])?;
    assert_eq!(repository.inventory(None).await?.entries().len(), 1);
    connection.execute("DELETE FROM puzzle_checkpoints", [])?;
    assert!(matches!(
        repository.inventory(None).await,
        Err(StoragePuzzleError::Corrupt)
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_does_not_decode_collection_checkpoint_or_journal_bodies()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("puzzles.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let mut owner = StoragePuzzleFixture::owner()?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Pause(
            chess::puzzle::timer::PuzzleTimerEnumPauseReason::Manual,
        ),
        ClockInstant::from_nanos(0),
    )?;
    repository
        .begin_save(StoragePuzzleFixture::request(
            &mut owner,
            ClockInstant::from_nanos(0),
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?)?
        .wait()
        .await?;
    let id = owner.run().context().session_id();
    assert!(repository.load(id).await?.is_some());
    let original = repository.inventory(None).await?;
    assert_eq!(original.entries()[0].persisted_event_count(), 1);
    let connection = rusqlite::Connection::open(path)?;
    let journal: String =
        connection.query_row("SELECT body FROM puzzle_events", [], |row| row.get(0))?;
    let checkpoint: String =
        connection.query_row("SELECT body FROM puzzle_checkpoints", [], |row| row.get(0))?;
    let collection: String =
        connection.query_row("SELECT body FROM puzzle_collections", [], |row| row.get(0))?;
    for (damage, restore, body) in [
        (
            "INSERT OR REPLACE INTO puzzle_events (run_id, ordinal, body) SELECT run_id, ordinal, '{}' FROM puzzle_events",
            "INSERT OR REPLACE INTO puzzle_events (run_id, ordinal, body) SELECT run_id, ordinal, ?1 FROM puzzle_events",
            journal,
        ),
        (
            "UPDATE puzzle_checkpoints SET body = '{}'",
            "UPDATE puzzle_checkpoints SET body = ?1",
            checkpoint,
        ),
        (
            "INSERT OR REPLACE INTO puzzle_collections (collection_id, schema_version, digest, body) SELECT collection_id, schema_version, digest, '{}' FROM puzzle_collections",
            "INSERT OR REPLACE INTO puzzle_collections (collection_id, schema_version, digest, body) SELECT collection_id, schema_version, digest, ?1 FROM puzzle_collections",
            collection,
        ),
    ] {
        assert_eq!(connection.execute(damage, [])?, 1);
        assert_eq!(
            repository.inventory(None).await?.entries(),
            original.entries()
        );
        assert!(repository.load(id).await.is_err());
        assert_eq!(connection.execute(restore, [&body])?, 1);
        assert!(repository.load(id).await?.is_some());
    }
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_keeps_accepted_admission_bounded_after_caller_cancellation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store =
        StorageDocumentStore::open(directory.path().join("puzzles.sqlite3"), budget.clone())
            .await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let held = budget.try_lease(1)?;
    let mut waiting = Vec::new();
    for _ in 0..StorageDocumentStore::QUEUE_CAPACITY {
        let mut request = Box::pin(repository.inventory(None));
        StoragePuzzleFixture::assert_pending(request.as_mut()).await;
        waiting.push(request);
    }
    drop(waiting.pop().ok_or("missing accepted inventory request")?);
    assert!(matches!(
        repository.inventory(None).await,
        Err(StoragePuzzleError::Document(
            StorageDocumentError::Saturated
        ))
    ));
    drop(held);
    for request in waiting {
        assert!(request.await?.entries().is_empty());
    }
    assert!(repository.inventory(None).await?.entries().is_empty());
    assert_eq!(budget.available(), 1);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn saved_run_inventory_borrowed_wait_allows_shutdown_and_preserves_accepted_reply()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store =
        StorageDocumentStore::open(directory.path().join("puzzles.sqlite3"), budget.clone())
            .await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let held = budget.try_lease(1)?;
    let mut waiting = Box::pin(repository.inventory(None));
    StoragePuzzleFixture::assert_pending(waiting.as_mut()).await;
    let mut closing = Box::pin(store.close());
    StoragePuzzleFixture::assert_pending(closing.as_mut()).await;
    assert!(matches!(
        repository.inventory(None).await,
        Err(StoragePuzzleError::Document(StorageDocumentError::Closed))
    ));
    drop(held);
    tokio::time::timeout(Duration::from_secs(3), closing).await??;
    assert!(waiting.await?.entries().is_empty());
    assert!(matches!(
        repository.inventory(None).await,
        Err(StoragePuzzleError::Document(StorageDocumentError::Closed))
    ));
    Ok(())
}
