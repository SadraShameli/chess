use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use rand::{SeedableRng, rngs::StdRng};
use rusqlite::{Connection, OpenFlags};
use tokio::time::Instant;

use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::game_bridge::DesktopGameBridgeProvider;
use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryError, DesktopGameRegistryFactoryProvider,
    DesktopGameRegistryOptions,
};
use crate::desktop::game_supervisor::{
    DesktopGameSupervisorEngineFactory, DesktopGameSupervisorError,
};
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestProof, DesktopLibraryServiceTestResult,
    DesktopLibraryServiceTestWall,
};
use crate::engine::live_game::EngineLiveGame;
use crate::engine::process_provider::EngineNativeProcess;
use crate::game::engine_provider::GameEngineContext;
use crate::game::enums::GameId;
use crate::storage::document_store::StorageDocumentStore;

#[derive(Default)]
struct DesktopGameRegistrySavedOpenReviewClock {
    fail_initialization: AtomicBool,
    reads: AtomicUsize,
}

impl ClockTimeProvider for DesktopGameRegistrySavedOpenReviewClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_initialization.swap(false, Ordering::SeqCst) {
            Err(ClockDurationError::Overflow)
        } else {
            Ok(ClockInstant::from_nanos(50))
        }
    }
}

struct DesktopGameRegistrySavedOpenReviewFactories {
    clock: Arc<DesktopGameRegistrySavedOpenReviewClock>,
    constructions: Arc<AtomicUsize>,
    failures: usize,
}

struct DesktopGameRegistrySavedOpenReviewFactory;

impl DesktopGameRegistryFactoryProvider for DesktopGameRegistrySavedOpenReviewFactories {
    type Factory = DesktopGameRegistrySavedOpenReviewFactory;
    type Error = std::io::Error;

    fn factory(&mut self, _game: GameId) -> Result<Self::Factory, Self::Error> {
        let construction = self.constructions.fetch_add(1, Ordering::SeqCst);
        self.clock
            .fail_initialization
            .store(construction < self.failures, Ordering::SeqCst);
        Ok(DesktopGameRegistrySavedOpenReviewFactory)
    }
}

impl DesktopGameSupervisorEngineFactory for DesktopGameRegistrySavedOpenReviewFactory {
    type Engine = EngineLiveGame<EngineNativeProcess, DesktopGameRegistrySavedOpenReviewClock>;
    type Error = std::io::Error;

    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        Err(std::io::Error::other(
            "saved local construction requested an engine",
        ))
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct DesktopGameRegistrySavedOpenReviewStoredEntry {
    id: String,
    document_revision: String,
    body: String,
    content_revision: String,
    modified: Option<i64>,
    location: String,
    digest: String,
    catalog: String,
}

impl DesktopGameRegistrySavedOpenReviewStoredEntry {
    async fn read_all(
        fixture: &DesktopLibraryServiceFixture,
    ) -> DesktopLibraryServiceTestResult<Vec<Self>> {
        let path = fixture.directory.path().join("library.sqlite");
        Ok(fixture
            .executor
            .submit_waiting(move |_cancellation| {
                let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                let mut statement = connection.prepare(
                    "SELECT d.id, d.revision, d.body, e.content_revision, e.modified_ms, e.location, hex(e.content_sha256), s.revision FROM documents d JOIN library_entries e ON e.game_id = d.id CROSS JOIN library_state s ORDER BY d.id",
                )?;
                statement
                    .query_map([], |row| {
                        Ok(Self {
                            id: row.get(0)?,
                            document_revision: row.get(1)?,
                            body: row.get(2)?,
                            content_revision: row.get(3)?,
                            modified: row.get(4)?,
                            location: row.get(5)?,
                            digest: row.get(6)?,
                            catalog: row.get(7)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, rusqlite::Error>>()
            })
            .await?
            .join()
            .await??)
    }
}

#[tokio::test]
async fn failed_saved_supervisor_initialization_releases_each_source_reservation_without_registry_shutdown()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let first = fixture.seed(100, false).await?;
    let second = fixture.seed(101, false).await?;
    let before = DesktopGameRegistrySavedOpenReviewStoredEntry::read_all(&fixture).await?;
    let initial_memory = fixture.available_memory()?;
    let clock = Arc::new(DesktopGameRegistrySavedOpenReviewClock::default());
    let constructions = Arc::new(AtomicUsize::new(0));
    let (mut registry, port) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
        factory_provider: DesktopGameRegistrySavedOpenReviewFactories {
            clock: Arc::clone(&clock),
            constructions: Arc::clone(&constructions),
            failures: 2,
        },
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::clone(&clock),
        wall_time: Arc::<DesktopLibraryServiceTestWall>::clone(&fixture.wall),
        proof: Arc::new(DesktopLibraryServiceTestProof),
        random: StdRng::seed_from_u64(7),
    });
    let mut observations = Vec::new();
    for (id, revision) in [(100, first), (101, second)] {
        let game = DesktopLibraryServiceFixture::id(id)?;
        let application = fixture.receipt()?;
        let mut receipt = port.try_open_saved_editor(game, revision, application.clone())?;
        let outcome = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                result = receipt.wait() => Ok::<_, Box<dyn std::error::Error>>(result),
                result = registry.run() => Err(format!("registry stopped before constructor outcome: {result:?}").into()),
            }
        })
        .await??;
        let initialization_failed = matches!(
            &outcome,
            Err(error) if matches!(error.as_ref(), DesktopGameRegistryError::Supervisor(error)
                if matches!(error.as_ref(), DesktopGameSupervisorError::Time(ClockDurationError::Overflow)))
        );
        let correlated = receipt.application_receipt().shares_outcome(&application);
        let no_editor = receipt.editor_receipt().is_none();
        let no_actor = port.snapshots().borrow().is_empty()
            && matches!(
                port.try_open_editor(game, fixture.receipt()?),
                Err(DesktopGameRegistryError::UnknownGame)
            );
        let memory = fixture.available_memory()?;
        let stored = DesktopGameRegistrySavedOpenReviewStoredEntry::read_all(&fixture).await?;
        observations.push((
            initialization_failed,
            correlated,
            no_editor,
            no_actor,
            memory,
            stored,
        ));
        drop((outcome, receipt, application));
    }
    let clock_reads = clock.reads.load(Ordering::SeqCst);
    let factory_constructions = constructions.load(Ordering::SeqCst);
    let utc_reads = fixture.wall.calls.load(Ordering::SeqCst);
    registry
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    let stopped = registry.is_shutdown_complete();
    drop((registry, port, clock, constructions));
    let memory_observer = fixture.store.client();
    let budget = fixture.budget.clone();
    fixture.close().await?;
    let released = memory_observer
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?
        .num_permits();
    let capacity = budget.available();
    drop((memory_observer, budget));
    assert!(stopped);
    assert_eq!(before.len(), 2);
    assert_eq!(clock_reads, 2);
    assert_eq!(factory_constructions, 2);
    assert_eq!(utc_reads, 0);
    assert_eq!(initial_memory, StorageDocumentStore::MEMORY_BUDGET);
    assert_eq!(released, initial_memory);
    assert_eq!(capacity, 1);
    for (failed, correlated, no_editor, no_actor, memory, stored) in observations {
        assert!(failed && correlated && no_editor && no_actor);
        assert_eq!(stored, before);
        assert_eq!(
            memory, initial_memory,
            "failed opening retained a loaded source reservation before registry shutdown"
        );
    }
    Ok(())
}

#[tokio::test]
async fn constructor_failure_then_same_game_retry_retains_only_the_installed_source()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, false).await?;
    let game = DesktopLibraryServiceFixture::id(100)?;
    let clock = Arc::new(DesktopGameRegistrySavedOpenReviewClock::default());
    let constructions = Arc::new(AtomicUsize::new(0));
    let (mut registry, port) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
        factory_provider: DesktopGameRegistrySavedOpenReviewFactories {
            clock: Arc::clone(&clock),
            constructions: Arc::clone(&constructions),
            failures: 1,
        },
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::clone(&clock),
        wall_time: Arc::<DesktopLibraryServiceTestWall>::clone(&fixture.wall),
        proof: Arc::new(DesktopLibraryServiceTestProof),
        random: StdRng::seed_from_u64(7),
    });
    let mut observations = Vec::new();
    for expected in [
        revision,
        revision,
        crate::game::storage_repository::GameStorageRevision::new(999),
    ] {
        let mut receipt = port.try_open_saved_editor(game, expected, fixture.receipt()?)?;
        let outcome = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                result = receipt.wait() => Ok::<_, Box<dyn std::error::Error>>(result),
                result = registry.run() => Err(format!("registry stopped before retry: {result:?}").into()),
            }
        }).await??;
        let context = match &outcome {
            Ok(
                crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumResult::Snapshot(
                    snapshot,
                ),
            ) => Some(snapshot.context()),
            Err(_) => None,
            Ok(_) => return Err("unexpected saved-open retry response".into()),
        };
        let failed = matches!(&outcome, Err(error) if matches!(error.as_ref(),
            DesktopGameRegistryError::Supervisor(error) if matches!(error.as_ref(),
                DesktopGameSupervisorError::Time(ClockDurationError::Overflow))));
        drop((outcome, receipt));
        observations.push((failed, context, fixture.available_memory()?));
    }
    let factory_constructions = constructions.load(Ordering::SeqCst);
    registry
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    drop((registry, port, clock, constructions));
    let released = fixture.available_memory()?;
    fixture.close().await?;
    assert!(observations[0].0);
    assert!(observations[0].1.is_none());
    assert_eq!(observations[0].2, StorageDocumentStore::MEMORY_BUDGET);
    assert!(observations[1].1.is_some());
    assert_eq!(observations[1].1, observations[2].1);
    assert!(observations[1].2 < StorageDocumentStore::MEMORY_BUDGET);
    assert!(observations[2].2 < StorageDocumentStore::MEMORY_BUDGET);
    assert_eq!(factory_constructions, 2);
    assert_eq!(released, StorageDocumentStore::MEMORY_BUDGET);
    Ok(())
}
