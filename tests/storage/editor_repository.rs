use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::task::Poll;

use chess::board::editor::{
    BoardEditorDraft, BoardEditorDraftFields, BoardEditorDraftId, BoardEditorDraftRevision,
    BoardEditorSession, BoardEditorSessionEnumEdit, BoardEditorSessionEnumOperation,
    BoardEditorSessionEnumSelection, BoardEditorSessionRequest, BoardEditorValidationEnumStatus,
};
use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use chess::storage::editor_repository::{
    StorageEditorRepository, StorageEditorRepositoryEnumMutation, StorageEditorRepositoryError,
    StorageEditorRepositoryRevision,
};
use chess::storage::migration::StorageMigration;
use rusqlite::{Connection, params};
use uuid::Uuid;

struct StorageEditorRepositoryFixture;

impl StorageEditorRepositoryFixture {
    fn id(value: u128) -> Result<BoardEditorDraftId, Box<dyn std::error::Error>> {
        Ok(Uuid::from_u128(value).to_string().parse()?)
    }

    fn draft(value: u128, valid: bool) -> Result<BoardEditorDraft, Box<dyn std::error::Error>> {
        let id = Self::id(value)?;
        Ok(if valid {
            BoardEditorDraft::new(id, None)
        } else {
            BoardEditorDraft::from_parts(
                id,
                BoardEditorDraftRevision::new(73),
                BoardEditorDraftFields::empty(BoardEnumVariant::Standard),
            )
        })
    }

    fn edit(
        session: &mut BoardEditorSession,
        operation: BoardEditorSessionEnumOperation,
    ) -> Result<(), Box<dyn std::error::Error>> {
        session.apply(BoardEditorSessionRequest {
            draft: session.draft().id(),
            expected_revision: session.draft().revision(),
            operation,
        })?;
        Ok(())
    }
}

#[tokio::test]
async fn valid_and_invalid_drafts_reopen_exact_fields_with_empty_history_and_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("editor.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StorageEditorRepository::new(store.client());
    let mut session = BoardEditorSession::new(StorageEditorRepositoryFixture::id(1)?, None);
    StorageEditorRepositoryFixture::edit(
        &mut session,
        BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Counters {
            halfmoves: -19,
            fullmoves: 1_000_001,
        }),
    )?;
    StorageEditorRepositoryFixture::edit(
        &mut session,
        BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Turn(
            BoardEnumColor::Black,
        )),
    )?;
    StorageEditorRepositoryFixture::edit(&mut session, BoardEditorSessionEnumOperation::Undo)?;
    StorageEditorRepositoryFixture::edit(
        &mut session,
        BoardEditorSessionEnumOperation::Select(BoardEditorSessionEnumSelection::Palette {
            color: BoardEnumColor::Black,
            kind: BoardEnumPieceKind::Queen,
        }),
    )?;
    assert!(session.has_undo());
    assert!(session.has_redo());
    let expected = [
        StorageEditorRepositoryFixture::draft(2, true)?,
        session.draft().clone(),
    ];
    for draft in &expected {
        let receipt = repository
            .save(draft, StorageEditorRepositoryRevision::ABSENT)?
            .wait()
            .await?;
        assert_eq!(receipt.id(), draft.id());
        assert_eq!(receipt.storage_revision().value(), 1);
        assert_eq!(
            receipt.mutation(),
            StorageEditorRepositoryEnumMutation::Saved {
                draft_revision: draft.revision(),
                status: draft.status(),
            }
        );
    }
    assert!(store.client().inventory(None).await?.is_empty());
    store.close().await?;
    let reopened = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StorageEditorRepository::new(reopened.client());
    for draft in &expected {
        let saved = repository.read(draft.id()).await?.ok_or("missing draft")?;
        assert_eq!(saved.draft(), draft);
        assert_eq!(saved.storage_revision().value(), 1);
        let session = saved.into_session();
        assert!(!session.has_undo());
        assert!(!session.has_redo());
        assert_eq!(session.selection(), BoardEditorSessionEnumSelection::None);
        assert_eq!(session.draft().validate().is_ok(), draft.validate().is_ok());
    }
    reopened.close().await?;
    let connection = Connection::open(path)?;
    let bodies = connection
        .prepare("SELECT body FROM editor_drafts")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for body in bodies {
        assert!(!body.contains("undo"));
        assert!(!body.contains("redo"));
        assert!(!body.contains("selection"));
        assert!(body.contains("\"status\":"));
    }
    Ok(())
}

#[tokio::test]
async fn duplicate_identity_stale_save_and_stale_delete_preserve_original_content()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("editor.sqlite3"),
        CapacityBudget::discover()?,
    )
    .await?;
    let repository = StorageEditorRepository::new(store.client());
    let original = StorageEditorRepositoryFixture::draft(1, true)?;
    let replacement = StorageEditorRepositoryFixture::draft(1, false)?;
    let first = repository
        .save(&original, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    assert!(
        matches!(repository.save(&replacement, StorageEditorRepositoryRevision::ABSENT)?.wait().await,
        Err(StorageEditorRepositoryError::Conflict { expected, actual }) if expected.value() == 0 && actual.value() == 1)
    );
    let second = repository
        .save(&replacement, first.storage_revision())?
        .wait()
        .await?;
    assert_eq!(second.storage_revision().value(), 2);
    assert_eq!(replacement.revision().value(), 73);
    assert!(matches!(
        repository
            .save(&original, first.storage_revision())?
            .wait()
            .await,
        Err(StorageEditorRepositoryError::Conflict { .. })
    ));
    assert!(matches!(
        repository
            .delete(original.id(), first.storage_revision())?
            .wait()
            .await,
        Err(StorageEditorRepositoryError::Conflict { .. })
    ));
    let saved = repository
        .read(original.id())
        .await?
        .ok_or("missing replacement")?;
    store.close().await?;
    assert_eq!(saved.draft(), &replacement);
    assert_eq!(saved.storage_revision(), second.storage_revision());
    Ok(())
}

#[tokio::test]
async fn delete_is_durable_and_tombstone_prevents_stale_identity_recreation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("editor.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let repository = StorageEditorRepository::new(store.client());
    let draft = StorageEditorRepositoryFixture::draft(1, false)?;
    let saved = repository
        .save(&draft, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    let deleted = repository
        .delete(draft.id(), saved.storage_revision())?
        .wait()
        .await?;
    assert_eq!(
        deleted.mutation(),
        StorageEditorRepositoryEnumMutation::Deleted
    );
    assert_eq!(deleted.storage_revision().value(), 2);
    store.close().await?;
    let reopened = StorageDocumentStore::open(path, CapacityBudget::discover()?).await?;
    let repository = StorageEditorRepository::new(reopened.client());
    let missing = repository.read(draft.id()).await?;
    let page = repository.list(None).await?;
    let stale = repository
        .save(&draft, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await;
    let recreation = repository
        .save(&draft, deleted.storage_revision())?
        .wait()
        .await;
    let repeated_delete = repository
        .delete(draft.id(), deleted.storage_revision())?
        .wait()
        .await;
    reopened.close().await?;
    assert!(missing.is_none());
    assert!(page.entries().is_empty());
    assert!(matches!(
        stale,
        Err(StorageEditorRepositoryError::Conflict { .. })
    ));
    assert!(matches!(
        recreation,
        Err(StorageEditorRepositoryError::Deleted { .. })
    ));
    assert!(matches!(
        repeated_delete,
        Err(StorageEditorRepositoryError::Deleted { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn canceled_wait_preserves_original_save_and_delete_receipts_after_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("editor.sqlite3");
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store = StorageDocumentStore::open(path.clone(), budget.clone()).await?;
    let repository = StorageEditorRepository::new(store.client());
    let draft = StorageEditorRepositoryFixture::draft(1, false)?;
    let held = budget.try_lease(1)?;
    let mut pending = repository.save(&draft, StorageEditorRepositoryRevision::ABSENT)?;
    let mut waiting = Box::pin(pending.wait());
    let save_was_pending =
        poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context).is_pending())).await;
    drop(waiting);
    drop(held);
    assert_eq!(pending.id(), draft.id());
    assert_eq!(
        pending.expected_storage_revision(),
        StorageEditorRepositoryRevision::ABSENT
    );
    assert_eq!(pending.draft_revision(), Some(draft.revision()));
    let receipt = pending.wait().await?;
    let consumed = pending.wait().await;
    let held = budget.try_lease(1)?;
    let mut deletion = repository.delete(draft.id(), receipt.storage_revision())?;
    let mut waiting = Box::pin(deletion.wait());
    let delete_was_pending =
        poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context).is_pending())).await;
    drop(waiting);
    drop(held);
    assert_eq!(deletion.id(), draft.id());
    assert_eq!(
        deletion.expected_storage_revision(),
        receipt.storage_revision()
    );
    assert_eq!(deletion.draft_revision(), None);
    let deleted = deletion.wait().await?;
    store.close().await?;
    let reopened = StorageDocumentStore::open(path, CapacityBudget::discover()?).await?;
    let absent = StorageEditorRepository::new(reopened.client())
        .read(draft.id())
        .await?;
    reopened.close().await?;
    assert!(save_was_pending);
    assert!(delete_was_pending);
    assert!(matches!(
        consumed,
        Err(StorageEditorRepositoryError::CompletionConsumed)
    ));
    assert_eq!(receipt.storage_revision().value(), 1);
    assert_eq!(deleted.storage_revision().value(), 2);
    assert!(absent.is_none());
    Ok(())
}

#[tokio::test]
async fn list_uses_stable_typed_keyset_pages_of_at_most_one_hundred()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("editor.sqlite3"),
        CapacityBudget::discover()?,
    )
    .await?;
    let repository = StorageEditorRepository::new(store.client());
    for value in (1..=103).rev() {
        let draft = StorageEditorRepositoryFixture::draft(value, value % 2 == 0)?;
        repository
            .save(&draft, StorageEditorRepositoryRevision::ABSENT)?
            .wait()
            .await?;
    }
    let first = repository.list(None).await?;
    assert_eq!(first.entries().len(), 100);
    let cursor = first.next_cursor().ok_or("missing cursor")?;
    let second = repository.list(Some(cursor)).await?;
    assert_eq!(second.entries().len(), 3);
    assert!(second.next_cursor().is_none());
    let mut ids = Vec::new();
    for entry in first.entries().iter().chain(second.entries()) {
        ids.push(entry.id());
        let draft = repository
            .read(entry.id())
            .await?
            .ok_or("missing listed draft")?;
        assert_eq!(entry.status(), draft.draft().status());
        assert_eq!(entry.draft_revision(), draft.draft().revision());
        assert_eq!(entry.storage_revision(), draft.storage_revision());
    }
    let expected = (1..=103)
        .map(StorageEditorRepositoryFixture::id)
        .collect::<Result<Vec<_>, _>>()?;
    store.close().await?;
    assert_eq!(ids, expected);
    assert_eq!(cursor.id(), StorageEditorRepositoryFixture::id(100)?);
    Ok(())
}

#[tokio::test]
async fn actor_queue_admission_is_shared_and_releases_after_accepted_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store =
        StorageDocumentStore::open(directory.path().join("editor.sqlite3"), budget.clone()).await?;
    let client = store.client();
    let repository = StorageEditorRepository::new(client.clone());
    let draft = StorageEditorRepositoryFixture::draft(1, true)?;
    let held = budget.try_lease(1)?;
    let mut pending = Vec::new();
    for _ in 0..StorageDocumentStore::QUEUE_CAPACITY {
        pending.push(repository.save(&draft, StorageEditorRepositoryRevision::ABSENT)?);
    }
    let refused = repository.save(&draft, StorageEditorRepositoryRevision::ABSENT);
    let mut generic = Box::pin(client.save(Uuid::from_u128(1), 0, "{}".into()));
    let generic_refused = poll_fn(|context| {
        Poll::Ready(match generic.as_mut().poll(context) {
            Poll::Ready(result) => Some(result),
            Poll::Pending => None,
        })
    })
    .await;
    drop(generic);
    drop(held);
    let mut successes = 0;
    let mut conflicts = 0;
    for mut write in pending {
        match write.wait().await {
            Ok(_) => successes += 1,
            Err(StorageEditorRepositoryError::Conflict { .. }) => conflicts += 1,
            Err(error) => return Err(error.into()),
        }
    }
    let restored = repository.read(draft.id()).await?;
    store.close().await?;
    assert!(matches!(
        refused,
        Err(StorageEditorRepositoryError::Document(
            StorageDocumentError::Saturated
        ))
    ));
    assert!(matches!(
        generic_refused,
        Some(Err(StorageDocumentError::Saturated))
    ));
    assert_eq!(successes, 1);
    assert_eq!(conflicts, StorageDocumentStore::QUEUE_CAPACITY - 1);
    assert!(restored.is_some());
    Ok(())
}

#[tokio::test]
async fn editor_admission_uses_actual_shared_document_memory_reservations()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("editor.sqlite3"),
        CapacityBudget::discover()?,
    )
    .await?;
    let client = store.client();
    let repository = StorageEditorRepository::new(client.clone());
    let id = Uuid::from_u128(1);
    let body = format!(
        "\"{}\"",
        " ".repeat(StorageDocumentStore::MEMORY_BUDGET / 2 - 2)
    );
    client.save(id, 0, body).await?;
    let first = client.read(id).await?.ok_or("missing document")?;
    let second = client.read(id).await?.ok_or("missing document")?;
    let draft = StorageEditorRepositoryFixture::draft(1, true)?;
    let refused = repository.save(&draft, StorageEditorRepositoryRevision::ABSENT);
    let read_refused = repository.read(draft.id()).await;
    let list_refused = repository.list(None).await;
    let delete_refused = repository.delete(draft.id(), StorageEditorRepositoryRevision::ABSENT);
    drop((first, second));
    let accepted = repository
        .save(&draft, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    store.close().await?;
    assert!(matches!(
        refused,
        Err(StorageEditorRepositoryError::Document(
            StorageDocumentError::Saturated
        ))
    ));
    assert!(matches!(
        read_refused,
        Err(StorageEditorRepositoryError::Document(
            StorageDocumentError::Saturated
        ))
    ));
    assert!(matches!(
        list_refused,
        Err(StorageEditorRepositoryError::Document(
            StorageDocumentError::Saturated
        ))
    ));
    assert!(matches!(
        delete_refused,
        Err(StorageEditorRepositoryError::Document(
            StorageDocumentError::Saturated
        ))
    ));
    assert_eq!(accepted.storage_revision().value(), 1);
    Ok(())
}

#[tokio::test]
async fn malformed_status_unknown_fields_and_future_draft_versions_preserve_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("editor.sqlite3");
    let draft = StorageEditorRepositoryFixture::draft(1, false)?;
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    StorageEditorRepository::new(store.client())
        .save(&draft, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    store.close().await?;
    let connection = Connection::open(&path)?;
    let original: String =
        connection.query_row("SELECT body FROM editor_drafts", [], |row| row.get(0))?;
    drop(connection);
    let wrong_id = original.replace(
        &draft.id().to_string(),
        &StorageEditorRepositoryFixture::id(2)?.to_string(),
    );
    let wrong_status = original.replace("\"status\":\"invalid\"", "\"status\":\"valid\"");
    let unknown_field = original.replacen('{', "{\"undo\":[],", 1);
    let oversized = format!(
        "{}{}",
        original,
        " ".repeat(StorageEditorRepository::MAX_DRAFT_BYTES)
    );
    for (version, body) in [
        (1, "{}".to_owned()),
        (1, wrong_id),
        (1, wrong_status),
        (1, unknown_field),
        (1, oversized),
        (2, original),
    ] {
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "ignore_check_constraints", true)?;
        connection.execute(
            "UPDATE editor_drafts SET schema_version = ?1, body = ?2",
            params![version, body],
        )?;
        drop(connection);
        let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
        let repository = StorageEditorRepository::new(store.client());
        let read = repository.read(draft.id()).await;
        let list = repository.list(None).await;
        let save = repository
            .save(&draft, StorageEditorRepositoryRevision::new(1))?
            .wait()
            .await;
        let delete = repository
            .delete(draft.id(), StorageEditorRepositoryRevision::new(1))?
            .wait()
            .await;
        store.close().await?;
        let connection = Connection::open(&path)?;
        let after: (u32, String, String) = connection.query_row(
            "SELECT schema_version, body, storage_revision FROM editor_drafts",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        drop(connection);
        assert!(read.is_err());
        assert!(list.is_err());
        assert!(save.is_err());
        assert!(delete.is_err());
        if version == 2 {
            for result in [
                read.map(|_| ()),
                list.map(|_| ()),
                save.map(|_| ()),
                delete.map(|_| ()),
            ] {
                assert!(matches!(
                    result,
                    Err(StorageEditorRepositoryError::FutureVersion {
                        found: 2,
                        supported: 1
                    })
                ));
            }
        }
        assert_eq!(after, (version, body, "1".into()));
    }
    Ok(())
}

#[tokio::test]
async fn failed_save_and_delete_transactions_leave_original_row_and_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("editor.sqlite3");
    let draft = StorageEditorRepositoryFixture::draft(1, true)?;
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    StorageEditorRepository::new(store.client())
        .save(&draft, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    store.close().await?;
    let connection = Connection::open(&path)?;
    connection.execute_batch("CREATE TABLE editor_commit_guard (document_id TEXT NOT NULL REFERENCES documents(id) DEFERRABLE INITIALLY DEFERRED) STRICT; CREATE TRIGGER reject_editor_write AFTER UPDATE ON editor_drafts BEGIN INSERT INTO editor_commit_guard VALUES ('missing-document'); END;")?;
    drop(connection);
    let store = StorageDocumentStore::open(path, CapacityBudget::discover()?).await?;
    let repository = StorageEditorRepository::new(store.client());
    let replacement = StorageEditorRepositoryFixture::draft(1, false)?;
    let save = repository
        .save(&replacement, StorageEditorRepositoryRevision::new(1))?
        .wait()
        .await;
    let delete = repository
        .delete(draft.id(), StorageEditorRepositoryRevision::new(1))?
        .wait()
        .await;
    let recovered = repository.read(draft.id()).await?.ok_or("lost draft")?;
    store.close().await?;
    assert!(matches!(save, Err(StorageEditorRepositoryError::Sqlite(_))));
    assert!(matches!(
        delete,
        Err(StorageEditorRepositoryError::Sqlite(_))
    ));
    assert_eq!(recovered.draft(), &draft);
    assert_eq!(recovered.storage_revision().value(), 1);
    Ok(())
}

#[tokio::test]
async fn migration_four_preserves_version_three_documents_and_rejects_future_database()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("editor.sqlite3");
    let migrations = StorageMigration::core().into_iter().take(3).collect();
    let store = StorageDocumentStore::open_with_migrations(
        path.clone(),
        migrations,
        CapacityBudget::discover()?,
    )
    .await?;
    let id = Uuid::from_u128(99);
    store.client().save(id, 0, "{}".into()).await?;
    store.close().await?;
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let preserved = store
        .client()
        .read(id)
        .await?
        .ok_or("lost legacy document")?;
    let draft = StorageEditorRepositoryFixture::draft(1, false)?;
    StorageEditorRepository::new(store.client())
        .save(&draft, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    store.close().await?;
    let connection = Connection::open(&path)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let body: String =
        connection.query_row("SELECT body FROM editor_drafts", [], |row| row.get(0))?;
    connection.pragma_update(None, "user_version", 5)?;
    drop(connection);
    let future = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await;
    let connection = Connection::open(path)?;
    let after: String =
        connection.query_row("SELECT body FROM editor_drafts", [], |row| row.get(0))?;
    assert_eq!(preserved.revision(), 1);
    assert_eq!(preserved.body(), "{}");
    assert_eq!(version, 4);
    assert!(matches!(
        future,
        Err(StorageDocumentError::FutureSchema {
            found: 5,
            supported: 4
        })
    ));
    assert_eq!(after, body);
    Ok(())
}

#[test]
fn editor_identity_and_revision_deserialization_enforce_canonical_values()
-> Result<(), Box<dyn std::error::Error>> {
    for id in [
        "00000000-0000-0000-0000-000000000000",
        "00000000-0000-0000-0000-00000000000A",
        "1",
    ] {
        assert!(id.parse::<BoardEditorDraftId>().is_err());
    }
    for value in ["\"01\"", "\"-1\"", "\"18446744073709551616\"", "1"] {
        assert!(serde_json::from_str::<StorageEditorRepositoryRevision>(value).is_err());
    }
    let revision: StorageEditorRepositoryRevision =
        serde_json::from_str("\"18446744073709551615\"")?;
    assert_eq!(revision.value(), u64::MAX);
    assert_eq!(
        StorageEditorRepositoryFixture::draft(1, false)?.status(),
        BoardEditorValidationEnumStatus::Invalid
    );
    Ok(())
}

#[tokio::test]
async fn maximum_raw_board_fields_fit_transport_without_serializing_session_history()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("editor.sqlite3"),
        CapacityBudget::discover()?,
    )
    .await?;
    let repository = StorageEditorRepository::new(store.client());
    let mut session = BoardEditorSession::from_draft(BoardEditorDraft::from_parts(
        StorageEditorRepositoryFixture::id(1)?,
        BoardEditorDraftRevision::ZERO,
        BoardEditorDraftFields::empty(BoardEnumVariant::Chess960),
    ));
    for index in 0_u8..64 {
        let square = index.try_into()?;
        StorageEditorRepositoryFixture::edit(
            &mut session,
            BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Place {
                square,
                color: BoardEnumColor::White,
                kind: BoardEnumPieceKind::King,
            }),
        )?;
        StorageEditorRepositoryFixture::edit(
            &mut session,
            BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::GrantCastling(
                square,
            )),
        )?;
    }
    StorageEditorRepositoryFixture::edit(
        &mut session,
        BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Counters {
            halfmoves: i64::MIN,
            fullmoves: i64::MAX,
        }),
    )?;
    let receipt = repository
        .save(session.draft(), StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    let recovered = repository
        .read(session.draft().id())
        .await?
        .ok_or("missing maximum draft")?;
    store.close().await?;
    assert_eq!(receipt.storage_revision().value(), 1);
    assert!(session.has_undo());
    assert_eq!(recovered.draft(), session.draft());
    assert_eq!(recovered.draft().fields().pieces().count(), 64);
    assert_eq!(recovered.draft().fields().castling_rights().count(), 64);
    assert!(recovered.draft().validate().is_err());
    assert!(!recovered.into_session().has_undo());
    Ok(())
}

#[tokio::test]
async fn malformed_and_exhausted_storage_revisions_cannot_overwrite_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("editor.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let draft = StorageEditorRepositoryFixture::draft(1, true)?;
    StorageEditorRepository::new(store.client())
        .save(&draft, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    store.close().await?;
    let exhausted = u64::MAX.to_string();
    for revision in ["0", "01", "+1", "18446744073709551616", exhausted.as_str()] {
        let connection = Connection::open(&path)?;
        connection.execute("UPDATE editor_drafts SET storage_revision = ?1", [revision])?;
        let original: String =
            connection.query_row("SELECT body FROM editor_drafts", [], |row| row.get(0))?;
        drop(connection);
        let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
        let repository = StorageEditorRepository::new(store.client());
        let expected = StorageEditorRepositoryRevision::new(u64::MAX);
        let save = repository.save(&draft, expected)?.wait().await;
        let delete = repository.delete(draft.id(), expected)?.wait().await;
        store.close().await?;
        let connection = Connection::open(&path)?;
        let after: (String, String) = connection.query_row(
            "SELECT storage_revision, body FROM editor_drafts",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        drop(connection);
        assert!(save.is_err());
        assert!(delete.is_err());
        if revision == exhausted {
            assert!(matches!(
                save,
                Err(StorageEditorRepositoryError::RevisionOverflow)
            ));
            assert!(matches!(
                delete,
                Err(StorageEditorRepositoryError::RevisionOverflow)
            ));
        }
        assert_eq!(after, (revision.to_owned(), original));
    }
    Ok(())
}
