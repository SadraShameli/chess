#[path = "closing_review/fixture.rs"]
mod fixture;

use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::Poll;

use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::game_registry::{DesktopGameRegistryCloseEnumProof, DesktopGameRegistryError};
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::game::enums::GameEnumPhase;
use crate::storage::library_record::StorageLibraryEnumContent;
use crate::storage::library_repository::StorageLibraryRepository;

use self::fixture::{
    DesktopGameRegistryClosingReviewFixture, DesktopGameRegistryClosingReviewGate,
    DesktopGameRegistryClosingReviewOpenObservation,
};

#[tokio::test]
async fn quit_completed_before_close_retains_actual_acknowledgment_and_releases_barrier()
-> DesktopLibraryServiceTestResult {
    let gate = DesktopGameRegistryClosingReviewGate::new();
    let mut fixture = DesktopGameRegistryClosingReviewFixture::new(Some(Arc::clone(&gate))).await?;
    let initial = fixture
        .library
        .seed(DesktopGameRegistryClosingReviewFixture::TARGET, false)
        .await?;
    let opened = fixture.open_target(initial).await?;
    let started = fixture
        .target_action(DesktopGameCommandsEnumAction::Start)
        .await?;
    fixture.poll().await?;
    let quit = fixture
        .target_action(DesktopGameCommandsEnumAction::Quit)
        .await?;
    gate.entered.notified().await;
    gate.release.add_permits(1);
    gate.finished.notified().await;
    let shutdowns_before_close = fixture.library.shutdowns.load(Ordering::SeqCst);
    fixture.poll().await?;
    let recovered = fixture.stored_target().await?;
    let stored = recovered.content_revision();
    let persisted_revision = match recovered.record().content() {
        StorageLibraryEnumContent::Played(session) => Some(session.revision()),
        StorageLibraryEnumContent::Notation(_) => None,
    };
    drop(recovered);
    let mut close = fixture.target_close(stored)?;
    let operation = close.operation_id();
    let outcome = fixture.drive(close.wait()).await?;
    let first_failure = outcome.as_ref().err().map(ToString::to_string);
    let mut retry_failure = None;
    let certificate = match outcome {
        Ok(certificate) => Some(certificate),
        Err(_) => {
            fixture.games.try_retry_close(&close)?;
            match fixture.drive(close.wait()).await? {
                Ok(certificate) => Some(certificate),
                Err(error) => {
                    retry_failure = Some(error.to_string());
                    None
                }
            }
        }
    };
    let blocked = fixture.target_is_blocked(stored)?;
    let reconciled = fixture.games.try_reconcile_close(quit.id(), operation)?;
    let proof = certificate.as_ref().and_then(|certificate| {
        if let DesktopGameRegistryCloseEnumProof::Closed {
            acknowledgment,
            snapshot,
        } = certificate.proof()
        {
            Some((
                *acknowledgment,
                snapshot.revision(),
                snapshot.is_save_required(),
            ))
        } else {
            None
        }
    });
    if let Some(certificate) = certificate {
        fixture.release(&mut close, certificate).await?;
    }
    let released = matches!(
        fixture.games.try_reconcile_close(quit.id(), operation),
        Err(DesktopGameRegistryError::CloseConflict)
    );
    let unchanged = fixture.stored_target().await?.content_revision() == stored;
    let factories = fixture.library.factories.load(Ordering::SeqCst);
    let reopened = if released {
        Some(fixture.open_target(stored).await?)
    } else {
        None
    };
    let reopened_storage = reopened
        .as_ref()
        .map(|snapshot| snapshot.context().storage_revision());
    let reopened_paused = reopened.as_ref().is_some_and(|snapshot| {
        matches!(
            snapshot.state(),
            crate::desktop::game_editor::DesktopGameEditorSnapshotEnumState::Played {
                phase: GameEnumPhase::Interrupted,
                ..
            }
        )
    });
    drop(reopened);
    drop((opened, reconciled, gate));
    Box::pin(fixture.close()).await?;
    assert_eq!(shutdowns_before_close, 1);
    assert_eq!(factories, 1);
    assert_eq!(started.phase(), GameEnumPhase::Active);
    assert_eq!(quit.phase(), GameEnumPhase::Interrupted);
    assert!(!quit.is_save_required());
    assert!(blocked);
    assert!(unchanged);
    assert_eq!(persisted_revision, Some(quit.revision()));
    assert!(
        first_failure.is_none(),
        "close lost the completed Quit owner: {first_failure:?}; retry: {retry_failure:?}"
    );
    let (acknowledgment, snapshot_revision, save_required) =
        proof.ok_or("completed Quit did not produce an actual Closed proof")?;
    assert_eq!(acknowledgment.game(), quit.id());
    assert_eq!(acknowledgment.game_revision(), quit.revision());
    assert_eq!(acknowledgment.game_revision(), snapshot_revision);
    assert_eq!(acknowledgment.storage_revision(), stored);
    assert!(!save_required);
    assert_eq!(close.operation_id(), operation);
    assert!(released);
    assert_eq!(reopened_storage, Some(stored));
    assert!(reopened_paused);
    Ok(())
}

#[tokio::test]
async fn unrelated_saved_open_progresses_while_target_certificate_is_retained()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopGameRegistryClosingReviewFixture::new(None).await?;
    let target_stored = fixture
        .library
        .seed(DesktopGameRegistryClosingReviewFixture::TARGET, false)
        .await?;
    let other_stored = fixture
        .library
        .seed(DesktopGameRegistryClosingReviewFixture::OTHER, false)
        .await?;
    let other = DesktopLibraryServiceFixture::id(DesktopGameRegistryClosingReviewFixture::OTHER)?;
    let opened = fixture.open_target(target_stored).await?;
    let mut close = fixture.target_close(target_stored)?;
    let certificate = fixture.drive(close.wait()).await??;
    let actual_closed = matches!(
        certificate.proof(),
        DesktopGameRegistryCloseEnumProof::Closed { .. }
    );
    let application = fixture.library.receipt()?;
    let admission = fixture
        .games
        .try_open_saved_editor(other, other_stored, application.clone());
    let admission_failure = admission.as_ref().err().map(ToString::to_string);
    let observation = if let Ok(mut receipt) = admission {
        let result = fixture.drive(receipt.wait()).await?;
        Some(DesktopGameRegistryClosingReviewOpenObservation::capture(
            &receipt,
            &application,
            result,
        ))
    } else {
        None
    };
    let blocked = fixture.target_is_blocked(certificate.storage_revision())?;
    let unchanged =
        fixture.stored_target().await?.content_revision() == certificate.storage_revision();
    fixture.release(&mut close, certificate).await?;
    drop((opened, application));
    Box::pin(fixture.close()).await?;
    assert!(actual_closed);
    assert!(blocked);
    assert!(unchanged);
    assert!(
        admission_failure.is_none(),
        "target close globally refused an unrelated saved open: {admission_failure:?}"
    );
    observation
        .ok_or("unrelated saved open was not admitted")?
        .assert_opened(other, other_stored);
    Ok(())
}

#[tokio::test]
async fn target_close_preserves_an_already_admitted_unrelated_saved_open()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopGameRegistryClosingReviewFixture::new(None).await?;
    let target_stored = fixture
        .library
        .seed(DesktopGameRegistryClosingReviewFixture::TARGET, false)
        .await?;
    let other_stored = fixture
        .library
        .seed(DesktopGameRegistryClosingReviewFixture::OTHER, false)
        .await?;
    let opened = fixture.open_target(target_stored).await?;
    let other = DesktopLibraryServiceFixture::id(DesktopGameRegistryClosingReviewFixture::OTHER)?;
    let application = fixture.library.receipt()?;
    let mut opening =
        fixture
            .games
            .try_open_saved_editor(other, other_stored, application.clone())?;
    let queued = opening.try_result().is_none() && opening.editor_receipt().is_none();
    let mut close = fixture.target_close(target_stored)?;
    let certificate = fixture.drive(close.wait()).await??;
    let result = fixture.drive(opening.wait()).await?;
    let observation =
        DesktopGameRegistryClosingReviewOpenObservation::capture(&opening, &application, result);
    let blocked = fixture.target_is_blocked(certificate.storage_revision())?;
    fixture.release(&mut close, certificate).await?;
    drop((opened, opening, application));
    Box::pin(fixture.close()).await?;
    assert!(queued);
    assert!(blocked);
    observation.assert_opened(other, other_stored);
    Ok(())
}

#[tokio::test]
async fn unrelated_saved_constructor_does_not_delay_actual_target_close_certification()
-> DesktopLibraryServiceTestResult {
    let gate = DesktopGameRegistryClosingReviewGate::new();
    let mut fixture = DesktopGameRegistryClosingReviewFixture::new(Some(Arc::clone(&gate))).await?;
    let initial = fixture
        .library
        .seed(DesktopGameRegistryClosingReviewFixture::TARGET, false)
        .await?;
    let other_stored = fixture
        .library
        .seed(DesktopGameRegistryClosingReviewFixture::OTHER, false)
        .await?;
    let other = DesktopLibraryServiceFixture::id(DesktopGameRegistryClosingReviewFixture::OTHER)?;
    let recovered =
        StorageLibraryRepository::new(fixture.library.store.client(), &fixture.library.executor)
            .load(other)
            .await?
            .ok_or("missing unrelated saved source")?;
    let opened = fixture.open_target(initial).await?;
    let started = fixture
        .target_action(DesktopGameCommandsEnumAction::Start)
        .await?;
    fixture.poll().await?;
    let quit = fixture
        .target_action(DesktopGameCommandsEnumAction::Quit)
        .await?;
    gate.entered.notified().await;
    fixture.poll().await?;
    let stored = fixture.stored_target().await?.content_revision();
    let lease = fixture.library.budget.lease(1).await?;
    let application = fixture.library.receipt()?;
    let mut opening =
        fixture
            .games
            .try_open_recovered_editor(recovered, other_stored, application.clone())?;
    fixture.poll().await?;
    let constructing = opening.try_result().is_none()
        && opening.editor_receipt().is_none()
        && fixture.library.factories.load(Ordering::SeqCst) == 2;
    let mut close = fixture.target_close(stored)?;
    let operation = close.operation_id();
    fixture.poll().await?;
    gate.release.add_permits(1);
    gate.finished.notified().await;
    fixture.poll().await?;
    let shutdowns_while_constructing = fixture.library.shutdowns.load(Ordering::SeqCst);
    let still_constructing = opening.try_result().is_none();
    let early = {
        let mut waiting = Box::pin(close.wait());
        poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context))).await
    };
    let certified_before_constructor = matches!(&early, Poll::Ready(Ok(_)));
    let target_blocked = fixture.target_is_blocked(stored)?;
    drop(lease);
    let result = fixture.drive(opening.wait()).await?;
    let observation =
        DesktopGameRegistryClosingReviewOpenObservation::capture(&opening, &application, result);
    let outcome = match early {
        Poll::Ready(outcome) => outcome,
        Poll::Pending => fixture.drive(close.wait()).await?,
    };
    let certificate = match outcome {
        Ok(certificate) => certificate,
        Err(_) => {
            fixture.games.try_retry_close(&close)?;
            fixture.drive(close.wait()).await??
        }
    };
    let proof = match certificate.proof() {
        DesktopGameRegistryCloseEnumProof::Closed {
            acknowledgment,
            snapshot,
        } => Some((*acknowledgment, snapshot.revision())),
        DesktopGameRegistryCloseEnumProof::NotOpen { .. } => None,
    };
    fixture.release(&mut close, certificate).await?;
    drop((opened, opening, application, gate));
    Box::pin(fixture.close()).await?;
    assert!(constructing);
    assert_eq!(started.phase(), GameEnumPhase::Active);
    assert_eq!(quit.phase(), GameEnumPhase::Interrupted);
    assert!(still_constructing);
    assert_eq!(shutdowns_while_constructing, 1);
    assert!(target_blocked);
    assert!(
        certified_before_constructor,
        "target close waited for an unrelated saved constructor despite completed target cleanup"
    );
    observation.assert_opened(other, other_stored);
    let (acknowledgment, snapshot_revision) =
        proof.ok_or("target close did not retain its actual shutdown acknowledgment")?;
    assert_eq!(acknowledgment.game(), quit.id());
    assert_eq!(acknowledgment.game_revision(), quit.revision());
    assert_eq!(acknowledgment.game_revision(), snapshot_revision);
    assert_eq!(acknowledgment.storage_revision(), stored);
    assert_eq!(close.operation_id(), operation);
    Ok(())
}
