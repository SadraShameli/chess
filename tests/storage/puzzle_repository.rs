use std::future::{Future, poll_fn};
use std::num::{NonZeroU64, NonZeroUsize};
use std::task::Poll;

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
    PuzzlePersistenceEnumCommand, PuzzlePersistenceRun, PuzzlePersistenceSaveRequest,
    PuzzlePersistenceSnapshot, PuzzlePersistenceStorageRevision,
};
use chess::puzzle::random_provider::PuzzleRandomProviderSeeded;
use chess::puzzle::run::{PuzzleRunEnumContent, PuzzleRunEnumStatus};
use chess::puzzle::selection::PuzzleSelectionFilter;
use chess::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use chess::puzzle::storage_repository::PuzzleStorageDriver;
use chess::puzzle::timer::PuzzleTimerEnumReadiness;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::puzzle_repository::{StoragePuzzleError, StoragePuzzleRepository};

struct StoragePuzzleFixture;

impl StoragePuzzleFixture {
    fn owner() -> Result<PuzzlePersistenceRun, Box<dyn std::error::Error>> {
        let initial = BoardPosition::standard();
        let movement = initial.parse_uci("e2e4")?;
        let definition = PuzzleDefinition::new(
            PuzzleDefinitionIdentity::new("fixture".into(), "v1".into(), "first".into())?,
            initial,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default(),
        )?;
        Ok(PuzzlePersistenceRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![definition],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(100),
        )?)
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
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 2);
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
        Some(StoragePuzzleError::Sqlite(_))
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
