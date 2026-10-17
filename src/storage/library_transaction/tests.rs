use std::num::NonZeroUsize;
use std::sync::Arc;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use tokio::sync::Semaphore;

use crate::board::start::BoardStart;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationRequest, GameLibraryOrganizationRevision,
    GameLibraryOrganizationTimestamp,
};
use crate::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataKnownTags, GameMetadataText,
};
use crate::game::storage_repository::GameStorageRevision;
use crate::game::tree::GameTree;
use crate::notation::game::{NotationGame, NotationGameParts};
use crate::notation::game_identity::NotationGameIdentityEnumResult;
use crate::storage::connection::StorageConnection;
use crate::storage::document_store::{StorageDocumentClient, StorageDocumentStore};
use crate::storage::game_document::StorageGameDocument;
use crate::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryPreparedMemory, StorageLibraryPreparedWrite,
    StorageLibraryPreparedWriteOptions,
};
use crate::storage::library_query::{
    StorageLibraryDateInterval, StorageLibraryEnumFilter, StorageLibraryQuery,
    StorageLibraryQueryOptions,
};
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::library_transaction::{
    StorageLibraryEnumTransactionRequest, StorageLibraryEnumTransactionResponse,
    StorageLibraryTransaction,
};
use crate::storage::migration::StorageMigrationRunner;
use crate::storage::schema::{
    documents, library_entries, library_entry_tags, library_folders, library_state,
};
use crate::storage::trash::{
    StorageTrashClosedGameEvidence, StorageTrashConfirmedRequest, StorageTrashPreviewRequest,
    StorageTrashRestoreRequest,
};

type StorageLibraryTransactionTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageLibraryTransactionFixture {
    _directory: tempfile::TempDir,
    connection: StorageConnection,
    executor: CapacityExecutor,
    memory: Arc<Semaphore>,
}

impl StorageLibraryTransactionFixture {
    fn document_body(&mut self) -> StorageLibraryTransactionTestResult<String> {
        Ok(documents::table
            .select(documents::body)
            .first(self.connection.diesel())?)
    }

    fn catalog_text(&mut self) -> StorageLibraryTransactionTestResult<String> {
        Ok(library_state::table
            .select(library_state::revision)
            .first(self.connection.diesel())?)
    }

    fn folder_count(&mut self) -> StorageLibraryTransactionTestResult<i64> {
        Ok(library_folders::table
            .count()
            .get_result(self.connection.diesel())?)
    }

    fn batch(&mut self, sql: &str) -> StorageLibraryTransactionTestResult {
        self.connection.diesel().batch_execute(sql)?;
        Ok(())
    }

    fn snapshot(
        &mut self,
    ) -> StorageLibraryTransactionTestResult<(String, String, String, Option<i64>, String)> {
        let (body, revision, organization, modified) = documents::table
            .inner_join(library_entries::table)
            .select((
                documents::body,
                documents::revision,
                library_entries::organization_revision,
                library_entries::modified_ms,
            ))
            .first::<(String, String, String, Option<i64>)>(self.connection.diesel())?;
        Ok((body, revision, organization, modified, self.catalog_text()?))
    }

    fn new() -> StorageLibraryTransactionTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("library.sqlite");
        let mut diesel = StorageConnection::open(&path)?;
        StorageMigrationRunner::run_embedded(diesel.diesel())?;
        Ok(Self {
            _directory: directory,
            connection: diesel,
            executor: CapacityExecutor::new(CapacityBudget::new(NonZeroUsize::MIN)?)?,
            memory: Arc::new(Semaphore::new(StorageDocumentStore::MEMORY_BUDGET)),
        })
    }

    fn id(value: u128) -> StorageLibraryTransactionTestResult<GameId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn body(
        id: GameId,
        date: Option<GameMetadataDate>,
    ) -> StorageLibraryTransactionTestResult<String> {
        let start = BoardStart::standard();
        let root = uuid::Uuid::from_u128(999).to_string().parse()?;
        let game = NotationGame::new(NotationGameParts {
            tree: GameTree::new(root, start.position().clone(), None),
            start,
            mainline_tip: root,
            metadata: GameMetadata::new(
                GameMetadataKnownTags {
                    white: Some(GameMetadataText::new("  STRAßE  ".to_owned())?),
                    date,
                    ..GameMetadataKnownTags::default()
                },
                Vec::new(),
            )?,
            result: NotationGameIdentityEnumResult::Unfinished,
            result_header: None,
            time_control: None,
            clocks: Vec::new(),
            warnings: Vec::new(),
        })?;
        Ok(StorageGameDocument::capture_notation(id, &game)?.json()?)
    }

    fn execute(
        &mut self,
        request: StorageLibraryEnumTransactionRequest,
    ) -> Result<StorageLibraryEnumTransactionResponse, StorageLibraryError> {
        StorageLibraryTransaction::execute(
            self.connection.diesel(),
            request,
            &self.memory,
            uuid::Uuid::nil(),
        )
    }

    fn prepared_memory(
        &self,
        body_bytes: usize,
    ) -> Result<StorageLibraryPreparedMemory, StorageLibraryError> {
        let reservation = StorageDocumentClient::reserve_memory(
            &self.memory,
            StorageLibraryPreparedMemory::reservation_bytes(body_bytes)?,
        )?;
        StorageLibraryPreparedMemory::new(body_bytes, reservation)
    }

    async fn save(
        &mut self,
        id: GameId,
        revision: u64,
        change: StorageLibraryEnumContentChange,
        date: Option<GameMetadataDate>,
    ) -> StorageLibraryTransactionTestResult {
        let body = Self::body(id, date)?;
        let memory = self.prepared_memory(body.capacity())?;
        let prepared = self
            .executor
            .submit_waiting(move |cancellation| {
                StorageLibraryPreparedWrite::from_encoded(
                    StorageLibraryPreparedWriteOptions {
                        id,
                        expected: GameStorageRevision::new(revision),
                        body,
                        change,
                        memory,
                    },
                    cancellation,
                )
            })
            .await?
            .join()
            .await??;
        assert!(matches!(
            self.execute(StorageLibraryEnumTransactionRequest::Save(prepared))?,
            StorageLibraryEnumTransactionResponse::Saved(_)
        ));
        Ok(())
    }

    async fn query(
        &mut self,
        query: StorageLibraryQuery,
    ) -> StorageLibraryTransactionTestResult<crate::storage::library_query::StorageLibraryQueryPage>
    {
        let query = self
            .executor
            .submit_waiting(move |cancellation| query.prepare(cancellation))
            .await?
            .join()
            .await??;
        let response = self.execute(StorageLibraryEnumTransactionRequest::Query(query))?;
        let StorageLibraryEnumTransactionResponse::Page(page) = response else {
            return Err("expected a library page".into());
        };
        Ok(page)
    }
}

#[tokio::test]
async fn deeply_nested_confirmed_folder_removal_unfiles_without_cascade_depth_failure()
-> StorageLibraryTransactionTestResult {
    use crate::game::library_organization::{
        GameLibraryOrganizationFolderId, GameLibraryOrganizationFolderTarget,
        GameLibraryOrganizationName,
    };
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let root: GameLibraryOrganizationFolderId =
        uuid::Uuid::from_u128(10_001).to_string().parse()?;
    let mut parent = None;
    for depth in 1..=1_100_u128 {
        let id: GameLibraryOrganizationFolderId =
            uuid::Uuid::from_u128(10_000 + depth).to_string().parse()?;
        fixture.organize(
            GameLibraryOrganizationEnumMutation::CreateFolder(
                id,
                parent,
                GameLibraryOrganizationName::new(format!("Depth {depth}"))?,
            ),
            1,
        )?;
        parent = Some(GameLibraryOrganizationFolderTarget::new(
            id,
            GameLibraryOrganizationRevision::ZERO,
        ));
    }
    let game = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            game,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            None,
        )
        .await?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::MoveToFolder(
            GameLibraryOrganizationGameTarget::new(game, GameLibraryOrganizationRevision::ZERO),
            parent,
        ),
        2,
    )?;
    let before: String = documents::table
        .filter(documents::id.eq(game.as_uuid().to_string()))
        .select(documents::body)
        .first(fixture.connection.diesel())?;
    let plan = fixture.preview(StorageTrashPreviewRequest::RemoveFolder(root))?;
    assert_eq!(plan.affected_folder_count(), 1_100);
    assert_eq!(plan.affected_game_count(), 1);
    let removed = fixture.execute(StorageLibraryEnumTransactionRequest::ApplyDestruction(
        StorageTrashConfirmedRequest::new(
            plan.approve(),
            GameLibraryOrganizationTimestamp::new(3)?,
            None,
        )?,
    ));
    let folders = fixture.folder_count()?;
    let (after, folder): (String, Option<String>) = documents::table
        .inner_join(library_entries::table)
        .filter(documents::id.eq(game.as_uuid().to_string()))
        .select((documents::body, library_entries::folder_id))
        .first(fixture.connection.diesel())?;
    fixture.executor.shutdown().await?;
    assert!(
        removed.is_ok(),
        "confirmed deep removal failed: {:?}",
        removed.err()
    );
    assert_eq!(folders, 0);
    assert_eq!(folder, None);
    assert_eq!(after, before);
    Ok(())
}

#[tokio::test]
async fn content_and_organization_cas_are_independent_and_checkpoints_preserve_time()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            id,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(200)?),
            None,
        )
        .await?;
    let before = fixture.document_body()?;
    fixture.execute(StorageLibraryEnumTransactionRequest::Organize(
        GameLibraryOrganizationRequest::new(
            GameLibraryOrganizationTimestamp::new(300)?,
            GameLibraryOrganizationEnumMutation::SetFavorite(
                GameLibraryOrganizationGameTarget::new(id, GameLibraryOrganizationRevision::ZERO),
                true,
            ),
        )?,
    ))?;
    let after = fixture.document_body()?;
    assert_eq!(before, after);
    fixture
        .save(id, 1, StorageLibraryEnumContentChange::Checkpoint, None)
        .await?;
    fixture
        .save(
            id,
            2,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(100)?),
            None,
        )
        .await?;
    let page = fixture.query(StorageLibraryQuery::default()).await?;
    assert_eq!(page.entries().len(), 1);
    let entry = &page.entries()[0];
    assert_eq!(entry.content_revision().value(), 3);
    assert_eq!(entry.organization().revision().value(), 1);
    assert!(entry.organization().is_favorite());
    assert_eq!(
        entry
            .modified()
            .map(GameLibraryOrganizationTimestamp::value),
        Some(300)
    );
    assert!(matches!(
        fixture.execute(StorageLibraryEnumTransactionRequest::Organize(
            GameLibraryOrganizationRequest::new(
                GameLibraryOrganizationTimestamp::new(400)?,
                GameLibraryOrganizationEnumMutation::SetFavorite(
                    GameLibraryOrganizationGameTarget::new(
                        id,
                        GameLibraryOrganizationRevision::ZERO
                    ),
                    false
                )
            )?
        )),
        Err(StorageLibraryError::Organization(_))
    ));
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn partial_dates_require_a_real_calendar_witness_in_the_interval()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let dates = [
        GameMetadataDate::new(Some(2024), None, Some(31))?,
        GameMetadataDate::new(None, Some(2), Some(29))?,
        GameMetadataDate::new(None, None, Some(15))?,
        GameMetadataDate::new(None, None, None)?,
    ];
    for (index, date) in dates.into_iter().enumerate() {
        fixture
            .save(
                StorageLibraryTransactionFixture::id(u128::try_from(index)? + 1)?,
                0,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
                Some(date),
            )
            .await?;
    }
    let interval = StorageLibraryDateInterval::new(
        GameMetadataDate::new(Some(2024), Some(2), Some(1))?,
        GameMetadataDate::new(Some(2024), Some(2), Some(29))?,
    )?;
    let page = fixture
        .query(StorageLibraryQuery::new(StorageLibraryQueryOptions {
            date: StorageLibraryEnumFilter::Value(interval),
            ..StorageLibraryQueryOptions::default()
        })?)
        .await?;
    assert_eq!(page.entries().len(), 2);
    assert!(
        page.entries()
            .iter()
            .all(|entry| entry.is_partial_date_match())
    );
    assert_eq!(
        page.entries()[0].id(),
        StorageLibraryTransactionFixture::id(2)?
    );
    assert_eq!(
        page.entries()[1].id(),
        StorageLibraryTransactionFixture::id(3)?
    );
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn pages_use_full_tie_breakers_and_reject_stale_catalogs()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    for value in 1..=101 {
        fixture
            .save(
                StorageLibraryTransactionFixture::id(value)?,
                0,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                    10,
                )?),
                None,
            )
            .await?;
    }
    let page = fixture.query(StorageLibraryQuery::default()).await?;
    assert_eq!(page.entries().len(), 100);
    let cursor = page.next_cursor().ok_or("missing continuation")?.clone();
    let last = fixture
        .query(StorageLibraryQuery::default().with_cursor(cursor.clone())?)
        .await?;
    assert_eq!(last.entries().len(), 1);
    assert_eq!(
        last.entries()[0].id(),
        StorageLibraryTransactionFixture::id(101)?
    );
    for sort in [
        crate::storage::library_query::StorageLibraryEnumSort::UpdatedAscending,
        crate::storage::library_query::StorageLibraryEnumSort::DateAscending,
        crate::storage::library_query::StorageLibraryEnumSort::DateDescending,
        crate::storage::library_query::StorageLibraryEnumSort::PliesAscending,
        crate::storage::library_query::StorageLibraryEnumSort::PliesDescending,
    ] {
        let options = StorageLibraryQueryOptions {
            sort,
            ..StorageLibraryQueryOptions::default()
        };
        let page = fixture
            .query(StorageLibraryQuery::new(options.clone())?)
            .await?;
        let cursor = page
            .next_cursor()
            .ok_or("missing sort continuation")?
            .clone();
        let next = fixture
            .query(StorageLibraryQuery::new(options)?.with_cursor(cursor)?)
            .await?;
        assert_eq!(page.entries().len(), 100);
        assert_eq!(next.entries().len(), 1);
        assert_eq!(
            next.entries()[0].id(),
            StorageLibraryTransactionFixture::id(101)?
        );
    }
    fixture
        .save(
            StorageLibraryTransactionFixture::id(102)?,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(11)?),
            None,
        )
        .await?;
    let query = StorageLibraryQuery::default().with_cursor(cursor)?;
    let query = fixture
        .executor
        .submit_waiting(move |cancellation| query.prepare(cancellation))
        .await?
        .join()
        .await??;
    assert!(matches!(
        fixture.execute(StorageLibraryEnumTransactionRequest::Query(query)),
        Err(StorageLibraryError::StaleCursor)
    ));
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn trash_requires_current_confirmation_and_restore_preserves_the_same_body()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            id,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            None,
        )
        .await?;
    let response = fixture.execute(StorageLibraryEnumTransactionRequest::PreviewDestruction(
        StorageTrashPreviewRequest::MoveToTrash(id),
    ))?;
    let StorageLibraryEnumTransactionResponse::DestructionPreview(preview) = response else {
        return Err("expected plan".into());
    };
    let approved = preview.approve();
    let confirmed = StorageTrashConfirmedRequest::new(
        approved,
        GameLibraryOrganizationTimestamp::new(2)?,
        Some(StorageTrashClosedGameEvidence::new(
            id,
            GameStorageRevision::new(1),
        )),
    )?;
    fixture.execute(StorageLibraryEnumTransactionRequest::ApplyDestruction(
        confirmed,
    ))?;
    let body = StorageLibraryTransactionFixture::body(id, None)?;
    let memory = fixture.prepared_memory(body.capacity())?;
    let prepared = fixture
        .executor
        .submit_waiting(move |cancellation| {
            StorageLibraryPreparedWrite::from_encoded(
                StorageLibraryPreparedWriteOptions {
                    id,
                    expected: GameStorageRevision::new(1),
                    body,
                    change: StorageLibraryEnumContentChange::Checkpoint,
                    memory,
                },
                cancellation,
            )
        })
        .await?
        .join()
        .await??;
    assert!(matches!(
        fixture.execute(StorageLibraryEnumTransactionRequest::Save(prepared)),
        Err(StorageLibraryError::InTrash(_))
    ));
    fixture.execute(StorageLibraryEnumTransactionRequest::Restore(
        StorageTrashRestoreRequest::new(
            GameLibraryOrganizationGameTarget::new(id, "1".parse()?),
            GameLibraryOrganizationTimestamp::new(3)?,
        ),
    ))?;
    let count: i64 = documents::table
        .filter(documents::id.eq(id.as_uuid().to_string()))
        .count()
        .get_result(fixture.connection.diesel())?;
    assert_eq!(count, 1);
    fixture.executor.shutdown().await?;
    Ok(())
}

impl StorageLibraryTransactionFixture {
    fn organize(
        &mut self,
        mutation: GameLibraryOrganizationEnumMutation,
        timestamp: i64,
    ) -> StorageLibraryTransactionTestResult<
        crate::game::library_organization::GameLibraryOrganizationReceipt,
    > {
        let response = self.execute(StorageLibraryEnumTransactionRequest::Organize(
            GameLibraryOrganizationRequest::new(
                GameLibraryOrganizationTimestamp::new(timestamp)?,
                mutation,
            )?,
        ))?;
        let StorageLibraryEnumTransactionResponse::Organized(receipt) = response else {
            return Err("expected organization receipt".into());
        };
        Ok(receipt)
    }
    fn preview(
        &mut self,
        action: StorageTrashPreviewRequest,
    ) -> StorageLibraryTransactionTestResult<crate::storage::trash::StorageTrashPlan> {
        let response = self.execute(StorageLibraryEnumTransactionRequest::PreviewDestruction(
            action,
        ))?;
        let StorageLibraryEnumTransactionResponse::DestructionPreview(plan) = response else {
            return Err("expected destructive preview".into());
        };
        Ok(plan)
    }
}

#[tokio::test]
async fn folder_removal_unfiles_library_and_trash_without_losing_tags_or_content()
-> StorageLibraryTransactionTestResult {
    use crate::game::library_organization::{
        GameLibraryOrganizationFolderTarget, GameLibraryOrganizationName,
        GameLibraryOrganizationTagSet,
    };
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let folder = "00000000-0000-0000-0000-000000000010".parse()?;
    let child = "00000000-0000-0000-0000-000000000011".parse()?;
    let tag = "00000000-0000-0000-0000-000000000012".parse()?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::CreateFolder(
            folder,
            None,
            GameLibraryOrganizationName::new("Root".to_owned())?,
        ),
        10,
    )?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::CreateFolder(
            child,
            Some(GameLibraryOrganizationFolderTarget::new(
                folder,
                GameLibraryOrganizationRevision::ZERO,
            )),
            GameLibraryOrganizationName::new("Child".to_owned())?,
        ),
        10,
    )?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::CreateTag(
            tag,
            GameLibraryOrganizationName::new("Study".to_owned())?,
        ),
        10,
    )?;
    for value in 1..=2 {
        let id = StorageLibraryTransactionFixture::id(value)?;
        fixture
            .save(
                id,
                0,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
                None,
            )
            .await?;
        fixture.organize(
            GameLibraryOrganizationEnumMutation::MoveToFolder(
                GameLibraryOrganizationGameTarget::new(id, GameLibraryOrganizationRevision::ZERO),
                Some(GameLibraryOrganizationFolderTarget::new(
                    child,
                    GameLibraryOrganizationRevision::ZERO,
                )),
            ),
            20,
        )?;
        fixture.organize(
            GameLibraryOrganizationEnumMutation::ReplaceTags(
                GameLibraryOrganizationGameTarget::new(id, "1".parse()?),
                GameLibraryOrganizationTagSet::new(vec![tag])?,
            ),
            30,
        )?;
    }
    let id = StorageLibraryTransactionFixture::id(2)?;
    let plan = fixture.preview(StorageTrashPreviewRequest::MoveToTrash(id))?;
    fixture.execute(StorageLibraryEnumTransactionRequest::ApplyDestruction(
        StorageTrashConfirmedRequest::new(
            plan.approve(),
            GameLibraryOrganizationTimestamp::new(40)?,
            Some(StorageTrashClosedGameEvidence::new(
                id,
                GameStorageRevision::new(1),
            )),
        )?,
    ))?;
    let plan = fixture.preview(StorageTrashPreviewRequest::RemoveFolder(folder))?;
    assert_eq!(plan.affected_folder_count(), 2);
    assert_eq!(plan.affected_game_count(), 2);
    fixture.execute(StorageLibraryEnumTransactionRequest::ApplyDestruction(
        StorageTrashConfirmedRequest::new(
            plan.approve(),
            GameLibraryOrganizationTimestamp::new(50)?,
            None,
        )?,
    ))?;
    let count = fixture.folder_count()?;
    assert_eq!(count, 0);
    let rows: Vec<(String, String, Option<String>, String)> = documents::table
        .inner_join(library_entries::table)
        .order(documents::id.asc())
        .select((
            documents::body,
            documents::revision,
            library_entries::folder_id,
            library_entries::organization_revision,
        ))
        .load(fixture.connection.diesel())?;
    assert_eq!(rows.len(), 2);
    for (index, (body, revision, folder, organization)) in rows.into_iter().enumerate() {
        assert_eq!(
            body,
            StorageLibraryTransactionFixture::body(
                StorageLibraryTransactionFixture::id(u128::try_from(index)? + 1)?,
                None
            )?
        );
        assert_eq!(revision, "1");
        assert_eq!(folder, None);
        assert_eq!(organization, if index == 0 { "3" } else { "4" });
    }
    let tags: i64 = library_entry_tags::table
        .count()
        .get_result(fixture.connection.diesel())?;
    assert_eq!(tags, 2);
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn folder_cycles_missing_references_and_noops_preserve_the_catalog()
-> StorageLibraryTransactionTestResult {
    use crate::game::library_organization::{
        GameLibraryOrganizationFolderTarget, GameLibraryOrganizationName,
    };
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let folder = "00000000-0000-0000-0000-000000000010".parse()?;
    let child = "00000000-0000-0000-0000-000000000011".parse()?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::CreateFolder(
            folder,
            None,
            GameLibraryOrganizationName::new("Root".to_owned())?,
        ),
        1,
    )?;
    let parent =
        GameLibraryOrganizationFolderTarget::new(folder, GameLibraryOrganizationRevision::ZERO);
    fixture.organize(
        GameLibraryOrganizationEnumMutation::CreateFolder(
            child,
            Some(parent),
            GameLibraryOrganizationName::new("Child".to_owned())?,
        ),
        1,
    )?;
    let before = fixture.catalog_text()?;
    let noop = fixture.organize(
        GameLibraryOrganizationEnumMutation::RenameFolder(
            parent,
            GameLibraryOrganizationName::new("Root".to_owned())?,
        ),
        100,
    )?;
    assert!(!noop.is_changed());
    let cycle = GameLibraryOrganizationEnumMutation::ReparentFolder(
        parent,
        Some(GameLibraryOrganizationFolderTarget::new(
            child,
            GameLibraryOrganizationRevision::ZERO,
        )),
    );
    assert!(fixture.organize(cycle, 100).is_err());
    let id = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            id,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            None,
        )
        .await?;
    let missing = GameLibraryOrganizationEnumMutation::MoveToFolder(
        GameLibraryOrganizationGameTarget::new(id, GameLibraryOrganizationRevision::ZERO),
        Some(GameLibraryOrganizationFolderTarget::new(
            "00000000-0000-0000-0000-000000000099".parse()?,
            GameLibraryOrganizationRevision::ZERO,
        )),
    );
    assert!(fixture.organize(missing, 100).is_err());
    let after = fixture.catalog_text()?;
    assert_eq!(before.parse::<u64>()? + 1, after.parse::<u64>()?);
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn stale_destruction_is_rejected_and_permanent_delete_cascades_only_after_trash()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            id,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            None,
        )
        .await?;
    assert!(
        fixture
            .preview(StorageTrashPreviewRequest::PermanentlyDelete(id))
            .is_err()
    );
    let stale = fixture.preview(StorageTrashPreviewRequest::MoveToTrash(id))?;
    fixture
        .save(id, 1, StorageLibraryEnumContentChange::Checkpoint, None)
        .await?;
    let request = StorageTrashConfirmedRequest::new(
        stale.approve(),
        GameLibraryOrganizationTimestamp::new(2)?,
        Some(StorageTrashClosedGameEvidence::new(
            id,
            GameStorageRevision::new(1),
        )),
    )?;
    assert!(matches!(
        fixture.execute(StorageLibraryEnumTransactionRequest::ApplyDestruction(
            request
        )),
        Err(StorageLibraryError::StalePlan)
    ));
    let current = fixture.preview(StorageTrashPreviewRequest::MoveToTrash(id))?;
    fixture.execute(StorageLibraryEnumTransactionRequest::ApplyDestruction(
        StorageTrashConfirmedRequest::new(
            current.approve(),
            GameLibraryOrganizationTimestamp::new(2)?,
            Some(StorageTrashClosedGameEvidence::new(
                id,
                GameStorageRevision::new(2),
            )),
        )?,
    ))?;
    let deletion = fixture.preview(StorageTrashPreviewRequest::PermanentlyDelete(id))?;
    fixture.execute(StorageLibraryEnumTransactionRequest::ApplyDestruction(
        StorageTrashConfirmedRequest::new(
            deletion.approve(),
            GameLibraryOrganizationTimestamp::new(3)?,
            Some(StorageTrashClosedGameEvidence::new(
                id,
                GameStorageRevision::new(2),
            )),
        )?,
    ))?;
    let counts: (i64, i64, i64) = (
        documents::table
            .count()
            .get_result(fixture.connection.diesel())?,
        library_entries::table
            .count()
            .get_result(fixture.connection.diesel())?,
        library_entry_tags::table
            .count()
            .get_result(fixture.connection.diesel())?,
    );
    assert_eq!(counts, (0, 0, 0));
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn catalog_exhaustion_rolls_back_the_document_and_projection_together()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            id,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            None,
        )
        .await?;
    diesel::update(library_state::table)
        .set(library_state::revision.eq(u64::MAX.to_string()))
        .execute(fixture.connection.diesel())?;
    let body = StorageLibraryTransactionFixture::body(id, None)?;
    let memory = fixture.prepared_memory(body.capacity())?;
    let prepared = fixture
        .executor
        .submit_waiting(move |cancellation| {
            StorageLibraryPreparedWrite::from_encoded(
                StorageLibraryPreparedWriteOptions {
                    id,
                    expected: GameStorageRevision::new(1),
                    body,
                    change: StorageLibraryEnumContentChange::Changed(
                        GameLibraryOrganizationTimestamp::new(2)?,
                    ),
                    memory,
                },
                cancellation,
            )
        })
        .await?
        .join()
        .await??;
    assert!(matches!(
        fixture.execute(StorageLibraryEnumTransactionRequest::Save(prepared)),
        Err(StorageLibraryError::CatalogExhausted)
    ));
    let stored: (String, String, Option<i64>) = documents::table
        .inner_join(library_entries::table)
        .select((
            documents::revision,
            library_entries::content_revision,
            library_entries::modified_ms,
        ))
        .first(fixture.connection.diesel())?;
    assert_eq!(stored, ("1".to_owned(), "1".to_owned(), Some(1)));
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn pending_wait_cancellation_retains_the_receiver_until_outcome_observation()
-> StorageLibraryTransactionTestResult {
    use crate::storage::library_repository::{StorageLibraryContentReceipt, StorageLibraryPending};
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut pending = StorageLibraryPending::new(receiver);
    {
        let wait = pending.wait();
        let mut wait = std::pin::pin!(wait);
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(wait.as_mut().poll(&mut context), Poll::Pending));
    }
    let response = StorageLibraryEnumTransactionResponse::Saved(StorageLibraryContentReceipt::new(
        StorageLibraryTransactionFixture::id(1)?,
        GameStorageRevision::new(1),
        "1".parse()?,
    ));
    assert!(sender.send(Ok(response)).is_ok());
    assert!(matches!(
        pending.wait().await?,
        StorageLibraryEnumTransactionResponse::Saved(_)
    ));
    assert!(matches!(
        pending.wait().await,
        Err(StorageLibraryError::AlreadyObserved)
    ));
    Ok(())
}

#[tokio::test]
async fn output_memory_is_retained_by_pages_and_saturation_is_explicit()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            id,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            None,
        )
        .await?;
    let before = fixture.memory.available_permits();
    let page = fixture.query(StorageLibraryQuery::default()).await?;
    assert!(fixture.memory.available_permits() < before);
    drop(page);
    assert_eq!(fixture.memory.available_permits(), before);
    let small = Arc::new(Semaphore::new(1));
    assert!(matches!(
        StorageLibraryTransaction::execute(
            fixture.connection.diesel(),
            StorageLibraryEnumTransactionRequest::Load(id),
            &small,
            uuid::Uuid::nil(),
        ),
        Err(StorageLibraryError::Store(
            crate::storage::document_store::StorageDocumentError::Saturated
        ))
    ));
    fixture.executor.shutdown().await?;
    Ok(())
}

#[test]
fn diesel_catalog_bump_is_monotonic_inside_an_immediate_transaction()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let before = StorageLibraryTransaction::diesel_catalog(fixture.connection.diesel())?;
    let bumped = fixture
        .connection
        .diesel()
        .immediate_transaction(StorageLibraryTransaction::diesel_bump_catalog)?;
    assert_eq!(bumped, before.next()?);
    assert_eq!(
        StorageLibraryTransaction::diesel_catalog(fixture.connection.diesel())?,
        bumped
    );
    let legacy = fixture.catalog_text()?;
    assert_eq!(
        legacy.parse::<crate::storage::library_repository::StorageLibraryCatalogRevision>()?,
        bumped
    );
    Ok(())
}

#[tokio::test]
async fn normalized_player_filters_exclude_missing_mode_and_preserve_display_text()
-> StorageLibraryTransactionTestResult {
    use crate::game::enums::GameEnumMode;
    use crate::storage::library_prepared::StorageLibraryEnumStartKind;
    use crate::storage::library_query::StorageLibraryPlyInterval;
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            id,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            None,
        )
        .await?;
    let page = fixture
        .query(StorageLibraryQuery::new(StorageLibraryQueryOptions {
            either_player: StorageLibraryEnumFilter::Value(GameMetadataText::new(
                "strasse".to_owned(),
            )?),
            start_kind: StorageLibraryEnumFilter::Value(StorageLibraryEnumStartKind::Standard),
            result: StorageLibraryEnumFilter::Value(NotationGameIdentityEnumResult::Unfinished),
            plies: StorageLibraryEnumFilter::Value(StorageLibraryPlyInterval::new(0, 0)?),
            ..StorageLibraryQueryOptions::default()
        })?)
        .await?;
    assert_eq!(page.entries().len(), 1);
    assert_eq!(
        page.entries()[0].white().map(GameMetadataText::as_str),
        Some("  STRAßE  ")
    );
    let page = fixture
        .query(StorageLibraryQuery::new(StorageLibraryQueryOptions {
            mode: StorageLibraryEnumFilter::Value(GameEnumMode::LocalTwoPlayer),
            ..StorageLibraryQueryOptions::default()
        })?)
        .await?;
    assert!(page.entries().is_empty());
    let page = fixture
        .query(StorageLibraryQuery::new(StorageLibraryQueryOptions {
            black: StorageLibraryEnumFilter::Value(GameMetadataText::new("known".to_owned())?),
            ..StorageLibraryQueryOptions::default()
        })?)
        .await?;
    assert!(page.entries().is_empty());
    let stored: (String, Vec<u8>, String) = documents::table
        .inner_join(library_entries::table)
        .select((
            documents::body,
            library_entries::content_sha256,
            library_entries::identity_mainline_uci,
        ))
        .first(fixture.connection.diesel())?;
    assert_eq!(
        stored.1.as_slice(),
        crate::package::digest::PackageDigest::hash_slice(stored.0.as_bytes())
            .bytes()
            .as_slice()
    );
    assert_eq!(stored.2, "[]");
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn preparation_checks_the_exact_body_identity_and_new_checkpoints_cannot_insert()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    let wrong_body =
        StorageLibraryTransactionFixture::body(StorageLibraryTransactionFixture::id(2)?, None)?;
    let memory = fixture.prepared_memory(wrong_body.capacity())?;
    let rejected = fixture
        .executor
        .submit_waiting(move |cancellation| {
            StorageLibraryPreparedWrite::from_encoded(
                StorageLibraryPreparedWriteOptions {
                    id,
                    expected: GameStorageRevision::INITIAL,
                    body: wrong_body,
                    change: StorageLibraryEnumContentChange::Checkpoint,
                    memory,
                },
                cancellation,
            )
        })
        .await?
        .join()
        .await?;
    assert!(matches!(rejected, Err(StorageLibraryError::Identity)));
    let body = StorageLibraryTransactionFixture::body(id, None)?;
    let memory = fixture.prepared_memory(body.capacity())?;
    let prepared = fixture
        .executor
        .submit_waiting(move |cancellation| {
            StorageLibraryPreparedWrite::from_encoded(
                StorageLibraryPreparedWriteOptions {
                    id,
                    expected: GameStorageRevision::INITIAL,
                    body,
                    change: StorageLibraryEnumContentChange::Checkpoint,
                    memory,
                },
                cancellation,
            )
        })
        .await?
        .join()
        .await??;
    assert!(matches!(
        fixture.execute(StorageLibraryEnumTransactionRequest::Save(prepared)),
        Err(StorageLibraryError::NewContentTimestamp)
    ));
    let count: i64 = documents::table
        .count()
        .get_result(fixture.connection.diesel())?;
    assert_eq!(count, 0);
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn partial_date_overlap_agrees_with_enumerated_valid_calendar_dates()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let patterns = [
        GameMetadataDate::new(Some(2023), None, Some(31))?,
        GameMetadataDate::new(Some(2024), None, Some(31))?,
        GameMetadataDate::new(None, Some(2), Some(29))?,
        GameMetadataDate::new(None, None, Some(15))?,
        GameMetadataDate::new(Some(2023), None, Some(29))?,
        GameMetadataDate::new(Some(2024), Some(2), None)?,
        GameMetadataDate::new(None, None, None)?,
    ];
    for (index, pattern) in patterns.iter().enumerate() {
        fixture
            .save(
                StorageLibraryTransactionFixture::id(u128::try_from(index)? + 1)?,
                0,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
                Some(*pattern),
            )
            .await?;
    }
    for (lower, upper) in [
        ((2023, 2, 1), (2023, 2, 28)),
        ((2024, 2, 1), (2024, 2, 29)),
        ((2023, 4, 16), (2023, 5, 15)),
        ((2023, 12, 31), (2024, 1, 1)),
    ] {
        let interval = StorageLibraryDateInterval::new(
            GameMetadataDate::new(Some(lower.0), Some(lower.1), Some(lower.2))?,
            GameMetadataDate::new(Some(upper.0), Some(upper.1), Some(upper.2))?,
        )?;
        let page = fixture
            .query(StorageLibraryQuery::new(StorageLibraryQueryOptions {
                date: StorageLibraryEnumFilter::Value(interval),
                ..StorageLibraryQueryOptions::default()
            })?)
            .await?;
        let actual: Vec<_> = page.entries().iter().map(|entry| entry.id()).collect();
        let mut expected = Vec::new();
        for (index, pattern) in patterns.iter().enumerate() {
            if pattern.year().is_none() && pattern.month().is_none() && pattern.day().is_none() {
                continue;
            }
            let mut matches = false;
            for year in lower.0..=upper.0 {
                for month in 1..=12 {
                    for day in 1..=31 {
                        let date = (year, month, day);
                        if date >= lower
                            && date <= upper
                            && pattern.year().is_none_or(|value| value == year)
                            && pattern.month().is_none_or(|value| value == month)
                            && pattern.day().is_none_or(|value| value == day)
                            && GameMetadataDate::new(Some(year), Some(month), Some(day)).is_ok()
                        {
                            matches = true;
                        }
                    }
                }
            }
            if matches {
                expected.push(StorageLibraryTransactionFixture::id(
                    u128::try_from(index)? + 1,
                )?);
            }
        }
        assert_eq!(actual, expected);
    }
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn displayed_folder_and_tag_changes_touch_affected_games_without_touching_content()
-> StorageLibraryTransactionTestResult {
    use crate::game::library_organization::{
        GameLibraryOrganizationFolderTarget, GameLibraryOrganizationName,
        GameLibraryOrganizationTagSet, GameLibraryOrganizationTagTarget,
    };
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let root = "00000000-0000-0000-0000-000000000010".parse()?;
    let child = "00000000-0000-0000-0000-000000000011".parse()?;
    let other = "00000000-0000-0000-0000-000000000012".parse()?;
    let tag = "00000000-0000-0000-0000-000000000013".parse()?;
    for (id, parent, name) in [
        (root, None, "Root"),
        (child, Some(root), "Child"),
        (other, None, "Other"),
    ] {
        fixture.organize(
            GameLibraryOrganizationEnumMutation::CreateFolder(
                id,
                parent.map(|id| {
                    GameLibraryOrganizationFolderTarget::new(
                        id,
                        GameLibraryOrganizationRevision::ZERO,
                    )
                }),
                GameLibraryOrganizationName::new(name.to_owned())?,
            ),
            1,
        )?;
    }
    fixture.organize(
        GameLibraryOrganizationEnumMutation::CreateTag(
            tag,
            GameLibraryOrganizationName::new("Original".to_owned())?,
        ),
        1,
    )?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            id,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            None,
        )
        .await?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::MoveToFolder(
            GameLibraryOrganizationGameTarget::new(id, GameLibraryOrganizationRevision::ZERO),
            Some(GameLibraryOrganizationFolderTarget::new(
                child,
                GameLibraryOrganizationRevision::ZERO,
            )),
        ),
        2,
    )?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::ReplaceTags(
            GameLibraryOrganizationGameTarget::new(id, "1".parse()?),
            GameLibraryOrganizationTagSet::new(vec![tag])?,
        ),
        3,
    )?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::RenameFolder(
            GameLibraryOrganizationFolderTarget::new(root, GameLibraryOrganizationRevision::ZERO),
            GameLibraryOrganizationName::new("Renamed root".to_owned())?,
        ),
        4,
    )?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::ReparentFolder(
            GameLibraryOrganizationFolderTarget::new(child, GameLibraryOrganizationRevision::ZERO),
            Some(GameLibraryOrganizationFolderTarget::new(
                other,
                GameLibraryOrganizationRevision::ZERO,
            )),
        ),
        5,
    )?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::RenameTag(
            GameLibraryOrganizationTagTarget::new(tag, GameLibraryOrganizationRevision::ZERO),
            GameLibraryOrganizationName::new("Renamed tag".to_owned())?,
        ),
        6,
    )?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::DeleteTag(GameLibraryOrganizationTagTarget::new(
            tag,
            "1".parse()?,
        )),
        7,
    )?;
    let page = fixture.query(StorageLibraryQuery::default()).await?;
    let entry = &page.entries()[0];
    assert_eq!(entry.organization().revision().value(), 6);
    assert!(entry.organization().tags().ids().is_empty());
    assert_eq!(entry.organization().folder(), Some(child));
    assert_eq!(
        entry
            .modified()
            .map(GameLibraryOrganizationTimestamp::value),
        Some(7)
    );
    assert_eq!(entry.content_revision().value(), 1);
    let body = fixture.document_body()?;
    assert_eq!(body, StorageLibraryTransactionFixture::body(id, None)?);
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn prepared_reservations_transfer_through_dispatch_and_release_after_observation()
-> StorageLibraryTransactionTestResult {
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    let before = fixture.memory.available_permits();
    let body = StorageLibraryTransactionFixture::body(id, None)?;
    let memory = fixture.prepared_memory(body.capacity())?;
    let reserved = before - fixture.memory.available_permits();
    assert!(reserved >= body.len());
    let prepared = fixture
        .executor
        .submit_waiting(move |cancellation| {
            StorageLibraryPreparedWrite::from_encoded(
                StorageLibraryPreparedWriteOptions {
                    id,
                    expected: GameStorageRevision::INITIAL,
                    body,
                    change: StorageLibraryEnumContentChange::Changed(
                        GameLibraryOrganizationTimestamp::new(1)?,
                    ),
                    memory,
                },
                cancellation,
            )
        })
        .await?
        .join()
        .await??;
    let request = StorageLibraryEnumTransactionRequest::Save(prepared);
    assert_eq!(request.unreserved_admission_bytes()?, 0);
    assert_eq!(before - fixture.memory.available_permits(), reserved);
    let remaining = fixture.memory.available_permits();
    let pressure = StorageDocumentClient::reserve_memory(&fixture.memory, remaining)?;
    let response = fixture.execute(request)?;
    assert!(matches!(
        response,
        StorageLibraryEnumTransactionResponse::Saved(_)
    ));
    assert_eq!(fixture.memory.available_permits(), reserved);
    drop(pressure);
    assert_eq!(fixture.memory.available_permits(), before);
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn dropping_unqueued_prepared_owners_and_cancelled_preparation_releases_reservations()
-> StorageLibraryTransactionTestResult {
    use std::future::{Future, poll_fn};
    use std::task::Poll;
    let fixture = StorageLibraryTransactionFixture::new()?;
    let id = StorageLibraryTransactionFixture::id(1)?;
    let before = fixture.memory.available_permits();
    let body = StorageLibraryTransactionFixture::body(id, None)?;
    let memory = fixture.prepared_memory(body.capacity())?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let capacity = budget.lease(1).await?;
    let executor = CapacityExecutor::new(budget)?;
    {
        let submitted = executor.submit_waiting(move |cancellation| {
            StorageLibraryPreparedWrite::from_encoded(
                StorageLibraryPreparedWriteOptions {
                    id,
                    expected: GameStorageRevision::INITIAL,
                    body,
                    change: StorageLibraryEnumContentChange::Checkpoint,
                    memory,
                },
                cancellation,
            )
        });
        let mut submitted = std::pin::pin!(submitted);
        let pending =
            poll_fn(|context| Poll::Ready(submitted.as_mut().poll(context).is_pending())).await;
        assert!(pending);
        assert!(fixture.memory.available_permits() < before);
    }
    assert_eq!(fixture.memory.available_permits(), before);
    drop(capacity);
    executor.shutdown().await?;
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn folder_removal_failure_rolls_back_prior_unfiling_revisions_and_deleted_leaves()
-> StorageLibraryTransactionTestResult {
    use crate::game::library_organization::{
        GameLibraryOrganizationFolderTarget, GameLibraryOrganizationName,
    };
    let mut fixture = StorageLibraryTransactionFixture::new()?;
    let root = "00000000-0000-0000-0000-000000000010".parse()?;
    let child = "00000000-0000-0000-0000-000000000011".parse()?;
    let sibling = "00000000-0000-0000-0000-000000000012".parse()?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::CreateFolder(
            root,
            None,
            GameLibraryOrganizationName::new("Root".to_owned())?,
        ),
        1,
    )?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::CreateFolder(
            child,
            Some(GameLibraryOrganizationFolderTarget::new(
                root,
                GameLibraryOrganizationRevision::ZERO,
            )),
            GameLibraryOrganizationName::new("Child".to_owned())?,
        ),
        1,
    )?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::CreateFolder(
            sibling,
            None,
            GameLibraryOrganizationName::new("Unrelated".to_owned())?,
        ),
        1,
    )?;
    let game = StorageLibraryTransactionFixture::id(1)?;
    fixture
        .save(
            game,
            0,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            None,
        )
        .await?;
    fixture.organize(
        GameLibraryOrganizationEnumMutation::MoveToFolder(
            GameLibraryOrganizationGameTarget::new(game, GameLibraryOrganizationRevision::ZERO),
            Some(GameLibraryOrganizationFolderTarget::new(
                child,
                GameLibraryOrganizationRevision::ZERO,
            )),
        ),
        2,
    )?;
    let before = fixture.snapshot()?;
    fixture.batch("CREATE TRIGGER refuse_root_folder_delete BEFORE DELETE ON library_folders WHEN OLD.parent_id IS NULL AND OLD.name = 'Root' BEGIN SELECT RAISE(ABORT, 'root deletion refused'); END;")?;
    let plan = fixture.preview(StorageTrashPreviewRequest::RemoveFolder(root))?;
    let result = fixture.execute(StorageLibraryEnumTransactionRequest::ApplyDestruction(
        StorageTrashConfirmedRequest::new(
            plan.approve(),
            GameLibraryOrganizationTimestamp::new(3)?,
            None,
        )?,
    ));
    assert!(matches!(result, Err(StorageLibraryError::Diesel(_))));
    let after = fixture.snapshot()?;
    let folder: Option<String> = library_entries::table
        .select(library_entries::folder_id)
        .first(fixture.connection.diesel())?;
    let folders = fixture.folder_count()?;
    assert_eq!(after, before);
    assert_eq!(folder, Some(child.to_string()));
    assert_eq!(folders, 3);
    fixture.executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn actor_enqueue_transfers_prepared_memory_at_a_full_budget_and_releases_after_commit()
-> StorageLibraryTransactionTestResult {
    use crate::storage::document_store::StorageDocumentError;
    use crate::storage::library_repository::{
        StorageLibraryEnumMutationReceipt, StorageLibraryRepository,
    };
    use std::future::{Future, poll_fn};
    use std::task::Poll;
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite"), budget.clone()).await?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let client = store.client();
    let repository = StorageLibraryRepository::new(client.clone(), &executor);
    let id = StorageLibraryTransactionFixture::id(1)?;
    let body = StorageLibraryTransactionFixture::body(id, None)?;
    let retained = StorageLibraryPreparedMemory::reservation_bytes(body.capacity())?;
    let prepared = repository
        .prepare_write(
            id,
            GameStorageRevision::INITIAL,
            body,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
        )
        .await?;
    let pressure = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - retained)?;
    let capacity = budget.lease(1).await?;
    let mut pending = repository.begin_save(prepared)?;
    {
        let mut wait = std::pin::pin!(pending.wait());
        let was_pending =
            poll_fn(|context| Poll::Ready(wait.as_mut().poll(context).is_pending())).await;
        assert!(was_pending);
    }
    assert!(matches!(
        client.reserve_library_memory(1),
        Err(StorageDocumentError::Saturated)
    ));
    drop(capacity);
    let receipt = pending.wait().await?;
    let StorageLibraryEnumMutationReceipt::Saved(receipt) = receipt else {
        return Err("expected a committed library receipt".into());
    };
    assert_eq!(receipt.content_revision().value(), 1);
    let released = client.reserve_library_memory(retained)?;
    drop((released, pressure));
    let complete = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(complete);
    store.close().await?;
    executor.shutdown().await?;
    Ok(())
}

struct StorageLibraryTransactionForeignBudgetFixture {
    _directory_a: tempfile::TempDir,
    _directory_b: tempfile::TempDir,
    store_a: StorageDocumentStore,
    store_b: StorageDocumentStore,
    executor: CapacityExecutor,
}

impl StorageLibraryTransactionForeignBudgetFixture {
    async fn new(budget: CapacityBudget) -> StorageLibraryTransactionTestResult<Self> {
        let directory_a = tempfile::tempdir()?;
        let directory_b = tempfile::tempdir()?;
        let store_a =
            StorageDocumentStore::open(directory_a.path().join("library.sqlite"), budget.clone())
                .await?;
        let store_b =
            StorageDocumentStore::open(directory_b.path().join("library.sqlite"), budget.clone())
                .await?;
        let executor = CapacityExecutor::new(budget)?;
        Ok(Self {
            _directory_a: directory_a,
            _directory_b: directory_b,
            store_a,
            store_b,
            executor,
        })
    }

    async fn close(self) -> StorageLibraryTransactionTestResult {
        let (closed_a, closed_b) = tokio::join!(self.store_a.close(), self.store_b.close());
        let closed_executor = self.executor.shutdown().await;
        closed_a?;
        closed_b?;
        closed_executor?;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct StorageLibraryTransactionForeignBudgetSnapshot {
    catalog: crate::storage::library_repository::StorageLibraryCatalogRevision,
    indexed_games: usize,
    document: Option<(GameStorageRevision, crate::package::digest::PackageDigest)>,
}

impl StorageLibraryTransactionForeignBudgetSnapshot {
    async fn read(
        client: StorageDocumentClient,
        executor: &CapacityExecutor,
        id: GameId,
    ) -> StorageLibraryTransactionTestResult<Self> {
        let repository = crate::storage::library_repository::StorageLibraryRepository::new(
            client.clone(),
            executor,
        );
        let page = repository.query(StorageLibraryQuery::default()).await?;
        let catalog = page.catalog_revision();
        let indexed_games = page.entries().len();
        drop(page);
        let stored = client.read(id.as_uuid()).await?;
        let document = stored.as_ref().map(|stored| {
            (
                GameStorageRevision::new(stored.revision()),
                crate::package::digest::PackageDigest::hash_slice(stored.body().as_bytes()),
            )
        });
        drop(stored);
        Ok(Self {
            catalog,
            indexed_games,
            document,
        })
    }
}

struct StorageLibraryTransactionForeignBudgetAttempt {
    refused: bool,
    completion: Result<(), StorageLibraryError>,
}

impl StorageLibraryTransactionForeignBudgetAttempt {
    async fn observe(
        attempt: Result<
            crate::storage::library_repository::StorageLibraryMutation,
            StorageLibraryError,
        >,
    ) -> Self {
        match attempt {
            Err(error) => Self {
                refused: true,
                completion: Err(error),
            },
            Ok(mut pending) => Self {
                refused: false,
                completion: pending.wait().await.map(drop),
            },
        }
    }
}

#[tokio::test]
async fn foreign_prepared_write_is_refused_without_mutating_an_exhausted_destination()
-> StorageLibraryTransactionTestResult {
    use crate::storage::library_repository::StorageLibraryRepository;
    let fixture =
        StorageLibraryTransactionForeignBudgetFixture::new(CapacityBudget::discover()?).await?;
    let outcome = async {
        let id = StorageLibraryTransactionFixture::id(1)?;
        let body = StorageLibraryTransactionFixture::body(id, None)?;
        let repository_a =
            StorageLibraryRepository::new(fixture.store_a.client(), &fixture.executor);
        let repository_b =
            StorageLibraryRepository::new(fixture.store_b.client(), &fixture.executor);
        let prepared = repository_a
            .prepare_write(
                id,
                GameStorageRevision::INITIAL,
                body,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(1)?),
            )
            .await?;
        let before = StorageLibraryTransactionForeignBudgetSnapshot::read(
            fixture.store_b.client(),
            &fixture.executor,
            id,
        )
        .await?;
        let pressure = fixture
            .store_b
            .client()
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
        let attempt = StorageLibraryTransactionForeignBudgetAttempt::observe(
            repository_b.begin_save(prepared),
        )
        .await;
        drop(pressure);
        let after = StorageLibraryTransactionForeignBudgetSnapshot::read(
            fixture.store_b.client(),
            &fixture.executor,
            id,
        )
        .await?;
        Ok::<_, Box<dyn std::error::Error>>((attempt, before, after))
    }
    .await;
    let closed = fixture.close().await;
    let (attempt, before, after) = outcome?;
    closed?;
    assert!(
        attempt.refused && before == after,
        "foreign prepared write bypassed destination admission: refused={}, completion={:?}, before={before:?}, after={after:?}",
        attempt.refused,
        attempt.completion
    );
    Ok(())
}
