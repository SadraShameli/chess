use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::board::editor::BoardEditorSessionEnumEdit;
use crate::board::editor::BoardEditorSessionEnumOperation;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::desktop::editor_commands::tests::fixture::{
    DesktopEditorCommandsFixture, DesktopEditorCommandsTestResult,
};
use crate::desktop::editor_commands::{
    DesktopEditorCommandsEnumOperation, DesktopEditorCommandsEnumResult,
    DesktopEditorCommandsError, DesktopEditorCommandsStudy, DesktopEditorCommandsStudyError,
    DesktopEditorCommandsStudyIdentities, DesktopEditorCommandsStudyResources,
};
use crate::game::enums::{GameId, GameMoveId};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::storage_repository::{
    GameStorageRevision, GameStorageWallTimeError, GameStorageWallTimeProvider,
};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};
use crate::storage::library_repository::StorageLibraryRepository;

struct DesktopEditorStudyIdentities {
    next: AtomicU64,
}

impl DesktopEditorCommandsStudyIdentities for DesktopEditorStudyIdentities {
    fn next_study(&self) -> Result<(GameId, GameMoveId), DesktopEditorCommandsStudyError> {
        let value = self.next.fetch_add(2, Ordering::SeqCst);
        let value = u128::from(value);
        Ok((
            uuid::Uuid::from_u128(value).to_string().parse()?,
            uuid::Uuid::from_u128(value + 1).to_string().parse()?,
        ))
    }
}

struct DesktopEditorStudyWallTime;

impl GameStorageWallTimeProvider for DesktopEditorStudyWallTime {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        Ok(GameLibraryOrganizationTimestamp::new(1_000)?)
    }
}

struct DesktopEditorStudyFixture {
    editor: DesktopEditorCommandsFixture,
    store: StorageDocumentStore,
    executor: Arc<CapacityExecutor>,
}

impl DesktopEditorStudyFixture {
    async fn new() -> DesktopEditorCommandsTestResult<Self> {
        let mut editor = DesktopEditorCommandsFixture::new().await?;
        let budget = CapacityBudget::discover()?;
        let store = StorageDocumentStore::open(editor.path.clone(), budget.clone()).await?;
        let executor = Arc::new(CapacityExecutor::new(budget)?);
        let placeholder =
            crate::desktop::editor_commands::DesktopEditorCommands::new(editor.repository.clone());
        let owner = std::mem::replace(&mut editor.owner, placeholder);
        editor.owner = owner.with_study(DesktopEditorCommandsStudyResources {
            client: store.client(),
            executor: Arc::clone(&executor),
            wall_time: Arc::new(DesktopEditorStudyWallTime),
            identities: Arc::new(DesktopEditorStudyIdentities {
                next: AtomicU64::new(0x1000),
            }),
        });
        Ok(Self {
            editor,
            store,
            executor,
        })
    }

    async fn open_study(&mut self) -> DesktopEditorCommandsTestResult<DesktopEditorCommandsStudy> {
        let DesktopEditorCommandsEnumResult::Study { study, context } = self
            .editor
            .dispatch(DesktopEditorCommandsEnumOperation::OpenStudy {
                expected: self.editor.snapshot()?.context(),
            })
            .await?
        else {
            return Err("missing study result".into());
        };
        assert_eq!(context, self.editor.snapshot()?.context());
        Ok(study)
    }

    async fn load(
        &self,
        id: GameId,
    ) -> DesktopEditorCommandsTestResult<Option<StorageLibraryRecord>> {
        Ok(
            StorageLibraryRepository::new(self.store.client(), &self.executor)
                .load(id)
                .await?
                .map(crate::storage::library_repository::StorageLibraryRecovered::into_record),
        )
    }

    async fn close(self) -> DesktopEditorCommandsTestResult {
        self.editor.close().await?;
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_shared| "executor still shared")?
            .shutdown()
            .await?;
        Ok(())
    }
}

#[tokio::test]
async fn valid_draft_opens_as_a_separate_persisted_study_and_leaves_the_draft_untouched()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorStudyFixture::new().await?;
    fixture.editor.create(1, None).await?;
    fixture
        .editor
        .edit(BoardEditorSessionEnumOperation::Edit(
            BoardEditorSessionEnumEdit::Counters {
                halfmoves: 3,
                fullmoves: 9,
            },
        ))
        .await?;
    let before = fixture.editor.snapshot()?;
    let study = fixture.open_study().await?;
    let after = fixture.editor.snapshot()?;
    assert_eq!(after.draft(), before.draft());
    assert_eq!(after.context(), before.context());
    assert_eq!(after.is_dirty(), before.is_dirty());
    assert_eq!(study.storage_revision(), GameStorageRevision::new(1));
    let record = fixture.load(study.game()).await?.ok_or("missing study")?;
    let StorageLibraryEnumContent::Notation(game) = record.content() else {
        return Err("study must be a notation document".into());
    };
    assert_eq!(game.start().position(), &before.draft().validate()?);
    assert_eq!(
        game.tree().root(),
        game.tree().path_to(game.tree().root())?[0]
    );
    let second = fixture.open_study().await?;
    assert_ne!(second.game(), study.game());
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn invalid_draft_and_stale_context_never_create_a_study() -> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorStudyFixture::new().await?;
    fixture.editor.create(1, None).await?;
    let stale = fixture.editor.snapshot()?.context();
    fixture
        .editor
        .edit(BoardEditorSessionEnumOperation::Edit(
            BoardEditorSessionEnumEdit::Counters {
                halfmoves: -1,
                fullmoves: 0,
            },
        ))
        .await?;
    let invalid = fixture
        .editor
        .dispatch(DesktopEditorCommandsEnumOperation::OpenStudy {
            expected: fixture.editor.snapshot()?.context(),
        })
        .await;
    assert!(matches!(
        invalid
            .err()
            .and_then(|error| error.downcast::<DesktopEditorCommandsError>().ok())
            .map(|error| *error),
        Some(DesktopEditorCommandsError::Validation(_))
    ));
    let rejected = fixture
        .editor
        .dispatch(DesktopEditorCommandsEnumOperation::OpenStudy { expected: stale })
        .await;
    assert!(rejected.is_err());
    let first: GameId = uuid::Uuid::from_u128(0x1000).to_string().parse()?;
    assert!(fixture.load(first).await?.is_none());
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn study_is_unavailable_without_its_resources() -> DesktopEditorCommandsTestResult {
    let mut editor = DesktopEditorCommandsFixture::new().await?;
    editor.create(1, None).await?;
    let result = editor
        .dispatch(DesktopEditorCommandsEnumOperation::OpenStudy {
            expected: editor.snapshot()?.context(),
        })
        .await;
    assert!(matches!(
        result
            .err()
            .and_then(|error| error.downcast::<DesktopEditorCommandsError>().ok())
            .map(|error| *error),
        Some(DesktopEditorCommandsError::Study(
            DesktopEditorCommandsStudyError::Unavailable
        ))
    ));
    editor.close().await?;
    Ok(())
}

#[tokio::test]
async fn all_four_promotion_choices_survive_in_the_opened_study() -> DesktopEditorCommandsTestResult
{
    let mut fixture = DesktopEditorStudyFixture::new().await?;
    fixture
        .editor
        .import(1, "k7/4P3/8/8/8/8/8/4K3 w - - 0 1")
        .await?;
    let study = fixture.open_study().await?;
    let mut record = fixture.load(study.game()).await?.ok_or("missing study")?;
    let game = record.notation_mut().ok_or("missing notation")?;
    let root = game.tree().root();
    let position = game.start().position().clone();
    for (index, promotion) in ["q", "r", "b", "n"].into_iter().enumerate() {
        let movement = position.parse_uci(&format!("e7e8{promotion}"))?;
        let node: GameMoveId = uuid::Uuid::from_u128(0x2000 + u128::try_from(index)?)
            .to_string()
            .parse()?;
        game.append_study_variation(root, node, movement)?;
        assert_eq!(game.tree().node(node)?.movement(), Some(movement));
    }
    assert_eq!(game.tree().len(), 5);
    fixture.close().await?;
    Ok(())
}
