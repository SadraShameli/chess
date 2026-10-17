use std::io::Cursor;
use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::Arc;

use crate::board::position::BoardPosition;

use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumAdmission, DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::annotation_owner::{DesktopAnnotationOwner, DesktopAnnotationOwnerResources};
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationReceipt, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider, DesktopApplicationWork,
};
use crate::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot, DesktopEnumValue};
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorPublication,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::game::enums::{GameId, GameMoveId, GameRevision};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::storage_repository::{
    GameStorageRevision, GameStorageWallTimeError, GameStorageWallTimeProvider,
};
use crate::notation::pgn_import::NotationPgnImport;
use crate::presentation::analysis::editor_intent::{
    PresentationAnalysisEditorError, PresentationAnalysisEditorHistory,
    PresentationAnalysisEditorIntent,
};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::library_notation::StorageLibraryNotationRepository;
use crate::storage::library_prepared::StorageLibraryEnumContentChange;
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};
use crate::storage::library_repository::StorageLibraryEnumMutationReceipt;

pub(crate) type PresentationAnalysisEditorTestResult<T = ()> =
    Result<T, Box<dyn std::error::Error>>;

pub(crate) struct PresentationAnalysisEditorTestTime;

impl ClockTimeProvider for PresentationAnalysisEditorTestTime {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(41))
    }
}

#[test]
fn completed_history_evicts_recent_entries_without_evicting_pending_inputs()
-> PresentationAnalysisEditorTestResult {
    let mut history = PresentationAnalysisEditorHistory::default();
    let pending = PresentationAnalysisEditorTestFixture::request(5000)?;
    assert!(history.begin(pending)?);
    for ordinal in 1..=768 {
        let request = PresentationAnalysisEditorTestFixture::request(5000 + ordinal)?;
        assert!(history.begin(request)?);
        history.complete(request);
        assert!(!history.record(request));
        assert!(!history.begin(pending)?);
    }
    assert_eq!(history.pending.len(), 1);
    assert_eq!(
        history.recent.len(),
        PresentationAnalysisEditorHistory::CAPACITY
    );
    assert!(!history.contains(PresentationAnalysisEditorTestFixture::request(5001)?));
    assert!(history.contains(pending));
    history.complete(pending);
    assert!(history.pending.is_empty());
    assert!(!history.begin(pending)?);
    Ok(())
}

impl GameStorageWallTimeProvider for PresentationAnalysisEditorTestTime {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        Ok(GameLibraryOrganizationTimestamp::new(1000)?)
    }
}

pub(crate) struct PresentationAnalysisEditorTestWake;

impl DesktopApplicationWakeProvider for PresentationAnalysisEditorTestWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

pub(crate) struct PresentationAnalysisEditorTestFixture {
    _directory: tempfile::TempDir,
    pub(crate) store: StorageDocumentStore,
    executor: Arc<CapacityExecutor>,
    application:
        DesktopApplication<PresentationAnalysisEditorTestTime, PresentationAnalysisEditorTestWake>,
}

pub(crate) struct PresentationAnalysisEditorTestClosedStorage {
    _directory: tempfile::TempDir,
    executor: Arc<CapacityExecutor>,
}

impl PresentationAnalysisEditorTestClosedStorage {
    pub(crate) async fn close(self) -> PresentationAnalysisEditorTestResult {
        Arc::try_unwrap(self.executor)
            .map_err(|_executor| "retained executor")?
            .shutdown()
            .await?;
        Ok(())
    }
}

impl PresentationAnalysisEditorTestFixture {
    pub(crate) async fn new() -> PresentationAnalysisEditorTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let store =
            StorageDocumentStore::open(directory.path().join("panels.sqlite"), budget.clone())
                .await?;
        Ok(Self {
            _directory: directory,
            store,
            executor: Arc::new(CapacityExecutor::new(budget)?),
            application: DesktopApplication::new(
                Arc::new(PresentationAnalysisEditorTestTime),
                Arc::new(PresentationAnalysisEditorTestWake),
            ),
        })
    }

    pub(crate) fn request(value: u128) -> PresentationAnalysisEditorTestResult<DesktopRequestId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    pub(crate) fn node(value: u128) -> PresentationAnalysisEditorTestResult<GameMoveId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    pub(crate) fn record(
        value: u128,
        text: &str,
    ) -> PresentationAnalysisEditorTestResult<StorageLibraryRecord> {
        let mut node = 0_u128;
        let mut parser = NotationPgnImport::new(Cursor::new(text), move || {
            node += 1;
            uuid::Uuid::from_u128(node).to_string().parse()
        });
        let game = parser.next_game(&|| false)?.ok_or("missing notation")?;
        let id: GameId = uuid::Uuid::from_u128(value).to_string().parse()?;
        Ok(StorageLibraryRecord::new(
            id,
            StorageLibraryEnumContent::Notation(Box::new(game)),
        )?)
    }

    pub(crate) fn receipt(
        &self,
    ) -> PresentationAnalysisEditorTestResult<DesktopApplicationReceipt> {
        Ok(self.receipt_work()?.0)
    }

    pub(crate) fn completed_receipt(
        &self,
    ) -> PresentationAnalysisEditorTestResult<DesktopApplicationReceipt> {
        let (receipt, work) = self.receipt_work()?;
        self.application.complete_value(
            &work,
            DesktopEnumValue::Bootstrap(DesktopBootstrapSnapshot::new(
                &BoardPosition::standard(),
                NonZeroU32::MIN,
                DesktopEnumEngineSnapshot::Unavailable {
                    message: "The notation fixture has no engine".to_owned(),
                },
            )),
            None,
        )?;
        Ok(receipt)
    }

    fn receipt_work(
        &self,
    ) -> PresentationAnalysisEditorTestResult<(DesktopApplicationReceipt, DesktopApplicationWork)>
    {
        let port = self.application.port();
        let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        let receipt = port.try_submit(&request)?;
        let work = self
            .application
            .try_next()?
            .ok_or("missing admitted work")?;
        assert!(receipt.shares_outcome(work.receipt()));
        Ok((receipt, work))
    }

    pub(crate) fn publish(
        &self,
        record: &StorageLibraryRecord,
        selected: GameMoveId,
        binding: DesktopGameEditorBinding,
    ) -> PresentationAnalysisEditorTestResult<DesktopGameEditorPublication> {
        let editor = DesktopGameEditor::notation(record, GameRevision::ZERO, selected, binding)?;
        Ok(editor.publish(editor.context(), &self.store.client())?)
    }

    pub(crate) async fn open(
        &self,
        value: u128,
        text: &str,
    ) -> PresentationAnalysisEditorTestResult<DesktopAnnotationOwner> {
        let record = Arc::new(Self::record(value, text)?);
        let repository = StorageLibraryNotationRepository::new(self.store.client(), &self.executor);
        let prepared = repository
            .prepare(
                GameStorageRevision::INITIAL,
                Arc::clone(&record),
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                    10,
                )?),
            )
            .await?;
        let mut mutation = repository.begin_save(prepared)?;
        let StorageLibraryEnumMutationReceipt::Saved(saved) = mutation.wait().await? else {
            return Err("missing initial save".into());
        };
        let record = Arc::try_unwrap(record).map_err(|_record| "retained preparation")?;
        let receipt = self.receipt()?;
        let mut owner = DesktopAnnotationOwner::open(
            record,
            saved.content_revision(),
            receipt.clone(),
            DesktopAnnotationOwnerResources {
                executor: Arc::clone(&self.executor),
                client: self.store.client(),
                wall_time: Arc::new(PresentationAnalysisEditorTestTime),
            },
        )?;
        owner.observe_compute(&receipt).await?;
        Ok(owner)
    }

    pub(crate) async fn execute(
        &self,
        owner: &mut DesktopAnnotationOwner,
        intent: PresentationAnalysisEditorIntent,
        receipt: &DesktopApplicationReceipt,
    ) -> PresentationAnalysisEditorTestResult<DesktopAnnotationCommandsEnumResult> {
        match owner.submit(intent.admit(receipt.clone())?)? {
            DesktopAnnotationCommandsEnumAdmission::Ready(result) => Ok(*result),
            DesktopAnnotationCommandsEnumAdmission::ComputePending => {
                Ok(owner.observe_compute(receipt).await?)
            }
            DesktopAnnotationCommandsEnumAdmission::SavePending
            | DesktopAnnotationCommandsEnumAdmission::PendingSnapshot { .. } => {
                Err("unexpected pending source".into())
            }
        }
    }

    pub(crate) fn checkpoint(
        &self,
        owner: &mut DesktopAnnotationOwner,
    ) -> PresentationAnalysisEditorTestResult<DesktopApplicationReceipt> {
        let receipt = self.receipt()?;
        let admission = owner.submit(DesktopAnnotationCommands::new(
            receipt.clone(),
            owner.snapshot()?.context(),
            DesktopAnnotationCommandsEnumAction::Checkpoint,
        ))?;
        assert!(matches!(
            admission,
            DesktopAnnotationCommandsEnumAdmission::SavePending
        ));
        Ok(receipt)
    }

    pub(crate) async fn close(self) -> PresentationAnalysisEditorTestResult {
        self.close_storage().await?.close().await
    }

    pub(crate) async fn close_storage(
        self,
    ) -> PresentationAnalysisEditorTestResult<PresentationAnalysisEditorTestClosedStorage> {
        self.store.close().await?;
        Ok(PresentationAnalysisEditorTestClosedStorage {
            _directory: self._directory,
            executor: self.executor,
        })
    }
}

#[tokio::test]
async fn intent_attaches_only_the_actual_admitted_request_and_complete_context()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let owner = fixture.open(100, "1. e4 *").await?;
    let receipt = fixture.receipt()?;
    let foreign = fixture.receipt()?;
    let context = owner.snapshot()?.context();
    let intent = PresentationAnalysisEditorIntent::new(
        receipt.request_id(),
        context,
        DesktopAnnotationCommandsEnumAction::Snapshot,
    );
    assert!(matches!(
        intent.clone().admit(foreign),
        Err(PresentationAnalysisEditorError::ForeignReceipt)
    ));
    let command = intent.admit(receipt.clone())?;
    assert_eq!(command.expected(), context);
    assert!(command.receipt().shares_outcome(&receipt));
    drop((command, owner));
    fixture.close().await
}
