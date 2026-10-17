#[path = "editor_commands/fixture.rs"]
pub(crate) mod fixture;

use std::future::{Future, poll_fn};
use std::task::Poll;

use crate::board::editor::{
    BoardEditorSessionEnumConfirmation, BoardEditorSessionEnumEdit,
    BoardEditorSessionEnumOperation, BoardEditorSessionEnumSelection, BoardEditorSessionEnumSetup,
    BoardEditorValidationEnumStatus,
};
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use crate::board::position::BoardPosition;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsEnumChoice, DesktopEditorCommandsEnumOperation,
    DesktopEditorCommandsEnumResult, DesktopEditorCommandsError,
};
use crate::storage::editor_repository::StorageEditorRepositoryRevision;

use fixture::{DesktopEditorCommandsFixture, DesktopEditorCommandsTestResult};

#[tokio::test]
async fn selected_position_and_standard_create_isolate_the_source_and_editor_history()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    let source = BoardPosition::from_fen(
        "4k3/8/8/8/8/8/8/4K3 b - - 41 92",
        BoardEnumVariant::Standard,
    )?;
    fixture.create(1, Some(source.clone())).await?;
    let initial = fixture.snapshot()?;
    assert_eq!(initial.draft().fields().turn(), BoardEnumColor::Black);
    fixture
        .edit(BoardEditorSessionEnumOperation::Edit(
            BoardEditorSessionEnumEdit::Place {
                square: "a3".parse()?,
                color: BoardEnumColor::White,
                kind: BoardEnumPieceKind::Queen,
            },
        ))
        .await?;
    fixture.edit(BoardEditorSessionEnumOperation::Undo).await?;
    let snapshot = fixture.snapshot()?;
    assert_eq!(snapshot.draft().validated_fen()?, source.fen());
    assert!(snapshot.has_redo());
    let before = snapshot.context();
    fixture.create(2, None).await?;
    assert_eq!(fixture.snapshot()?.context(), before);
    fixture
        .resolve(DesktopEditorCommandsEnumChoice::Discard)
        .await?;
    let standard = fixture.snapshot()?;
    assert_eq!(
        standard.draft().validated_fen()?,
        BoardPosition::standard().fen()
    );
    assert!(!standard.has_undo());
    assert!(!standard.has_redo());
    assert_eq!(source.fen(), "4k3/8/8/8/8/8/8/4K3 b - - 41 92");
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn invalid_save_reopens_raw_fields_with_empty_history_and_blocks_play_and_fen()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    fixture
        .edit(BoardEditorSessionEnumOperation::Replace {
            setup: BoardEditorSessionEnumSetup::ClearBoard,
            confirmation: BoardEditorSessionEnumConfirmation::Confirmed,
        })
        .await?;
    let expected = fixture.snapshot()?.context();
    assert!(
        fixture
            .dispatch(DesktopEditorCommandsEnumOperation::Play { expected })
            .await
            .err()
            .is_some_and(|error| matches!(
                error.downcast_ref::<DesktopEditorCommandsError>(),
                Some(DesktopEditorCommandsError::Validation(_))
            ))
    );
    assert!(
        fixture
            .dispatch(DesktopEditorCommandsEnumOperation::ExportFen { expected })
            .await
            .err()
            .is_some_and(|error| matches!(
                error.downcast_ref::<DesktopEditorCommandsError>(),
                Some(DesktopEditorCommandsError::Validation(_))
            ))
    );
    fixture.save().await?;
    assert!(fixture.snapshot()?.is_dirty());
    fixture.owner.finish_write().await?;
    assert!(!fixture.snapshot()?.is_dirty());
    fixture.create(2, None).await?;
    fixture.read(1).await?;
    fixture
        .resolve(DesktopEditorCommandsEnumChoice::Discard)
        .await?;
    let restored = fixture.snapshot()?;
    assert_eq!(restored.status(), BoardEditorValidationEnumStatus::Invalid);
    assert_eq!(restored.draft().fields().pieces().count(), 0);
    assert!(!restored.findings().is_empty());
    assert!(!restored.has_undo());
    assert!(!restored.has_redo());
    assert_eq!(restored.selection(), BoardEditorSessionEnumSelection::None);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn original_write_acknowledgement_preserves_newer_edits_and_requires_another_leave_choice()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    fixture
        .dispatch(DesktopEditorCommandsEnumOperation::Leave {
            expected: fixture.snapshot()?.context(),
        })
        .await?;
    fixture
        .resolve(DesktopEditorCommandsEnumChoice::Save)
        .await?;
    let original = fixture
        .owner
        .pending_draft()
        .ok_or("original save not retained")?
        .clone();
    fixture
        .edit(BoardEditorSessionEnumOperation::Edit(
            BoardEditorSessionEnumEdit::Turn(BoardEnumColor::Black),
        ))
        .await?;
    fixture.owner.finish_write().await?;
    let newer = fixture.snapshot()?;
    assert!(newer.is_dirty());
    assert!(newer.prompt().is_some());
    assert_ne!(original.revision(), newer.draft().revision());
    assert_eq!(
        fixture
            .repository
            .read(original.id())
            .await?
            .ok_or("missing save")?
            .draft(),
        &original
    );
    fixture
        .resolve(DesktopEditorCommandsEnumChoice::Stay)
        .await?;
    assert!(fixture.snapshot()?.prompt().is_none());
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn cancelled_save_wait_keeps_the_exact_write_and_original_draft()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let blocker = rusqlite::Connection::open(&fixture.path)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    fixture.save().await?;
    let original = fixture
        .owner
        .pending_draft()
        .ok_or("missing original")?
        .clone();
    let mut waiting = Box::pin(fixture.owner.finish_write());
    let was_pending =
        poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context).is_pending())).await;
    drop(waiting);
    let retained = fixture.owner.pending_draft().cloned();
    let dirty = fixture.snapshot()?.is_dirty();
    blocker.execute_batch("ROLLBACK")?;
    drop(blocker);
    fixture.owner.finish_write().await?;
    let saved = fixture.snapshot()?;
    fixture.close().await?;
    assert!(was_pending);
    assert_eq!(retained.as_ref(), Some(&original));
    assert!(dirty);
    assert!(!saved.is_dirty());
    Ok(())
}

#[tokio::test]
async fn conflicting_save_retains_original_and_newer_draft_without_rebasing()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let original = fixture.snapshot()?.draft().clone();
    fixture
        .repository
        .save(&original, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    fixture.save().await?;
    fixture
        .edit(BoardEditorSessionEnumOperation::Edit(
            BoardEditorSessionEnumEdit::Counters {
                halfmoves: -3,
                fullmoves: 0,
            },
        ))
        .await?;
    assert!(fixture.owner.finish_write().await.is_err());
    assert_eq!(fixture.owner.pending_draft(), Some(&original));
    let snapshot = fixture.snapshot()?;
    assert!(snapshot.is_dirty());
    assert_eq!(
        snapshot.storage_revision(),
        StorageEditorRepositoryRevision::ABSENT
    );
    assert_eq!(snapshot.draft().fields().halfmoves(), -3);
    assert!(fixture.owner.finish_write().await.is_err());
    assert_eq!(
        fixture
            .repository
            .read(original.id())
            .await?
            .ok_or("missing original")?
            .draft(),
        &original
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn fen_transfer_requires_resolution_and_creates_a_separate_draft_then_play_only_prefills()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let old = fixture.snapshot()?.context();
    let fen = "4k3/8/8/8/8/8/8/4K3 b - - 77 101";
    fixture.import(2, fen).await?;
    assert_eq!(fixture.snapshot()?.context(), old);
    fixture
        .resolve(DesktopEditorCommandsEnumChoice::Stay)
        .await?;
    assert_eq!(fixture.snapshot()?.context(), old);
    fixture.import(2, fen).await?;
    fixture
        .resolve(DesktopEditorCommandsEnumChoice::Save)
        .await?;
    fixture.owner.finish_write().await?;
    let imported = fixture.snapshot()?;
    assert_ne!(imported.context().draft(), old.draft());
    assert_eq!(imported.draft().validated_fen()?, fen);
    assert!(fixture.repository.read(old.draft()).await?.is_some());
    let result = fixture
        .dispatch(DesktopEditorCommandsEnumOperation::Play {
            expected: imported.context(),
        })
        .await?;
    let DesktopEditorCommandsEnumResult::Play { start, context } = result else {
        return Err("expected New Game prefill".into());
    };
    assert_eq!(context, imported.context());
    assert_eq!(start.initial_fen(), fen);
    assert_eq!(fixture.snapshot()?.context(), imported.context());
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn shared_canonical_page_and_delete_receipt_preserve_other_drafts()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    for id in 1..=101 {
        let draft = crate::board::editor::BoardEditorDraft::new(
            DesktopEditorCommandsFixture::id(id)?,
            None,
        );
        fixture
            .repository
            .save(&draft, StorageEditorRepositoryRevision::ABSENT)?
            .wait()
            .await?;
    }
    let DesktopEditorCommandsEnumResult::List(page) = fixture
        .dispatch(DesktopEditorCommandsEnumOperation::List { after: None })
        .await?
    else {
        return Err("missing canonical page".into());
    };
    assert_eq!(page.page().entries().len(), 100);
    let next = fixture.repository.list(page.page().next_cursor()).await?;
    assert_eq!(next.entries().len(), 1);
    let first = page.page().entries().first().ok_or("missing first row")?;
    fixture
        .dispatch(DesktopEditorCommandsEnumOperation::Delete {
            id: first.id(),
            storage_revision: first.storage_revision(),
            expected: None,
        })
        .await?;
    fixture.owner.finish_write().await?;
    assert!(fixture.repository.read(first.id()).await?.is_none());
    assert_eq!(fixture.repository.list(None).await?.entries().len(), 100);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn actual_receipt_cancelled_before_first_poll_refuses_the_save_without_sql_effect()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let operation = DesktopEditorCommandsEnumOperation::Save {
        expected: fixture.snapshot()?.context(),
    };
    let work = fixture.admit(&operation)?;
    work.receipt().cancellation().request();
    let result = fixture.owner.handle(operation, work.receipt()).await;
    let pending = fixture.owner.has_pending_write();
    let stored = fixture
        .repository
        .read(fixture.snapshot()?.context().draft())
        .await?
        .is_some();
    fixture.close().await?;
    assert!(matches!(result, Err(DesktopEditorCommandsError::Cancelled)));
    assert!(!pending);
    assert!(!stored);
    Ok(())
}

#[tokio::test]
async fn cancellation_after_staged_read_preserves_the_original_editor_session()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    fixture.save().await?;
    fixture.owner.finish_write().await?;
    let original = fixture.snapshot()?;
    let target =
        crate::board::editor::BoardEditorDraft::new(DesktopEditorCommandsFixture::id(2)?, None);
    fixture
        .repository
        .save(&target, StorageEditorRepositoryRevision::ABSENT)?
        .wait()
        .await?;
    let blocker = rusqlite::Connection::open(&fixture.path)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    let mut blocking_write = fixture
        .repository
        .save(&target, StorageEditorRepositoryRevision::new(1))?;
    let opening = crate::desktop::editor_commands::DesktopEditorCommandsOpening::new(
        target.id(),
        "00000000-0000-0002-0000-000000000002".parse()?,
    );
    let operation = DesktopEditorCommandsEnumOperation::Read {
        opening,
        expected: Some(original.context()),
    };
    let work = fixture.admit(&operation)?;
    let mut reading = Box::pin(fixture.owner.handle(operation, work.receipt()));
    let was_pending =
        poll_fn(|context| Poll::Ready(reading.as_mut().poll(context).is_pending())).await;
    work.receipt().cancellation().request();
    blocker.execute_batch("ROLLBACK")?;
    drop(blocker);
    blocking_write.wait().await?;
    let result = reading.await;
    let current = fixture.snapshot()?;
    fixture.close().await?;
    assert!(was_pending);
    assert!(matches!(result, Err(DesktopEditorCommandsError::Cancelled)));
    assert_eq!(current.context(), original.context());
    assert_eq!(current.draft(), original.draft());
    Ok(())
}

#[tokio::test]
async fn cancelled_accepted_save_finishes_the_same_write_and_keeps_the_pending_destination()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let original = fixture.snapshot()?;
    fixture
        .dispatch(DesktopEditorCommandsEnumOperation::Leave {
            expected: original.context(),
        })
        .await?;
    fixture
        .resolve(DesktopEditorCommandsEnumChoice::Save)
        .await?;
    fixture
        .owner
        .pending_application_receipt()
        .ok_or("missing real receipt")?
        .cancellation()
        .request();
    fixture.owner.finish_write().await?;
    let current = fixture.snapshot()?;
    let saved = fixture
        .repository
        .read(original.context().draft())
        .await?
        .ok_or("accepted save lost")?;
    let saved_draft = saved.draft().clone();
    drop(saved);
    fixture.close().await?;
    assert_eq!(current.context(), original.context());
    assert!(current.prompt().is_some());
    assert!(!current.is_dirty());
    assert_eq!(&saved_draft, original.draft());
    Ok(())
}

#[tokio::test]
async fn invalid_fen_unconfirmed_setup_and_old_session_requests_preserve_the_current_draft()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let original = fixture.snapshot()?;
    let invalid_fen = fixture.import(2, "not a position").await;
    let no_confirmation = fixture
        .edit(BoardEditorSessionEnumOperation::Replace {
            setup: BoardEditorSessionEnumSetup::ClearBoard,
            confirmation: BoardEditorSessionEnumConfirmation::Required,
        })
        .await;
    fixture.save().await?;
    fixture.owner.finish_write().await?;
    fixture.read(1).await?;
    let reopened = fixture.snapshot()?;
    let stale = fixture
        .dispatch(DesktopEditorCommandsEnumOperation::Apply {
            expected: original.context(),
            operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Turn(
                BoardEnumColor::Black,
            )),
        })
        .await;
    let after = fixture.snapshot()?;
    fixture.close().await?;
    assert!(invalid_fen.is_err());
    assert!(no_confirmation.is_err());
    assert_ne!(reopened.context().session(), original.context().session());
    assert!(stale.is_err());
    assert_eq!(after.context(), reopened.context());
    assert_eq!(after.draft(), original.draft());
    assert!(!after.has_undo());
    Ok(())
}

#[tokio::test]
async fn cancellation_after_awaited_list_does_not_publish_the_staged_page()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let original = fixture.snapshot()?;
    let blocker = rusqlite::Connection::open(&fixture.path)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    let other =
        crate::board::editor::BoardEditorDraft::new(DesktopEditorCommandsFixture::id(2)?, None);
    let mut blocking_write = fixture
        .repository
        .save(&other, StorageEditorRepositoryRevision::ABSENT)?;
    let operation = DesktopEditorCommandsEnumOperation::List { after: None };
    let work = fixture.admit(&operation)?;
    let mut listing = Box::pin(fixture.owner.handle(operation, work.receipt()));
    let was_pending =
        poll_fn(|context| Poll::Ready(listing.as_mut().poll(context).is_pending())).await;
    work.receipt().cancellation().request();
    blocker.execute_batch("ROLLBACK")?;
    drop(blocker);
    blocking_write.wait().await?;
    let result = listing.await;
    let after = fixture.snapshot()?;
    fixture.close().await?;
    assert!(was_pending);
    assert!(matches!(result, Err(DesktopEditorCommandsError::Cancelled)));
    assert_eq!(after.context(), original.context());
    assert_eq!(after.draft(), original.draft());
    Ok(())
}
