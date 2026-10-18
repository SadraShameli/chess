#[path = "write_recovery_fixture.rs"]
mod fixture;

use std::future::{Future, poll_fn};
use std::task::Poll;

use chess::board::editor::BoardEditorSessionEnumEdit;
use chess::desktop::editor_commands::{
    DesktopEditorCommandsEnumChoice, DesktopEditorCommandsEnumOperation,
    DesktopEditorCommandsEnumPrompt, DesktopEditorCommandsEnumResult,
    DesktopEditorCommandsEnumWriteStatus, DesktopEditorCommandsError,
};
use chess::storage::editor_repository::{
    StorageEditorRepositoryEnumMutation, StorageEditorRepositoryError,
    StorageEditorRepositoryRevision,
};
use tokio::time::timeout;

use fixture::{
    DesktopEditorCommandsWriteRecoveryFixture, DesktopEditorCommandsWriteRecoveryTestResult,
};

#[tokio::test]
async fn explicit_save_after_definite_rollback_recovers_original_submission_and_keeps_later_edits()
-> DesktopEditorCommandsWriteRecoveryTestResult {
    let mut fixture = DesktopEditorCommandsWriteRecoveryFixture::new().await?;
    let observation = async {
        let rollback = fixture.rollback().await?;
        let retry = fixture.save().await?;
        let has_original_receipt = fixture
            .owner
            .pending_application_receipt()
            .is_some_and(|receipt| receipt.shares_outcome(&rollback.receipt));
        let finished = fixture.finish().await?;
        let current = fixture.snapshot()?;
        let stored = fixture.read().await?;
        let has_pending = fixture.owner.has_pending_write();
        let mut blocked = Vec::new();
        if matches!(&retry, Err(DesktopEditorCommandsError::WritePending)) && finished.is_err() {
            let expected = current.context();
            for operation in [
                DesktopEditorCommandsEnumOperation::Delete {
                    id: expected.draft(),
                    storage_revision: current.storage_revision(),
                    expected: Some(expected),
                },
                DesktopEditorCommandsEnumOperation::Resolve {
                    expected,
                    choice: DesktopEditorCommandsEnumChoice::Save,
                },
                DesktopEditorCommandsEnumOperation::Resolve {
                    expected,
                    choice: DesktopEditorCommandsEnumChoice::Discard,
                },
            ] {
                blocked.push(fixture.dispatch(operation).await?);
            }
        }
        Ok::<_, Box<dyn std::error::Error>>((
            rollback,
            retry,
            has_original_receipt,
            finished,
            current,
            stored,
            has_pending,
            blocked,
        ))
    }
    .await;
    let cleanup = fixture.close().await;
    let (rollback, retry, has_original_receipt, finished, current, stored, has_pending, blocked) =
        observation?;
    cleanup?;

    assert!(matches!(
        &rollback.failure,
        Err(DesktopEditorCommandsError::WriteFailed(error))
            if matches!(error.as_ref(), StorageEditorRepositoryError::Database(
                diesel::result::Error::DatabaseError(_, information)
            ) if information.message().contains("controlled editor rollback"))
    ));
    assert_eq!(
        rollback.stored,
        Some((
            rollback.baseline.draft().clone(),
            rollback.baseline.storage_revision(),
        ))
    );
    assert_eq!(rollback.pending.as_ref(), Some(rollback.submitted.draft()));
    assert!(rollback.has_original_receipt);
    assert_eq!(
        rollback.local.storage_revision(),
        rollback.submitted.storage_revision()
    );
    assert_eq!(
        rollback.local.context().session(),
        rollback.submitted.context().session()
    );
    assert_ne!(rollback.local.draft(), rollback.submitted.draft());
    assert!(rollback.local.is_dirty());
    assert!(matches!(
        rollback.local.write(),
        DesktopEditorCommandsEnumWriteStatus::Unresolved { .. }
    ));
    assert!(
        finished.is_ok(),
        "Explicit Save/finish_write cannot recover a definite SQLite rollback: retry={retry:?}, finish={finished:?}, Delete/ResolveSave/ResolveDiscard={blocked:?}"
    );
    assert!(has_original_receipt);
    let DesktopEditorCommandsEnumResult::Written { receipt, .. } = finished? else {
        return Err("retry did not acknowledge the original editor save".into());
    };
    assert_eq!(receipt.id(), rollback.submitted.context().draft());
    assert_eq!(
        receipt.storage_revision(),
        StorageEditorRepositoryRevision::new(rollback.submitted.storage_revision().value() + 1)
    );
    assert_eq!(
        receipt.mutation(),
        StorageEditorRepositoryEnumMutation::Saved {
            draft_revision: rollback.submitted.context().revision(),
            status: rollback.submitted.draft().status(),
        }
    );
    assert_eq!(
        stored,
        Some((
            rollback.submitted.draft().clone(),
            receipt.storage_revision()
        ))
    );
    assert_eq!(current.context(), rollback.local.context());
    assert_eq!(current.draft(), rollback.local.draft());
    assert_eq!(current.storage_revision(), receipt.storage_revision());
    assert!(current.is_dirty());
    assert!(current.has_undo());
    assert_eq!(
        current.prompt(),
        Some(DesktopEditorCommandsEnumPrompt::Leave)
    );
    assert_eq!(current.write(), &DesktopEditorCommandsEnumWriteStatus::Idle);
    assert!(!has_pending);
    Ok(())
}

#[tokio::test]
async fn rollback_retry_keeps_original_expected_storage_revision_and_refuses_external_update()
-> DesktopEditorCommandsWriteRecoveryTestResult {
    let mut fixture = DesktopEditorCommandsWriteRecoveryFixture::new().await?;
    let observation = async {
        let rollback = fixture.rollback().await?;
        let (external, external_receipt) = fixture.external_update(&rollback.baseline).await?;
        let retry = fixture.save().await?;
        let finished = fixture.finish().await?;
        let current = fixture.snapshot()?;
        let stored = fixture.read().await?;
        let pending = fixture.owner.pending_draft().cloned();
        Ok::<_, Box<dyn std::error::Error>>((
            rollback,
            external,
            external_receipt,
            retry,
            finished,
            current,
            stored,
            pending,
        ))
    }
    .await;
    let cleanup = fixture.close().await;
    let (rollback, external, external_receipt, retry, finished, current, stored, pending) =
        observation?;
    cleanup?;

    assert_eq!(
        stored,
        Some((external, external_receipt.storage_revision()))
    );
    assert_eq!(current.context(), rollback.local.context());
    assert_eq!(current.draft(), rollback.local.draft());
    assert_eq!(
        current.storage_revision(),
        rollback.submitted.storage_revision()
    );
    assert!(current.is_dirty());
    assert_eq!(pending.as_ref(), Some(rollback.submitted.draft()));
    assert!(
        matches!(
            &finished,
            Err(DesktopEditorCommandsError::WriteFailed(error))
                if matches!(error.as_ref(), StorageEditorRepositoryError::Conflict { expected, actual }
                    if *expected == rollback.submitted.storage_revision()
                        && *actual == external_receipt.storage_revision())
        ),
        "Definite rollback retry never reaches the original storage revision check: retry={retry:?}, finish={finished:?}"
    );
    Ok(())
}

#[tokio::test]
async fn stale_storage_revision_refuses_save_and_keeps_external_and_local_drafts_separate()
-> DesktopEditorCommandsWriteRecoveryTestResult {
    let mut fixture = DesktopEditorCommandsWriteRecoveryFixture::new().await?;
    let observation = async {
        let baseline = fixture.seed().await?;
        let (external_draft, external_receipt) = fixture.external_update(&baseline).await?;
        fixture
            .edit(BoardEditorSessionEnumEdit::Counters {
                halfmoves: 17,
                fullmoves: 83,
            })
            .await?;
        let submitted = fixture.snapshot()?;
        fixture.save().await??;
        let failed = fixture.finish().await?;
        let repeated = fixture.finish().await?;
        let current = fixture.snapshot()?;
        let pending = fixture.owner.pending_draft().cloned();
        let stored = fixture.read().await?;
        let last_receipt = fixture.owner.last_receipt();
        Ok::<_, Box<dyn std::error::Error>>((
            submitted,
            external_draft,
            external_receipt,
            failed,
            repeated,
            current,
            pending,
            stored,
            last_receipt,
        ))
    }
    .await;
    let cleanup = fixture.close().await;
    let (
        submitted,
        external_draft,
        external_receipt,
        failed,
        repeated,
        current,
        pending,
        stored,
        last_receipt,
    ) = observation?;
    cleanup?;

    for result in [failed, repeated] {
        assert!(matches!(
            result,
            Err(DesktopEditorCommandsError::WriteFailed(error))
                if matches!(error.as_ref(), StorageEditorRepositoryError::Conflict { expected, actual }
                    if *expected == submitted.storage_revision()
                        && *actual == external_receipt.storage_revision())
        ));
    }
    assert_eq!(
        stored,
        Some((external_draft, external_receipt.storage_revision()))
    );
    assert_eq!(current.context(), submitted.context());
    assert_eq!(current.draft(), submitted.draft());
    assert_eq!(current.storage_revision(), submitted.storage_revision());
    assert_eq!(pending.as_ref(), Some(submitted.draft()));
    assert!(current.is_dirty());
    assert!(last_receipt.is_none());
    Ok(())
}

#[tokio::test]
async fn abandoned_acknowledgement_wait_preserves_one_write_and_recovers_its_actual_receipt()
-> DesktopEditorCommandsWriteRecoveryTestResult {
    let mut fixture = DesktopEditorCommandsWriteRecoveryFixture::new().await?;
    let observation = async {
        fixture.seed().await?;
        fixture
            .edit(BoardEditorSessionEnumEdit::Counters {
                halfmoves: 29,
                fullmoves: 61,
            })
            .await?;
        let submitted = fixture.snapshot()?;
        let lease = fixture.budget.try_lease(fixture.budget.workers())?;
        let accepted = fixture.save().await;
        let was_pending = if matches!(&accepted, Ok(Ok(_))) {
            let mut waiting = Box::pin(fixture.owner.finish_write());
            poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context).is_pending())).await
        } else {
            false
        };
        drop(lease);
        accepted??;
        let application_receipt = fixture
            .owner
            .pending_application_receipt()
            .ok_or("missing unobserved editor application receipt")?
            .clone();
        let committed_before_observation = fixture.read().await?;
        let retry = fixture.save().await?;
        let retained_receipt = fixture
            .owner
            .pending_application_receipt()
            .is_some_and(|receipt| receipt.shares_outcome(&application_receipt));
        let pending_error = fixture.owner.pending_error().is_some();
        let pending_draft = fixture.owner.pending_draft().cloned();
        let finished = fixture.finish().await?;
        let current = fixture.snapshot()?;
        let stored = fixture.read().await?;
        let repeated = fixture.finish().await?;
        Ok::<_, Box<dyn std::error::Error>>((
            submitted,
            was_pending,
            committed_before_observation,
            retry,
            retained_receipt,
            pending_error,
            pending_draft,
            finished,
            current,
            stored,
            repeated,
        ))
    }
    .await;
    let cleanup = fixture.close().await;
    let (
        submitted,
        was_pending,
        committed_before_observation,
        retry,
        retained_receipt,
        pending_error,
        pending_draft,
        finished,
        current,
        stored,
        repeated,
    ) = observation?;
    cleanup?;

    assert!(was_pending);
    assert!(matches!(
        retry,
        Err(DesktopEditorCommandsError::WritePending)
    ));
    assert!(retained_receipt);
    assert!(!pending_error);
    assert_eq!(pending_draft.as_ref(), Some(submitted.draft()));
    let DesktopEditorCommandsEnumResult::Written { receipt, .. } = finished? else {
        return Err("unobserved write did not return its original receipt".into());
    };
    assert_eq!(receipt.id(), submitted.context().draft());
    assert_eq!(
        receipt.storage_revision(),
        StorageEditorRepositoryRevision::new(submitted.storage_revision().value() + 1)
    );
    assert_eq!(
        receipt.mutation(),
        StorageEditorRepositoryEnumMutation::Saved {
            draft_revision: submitted.context().revision(),
            status: submitted.draft().status(),
        }
    );
    assert_eq!(committed_before_observation, stored);
    assert_eq!(
        stored,
        Some((submitted.draft().clone(), receipt.storage_revision()))
    );
    assert_eq!(current.context(), submitted.context());
    assert_eq!(current.draft(), submitted.draft());
    assert!(!current.is_dirty());
    assert_eq!(current.write(), &DesktopEditorCommandsEnumWriteStatus::Idle);
    assert!(matches!(repeated, Err(DesktopEditorCommandsError::NoWrite)));
    Ok(())
}

#[tokio::test]
async fn consumed_storage_acknowledgement_error_preserves_commit_and_refuses_stale_command_save()
-> DesktopEditorCommandsWriteRecoveryTestResult {
    let mut fixture = DesktopEditorCommandsWriteRecoveryFixture::new().await?;
    let observation = async {
        let baseline = fixture.seed().await?;
        let mut write = fixture
            .repository
            .save(baseline.draft(), baseline.storage_revision())?;
        let receipt = timeout(
            DesktopEditorCommandsWriteRecoveryFixture::WAIT,
            write.wait(),
        )
        .await??;
        let acknowledgement_error = timeout(
            DesktopEditorCommandsWriteRecoveryFixture::WAIT,
            write.wait(),
        )
        .await?;
        let committed = fixture.read().await?;
        fixture
            .edit(BoardEditorSessionEnumEdit::Counters {
                halfmoves: 43,
                fullmoves: 101,
            })
            .await?;
        let submitted = fixture.snapshot()?;
        fixture.save().await??;
        let failed = fixture.finish().await?;
        let current = fixture.snapshot()?;
        let stored = fixture.read().await?;
        Ok::<_, Box<dyn std::error::Error>>((
            baseline,
            receipt,
            acknowledgement_error,
            committed,
            submitted,
            failed,
            current,
            stored,
        ))
    }
    .await;
    let cleanup = fixture.close().await;
    let (baseline, receipt, acknowledgement_error, committed, submitted, failed, current, stored) =
        observation?;
    cleanup?;

    assert!(matches!(
        acknowledgement_error,
        Err(StorageEditorRepositoryError::CompletionConsumed)
    ));
    assert_eq!(committed, stored);
    assert_eq!(
        stored,
        Some((baseline.draft().clone(), receipt.storage_revision()))
    );
    assert!(matches!(
        failed,
        Err(DesktopEditorCommandsError::WriteFailed(error))
            if matches!(error.as_ref(), StorageEditorRepositoryError::Conflict { expected, actual }
                if *expected == submitted.storage_revision()
                    && *actual == receipt.storage_revision())
    ));
    assert_eq!(current.context(), submitted.context());
    assert_eq!(current.draft(), submitted.draft());
    assert_eq!(current.storage_revision(), submitted.storage_revision());
    assert!(current.is_dirty());
    Ok(())
}
