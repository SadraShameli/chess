use std::future::{Future, poll_fn};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::time::Instant;

use crate::board::position::BoardPosition;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::ClockInstant;
use crate::clock::time_provider::{ClockTimeProvider, ClockTimeProviderMonotonic};
use crate::desktop::application::DesktopApplication;
use crate::desktop::messages::{DesktopCommand, DesktopMessagesEntityId};
use crate::desktop::native::bootstrap::DesktopNativeBootstrapLifecycle;
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
use crate::desktop::native::puzzle_runtime::DesktopNativePuzzleRuntimeSchedulerTokio;
use crate::desktop::native::puzzle_service::{
    DesktopNativePuzzleService, DesktopNativePuzzleServiceOptions,
};
use crate::desktop::puzzle_commands::DesktopPuzzleCommands;
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceEnumOperation,
    DesktopPuzzleServiceProvider, DesktopPuzzleServiceRequest,
};
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::PuzzleEnumCountdownMode;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::PuzzleDefinitionMetadata;
use crate::puzzle::persistence::{PuzzlePersistenceRun, PuzzlePersistenceStorageRevision};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::selection::PuzzleSelectionFilter;
use crate::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::puzzle_repository::StoragePuzzleRepository;

struct DesktopNativeBootstrapPuzzleFixture;

impl DesktopNativeBootstrapPuzzleFixture {
    fn owner() -> Result<PuzzlePersistenceRun, Box<dyn std::error::Error>> {
        let position = BoardPosition::standard();
        let movement = position.parse_uci("e2e4")?;
        let definition = PuzzleDefinition::new(
            PuzzleDefinitionIdentity::new("bootstrap".into(), "1".into(), "one".into())?,
            position,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default(),
        )?;
        Ok(PuzzlePersistenceRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![definition],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(0),
        )?)
    }
}

#[tokio::test]
async fn dropped_puzzle_cleanup_retains_ownership_before_database_close()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget.clone())
            .await?;
    let repository = StoragePuzzleRepository::new(store.client());
    let clock = Arc::new(ClockTimeProviderMonotonic::new());
    let wake = Arc::new(DesktopNativeEventLoopWake::new());
    let mut initial = DesktopPuzzleCommands::new(
        DesktopNativeBootstrapPuzzleFixture::owner()?,
        repository.clone(),
        ClockInstant::from_nanos(0),
        PuzzlePersistenceStorageRevision::INITIAL,
        0,
    )?;
    initial.begin_write(clock.now()?)?;
    while initial.has_retained_write() {
        initial.wait().await?;
        initial.settle(clock.now()?)?;
    }
    drop(initial);
    let page = repository.inventory(None).await?;
    let saved = *page.entries().first().ok_or("missing saved puzzle run")?;
    drop(page);
    let mut puzzles = DesktopNativePuzzleService::new(DesktopNativePuzzleServiceOptions {
        repository,
        executor: Arc::clone(&executor),
        clock: Arc::clone(&clock),
        wake: Arc::clone(&wake),
        runtime: Handle::current(),
        scheduler: DesktopNativePuzzleRuntimeSchedulerTokio::new(
            Handle::current(),
            clock.now()?,
            Instant::now(),
        ),
        replacement_timeout: Duration::from_secs(3),
    });
    let port = puzzles.start()?;
    let mut state = port.state();
    let lifecycle = DesktopNativeBootstrapLifecycle::new();
    {
        let mut services = lifecycle.services.lock().await;
        services.storage = Some(store);
        services.puzzles = Some(puzzles);
    }
    let held = budget.try_lease(1)?;
    let application = DesktopApplication::new(clock, wake);
    let request = application.port().request(DesktopCommand::puzzle(
        DesktopMessagesEntityId::new(),
        DesktopPuzzleServiceEnumOperation::OpenSaved {
            id: saved.id(),
            storage_revision: saved.storage_revision(),
        },
    ))?;
    let receipt = application.port().try_submit(&request)?;
    let mut opened = port.submit(DesktopPuzzleServiceRequest {
        receipt,
        operation: DesktopPuzzleServiceEnumOperation::OpenSaved {
            id: saved.id(),
            storage_revision: saved.storage_revision(),
        },
    })?;
    loop {
        if matches!(
            state.borrow_and_update().activity(),
            DesktopPuzzleServiceEnumActivity::Loading { id } if id == saved.id()
        ) {
            break;
        }
        state.changed().await?;
    }
    let original = Instant::now() + Duration::from_secs(5);
    let mut waiting = Box::pin(lifecycle.cleanup_feature_services(original));
    let was_pending =
        poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context).is_pending())).await;
    drop(waiting);
    let (retained, open_database) = {
        let services = lifecycle.services.try_lock()?;
        (
            services
                .puzzle_cleanup
                .as_ref()
                .is_some_and(|cleanup| cleanup.deadline == original),
            services
                .storage
                .as_ref()
                .is_some_and(|store| !store.is_closed()),
        )
    };
    drop(held);
    lifecycle.cleanup(original + Duration::from_secs(5)).await?;
    let open_refused = opened.wait().await.is_err();
    let fully_observed = lifecycle.services.lock().await.is_shutdown_complete();
    drop(lifecycle);
    drop(application);
    Arc::try_unwrap(executor)
        .map_err(|_executor| "puzzle cleanup retains its executor")?
        .shutdown()
        .await?;
    directory.close()?;
    assert!(was_pending);
    assert!(retained);
    assert!(open_database);
    assert!(open_refused);
    assert!(fully_observed);
    Ok(())
}
