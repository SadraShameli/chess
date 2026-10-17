#[path = "closing/fixture.rs"]
mod fixture;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::Instant;

use crate::clock::duration::ClockInstant;
use crate::desktop::game_registry::{
    DesktopGameRegistryCloseEnumObservation, DesktopGameRegistryCloseEnumProof,
    DesktopGameRegistryError, DesktopGameRegistryPort, DesktopGameRegistryRestored,
};
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestProof, DesktopLibraryServiceTestResult,
};
use crate::game::enums::{GameEnumEngineReadiness, GameEnumPhase};
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_repository::StorageLibraryRepository;

use self::fixture::{DesktopGameRegistryClosingFixture, DesktopGameRegistryClosingGate};

#[tokio::test]
async fn close_retains_real_ack_and_blocks_open_until_confirmed_cancel()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let stored = fixture.seed(100, false).await?;
    let opened = fixture.open(100, stored).await?;
    let game = opened.context().game();
    let target = fixture.games.try_close_target(
        game,
        fixture.games.snapshots.borrow()[0].revision(),
        opened.context().storage_revision(),
    )?;
    let mut receipt = fixture.games.try_close_game(target)?;
    let certificate = DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut receipt).await?;
    let DesktopGameRegistryCloseEnumProof::Closed {
        acknowledgment,
        snapshot,
    } = certificate.proof()
    else {
        return Err("installed source did not produce an actual close acknowledgment".into());
    };
    let acknowledgment = *acknowledgment;
    let snapshot_revision = snapshot.revision();
    let saved = !snapshot.is_save_required();
    let removed = fixture.games.sessions.read().await.get(&game).is_none();
    let blocked = matches!(
        fixture.games.try_open_saved_editor(
            game,
            acknowledgment.storage_revision(),
            fixture.receipt()?
        ),
        Err(DesktopGameRegistryError::Busy)
    );
    let recovered = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(game)
        .await?
        .ok_or("missing final source")?;
    let recovered_revision = recovered.content_revision();
    drop((opened, recovered));
    fixture.games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
    )?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut receipt).await?;
    let snapshot_removed = !fixture
        .games
        .snapshots
        .borrow()
        .iter()
        .any(|snapshot| snapshot.id() == game);
    drop(certificate);
    let reopened = fixture.open(100, recovered_revision).await?;
    let paused = matches!(
        reopened.state(),
        crate::desktop::game_editor::DesktopGameEditorSnapshotEnumState::Played {
            phase: GameEnumPhase::Setup | GameEnumPhase::Interrupted,
            ..
        }
    );
    drop(reopened);
    fixture.close().await?;
    assert_eq!(acknowledgment.game(), game);
    assert_eq!(acknowledgment.game_revision(), snapshot_revision);
    assert_eq!(recovered_revision, acknowledgment.storage_revision());
    assert!(saved);
    assert!(removed);
    assert!(snapshot_removed);
    assert!(blocked);
    assert!(paused);
    Ok(())
}

#[tokio::test]
async fn held_close_certificate_refuses_restore_before_replacement_factory_admission()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let stored = fixture.seed(100, false).await?;
    let opened = fixture.open(100, stored).await?;
    let game = opened.context().game();
    let target = fixture.games.try_close_target(
        game,
        fixture.games.snapshots.borrow()[0].revision(),
        stored,
    )?;
    let mut close = fixture.games.try_close_game(target)?;
    let certificate = DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut close).await?;
    let body_before = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    let recovered = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(game)
        .await?
        .ok_or("missing certified source")?;
    let (record, memory) = recovered.into_retained_record();
    let crate::storage::library_record::StorageLibraryEnumContent::Played(session) =
        record.into_content()
    else {
        return Err("certified source changed content kind".into());
    };
    let factories_before = fixture.factories.load(Ordering::SeqCst);
    let result = fixture
        .registry
        .restore(DesktopGameRegistryRestored {
            session: *session,
            storage_revision: certificate.storage_revision(),
        })
        .await;
    let refused = matches!(result, Err(DesktopGameRegistryError::Busy));
    let factories_after = fixture.factories.load(Ordering::SeqCst);
    let absent = !fixture.games.sessions.read().await.contains_key(&game);
    let body_after = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    drop(memory);
    fixture.games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
    )?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut close).await?;
    drop((certificate, opened));
    fixture.close().await?;
    assert!(refused);
    assert!(absent);
    assert_eq!(factories_before, factories_after);
    assert_eq!(body_before, body_after);
    Ok(())
}

#[tokio::test]
async fn save_failure_retains_owner_and_explicit_retry_uses_original_close_receipt()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let mut session = DesktopLibraryServiceFixture::played(100)?;
    session.start(
        session.revision(),
        ClockInstant::from_nanos(50),
        GameEnumEngineReadiness::NotRequired,
        &DesktopLibraryServiceTestProof,
    )?;
    let stored = fixture.seed_played(&session).await?;
    let revision = session.revision();
    let game = session.id();
    fixture
        .registry
        .restore(DesktopGameRegistryRestored {
            session,
            storage_revision: stored,
        })
        .await?;
    fixture.sql("CREATE TRIGGER reject_registry_close BEFORE UPDATE ON documents BEGIN SELECT RAISE(ABORT, 'injected close save failure'); END;").await?;
    let before = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    let target = fixture.games.try_close_target(game, revision, stored)?;
    let mut receipt = fixture.games.try_close_game(target)?;
    let id = receipt.operation_id();
    let failed = DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut receipt)
        .await
        .is_err();
    let owner = fixture
        .games
        .sessions
        .read()
        .await
        .get(&game)
        .ok_or("failed close removed owner")?
        .snapshot();
    let failed_body = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    fixture.sql("DROP TRIGGER reject_registry_close;").await?;
    fixture.games.try_retry_close(&receipt)?;
    let certificate = DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut receipt).await?;
    let proof_revision = certificate.storage_revision();
    let DesktopGameRegistryCloseEnumProof::Closed {
        acknowledgment,
        snapshot,
    } = certificate.proof()
    else {
        return Err("retry fabricated a not-open certificate".into());
    };
    let final_game_revision = acknowledgment.game_revision();
    let final_snapshot_revision = snapshot.revision();
    let final_snapshot_phase = snapshot.phase();
    let final_body = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    let recovered = crate::storage::game_document::StorageGameDocument::restore(&final_body)?;
    let final_stored_revision = recovered.revision();
    fixture.games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
    )?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut receipt).await?;
    drop(certificate);
    let after_release = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    let reopened = fixture.open(100, proof_revision).await?;
    let reopened_paused = matches!(
        reopened.state(),
        crate::desktop::game_editor::DesktopGameEditorSnapshotEnumState::Played {
            phase: GameEnumPhase::Interrupted,
            ..
        }
    );
    drop(reopened);
    fixture.close().await?;
    assert!(failed);
    assert_eq!(before, failed_body);
    assert_eq!(owner.snapshot.phase(), GameEnumPhase::Interrupted);
    assert!(owner.snapshot.is_save_required());
    assert_eq!(receipt.operation_id(), id);
    assert_eq!(final_game_revision, final_snapshot_revision);
    assert_eq!(final_game_revision, final_stored_revision);
    assert_eq!(final_snapshot_phase, GameEnumPhase::Interrupted);
    assert_eq!(final_body, after_release);
    assert!(reopened_paused);
    Ok(())
}

#[tokio::test]
async fn repeated_close_joins_original_receipt_and_certificate_drop_keeps_admission_closed()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let stored = fixture.seed(100, false).await?;
    let opened = fixture.open(100, stored).await?;
    let game = opened.context().game();
    let revision = fixture.games.snapshots.borrow()[0].revision();
    let first = fixture.games.try_close_target(game, revision, stored)?;
    let second = fixture.games.try_close_target(game, revision, stored)?;
    let mut receipt = fixture.games.try_close_game(first)?;
    let mut repeated = fixture.games.try_close_game(second)?;
    let certificate = DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut receipt).await?;
    let final_body = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    let final_revision = certificate.storage_revision();
    drop(certificate);
    let reclaimed = repeated.wait().await?;
    let blocked = matches!(
        fixture
            .games
            .try_open_saved_editor(game, final_revision, fixture.receipt()?),
        Err(DesktopGameRegistryError::Busy)
    );
    let repeated_body = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    let same_id = receipt.operation_id() == repeated.operation_id();
    fixture.games.try_finish_close(
        &reclaimed,
        DesktopGameRegistryCloseEnumObservation::RollbackObserved,
    )?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut receipt).await?;
    drop((opened, reclaimed));
    let factories = fixture.factories.load(Ordering::SeqCst);
    fixture.close().await?;
    assert_eq!(factories, 1);
    assert!(same_id);
    assert!(blocked);
    assert_eq!(final_body, repeated_body);
    Ok(())
}

#[tokio::test]
async fn not_open_admission_is_bounded_and_never_claims_a_storage_ack()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let mut receipts = Vec::new();
    for index in 0..DesktopGameRegistryPort::CREATE_CAPACITY {
        let game = DesktopLibraryServiceFixture::id(100 + u128::try_from(index)?)?;
        let target = fixture
            .games
            .try_not_open_close_target(game, GameStorageRevision::new(17))?;
        receipts.push(fixture.games.try_close_game(target)?);
    }
    let extra = fixture.games.try_not_open_close_target(
        DesktopLibraryServiceFixture::id(999)?,
        GameStorageRevision::INITIAL,
    )?;
    let bounded = matches!(
        fixture.games.try_close_game(extra),
        Err(DesktopGameRegistryError::Busy)
    );
    let mut proofs = true;
    for receipt in &mut receipts {
        let certificate = DesktopGameRegistryClosingFixture::drive(&mut fixture, receipt).await?;
        proofs &= matches!(certificate.proof(), DesktopGameRegistryCloseEnumProof::NotOpen { expected_storage_revision, .. } if *expected_storage_revision == GameStorageRevision::new(17));
        fixture.games.try_finish_close(
            &certificate,
            DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
        )?;
        DesktopGameRegistryClosingFixture::released(&mut fixture, receipt).await?;
    }
    fixture.close().await?;
    assert!(bounded);
    assert!(proofs);
    Ok(())
}

#[tokio::test]
async fn source_constructor_failure_settles_before_not_open_barrier_certification()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let stored = fixture.seed(100, false).await?;
    let game = DesktopLibraryServiceFixture::id(100)?;
    let (mut registry, games) = DesktopGameRegistryClosingFixture::failing_registry(&fixture);
    let lease = fixture.budget.lease(1).await?;
    let mut opening = games.try_open_saved_editor(game, stored, fixture.receipt()?)?;
    let mut running = Box::pin(registry.run());
    std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(running.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(running);
    let target = games.try_not_open_close_target(game, stored)?;
    let mut close = games.try_close_game(target)?;
    drop(lease);
    let mut running = Box::pin(registry.run());
    let certificate = tokio::select! {
        result = close.wait() => result?,
        result = &mut running => return Err(format!("not-open close stopped registry: {result:?}").into()),
    };
    drop(running);
    let failed = opening.wait().await.is_err();
    let not_open = matches!(
        certificate.proof(),
        DesktopGameRegistryCloseEnumProof::NotOpen { .. }
    );
    games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
    )?;
    let mut running = Box::pin(registry.run());
    tokio::select! {
        result = close.wait_released() => result?,
        result = &mut running => return Err(format!("release stopped registry: {result:?}").into()),
    }
    drop((running, certificate, opening));
    registry
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    drop((registry, games));
    fixture.close().await?;
    assert!(failed);
    assert!(not_open);
    Ok(())
}

#[tokio::test]
async fn completed_uninstalled_source_gets_only_admission_proof_without_factory_or_write()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let mut session = DesktopLibraryServiceFixture::played(100)?;
    session.start(
        session.revision(),
        ClockInstant::from_nanos(50),
        GameEnumEngineReadiness::NotRequired,
        &DesktopLibraryServiceTestProof,
    )?;
    session.resign(
        crate::game::result::GameResultResignRequest {
            expected_revision: session.revision(),
            resigning: crate::board::enums::BoardEnumColor::White,
            received_at: ClockInstant::from_nanos(50),
            engine: crate::game::pause::GamePauseEnumEngineBarrier::NoEngine,
        },
        &DesktopLibraryServiceTestProof,
    )?;
    let stored = fixture.seed_played(&session).await?;
    let game = session.id();
    let before = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    let target = fixture.games.try_not_open_close_target(game, stored)?;
    let mut close = fixture.games.try_close_game(target)?;
    let certificate = DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut close).await?;
    let not_open = matches!(certificate.proof(), DesktopGameRegistryCloseEnumProof::NotOpen { expected_storage_revision, .. } if *expected_storage_revision == stored);
    let factories = fixture.factories.load(Ordering::SeqCst);
    let after = DesktopGameRegistryClosingFixture::document(&fixture, game).await?;
    fixture.games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
    )?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut close).await?;
    drop(certificate);
    fixture.close().await?;
    assert!(not_open);
    assert_eq!(factories, 0);
    assert_eq!(before, after);
    Ok(())
}

#[tokio::test]
async fn abandoned_client_keeps_owner_until_actual_factory_cleanup_is_observed()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let stored = fixture.seed(100, false).await?;
    let game = DesktopLibraryServiceFixture::id(100)?;
    let gate = DesktopGameRegistryClosingGate::new();
    let (mut registry, games) =
        DesktopGameRegistryClosingFixture::gated_registry(&fixture, Arc::clone(&gate));
    let mut opening = games.try_open_saved_editor(game, stored, fixture.receipt()?)?;
    let mut running = Box::pin(registry.run());
    let opened = tokio::select! {
        result = opening.wait() => result?,
        result = &mut running => return Err(format!("source opening stopped registry: {result:?}").into()),
    };
    drop(running);
    let memory_before_close = fixture.available_memory()?;
    let revision = games
        .sessions
        .read()
        .await
        .get(&game)
        .ok_or("missing installed source")?
        .snapshot()
        .snapshot
        .revision();
    let target = games.try_close_target(game, revision, stored)?;
    let receipt = games.try_close_game(target)?;
    let operation = receipt.operation_id();
    drop(receipt);
    let mut running = Box::pin(registry.run());
    tokio::select! {
        () = gate.entered.notified() => {}
        result = &mut running => return Err(format!("registry stopped during cleanup: {result:?}").into()),
    }
    drop(running);
    let held_owner = games.sessions.read().await.contains_key(&game);
    let held_source_memory = registry.saved_sources.contains_key(&game);
    let memory_during_cleanup = fixture.available_memory()?;
    let mut reconciled = games.try_reconcile_close(game, operation)?;
    let mut waiting = Box::pin(reconciled.wait());
    let pending = std::future::poll_fn(|context| {
        std::task::Poll::Ready(std::future::Future::poll(waiting.as_mut(), context).is_pending())
    })
    .await;
    drop(waiting);
    gate.release.add_permits(1);
    let mut running = Box::pin(registry.run());
    let certificate = tokio::select! {
        result = reconciled.wait() => result?,
        result = &mut running => return Err(format!("registry stopped after cleanup: {result:?}").into()),
    };
    drop(running);
    let removed = !games.sessions.read().await.contains_key(&game);
    let released_source_memory = !registry.saved_sources.contains_key(&game);
    let memory_after_cleanup = fixture.available_memory()?;
    games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
    )?;
    let mut running = Box::pin(registry.run());
    tokio::select! {
        result = reconciled.wait_released() => result?,
        result = &mut running => return Err(format!("registry stopped before cleanup release: {result:?}").into()),
    }
    drop((running, certificate, opened, opening));
    registry
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    drop((registry, games));
    fixture.close().await?;
    assert!(held_owner);
    assert!(held_source_memory);
    assert_eq!(memory_before_close, memory_during_cleanup);
    assert!(pending);
    assert!(removed);
    assert!(released_source_memory);
    assert!(memory_after_cleanup > memory_during_cleanup);
    Ok(())
}

#[tokio::test]
async fn abandoned_wait_reconciles_original_operation_without_second_factory()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let stored = fixture.seed(100, false).await?;
    let opened = fixture.open(100, stored).await?;
    let target = fixture.games.try_close_target(
        opened.context().game(),
        fixture.games.snapshots.borrow()[0].revision(),
        stored,
    )?;
    let receipt = fixture.games.try_close_game(target)?;
    let operation = receipt.operation_id();
    let lease = fixture.budget.lease(1).await?;
    let mut abandoned = receipt.clone();
    let mut waiting = Box::pin(abandoned.wait());
    std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(waiting.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(waiting);
    drop(receipt);
    DesktopGameRegistryClosingFixture::poll(&mut fixture).await;
    drop(lease);
    let mut reconciled = fixture
        .games
        .try_reconcile_close(opened.context().game(), operation)?;
    let certificate =
        DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut reconciled).await?;
    let factories = fixture.factories.load(Ordering::SeqCst);
    fixture.games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::RollbackObserved,
    )?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut reconciled).await?;
    drop((certificate, opened));
    fixture.close().await?;
    assert_eq!(reconciled.operation_id(), operation);
    assert_eq!(factories, 1);
    Ok(())
}

#[tokio::test]
async fn stale_revision_and_replaced_owner_do_not_close_the_current_source()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let stored = fixture.seed(100, false).await?;
    let opened = fixture.open(100, stored).await?;
    let game = opened.context().game();
    let revision = fixture.games.snapshots.borrow()[0].revision();
    let storage_conflict = fixture
        .games
        .try_close_target(game, revision, GameStorageRevision::new(999))
        .is_err();
    let revision_conflict = fixture
        .games
        .try_close_target(game, revision.next()?, stored)
        .is_err();
    let stale = fixture.games.try_close_target(game, revision, stored)?;
    let target = fixture.games.try_close_target(game, revision, stored)?;
    let mut first = fixture.games.try_close_game(target)?;
    let certificate = DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut first).await?;
    let storage = certificate.storage_revision();
    fixture.games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
    )?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut first).await?;
    drop((certificate, opened));
    let reopened = fixture.open(100, storage).await?;
    let mut rejected = fixture.games.try_close_game(stale)?;
    let mut running = Box::pin(fixture.registry.run());
    let error = tokio::select! {
        result = rejected.wait() => result.err().ok_or("stale owner was closed")?,
        result = &mut running => return Err(format!("registry stopped: {result:?}").into()),
    };
    drop(running);
    fixture.games.try_cancel_rejected_close(&rejected)?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut rejected).await?;
    drop(reopened);
    fixture.close().await?;
    assert!(storage_conflict);
    assert!(revision_conflict);
    assert!(matches!(
        error.as_ref(),
        DesktopGameRegistryError::CloseConflict
    ));
    Ok(())
}

#[tokio::test]
async fn target_close_preserves_unrelated_active_owner() -> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let mut other = DesktopLibraryServiceFixture::played(200)?;
    other.start(
        other.revision(),
        ClockInstant::from_nanos(50),
        GameEnumEngineReadiness::NotRequired,
        &DesktopLibraryServiceTestProof,
    )?;
    let other_revision = other.revision();
    fixture
        .registry
        .restore(DesktopGameRegistryRestored {
            session: other,
            storage_revision: GameStorageRevision::INITIAL,
        })
        .await?;
    let stored = fixture.seed(100, false).await?;
    let opened = fixture.open(100, stored).await?;
    let game = opened.context().game();
    let revision = fixture
        .games
        .sessions
        .read()
        .await
        .get(&game)
        .ok_or("missing target")?
        .snapshot()
        .snapshot
        .revision();
    let target = fixture.games.try_close_target(game, revision, stored)?;
    let mut receipt = fixture.games.try_close_game(target)?;
    let certificate = DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut receipt).await?;
    let other = fixture
        .games
        .sessions
        .read()
        .await
        .get(&DesktopLibraryServiceFixture::id(200)?)
        .ok_or("unrelated owner removed")?
        .snapshot();
    let other_clock_running = other.snapshot.clock().is_running();
    let other_elapsed = other
        .snapshot
        .clock()
        .view(ClockInstant::from_nanos(100))?
        .elapsed();
    fixture.games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
    )?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut receipt).await?;
    drop((certificate, opened));
    fixture.close().await?;
    assert_eq!(other.snapshot.phase(), GameEnumPhase::Active);
    assert_eq!(other.snapshot.revision(), other_revision);
    assert!(other_clock_running);
    assert_eq!(other_elapsed.as_nanos(), 50);
    Ok(())
}

#[tokio::test]
async fn certificate_covers_actual_trash_commit_and_old_preview_cannot_supply_evidence()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let stored = fixture.seed(100, false).await?;
    let opened = fixture.open(100, stored).await?;
    let game = opened.context().game();
    let old_plan = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .preview_destruction(crate::storage::trash::StorageTrashPreviewRequest::MoveToTrash(game))
        .await?;
    let target = fixture.games.try_close_target(
        game,
        fixture.games.snapshots.borrow()[0].revision(),
        stored,
    )?;
    let mut close = fixture.games.try_close_game(target)?;
    let certificate = DesktopGameRegistryClosingFixture::drive(&mut fixture, &mut close).await?;
    let repository = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor);
    let timestamp = crate::game::library_organization::GameLibraryOrganizationTimestamp::new(2000)?;
    let stale_preview =
        crate::desktop::library_commands::DesktopLibraryCommandsConfirmation::confirm(old_plan)
            .prepare(&certificate, timestamp)
            .is_err();
    let plan = repository
        .preview_destruction(crate::storage::trash::StorageTrashPreviewRequest::MoveToTrash(game))
        .await?;
    let request =
        crate::desktop::library_commands::DesktopLibraryCommandsConfirmation::confirm(plan)
            .prepare(&certificate, timestamp)?;
    let mut mutation = repository.begin_destruction(request)?;
    let before_ack_blocked = matches!(
        fixture.games.try_open_saved_editor(
            game,
            certificate.storage_revision(),
            fixture.receipt()?
        ),
        Err(DesktopGameRegistryError::Busy)
    );
    let crate::storage::library_repository::StorageLibraryEnumMutationReceipt::DestructionApplied(
        acknowledgment,
    ) = mutation.wait().await?
    else {
        return Err("missing actual Trash commit acknowledgment".into());
    };
    let after_ack_blocked = matches!(
        fixture.games.try_open_saved_editor(
            game,
            certificate.storage_revision(),
            fixture.receipt()?
        ),
        Err(DesktopGameRegistryError::Busy)
    );
    let location = repository
        .load(game)
        .await?
        .ok_or("missing trashed source")?
        .location();
    fixture.games.try_finish_close(
        &certificate,
        DesktopGameRegistryCloseEnumObservation::TrashCommitted(&acknowledgment),
    )?;
    DesktopGameRegistryClosingFixture::released(&mut fixture, &mut close).await?;
    drop((certificate, opened, mutation));
    fixture.close().await?;
    assert!(stale_preview);
    assert!(before_ack_blocked);
    assert!(after_ack_blocked);
    assert_eq!(
        location,
        crate::storage::trash::StorageTrashEnumLocation::Trash
    );
    Ok(())
}
