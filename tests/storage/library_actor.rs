use std::future::{Future, poll_fn};
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::task::Poll;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::game::enums::{GameId, GameIdentityError};
use chess::game::library_organization::GameLibraryOrganizationTimestamp;
use chess::game::storage_repository::GameStorageRevision;
use chess::notation::pgn_import::NotationPgnImport;
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use chess::storage::game_document::StorageGameDocument;
use chess::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryPreparedWrite,
};
use chess::storage::library_query::StorageLibraryQuery;
use chess::storage::library_repository::{
    StorageLibraryEnumMutationReceipt, StorageLibraryError, StorageLibraryRepository,
};

type StorageLibraryActorTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageLibraryActorFixture {
    _directory: tempfile::TempDir,
    budget: CapacityBudget,
    store: StorageDocumentStore,
    executor: CapacityExecutor,
}

impl StorageLibraryActorFixture {
    async fn new() -> StorageLibraryActorTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid capacity")?)?;
        let store =
            StorageDocumentStore::open(directory.path().join("library.sqlite"), budget.clone())
                .await?;
        let executor = CapacityExecutor::new(budget.clone())?;
        Ok(Self {
            _directory: directory,
            budget,
            store,
            executor,
        })
    }

    fn repository(&self) -> StorageLibraryRepository<'_> {
        StorageLibraryRepository::new(self.store.client(), &self.executor)
    }

    async fn prepare(
        &self,
        body: String,
        modified: i64,
    ) -> Result<StorageLibraryPreparedWrite, StorageLibraryError> {
        let repository = self.repository();
        let prepared = repository
            .prepare_write(
                Self::id()?,
                GameStorageRevision::INITIAL,
                body,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                    modified,
                )?),
            )
            .await?;
        Ok(prepared)
    }

    fn close(self) -> Pin<Box<dyn Future<Output = StorageLibraryActorTestResult>>> {
        Box::pin(async move {
            self.store.close().await?;
            self.executor.shutdown().await?;
            Ok(())
        })
    }

    fn id() -> Result<GameId, GameIdentityError> {
        uuid::Uuid::from_u128(7_001).to_string().parse()
    }

    fn body() -> StorageLibraryActorTestResult<String> {
        Self::body_with_notes(String::new())
    }

    fn body_with_notes(notes: String) -> StorageLibraryActorTestResult<String> {
        Self::body_with_fields("Elodie", notes)
    }

    fn body_with_fields(player: &str, notes: String) -> StorageLibraryActorTestResult<String> {
        let mut next = 8_000_u128;
        let source = format!("[White \"{player}\"]\n[Black \"Bob\"]\n\n1. e4 e5 *");
        let mut parser = NotationPgnImport::new(Cursor::new(source.as_bytes()), move || {
            next += 1;
            uuid::Uuid::from_u128(next).to_string().parse()
        });
        let mut game = parser.next_game(&|| false)?.ok_or("missing fixture game")?;
        game.save_notes_and_nags(game.tree().root(), notes, Vec::new())?;
        Ok(StorageGameDocument::capture_notation(Self::id()?, &game)?.json()?)
    }
}

#[tokio::test]
async fn generic_document_writes_invalidate_the_library_catalog_snapshot()
-> StorageLibraryActorTestResult {
    let fixture = StorageLibraryActorFixture::new().await?;
    let repository = fixture.repository();
    let before = repository
        .query(StorageLibraryQuery::default())
        .await?
        .catalog_revision();
    fixture
        .store
        .client()
        .save(
            StorageLibraryActorFixture::id()?.as_uuid(),
            0,
            StorageLibraryActorFixture::body()?,
        )
        .await?;
    let after = repository
        .query(StorageLibraryQuery::default())
        .await?
        .catalog_revision();
    fixture.close().await?;
    assert!(
        after.value() > before.value(),
        "a new generic game candidate did not invalidate the catalog"
    );
    Ok(())
}

#[tokio::test]
async fn retained_prepared_writes_cannot_escape_the_shared_memory_budget()
-> StorageLibraryActorTestResult {
    let fixture = StorageLibraryActorFixture::new().await?;
    let body = StorageLibraryActorFixture::body_with_notes("x".repeat(1_000_000))?;
    let attempts = StorageDocumentStore::MEMORY_BUDGET / body.len() + 1;
    let mut retained = Vec::new();
    let mut saturated = false;
    let mut failure = None;
    for _ in 0..attempts {
        match fixture.prepare(body.clone(), 3_000).await {
            Ok(prepared) => retained.push(prepared),
            Err(StorageLibraryError::Store(StorageDocumentError::Saturated)) => {
                saturated = true;
                break;
            }
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    drop(retained);
    fixture.close().await?;
    if let Some(error) = failure {
        return Err(error.into());
    }
    assert!(
        saturated,
        "retained prepared writes exceeded the shared byte budget"
    );
    Ok(())
}

#[tokio::test]
async fn generic_save_cannot_overwrite_an_indexed_library_document() -> StorageLibraryActorTestResult
{
    let fixture = StorageLibraryActorFixture::new().await?;
    let client = fixture.store.client();
    let body = StorageLibraryActorFixture::body()?;
    let prepared = fixture.prepare(body.clone(), 1_000).await?;
    let receipt = fixture.repository().begin_save(prepared)?.wait().await?;
    let StorageLibraryEnumMutationReceipt::Saved(receipt) = receipt else {
        return Err("unexpected library save response".into());
    };
    let overwrite = client
        .save(
            receipt.id().as_uuid(),
            receipt.content_revision().value(),
            body.clone(),
        )
        .await;
    let saved = client
        .read(receipt.id().as_uuid())
        .await?
        .ok_or("missing durable document")?;
    let revision = saved.revision();
    let stored_body = saved.body().to_owned();
    drop(saved);
    fixture.close().await?;
    assert!(
        overwrite.is_err(),
        "generic save bypassed the library transaction"
    );
    assert_eq!(revision, receipt.content_revision().value());
    assert_eq!(stored_body, body);
    Ok(())
}

#[tokio::test]
async fn cancelling_a_library_wait_retains_the_accepted_write_on_one_worker()
-> StorageLibraryActorTestResult {
    let fixture = StorageLibraryActorFixture::new().await?;
    let repository = fixture.repository();
    let body = StorageLibraryActorFixture::body()?;
    let prepared = fixture.prepare(body, 2_000).await?;
    let capacity = fixture.budget.lease(1).await?;
    let mut pending = repository.begin_save(prepared)?;
    let was_pending = {
        let mut wait = std::pin::pin!(pending.wait());
        poll_fn(|context| Poll::Ready(wait.as_mut().poll(context).is_pending())).await
    };
    drop(capacity);
    let receipt = pending.wait().await?;
    let StorageLibraryEnumMutationReceipt::Saved(receipt) = receipt else {
        return Err("unexpected library save response".into());
    };
    let recovered = repository
        .load(receipt.id())
        .await?
        .ok_or("missing saved library record")?;
    let content_revision = recovered.content_revision();
    let mode = recovered.record().mode();
    let plies = recovered.record().identity_line_len()?;
    drop(recovered);
    let page = repository.query(StorageLibraryQuery::default()).await?;
    let count = page.entries().len();
    drop(page);
    fixture.close().await?;
    assert!(was_pending);
    assert_eq!(content_revision, GameStorageRevision::new(1));
    assert_eq!(mode, None);
    assert_eq!(plies, 2);
    assert_eq!(count, 1);
    Ok(())
}
