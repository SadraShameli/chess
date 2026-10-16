use std::num::NonZeroUsize;
use std::sync::Arc;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::storage::document_store::StorageDocumentStore;

type DesktopGameSupervisorTestResult = Result<(), Box<dyn std::error::Error>>;

pub(crate) struct DesktopGameSupervisorFixtureStorage {
    pub(crate) directory: tempfile::TempDir,
    pub(crate) budget: CapacityBudget,
    pub(crate) executor: Arc<CapacityExecutor>,
    pub(crate) store: StorageDocumentStore,
}

impl DesktopGameSupervisorFixtureStorage {
    pub(crate) async fn new(workers: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::new(workers).ok_or("invalid capacity")?)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget.clone())
                .await?;
        Ok(Self {
            directory,
            budget,
            executor,
            store,
        })
    }

    pub(crate) async fn close(self) -> DesktopGameSupervisorTestResult {
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_owner| "executor still borrowed")?
            .shutdown()
            .await?;
        self.directory.close()?;
        Ok(())
    }
}
