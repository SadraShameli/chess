use std::sync::Arc;
use std::time::Duration;

use chess::board::editor::{
    BoardEditorDraft, BoardEditorSession, BoardEditorSessionEnumEdit,
    BoardEditorSessionEnumOperation, BoardEditorSessionRequest,
};
use chess::board::enums::BoardEnumColor;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::application::{
    DesktopApplication, DesktopApplicationReceipt, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use chess::desktop::dto::DesktopEnumValue;
use chess::desktop::editor_commands::{
    DesktopEditorCommands, DesktopEditorCommandsEnumChoice, DesktopEditorCommandsEnumOperation,
    DesktopEditorCommandsEnumResult, DesktopEditorCommandsError, DesktopEditorCommandsOpening,
    DesktopEditorCommandsSnapshot,
};
use chess::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use chess::desktop::identity::DesktopRevision;
use chess::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::editor_repository::{
    StorageEditorRepository, StorageEditorRepositoryReceipt, StorageEditorRepositoryRevision,
};
use tokio::time::timeout;

pub(super) type DesktopEditorCommandsWriteRecoveryTestResult<T = ()> =
    Result<T, Box<dyn std::error::Error>>;

pub(super) struct DesktopEditorCommandsWriteRecoveryFixture {
    pub(super) owner: DesktopEditorCommands,
    pub(super) repository: StorageEditorRepository,
    pub(super) budget: CapacityBudget,
    store: StorageDocumentStore,
    directory: tempfile::TempDir,
    application: DesktopApplication<
        DesktopEditorCommandsWriteRecoveryFixtureClock,
        DesktopEditorCommandsWriteRecoveryFixtureWake,
    >,
}

struct DesktopEditorCommandsWriteRecoveryFixtureClock;

impl ClockTimeProvider for DesktopEditorCommandsWriteRecoveryFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(100))
    }
}

struct DesktopEditorCommandsWriteRecoveryFixtureWake;

impl DesktopApplicationWakeProvider for DesktopEditorCommandsWriteRecoveryFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

pub(super) struct DesktopEditorCommandsWriteRecoveryRollback {
    pub(super) baseline: DesktopEditorCommandsSnapshot,
    pub(super) submitted: DesktopEditorCommandsSnapshot,
    pub(super) local: DesktopEditorCommandsSnapshot,
    pub(super) failure: Result<DesktopEditorCommandsEnumResult, DesktopEditorCommandsError>,
    pub(super) stored: Option<(BoardEditorDraft, StorageEditorRepositoryRevision)>,
    pub(super) pending: Option<BoardEditorDraft>,
    pub(super) receipt: DesktopApplicationReceipt,
    pub(super) has_original_receipt: bool,
}

impl DesktopEditorCommandsWriteRecoveryFixture {
    pub(super) const WAIT: Duration = Duration::from_secs(5);
    const DATABASE: &str = "editor-write-recovery.sqlite3";
    const DRAFT_ID: &str = "00000000-0000-0000-0000-000000000001";
    const REFUSE_UPDATE: &str = "CREATE TRIGGER editor_review_rollback AFTER UPDATE ON editor_drafts BEGIN SELECT RAISE(ROLLBACK, 'controlled editor rollback'); END;";
    const ALLOW_UPDATE: &str = "DROP TRIGGER editor_review_rollback;";

    pub(super) async fn new() -> DesktopEditorCommandsWriteRecoveryTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::discover()?;
        let store = timeout(
            Self::WAIT,
            StorageDocumentStore::open(directory.path().join(Self::DATABASE), budget.clone()),
        )
        .await??;
        let repository = StorageEditorRepository::new(store.client());
        Ok(Self {
            owner: DesktopEditorCommands::new(repository.clone()),
            repository,
            budget,
            store,
            directory,
            application: DesktopApplication::new(
                Arc::new(DesktopEditorCommandsWriteRecoveryFixtureClock),
                Arc::new(DesktopEditorCommandsWriteRecoveryFixtureWake),
            ),
        })
    }

    pub(super) fn snapshot(
        &self,
    ) -> DesktopEditorCommandsWriteRecoveryTestResult<DesktopEditorCommandsSnapshot> {
        self.owner
            .snapshot()
            .ok_or_else(|| "missing editor recovery session".into())
    }

    pub(super) async fn dispatch(
        &mut self,
        operation: DesktopEditorCommandsEnumOperation,
    ) -> DesktopEditorCommandsWriteRecoveryTestResult<
        Result<DesktopEditorCommandsEnumResult, DesktopEditorCommandsError>,
    > {
        let id = Self::DRAFT_ID.parse()?;
        let revision = operation
            .expected()
            .map_or(DesktopRevision::ZERO, |context| {
                DesktopRevision::for_editor(context.revision())
            });
        let port = self.application.port();
        let request = port.request(DesktopCommand::new(
            DesktopEnumCommand::Editor {
                owner: DesktopMessagesEntityId::for_editor(id),
                operation: operation.clone(),
            },
            Some(revision),
        ))?;
        let receipt = port.try_submit(&request)?;
        let work = self
            .application
            .try_next()?
            .ok_or("missing admitted editor recovery work")?;
        if !receipt.shares_outcome(work.receipt()) {
            return Err("editor recovery application receipt changed".into());
        }
        self.application.begin_commit(&work)?;
        let result = timeout(Self::WAIT, self.owner.handle(operation, work.receipt())).await?;
        match &result {
            Ok(value) => self.application.complete_value(
                &work,
                DesktopEnumValue::Editor(Box::new(value.clone())),
                self.owner
                    .snapshot()
                    .map(|snapshot| DesktopRevision::for_editor(snapshot.context().revision())),
            )?,
            Err(error) => self.application.complete_failure(
                &work,
                DesktopFailure::new(
                    DesktopEnumFailureCode::InvalidInput,
                    error.to_string(),
                    false,
                ),
            )?,
        }
        Ok(result)
    }

    pub(super) async fn seed(
        &mut self,
    ) -> DesktopEditorCommandsWriteRecoveryTestResult<DesktopEditorCommandsSnapshot> {
        self.dispatch(DesktopEditorCommandsEnumOperation::Create {
            opening: DesktopEditorCommandsOpening::new(
                Self::DRAFT_ID.parse()?,
                "00000000-0000-0001-0000-000000000001".parse()?,
            ),
            selected: None,
            expected: None,
        })
        .await??;
        self.save().await??;
        self.finish().await??;
        self.snapshot()
    }

    pub(super) async fn edit(
        &mut self,
        edit: BoardEditorSessionEnumEdit,
    ) -> DesktopEditorCommandsWriteRecoveryTestResult {
        self.dispatch(DesktopEditorCommandsEnumOperation::Apply {
            expected: self.snapshot()?.context(),
            operation: BoardEditorSessionEnumOperation::Edit(edit),
        })
        .await??;
        Ok(())
    }

    pub(super) async fn save(
        &mut self,
    ) -> DesktopEditorCommandsWriteRecoveryTestResult<
        Result<DesktopEditorCommandsEnumResult, DesktopEditorCommandsError>,
    > {
        self.dispatch(DesktopEditorCommandsEnumOperation::Save {
            expected: self.snapshot()?.context(),
        })
        .await
    }

    pub(super) async fn finish(
        &mut self,
    ) -> DesktopEditorCommandsWriteRecoveryTestResult<
        Result<DesktopEditorCommandsEnumResult, DesktopEditorCommandsError>,
    > {
        Ok(timeout(Self::WAIT, self.owner.finish_write()).await?)
    }

    pub(super) async fn read(
        &self,
    ) -> DesktopEditorCommandsWriteRecoveryTestResult<
        Option<(BoardEditorDraft, StorageEditorRepositoryRevision)>,
    > {
        let recovered = timeout(
            Self::WAIT,
            self.repository.read(self.snapshot()?.context().draft()),
        )
        .await??;
        Ok(recovered.map(|stored| (stored.draft().clone(), stored.storage_revision())))
    }

    pub(super) async fn external_update(
        &self,
        baseline: &DesktopEditorCommandsSnapshot,
    ) -> DesktopEditorCommandsWriteRecoveryTestResult<(
        BoardEditorDraft,
        StorageEditorRepositoryReceipt,
    )> {
        let mut external = BoardEditorSession::from_draft(baseline.draft().clone());
        external.apply(BoardEditorSessionRequest {
            draft: external.draft().id(),
            expected_revision: external.draft().revision(),
            operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Turn(
                BoardEnumColor::Black,
            )),
        })?;
        let draft = external.draft().clone();
        let mut write = self.repository.save(&draft, baseline.storage_revision())?;
        let receipt = timeout(Self::WAIT, write.wait()).await??;
        Ok((draft, receipt))
    }

    async fn sql(&self, sql: &'static str) -> DesktopEditorCommandsWriteRecoveryTestResult {
        let path = self.directory.path().join(Self::DATABASE);
        timeout(
            Self::WAIT,
            tokio::task::spawn_blocking(move || {
                let connection = rusqlite::Connection::open(path)?;
                connection.execute_batch(sql)
            }),
        )
        .await???;
        Ok(())
    }

    pub(super) async fn rollback(
        &mut self,
    ) -> DesktopEditorCommandsWriteRecoveryTestResult<DesktopEditorCommandsWriteRecoveryRollback>
    {
        let baseline = self.seed().await?;
        self.edit(BoardEditorSessionEnumEdit::Counters {
            halfmoves: 37,
            fullmoves: 91,
        })
        .await?;
        self.dispatch(DesktopEditorCommandsEnumOperation::Leave {
            expected: self.snapshot()?.context(),
        })
        .await??;
        let submitted = self.snapshot()?;
        self.sql(Self::REFUSE_UPDATE).await?;
        self.dispatch(DesktopEditorCommandsEnumOperation::Resolve {
            expected: submitted.context(),
            choice: DesktopEditorCommandsEnumChoice::Save,
        })
        .await??;
        let receipt = self
            .owner
            .pending_application_receipt()
            .ok_or("missing original editor recovery application receipt")?
            .clone();
        self.edit(BoardEditorSessionEnumEdit::Counters {
            halfmoves: -19,
            fullmoves: 0,
        })
        .await?;
        let failure = self.finish().await?;
        let local = self.snapshot()?;
        let stored = self.read().await?;
        let pending = self.owner.pending_draft().cloned();
        let has_original_receipt = self
            .owner
            .pending_application_receipt()
            .is_some_and(|pending| pending.shares_outcome(&receipt));
        self.sql(Self::ALLOW_UPDATE).await?;
        Ok(DesktopEditorCommandsWriteRecoveryRollback {
            baseline,
            submitted,
            local,
            failure,
            stored,
            pending,
            receipt,
            has_original_receipt,
        })
    }

    pub(super) async fn close(self) -> DesktopEditorCommandsWriteRecoveryTestResult {
        let Self {
            owner,
            repository,
            budget,
            store,
            directory,
            application,
        } = self;
        drop((owner, repository, budget, application));
        timeout(Self::WAIT, store.close()).await??;
        timeout(
            Self::WAIT,
            tokio::task::spawn_blocking(move || directory.close()),
        )
        .await???;
        Ok(())
    }
}
