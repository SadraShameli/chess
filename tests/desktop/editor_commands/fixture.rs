use std::path::PathBuf;
use std::sync::Arc;

use crate::board::editor::{BoardEditorDraftId, BoardEditorSessionEnumOperation};
use crate::board::enums::BoardEnumVariant;
use crate::board::position::BoardPosition;
use crate::capacity::budget::CapacityBudget;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationWakeError, DesktopApplicationWakeProvider,
    DesktopApplicationWork,
};
use crate::desktop::application::{DesktopApplicationError, DesktopApplicationReceipt};
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::editor_commands::DesktopEditorCommandsEnumResult;
use crate::desktop::editor_commands::{
    DesktopEditorCommands, DesktopEditorCommandsEnumChoice, DesktopEditorCommandsEnumOperation,
    DesktopEditorCommandsOpening, DesktopEditorCommandsSnapshot,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::identity::DesktopRevision;
use crate::desktop::messages::DesktopRequest;
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};
use crate::presentation::editor::controller::PresentationEditorCommandPort;
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::editor_repository::StorageEditorRepository;

pub(crate) type DesktopEditorCommandsTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(crate) struct DesktopEditorCommandsFixture {
    pub(crate) owner: DesktopEditorCommands,
    pub(crate) repository: StorageEditorRepository,
    pub(crate) path: PathBuf,
    store: StorageDocumentStore,
    directory: tempfile::TempDir,
    application:
        DesktopApplication<DesktopEditorCommandsFixtureClock, DesktopEditorCommandsFixtureWake>,
    owner_id: DesktopMessagesEntityId,
    opening_sequence: u128,
}

struct DesktopEditorCommandsFixtureClock;

impl ClockTimeProvider for DesktopEditorCommandsFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(100))
    }
}

struct DesktopEditorCommandsFixtureWake;

impl DesktopApplicationWakeProvider for DesktopEditorCommandsFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

impl DesktopEditorCommandsFixture {
    pub(crate) async fn new() -> DesktopEditorCommandsTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("editor.sqlite3");
        let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
        let repository = StorageEditorRepository::new(store.client());
        Ok(Self {
            owner: DesktopEditorCommands::new(repository.clone()),
            repository,
            path,
            store,
            directory,
            application: DesktopApplication::new(
                Arc::new(DesktopEditorCommandsFixtureClock),
                Arc::new(DesktopEditorCommandsFixtureWake),
            ),
            owner_id: DesktopMessagesEntityId::for_editor(Self::id(1)?),
            opening_sequence: 0,
        })
    }

    pub(crate) fn id(value: u128) -> DesktopEditorCommandsTestResult<BoardEditorDraftId> {
        Ok(BoardEditorDraftId::try_from(uuid::Uuid::from_u128(value))?)
    }

    fn opening(
        &mut self,
        value: u128,
    ) -> DesktopEditorCommandsTestResult<DesktopEditorCommandsOpening> {
        self.opening_sequence = self
            .opening_sequence
            .checked_add(1)
            .ok_or("opening sequence exhausted")?;
        let sequence = self.opening_sequence;
        let session: DesktopRequestId =
            format!("00000000-0000-0001-0000-{sequence:012x}").parse()?;
        Ok(DesktopEditorCommandsOpening::new(Self::id(value)?, session))
    }

    pub(crate) fn admit(
        &self,
        operation: &DesktopEditorCommandsEnumOperation,
    ) -> DesktopEditorCommandsTestResult<DesktopApplicationWork> {
        let port = self.application.port();
        let request = self.prepare(operation.clone())?;
        let receipt = port.try_submit(&request)?;
        let work = self
            .application
            .try_next()?
            .ok_or("missing admitted editor work")?;
        if !receipt.shares_outcome(work.receipt()) {
            return Err("receipt changed".into());
        }
        self.application.begin_commit(&work)?;
        Ok(work)
    }

    pub(crate) fn next_work(&self) -> DesktopEditorCommandsTestResult<DesktopApplicationWork> {
        let work = self
            .application
            .try_next()?
            .ok_or("missing admitted editor work")?;
        self.application.begin_commit(&work)?;
        Ok(work)
    }

    pub(crate) async fn apply_work(
        &mut self,
        work: &DesktopApplicationWork,
    ) -> DesktopEditorCommandsTestResult<DesktopEditorCommandsEnumResult> {
        let DesktopEnumCommand::Editor { operation, .. } = work.request().command().operation()
        else {
            return Err("expected editor work".into());
        };
        let result = self.owner.handle(operation.clone(), work.receipt()).await?;
        self.complete_work(work, &result)?;
        Ok(result)
    }

    pub(crate) async fn dispatch(
        &mut self,
        operation: DesktopEditorCommandsEnumOperation,
    ) -> DesktopEditorCommandsTestResult<DesktopEditorCommandsEnumResult> {
        let work = self.admit(&operation)?;
        let result = self.owner.handle(operation, work.receipt()).await;
        match &result {
            Ok(value) => self.complete_work(&work, value)?,
            Err(error) => self.application.complete_failure(
                &work,
                crate::desktop::error::DesktopFailure::new(
                    crate::desktop::error::DesktopEnumFailureCode::InvalidInput,
                    error.to_string(),
                    false,
                ),
            )?,
        }
        Ok(result?)
    }

    fn complete_work(
        &self,
        work: &DesktopApplicationWork,
        result: &DesktopEditorCommandsEnumResult,
    ) -> Result<(), DesktopApplicationError> {
        let revision = self
            .owner
            .snapshot()
            .map(|snapshot| DesktopRevision::for_editor(snapshot.context().revision()));
        self.application.complete_value(
            work,
            DesktopEnumValue::Editor(Box::new(result.clone())),
            revision,
        )
    }

    pub(crate) fn snapshot(
        &self,
    ) -> DesktopEditorCommandsTestResult<DesktopEditorCommandsSnapshot> {
        self.owner
            .snapshot()
            .ok_or_else(|| "missing editor session".into())
    }

    pub(crate) async fn create(
        &mut self,
        id: u128,
        selected: Option<BoardPosition>,
    ) -> DesktopEditorCommandsTestResult {
        let opening = self.opening(id)?;
        self.dispatch(DesktopEditorCommandsEnumOperation::Create {
            opening,
            selected,
            expected: self.owner.snapshot().map(|snapshot| snapshot.context()),
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn read(&mut self, id: u128) -> DesktopEditorCommandsTestResult {
        let opening = self.opening(id)?;
        self.dispatch(DesktopEditorCommandsEnumOperation::Read {
            opening,
            expected: self.owner.snapshot().map(|snapshot| snapshot.context()),
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn import(&mut self, id: u128, fen: &str) -> DesktopEditorCommandsTestResult {
        let opening = self.opening(id)?;
        self.dispatch(DesktopEditorCommandsEnumOperation::ImportFen {
            opening,
            fen: fen.to_owned(),
            variant: BoardEnumVariant::Standard,
            expected: self.owner.snapshot().map(|snapshot| snapshot.context()),
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn edit(
        &mut self,
        operation: BoardEditorSessionEnumOperation,
    ) -> DesktopEditorCommandsTestResult {
        self.dispatch(DesktopEditorCommandsEnumOperation::Apply {
            expected: self.snapshot()?.context(),
            operation,
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn resolve(
        &mut self,
        choice: DesktopEditorCommandsEnumChoice,
    ) -> DesktopEditorCommandsTestResult {
        self.dispatch(DesktopEditorCommandsEnumOperation::Resolve {
            expected: self.snapshot()?.context(),
            choice,
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn save(&mut self) -> DesktopEditorCommandsTestResult {
        self.dispatch(DesktopEditorCommandsEnumOperation::Save {
            expected: self.snapshot()?.context(),
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn close(self) -> DesktopEditorCommandsTestResult {
        self.store.close().await?;
        self.directory.close()?;
        Ok(())
    }
}

impl PresentationEditorCommandPort for DesktopEditorCommandsFixture {
    fn prepare(
        &self,
        operation: DesktopEditorCommandsEnumOperation,
    ) -> Result<DesktopRequest, DesktopApplicationError> {
        let revision = operation
            .expected()
            .map_or(DesktopRevision::ZERO, |context| {
                DesktopRevision::for_editor(context.revision())
            });
        self.application.port().request(DesktopCommand::new(
            DesktopEnumCommand::Editor {
                owner: self.owner_id,
                operation,
            },
            Some(revision),
        ))
    }

    fn try_submit(
        &self,
        request: &DesktopRequest,
    ) -> Result<DesktopApplicationReceipt, DesktopApplicationError> {
        self.application.port().try_submit(request)
    }
}
