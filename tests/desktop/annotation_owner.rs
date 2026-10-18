use crate::storage_sqlite_inspector::StorageSqliteInspector;
use std::future::{Future, poll_fn};
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::task::Poll;

use crate::board::start::BoardStart;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumAdmission, DesktopAnnotationCommandsEnumPending,
    DesktopAnnotationCommandsEnumResult, DesktopAnnotationCommandsEnumScope,
};
use crate::desktop::annotation_owner::{
    DesktopAnnotationOwner, DesktopAnnotationOwnerEnumSaveProgress, DesktopAnnotationOwnerError,
    DesktopAnnotationOwnerResources,
};
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationReceipt, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorEnumSource,
    DesktopGameEditorEpoch, DesktopGameEditorPublicationError, DesktopGameEditorSnapshotEnumState,
};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::game::annotations::{GameAnnotationsEnumChange, GameAnnotationsEnumVariationRequest};
use crate::game::enums::{GameEnumMarkingColor, GameId, GameMoveId, GameRevision};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::move_node::{GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag};
use crate::game::session::{
    GameMetadata, GameMetadataExtension, GameMetadataKnownTags, GameMetadataOpeningCode,
    GameMetadataText, GameSeats, GameSession, GameSessionOptions,
};
use crate::game::storage_repository::{
    GameStorageRevision, GameStorageWallTimeError, GameStorageWallTimeProvider,
};
use crate::game::tree::GameTree;
use crate::notation::game::{NotationGame, NotationGameParts};
use crate::notation::game_identity::NotationGameIdentityEnumResult;
use crate::notation::pgn_import::NotationPgnImport;
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::library_notation::StorageLibraryNotationRepository;
use crate::storage::library_prepared::StorageLibraryEnumContentChange;
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};
use crate::storage::library_repository::{
    StorageLibraryEnumMutationReceipt, StorageLibraryError, StorageLibraryRepository,
};

type DesktopAnnotationOwnerTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn pending_owner_moves_to_a_retained_task_and_resumes_after_capacity_returns()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let receipt = fixture.receipt()?;
    let lease = fixture.budget.try_lease(1)?;
    let mut owner = DesktopAnnotationOwner::open(
        DesktopAnnotationOwnerFixture::record(100)?,
        GameStorageRevision::INITIAL,
        receipt.clone(),
        fixture.resources(),
    )?;
    let mut waiting = Box::pin(owner.observe_compute(&receipt));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    let task = tokio::spawn(async move {
        let result = owner.observe_compute(&receipt).await;
        (owner, result)
    });
    drop(lease);
    let (owner, result) = task.await?;
    let DesktopAnnotationCommandsEnumResult::Snapshot(snapshot) = result? else {
        return Err("missing retained open snapshot".into());
    };
    assert_eq!(
        snapshot.context().game(),
        DesktopAnnotationOwnerFixture::id(100)?
    );
    assert_eq!(snapshot.annotations().notes(), "original");
    assert!(!owner.status().has_compute());
    drop((snapshot, owner));
    fixture.close().await
}

#[tokio::test]
async fn save_completion_returns_actual_sql_acknowledgment_for_its_covered_revision()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, "first accepted edit").await?;
    let covered = owner
        .status()
        .accepted_edit()
        .ok_or("missing accepted edit")?
        .revision();
    let previous = owner.snapshot()?.context().storage_revision();
    let application_receipt = fixture.checkpoint(&mut owner, false)?;
    assert!(matches!(
        owner.observe_save(&application_receipt).await?,
        DesktopAnnotationOwnerEnumSaveProgress::Committing
    ));
    fixture.wall.value.store(2000, Ordering::SeqCst);
    fixture.notes(&mut owner, "newer accepted edit").await?;
    let DesktopAnnotationOwnerEnumSaveProgress::Saved {
        receipt,
        covered: saved_covered,
    } = owner.observe_save(&application_receipt).await?
    else {
        return Err("missing actual save acknowledgment".into());
    };
    assert_eq!(saved_covered, covered);
    assert_eq!(receipt.id(), owner.snapshot()?.context().game());
    assert_eq!(receipt.content_revision().value(), previous.value() + 1);
    assert_eq!(
        owner.snapshot()?.annotations().notes(),
        "newer accepted edit"
    );
    assert!(owner.status().is_dirty());
    let stored = fixture
        .store
        .client()
        .read(receipt.id().as_uuid())
        .await?
        .ok_or("missing committed document")?;
    assert_eq!(stored.revision(), receipt.content_revision().value());
    let page = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .query(crate::storage::library_query::StorageLibraryQuery::default())
        .await?;
    assert_eq!(page.catalog_revision(), receipt.catalog_revision());
    drop(page);
    drop((stored, receipt, owner));
    fixture.close().await
}

#[tokio::test]
async fn published_snapshot_clone_retains_receiving_bytes_until_its_final_drop()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, &"x".repeat(256 * 1024)).await?;
    fixture.save(&mut owner, false).await?;
    assert!(!owner.status().is_dirty());
    let result = fixture
        .execute(&mut owner, DesktopAnnotationCommandsEnumAction::Snapshot)
        .await?;
    let retained = result.clone();
    drop((result, owner));
    let client = fixture.store.client();
    let probe = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET);
    let charged = matches!(&probe, Err(StorageDocumentError::Saturated));
    drop(probe);
    drop(retained);
    let released = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET);
    let all_released = released.is_ok();
    drop(released);
    fixture.close().await?;
    assert!(all_released);
    assert!(
        charged,
        "a published snapshot clone must retain its receiving byte reservation"
    );
    Ok(())
}

#[tokio::test]
async fn result_and_observation_clones_share_the_charged_projection_under_pressure()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let notes = "x".repeat(256 * 1024);
    fixture.notes(&mut owner, &notes).await?;
    let cached = owner.snapshot()?.clone();
    let memory = fixture.fill_remaining_memory()?;
    let result = fixture
        .execute(&mut owner, DesktopAnnotationCommandsEnumAction::Snapshot)
        .await?;
    let retained = result.clone();
    let DesktopAnnotationCommandsEnumResult::Snapshot(snapshot) = &retained else {
        return Err("wrong snapshot result".into());
    };
    assert!(std::ptr::eq(cached.annotations(), snapshot.annotations()));
    assert!(std::ptr::eq(cached.metadata(), snapshot.metadata()));
    assert!(std::ptr::eq(cached.board(), snapshot.board()));
    drop((memory, result, owner, cached));
    let probe = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - notes.len() + 1);
    assert!(matches!(probe, Err(StorageDocumentError::Saturated)));
    drop((probe, retained));
    let released = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(released);
    fixture.close().await
}

#[tokio::test]
async fn refused_publication_retains_accepted_edit_and_receipt_until_cancelled_retry_completes()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let previous = owner.snapshot()?.context();
    let receipt = fixture.receipt()?;
    let foreign = fixture.receipt()?;
    let memory = fixture.fill_remaining_memory()?;
    owner.submit(DesktopAnnotationCommands::new(
        receipt.clone(),
        previous,
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "accepted under pressure".to_owned(),
            nags: vec![GameMoveNodeNag::new(7)],
        },
    ))?;
    assert!(owner.observe_compute(&receipt).await.is_err());
    let edit = owner.status().accepted_edit().ok_or("lost accepted edit")?;
    assert_eq!(edit.revision(), GameRevision::ZERO.next()?);
    assert_eq!(
        edit.timestamp(),
        GameLibraryOrganizationTimestamp::new(1000)?
    );
    assert!(owner.status().has_compute());
    assert!(owner.status().is_dirty());
    assert!(matches!(
        owner.status().failure().map(|error| error.as_ref()),
        Some(DesktopAnnotationOwnerError::Publication(
            DesktopGameEditorPublicationError::Memory(StorageDocumentError::Saturated)
        ))
    ));
    assert!(matches!(
        owner.snapshot(),
        Err(DesktopAnnotationOwnerError::PublicationPending { .. })
    ));
    assert!(matches!(
        owner.read_snapshot()?.pending(),
        Some(DesktopAnnotationCommandsEnumPending::Publishing { context })
            if context.source() == DesktopGameEditorEnumSource::Notation {
                revision: edit.revision()
            }
    ));
    assert!(matches!(
        owner.submit(DesktopAnnotationCommands::new(
            foreign.clone(),
            previous,
            DesktopAnnotationCommandsEnumAction::Snapshot
        )),
        Err(DesktopAnnotationOwnerError::PublicationPending { .. })
    ));
    assert!(matches!(
        owner.observe_compute(&foreign).await,
        Err(DesktopAnnotationOwnerError::ForeignReceipt)
    ));
    assert!(owner.observe_compute(&receipt).await.is_err());
    assert_eq!(owner.status().accepted_edit(), Some(edit));
    let calls = fixture.wall.calls.load(Ordering::SeqCst);
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    drop(memory);
    let lease = fixture.budget.try_lease(1)?;
    let mut waiting = Box::pin(owner.observe_compute(&receipt));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert!(owner.status().has_compute());
    let port = fixture.application.port();
    let quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    port.try_submit(&quit)?;
    assert!(receipt.is_cancellation_requested());
    drop(lease);
    let result = owner.observe_compute(&receipt).await?;
    let DesktopAnnotationCommandsEnumResult::Edited { change, snapshot } = result else {
        return Err("lost accepted completion".into());
    };
    assert_eq!(change, GameAnnotationsEnumChange::Changed);
    assert_eq!(snapshot.annotations().notes(), "accepted under pressure");
    assert_eq!(snapshot.annotations().nags(), &[GameMoveNodeNag::new(7)]);
    assert_eq!(owner.status().accepted_edit(), Some(edit));
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), calls);
    assert!(!owner.status().has_compute());
    assert!(owner.read_snapshot()?.pending().is_none());
    assert_ne!(owner.snapshot()?.context(), previous);
    drop((owner, snapshot));
    fixture.close().await
}

#[tokio::test]
async fn initial_publication_refusal_is_retryable_without_losing_the_record()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let memory = fixture.fill_remaining_memory()?;
    let receipt = fixture.receipt()?;
    let mut owner = DesktopAnnotationOwner::open(
        DesktopAnnotationOwnerFixture::record(100)?,
        GameStorageRevision::INITIAL,
        receipt.clone(),
        fixture.resources(),
    )?;
    assert!(owner.observe_compute(&receipt).await.is_err());
    assert!(owner.status().has_compute());
    assert!(owner.status().accepted_edit().is_none());
    drop(memory);
    owner.observe_compute(&receipt).await?;
    assert_eq!(owner.snapshot()?.annotations().notes(), "original");
    assert!(!owner.status().has_compute());
    drop(owner);
    let released = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(released);
    fixture.close().await
}

#[tokio::test]
async fn dropping_a_refused_publication_releases_both_cached_observations()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let memory = fixture.fill_remaining_memory()?;
    let receipt = fixture.receipt()?;
    owner.submit(DesktopAnnotationCommands::new(
        receipt.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "pending at final drop".to_owned(),
            nags: Vec::new(),
        },
    ))?;
    assert!(owner.observe_compute(&receipt).await.is_err());
    assert!(owner.status().has_compute());
    drop((memory, owner));
    let released = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(released);
    fixture.close().await
}

#[tokio::test]
async fn unchanged_annotations_reuse_the_existing_publication_when_no_bytes_remain()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, "unchanged").await?;
    let cached = owner.snapshot()?.clone();
    let edit = owner.status().accepted_edit();
    let memory = fixture.fill_remaining_memory()?;
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "unchanged".to_owned(),
                nags: vec![GameMoveNodeNag::new(1)],
            },
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Edited { change, snapshot } = result else {
        return Err("wrong no-op completion".into());
    };
    assert_eq!(change, GameAnnotationsEnumChange::Unchanged);
    assert_eq!(owner.status().accepted_edit(), edit);
    assert_eq!(snapshot.context(), cached.context());
    assert!(std::ptr::eq(snapshot.annotations(), cached.annotations()));
    drop((memory, owner, cached, snapshot));
    fixture.close().await
}

#[tokio::test]
async fn unchanged_notes_and_markings_do_not_require_an_available_wall_clock()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, "already saved").await?;
    let marking = GameMoveNodeMarking::circle(GameEnumMarkingColor::Green, "e4".parse()?);
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::SaveMarkings {
                markings: vec![marking],
            },
        )
        .await?;
    fixture.save(&mut owner, false).await?;
    let context = owner.snapshot()?.context();
    let calls = fixture.wall.calls.load(Ordering::SeqCst);
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    let mut results = Vec::new();
    for action in [
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "already saved".to_owned(),
            nags: vec![GameMoveNodeNag::new(1)],
        },
        DesktopAnnotationCommandsEnumAction::SaveMarkings {
            markings: vec![marking],
        },
    ] {
        results.push(
            fixture
                .execute(&mut owner, action)
                .await
                .and_then(|result| match result {
                    DesktopAnnotationCommandsEnumResult::Edited { change, snapshot } => {
                        Ok((change, snapshot.context()))
                    }
                    DesktopAnnotationCommandsEnumResult::Snapshot(_)
                    | DesktopAnnotationCommandsEnumResult::Tree(_)
                    | DesktopAnnotationCommandsEnumResult::Children(_)
                    | DesktopAnnotationCommandsEnumResult::Variation(_)
                    | DesktopAnnotationCommandsEnumResult::Scope(_)
                    | DesktopAnnotationCommandsEnumResult::PendingProjection { .. } => {
                        Err("unchanged annotation returned another result kind".into())
                    }
                }),
        );
    }
    let observed_calls = fixture.wall.calls.load(Ordering::SeqCst);
    let dirty = owner.status().is_dirty();
    let after = owner.snapshot()?.context();
    drop(owner);
    fixture.close().await?;
    assert!(!dirty);
    assert_eq!(after, context);
    for result in results {
        assert_eq!(result?, (GameAnnotationsEnumChange::Unchanged, context));
    }
    assert_eq!(observed_calls, calls);
    Ok(())
}

#[tokio::test]
async fn committed_projection_byte_refusal_retains_the_real_receipt_for_retry()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture
        .notes(&mut owner, "committed under pressure")
        .await?;
    let previous = owner.snapshot()?.context();
    let edit = owner.status().accepted_edit();
    let receipt = fixture.checkpoint(&mut owner, false)?;
    owner.observe_save(&receipt).await?;
    let document = fixture
        .store
        .client()
        .read(previous.game().as_uuid())
        .await?
        .ok_or("missing committed document")?;
    assert_eq!(document.revision(), previous.storage_revision().value() + 1);
    drop(document);
    let memory = fixture.fill_remaining_memory()?;
    assert!(owner.observe_save(&receipt).await.is_err());
    assert!(owner.status().has_save());
    assert!(owner.status().is_dirty());
    assert_eq!(owner.status().accepted_edit(), edit);
    assert!(matches!(
        owner.snapshot(),
        Err(DesktopAnnotationOwnerError::ProjectionPending { .. })
    ));
    assert!(matches!(
        owner.read_snapshot()?.pending(),
        Some(DesktopAnnotationCommandsEnumPending::Projecting { .. })
    ));
    assert!(matches!(
        owner.observe_save(&fixture.receipt()?).await,
        Err(DesktopAnnotationOwnerError::ForeignReceipt)
    ));
    drop(memory);
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    assert!(matches!(
        owner.observe_save(&receipt).await?,
        DesktopAnnotationOwnerEnumSaveProgress::Saved { .. }
    ));
    assert_eq!(
        owner.snapshot()?.context().storage_revision().value(),
        previous.storage_revision().value() + 1
    );
    assert_eq!(owner.status().accepted_edit(), edit);
    assert!(!owner.status().is_dirty());
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn publication_retry_preserves_newer_utc_and_shared_result_during_older_durable_projection()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, "older durable edit").await?;
    let older = owner.status().accepted_edit().ok_or("missing older edit")?;
    let save = fixture.checkpoint(&mut owner, false)?;
    owner.observe_save(&save).await?;
    fixture.wall.value.store(2000, Ordering::SeqCst);
    let notes = "x".repeat(256 * 1024);
    let edit = fixture.receipt()?;
    owner.submit(DesktopAnnotationCommands::new(
        edit.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: notes.clone(),
            nags: Vec::new(),
        },
    ))?;
    assert!(matches!(
        owner.observe_save(&save).await,
        Err(DesktopAnnotationOwnerError::ComputePending)
    ));
    let memory = fixture.fill_remaining_memory()?;
    assert!(owner.observe_compute(&edit).await.is_err());
    let accepted = owner.status().accepted_edit().ok_or("lost newer edit")?;
    assert_eq!(accepted.revision(), older.revision().next()?);
    assert_eq!(
        accepted.timestamp(),
        GameLibraryOrganizationTimestamp::new(2000)?
    );
    assert!(matches!(
        owner.read_snapshot()?.pending(),
        Some(DesktopAnnotationCommandsEnumPending::Publishing { .. })
    ));
    assert!(matches!(
        owner.observe_save(&save).await,
        Err(DesktopAnnotationOwnerError::ComputePending)
    ));
    drop(memory);
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    let result = owner.observe_compute(&edit).await?;
    let DesktopAnnotationCommandsEnumResult::PendingProjection {
        result: completion,
        covered,
        storage_revision,
    } = &result
    else {
        return Err("publication retry was falsely reported ready".into());
    };
    assert_eq!(*covered, older.revision());
    assert_eq!(*storage_revision, GameStorageRevision::new(2));
    let DesktopAnnotationCommandsEnumResult::Edited { change, snapshot } = completion.as_ref()
    else {
        return Err("lost newer edit completion".into());
    };
    assert_eq!(*change, GameAnnotationsEnumChange::Changed);
    assert_eq!(snapshot.annotations().notes(), notes);
    let retained = result.clone();
    assert!(matches!(
        owner.snapshot(),
        Err(DesktopAnnotationOwnerError::ProjectionPending { .. })
    ));
    owner.observe_save(&save).await?;
    assert_eq!(owner.status().durable_revision(), older.revision());
    assert_eq!(owner.status().accepted_edit(), Some(accepted));
    assert!(owner.status().is_dirty());
    assert_eq!(owner.snapshot()?.annotations().notes(), notes);
    assert_eq!(
        owner.snapshot()?.context().storage_revision(),
        GameStorageRevision::new(2)
    );
    drop((owner, result));
    let charged = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - notes.len() + 1);
    assert!(matches!(charged, Err(StorageDocumentError::Saturated)));
    drop((charged, retained));
    let released = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(released);
    fixture.close().await
}

#[tokio::test]
async fn published_observation_charges_only_its_receiving_client()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let other = DesktopAnnotationOwnerFixture::new().await?;
    let owner = fixture.open(100).await?;
    let observation = owner.snapshot()?.clone();
    drop(owner);
    let foreign = other
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    let charged = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET);
    assert!(matches!(charged, Err(StorageDocumentError::Saturated)));
    drop((foreign, charged, observation));
    let released = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(released);
    fixture.close().await?;
    other.close().await
}

#[tokio::test]
async fn pending_snapshot_admission_clones_retain_the_shared_charged_cache()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let cached = owner.snapshot()?.clone();
    let save = fixture.checkpoint(&mut owner, false)?;
    let memory = fixture.fill_remaining_memory()?;
    let admission = owner.submit(DesktopAnnotationCommands::new(
        fixture.receipt()?,
        cached.context(),
        DesktopAnnotationCommandsEnumAction::Snapshot,
    ))?;
    let retained = admission.clone();
    let DesktopAnnotationCommandsEnumAdmission::PendingSnapshot { snapshot, pending } = &retained
    else {
        return Err("missing pending snapshot admission".into());
    };
    assert_eq!(
        *pending,
        DesktopAnnotationCommandsEnumPending::Preparing { computing: false }
    );
    assert!(std::ptr::eq(snapshot.annotations(), cached.annotations()));
    drop((memory, save, admission, owner, cached));
    let charged = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET);
    assert!(matches!(charged, Err(StorageDocumentError::Saturated)));
    drop((charged, retained));
    let released = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(released);
    fixture.close().await
}

#[tokio::test]
async fn shared_publisher_charges_all_owned_metadata_annotation_and_time_control_bytes()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let root = DesktopAnnotationOwnerFixture::node(1)?;
    let text = GameMetadataText::new("m".repeat(256 * 1024))?;
    let known = GameMetadataKnownTags {
        event: Some(text.clone()),
        site: Some(text.clone()),
        round: Some(text.clone()),
        white: Some(text.clone()),
        black: Some(text.clone()),
        opening: Some(text.clone()),
        variation: Some(text.clone()),
        annotator: Some(text.clone()),
        eco: Some(GameMetadataOpeningCode::new("A00".to_owned())?),
        ..GameMetadataKnownTags::default()
    };
    let metadata = GameMetadata::new(
        known,
        vec![GameMetadataExtension::new(
            "Source".to_owned(),
            text.clone(),
        )?],
    )?;
    let notes = "n".repeat(256 * 1024);
    let nags = vec![GameMoveNodeNag::new(1); 1024];
    let markings =
        vec![GameMoveNodeMarking::circle(GameEnumMarkingColor::Red, "a1".parse()?); 1024];
    let owned_bytes = notes.len()
        + std::mem::size_of_val(nags.as_slice())
        + std::mem::size_of_val(markings.as_slice())
        + 10 * text.as_str().len();
    let start = BoardStart::standard();
    let mut tree = GameTree::new(root, start.position().clone(), None);
    tree.set_annotations(root, GameMoveNodeAnnotations::new(notes, nags, markings)?)?;
    let notation = NotationGame::new(NotationGameParts {
        start,
        tree,
        mainline_tip: root,
        metadata,
        result: NotationGameIdentityEnumResult::Unfinished,
        result_header: None,
        time_control: Some(text),
        clocks: Vec::new(),
        warnings: Vec::new(),
    })?;
    let record = StorageLibraryRecord::new(
        DesktopAnnotationOwnerFixture::id(100)?,
        StorageLibraryEnumContent::Notation(Box::new(notation)),
    )?;
    let receipt = fixture.receipt()?;
    let binding = DesktopGameEditorBinding::new(
        GameStorageRevision::INITIAL,
        DesktopGameEditorEpoch::from_request_id(receipt.request_id()),
        crate::desktop::game_editor::DesktopGameEditorGeneration::ZERO,
    );
    let client = fixture.store.client();
    let publication = fixture
        .executor
        .submit_waiting(move |_cancellation| {
            let editor = DesktopGameEditor::notation(&record, GameRevision::ZERO, root, binding)?;
            editor.publish(editor.context(), &client)
        })
        .await?
        .join()
        .await??;
    let retained = publication.clone();
    drop(publication);
    let charged = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - owned_bytes + 1);
    assert!(matches!(charged, Err(StorageDocumentError::Saturated)));
    assert_eq!(retained.metadata().extensions().len(), 1);
    assert_eq!(retained.annotations().nags().len(), 1024);
    assert_eq!(retained.annotations().markings().len(), 1024);
    drop((charged, retained));
    let released = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(released);
    fixture.close().await
}

#[tokio::test]
async fn shared_publisher_refuses_played_projection_under_receiving_byte_pressure()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let record = DesktopAnnotationOwnerFixture::played_record(100)?;
    let receipt = fixture.receipt()?;
    let binding = DesktopGameEditorBinding::new(
        GameStorageRevision::INITIAL,
        DesktopGameEditorEpoch::from_request_id(receipt.request_id()),
        crate::desktop::game_editor::DesktopGameEditorGeneration::ZERO,
    );
    let memory = fixture.fill_remaining_memory()?;
    let client = fixture.store.client();
    let record = fixture
        .executor
        .submit_waiting(move |_cancellation| {
            let StorageLibraryEnumContent::Played(session) = record.content() else {
                return Err("wrong played fixture".into());
            };
            let editor = DesktopGameEditor::played(session, binding)?;
            assert!(matches!(
                editor.publish(editor.context(), &client),
                Err(DesktopGameEditorPublicationError::Memory(
                    StorageDocumentError::Saturated
                ))
            ));
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(record)
        })
        .await?
        .join()
        .await?
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;
    drop(memory);
    let client = fixture.store.client();
    let publication = fixture
        .executor
        .submit_waiting(move |_cancellation| {
            let StorageLibraryEnumContent::Played(session) = record.content() else {
                return Err("wrong played fixture".into());
            };
            let editor = DesktopGameEditor::played(session, binding)?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                editor.publish(editor.context(), &client)?,
            )
        })
        .await?
        .join()
        .await?
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;
    assert!(matches!(
        publication.state(),
        DesktopGameEditorSnapshotEnumState::Played { .. }
    ));
    let charged = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET);
    assert!(matches!(charged, Err(StorageDocumentError::Saturated)));
    drop((charged, publication));
    let released = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(released);
    fixture.close().await
}

struct DesktopAnnotationOwnerClock;

impl ClockTimeProvider for DesktopAnnotationOwnerClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(50))
    }
}

struct DesktopAnnotationOwnerWake;

impl DesktopApplicationWakeProvider for DesktopAnnotationOwnerWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

struct DesktopAnnotationOwnerWall {
    value: AtomicI64,
    unavailable: AtomicBool,
    calls: AtomicUsize,
}

impl GameStorageWallTimeProvider for DesktopAnnotationOwnerWall {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(GameStorageWallTimeError::Unavailable);
        }
        Ok(GameLibraryOrganizationTimestamp::new(
            self.value.load(Ordering::SeqCst),
        )?)
    }
}

struct DesktopAnnotationOwnerFixture {
    _directory: tempfile::TempDir,
    budget: CapacityBudget,
    store: StorageDocumentStore,
    executor: Arc<CapacityExecutor>,
    wall: Arc<DesktopAnnotationOwnerWall>,
    application: DesktopApplication<DesktopAnnotationOwnerClock, DesktopAnnotationOwnerWake>,
}

impl DesktopAnnotationOwnerFixture {
    async fn new() -> DesktopAnnotationOwnerTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let store =
            StorageDocumentStore::open(directory.path().join("annotation.sqlite"), budget.clone())
                .await?;
        Ok(Self {
            executor: Arc::new(CapacityExecutor::new(budget.clone())?),
            budget,
            store,
            _directory: directory,
            wall: Arc::new(DesktopAnnotationOwnerWall {
                value: AtomicI64::new(1000),
                unavailable: AtomicBool::new(false),
                calls: AtomicUsize::new(0),
            }),
            application: DesktopApplication::new(
                Arc::new(DesktopAnnotationOwnerClock),
                Arc::new(DesktopAnnotationOwnerWake),
            ),
        })
    }

    fn receipt(&self) -> DesktopAnnotationOwnerTestResult<DesktopApplicationReceipt> {
        let port = self.application.port();
        let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        let receipt = port.try_submit(&request)?;
        let work = self
            .application
            .try_next()?
            .ok_or("missing application work")?;
        assert!(receipt.shares_outcome(work.receipt()));
        Ok(receipt)
    }

    fn id(value: u128) -> DesktopAnnotationOwnerTestResult<GameId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn node(value: u128) -> DesktopAnnotationOwnerTestResult<GameMoveId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn record(id: u128) -> DesktopAnnotationOwnerTestResult<StorageLibraryRecord> {
        let mut node = 0_u128;
        let mut parser = NotationPgnImport::new(
            Cursor::new("{original} 1. e4 (1. d4 d5 2. c4) e5 *"),
            move || {
                node += 1;
                uuid::Uuid::from_u128(node).to_string().parse()
            },
        );
        let notation = parser.next_game(&|| false)?.ok_or("missing notation")?;
        Ok(StorageLibraryRecord::new(
            Self::id(id)?,
            StorageLibraryEnumContent::Notation(Box::new(notation)),
        )?)
    }

    fn played_record(id: u128) -> DesktopAnnotationOwnerTestResult<StorageLibraryRecord> {
        let session = GameSession::new(GameSessionOptions {
            id: Self::id(id)?,
            root: Self::node(1)?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                crate::game::enums::GameEnumMode::LocalTwoPlayer,
                crate::game::enums::GameEnumSeat::Human,
                crate::game::enums::GameEnumSeat::Human,
            )?,
            clock: crate::clock::state::ClockState::new(
                crate::clock::time_control::ClockTimeControl::untimed(),
                crate::board::enums::BoardEnumColor::White,
            ),
            metadata: GameMetadata::default(),
        })?;
        Ok(StorageLibraryRecord::new(
            Self::id(id)?,
            StorageLibraryEnumContent::Played(Box::new(session)),
        )?)
    }

    fn resources(&self) -> DesktopAnnotationOwnerResources {
        let wall_time = Arc::clone(&self.wall);
        DesktopAnnotationOwnerResources {
            executor: Arc::clone(&self.executor),
            client: self.store.client(),
            wall_time,
        }
    }

    fn fill_remaining_memory(
        &self,
    ) -> DesktopAnnotationOwnerTestResult<Vec<tokio::sync::OwnedSemaphorePermit>> {
        let client = self.store.client();
        let mut reservations = Vec::new();
        let mut bytes = StorageDocumentStore::MEMORY_BUDGET;
        while bytes != 0 {
            match client.reserve_library_memory(bytes) {
                Ok(reservation) => reservations.push(reservation),
                Err(StorageDocumentError::Saturated) => bytes /= 2,
                Err(error) => return Err(error.into()),
            }
        }
        assert!(matches!(
            client.reserve_library_memory(1),
            Err(StorageDocumentError::Saturated)
        ));
        Ok(reservations)
    }

    async fn open(&self, id: u128) -> DesktopAnnotationOwnerTestResult<DesktopAnnotationOwner> {
        let record = Arc::new(Self::record(id)?);
        let repository = StorageLibraryNotationRepository::new(self.store.client(), &self.executor);
        let prepared = repository
            .prepare(
                GameStorageRevision::INITIAL,
                Arc::clone(&record),
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                    10,
                )?),
            )
            .await?;
        let mut mutation = repository.begin_save(prepared)?;
        let StorageLibraryEnumMutationReceipt::Saved(saved) = mutation.wait().await? else {
            return Err("wrong initial save result".into());
        };
        let record = Arc::try_unwrap(record).map_err(|_record| "retained preparation borrow")?;
        let receipt = self.receipt()?;
        let mut owner = DesktopAnnotationOwner::open(
            record,
            saved.content_revision(),
            receipt.clone(),
            self.resources(),
        )?;
        owner.observe_compute(&receipt).await?;
        Ok(owner)
    }

    async fn execute(
        &self,
        owner: &mut DesktopAnnotationOwner,
        action: DesktopAnnotationCommandsEnumAction,
    ) -> DesktopAnnotationOwnerTestResult<DesktopAnnotationCommandsEnumResult> {
        let receipt = self.receipt()?;
        let expected = owner.snapshot()?.context();
        match owner.submit(DesktopAnnotationCommands::new(
            receipt.clone(),
            expected,
            action,
        ))? {
            DesktopAnnotationCommandsEnumAdmission::Ready(result) => Ok(*result),
            DesktopAnnotationCommandsEnumAdmission::ComputePending => {
                Ok(owner.observe_compute(&receipt).await?)
            }
            DesktopAnnotationCommandsEnumAdmission::SavePending => Err("unexpected save".into()),
            DesktopAnnotationCommandsEnumAdmission::PendingSnapshot { .. } => {
                Err("pending snapshot requires explicit inspection".into())
            }
        }
    }

    async fn notes(
        &self,
        owner: &mut DesktopAnnotationOwner,
        text: &str,
    ) -> DesktopAnnotationOwnerTestResult {
        self.execute(
            owner,
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: text.to_owned(),
                nags: vec![GameMoveNodeNag::new(1)],
            },
        )
        .await?;
        Ok(())
    }

    fn checkpoint(
        &self,
        owner: &mut DesktopAnnotationOwner,
        retry: bool,
    ) -> DesktopAnnotationOwnerTestResult<DesktopApplicationReceipt> {
        let receipt = self.receipt()?;
        let action = if retry {
            DesktopAnnotationCommandsEnumAction::RetrySave
        } else {
            DesktopAnnotationCommandsEnumAction::Checkpoint
        };
        let admission = owner.submit(DesktopAnnotationCommands::new(
            receipt.clone(),
            owner.snapshot()?.context(),
            action,
        ))?;
        assert!(matches!(
            admission,
            DesktopAnnotationCommandsEnumAdmission::SavePending
        ));
        Ok(receipt)
    }

    async fn save(
        &self,
        owner: &mut DesktopAnnotationOwner,
        retry: bool,
    ) -> DesktopAnnotationOwnerTestResult {
        let receipt = self.checkpoint(owner, retry)?;
        assert!(matches!(
            owner.observe_save(&receipt).await?,
            DesktopAnnotationOwnerEnumSaveProgress::Committing
        ));
        assert!(matches!(
            owner.observe_save(&receipt).await?,
            DesktopAnnotationOwnerEnumSaveProgress::Saved { .. }
        ));
        Ok(())
    }

    async fn close(self) -> DesktopAnnotationOwnerTestResult {
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_executor| "annotation executor still has an owner")?
            .shutdown()
            .await?;
        Ok(())
    }
}

#[tokio::test]
async fn opens_canonical_clockless_notation_and_preserves_checkpoint_utc()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let snapshot = owner.snapshot()?.clone();
    assert_eq!(
        snapshot.context().game(),
        DesktopAnnotationOwnerFixture::id(100)?
    );
    assert_eq!(
        snapshot.context().source(),
        DesktopGameEditorEnumSource::Notation {
            revision: GameRevision::ZERO
        }
    );
    assert!(matches!(
        snapshot.state(),
        DesktopGameEditorSnapshotEnumState::Notation {
            selected_clock_annotation: None,
            ..
        }
    ));
    assert_eq!(snapshot.annotations().notes(), "original");
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    fixture.save(&mut owner, false).await?;
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 0);
    let loaded = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(snapshot.context().game())
        .await?
        .ok_or("missing saved game")?;
    assert_eq!(
        loaded.modified(),
        Some(GameLibraryOrganizationTimestamp::new(10)?)
    );
    drop((loaded, owner));
    fixture.close().await
}

#[tokio::test]
async fn equal_fen_foreign_source_and_every_stale_binding_component_are_refused()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let context = owner.snapshot()?.context();
    let record = DesktopAnnotationOwnerFixture::record(101)?;
    let root = owner.snapshot()?.root();
    let foreign = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO,
        root,
        DesktopGameEditorBinding::new(
            context.storage_revision(),
            context.epoch(),
            context.generation(),
        ),
    )?
    .context();
    let other_epoch = DesktopGameEditorEpoch::from_request_id(fixture.receipt()?.request_id());
    let own_record = DesktopAnnotationOwnerFixture::record(100)?;
    let played_record = DesktopAnnotationOwnerFixture::played_record(100)?;
    let StorageLibraryEnumContent::Played(played) = played_record.content() else {
        return Err("expected actual played source".into());
    };
    let played_context = DesktopGameEditor::played(
        played,
        DesktopGameEditorBinding::new(
            context.storage_revision(),
            context.epoch(),
            context.generation(),
        ),
    )?
    .context();
    let cases = [
        foreign,
        played_context,
        DesktopGameEditor::notation(
            &own_record,
            GameRevision::ZERO,
            root,
            DesktopGameEditorBinding::new(
                context.storage_revision(),
                other_epoch,
                context.generation(),
            ),
        )?
        .context(),
        DesktopGameEditor::notation(
            &own_record,
            GameRevision::ZERO,
            root,
            DesktopGameEditorBinding::new(
                context.storage_revision(),
                context.epoch(),
                context.generation().next()?,
            ),
        )?
        .context(),
        DesktopGameEditor::notation(
            &own_record,
            GameRevision::ZERO,
            root,
            DesktopGameEditorBinding::new(
                GameStorageRevision::new(2),
                context.epoch(),
                context.generation(),
            ),
        )?
        .context(),
        DesktopGameEditor::notation(
            &own_record,
            GameRevision::ZERO.next()?,
            root,
            DesktopGameEditorBinding::new(
                context.storage_revision(),
                context.epoch(),
                context.generation(),
            ),
        )?
        .context(),
        DesktopGameEditor::notation(
            &own_record,
            GameRevision::ZERO,
            DesktopAnnotationOwnerFixture::node(2)?,
            DesktopGameEditorBinding::new(
                context.storage_revision(),
                context.epoch(),
                context.generation(),
            ),
        )?
        .context(),
    ];
    for stale in cases {
        let result = owner.submit(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            stale,
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "refused".to_owned(),
                nags: Vec::new(),
            },
        ));
        assert!(matches!(
            result,
            Err(DesktopAnnotationOwnerError::Editor(_))
        ));
    }
    assert_eq!(owner.snapshot()?.context(), context);
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 0);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn explicit_notes_nags_and_markings_are_separate_and_noop_preserves_edit_watermark()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture
        .notes(&mut owner, "accepted <b>plain text</b>")
        .await?;
    let first = owner
        .status()
        .accepted_edit()
        .ok_or("missing accepted edit")?;
    let markings = vec![GameMoveNodeMarking::circle(
        GameEnumMarkingColor::Green,
        "e4".parse()?,
    )];
    fixture.wall.value.store(2000, Ordering::SeqCst);
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::SaveMarkings {
                markings: markings.clone(),
            },
        )
        .await?;
    assert_eq!(
        owner.snapshot()?.annotations().notes(),
        "accepted <b>plain text</b>"
    );
    assert_eq!(
        owner.snapshot()?.annotations().nags(),
        &[GameMoveNodeNag::new(1)]
    );
    assert_eq!(owner.snapshot()?.annotations().markings(), markings);
    let second = owner
        .status()
        .accepted_edit()
        .ok_or("missing marking edit")?;
    assert!(second.revision() > first.revision());
    fixture.wall.value.store(9000, Ordering::SeqCst);
    fixture
        .notes(&mut owner, "accepted <b>plain text</b>")
        .await?;
    assert_eq!(owner.status().accepted_edit(), Some(second));
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::SaveMarkings { markings },
        )
        .await?;
    assert_eq!(owner.status().accepted_edit(), Some(second));
    fixture.save(&mut owner, false).await?;
    let recovered = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(DesktopAnnotationOwnerFixture::id(100)?)
        .await?
        .ok_or("missing game")?;
    assert_eq!(recovered.modified(), Some(second.timestamp()));
    drop((recovered, owner));
    fixture.close().await
}

#[tokio::test]
async fn legal_branch_reuses_existing_move_without_new_content_utc()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let root = owner.snapshot()?.root();
    let movement = BoardStart::standard().position().parse_uci("g1f3")?;
    let node = DesktopAnnotationOwnerFixture::node(90)?;
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::AppendStudyBranch { node, movement },
        )
        .await?;
    assert_eq!(owner.snapshot()?.context().selected(), node);
    let accepted = owner.status().accepted_edit();
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::Navigate { selected: root },
        )
        .await?;
    fixture.wall.value.store(9000, Ordering::SeqCst);
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::AppendStudyBranch {
                node: DesktopAnnotationOwnerFixture::node(91)?,
                movement,
            },
        )
        .await?;
    assert!(matches!(
        result,
        DesktopAnnotationCommandsEnumResult::Edited {
            change: GameAnnotationsEnumChange::Unchanged,
            ..
        }
    ));
    assert_eq!(owner.snapshot()?.context().selected(), node);
    assert_eq!(owner.status().accepted_edit(), accepted);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn variation_proof_stays_at_source_and_wrong_token_cannot_consume_subtree_confirmation()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::PreviewVariation(
                GameAnnotationsEnumVariationRequest::Delete {
                    root: DesktopAnnotationOwnerFixture::node(3)?,
                },
            ),
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Variation(preview) = result else {
        return Err("missing variation preview".into());
    };
    assert_eq!(preview.deleted_count(), 3);
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::VariationScope {
                token: preview.token(),
                scope: DesktopAnnotationCommandsEnumScope::DeletedNodes,
                offset: 0,
            },
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Scope(page) = result else {
        return Err("missing scope".into());
    };
    assert_eq!(page.nodes().len(), 3);
    let context = owner.snapshot()?.context();
    let result = owner.submit(DesktopAnnotationCommands::new(
        fixture.receipt()?,
        context,
        DesktopAnnotationCommandsEnumAction::ConfirmVariation {
            token: fixture.receipt()?.request_id(),
        },
    ));
    assert!(matches!(
        result,
        Err(DesktopAnnotationOwnerError::VariationToken)
    ));
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: preview.token(),
            },
        )
        .await?;
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::TreePage { cursor: None },
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Tree(page) = result else {
        return Err("missing tree page".into());
    };
    for deleted in page.nodes() {
        assert!(
            ![3, 4, 5]
                .into_iter()
                .map(DesktopAnnotationOwnerFixture::node)
                .collect::<Result<Vec<_>, _>>()?
                .contains(&deleted.id())
        );
    }
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn navigation_and_new_content_refuse_old_confirmation_without_rebasing()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::PreviewVariation(
                GameAnnotationsEnumVariationRequest::Promote {
                    tip: DesktopAnnotationOwnerFixture::node(5)?,
                },
            ),
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Variation(preview) = result else {
        return Err("missing preview".into());
    };
    fixture.notes(&mut owner, "changed after preview").await?;
    let result = owner.submit(DesktopAnnotationCommands::new(
        fixture.receipt()?,
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::ConfirmVariation {
            token: preview.token(),
        },
    ));
    assert!(matches!(
        result,
        Err(DesktopAnnotationOwnerError::Editor(_))
    ));
    assert_eq!(
        owner.snapshot()?.annotations().notes(),
        "changed after preview"
    );
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn wrong_compute_receipt_preserves_the_genuine_pending_result()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let receipt = fixture.receipt()?;
    owner.submit(DesktopAnnotationCommands::new(
        receipt.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "retained".to_owned(),
            nags: Vec::new(),
        },
    ))?;
    assert!(matches!(
        owner.observe_compute(&fixture.receipt()?).await,
        Err(DesktopAnnotationOwnerError::ForeignReceipt)
    ));
    owner.observe_compute(&receipt).await?;
    assert_eq!(owner.snapshot()?.annotations().notes(), "retained");
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn cancelled_compute_wait_keeps_canonical_source_and_observable_result()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let lease = fixture.budget.try_lease(1)?;
    let receipt = fixture.receipt()?;
    owner.submit(DesktopAnnotationCommands::new(
        receipt.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "after cancelled wait".to_owned(),
            nags: Vec::new(),
        },
    ))?;
    let mut waiting = Box::pin(owner.observe_compute(&receipt));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert!(owner.status().has_compute());
    assert_eq!(owner.snapshot()?.annotations().notes(), "original");
    drop(lease);
    owner.observe_compute(&receipt).await?;
    assert_eq!(
        owner.snapshot()?.annotations().notes(),
        "after cancelled wait"
    );
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn cancelled_preparation_wait_and_foreign_receipt_preserve_pending_save()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, "accepted").await?;
    let edit = owner.status().accepted_edit();
    let lease = fixture.budget.try_lease(1)?;
    let receipt = fixture.checkpoint(&mut owner, false)?;
    let mut waiting = Box::pin(owner.observe_save(&receipt));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert!(matches!(
        owner.observe_save(&fixture.receipt()?).await,
        Err(DesktopAnnotationOwnerError::ForeignReceipt)
    ));
    assert!(owner.status().has_save());
    assert_eq!(owner.status().accepted_edit(), edit);
    assert_eq!(owner.snapshot()?.annotations().notes(), "accepted");
    drop(lease);
    owner.observe_save(&receipt).await?;
    owner.observe_save(&receipt).await?;
    assert!(!owner.status().is_dirty());
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn older_save_completion_cannot_erase_newer_accepted_notes_or_timestamp()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, "first").await?;
    let receipt = fixture.checkpoint(&mut owner, false)?;
    owner.observe_save(&receipt).await?;
    fixture.wall.value.store(2000, Ordering::SeqCst);
    fixture.notes(&mut owner, "newest").await?;
    let newest = owner.status().accepted_edit();
    owner.observe_save(&receipt).await?;
    assert!(owner.status().is_dirty());
    assert_eq!(owner.status().accepted_edit(), newest);
    assert_eq!(owner.snapshot()?.annotations().notes(), "newest");
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    fixture.save(&mut owner, false).await?;
    let saved = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(DesktopAnnotationOwnerFixture::id(100)?)
        .await?
        .ok_or("missing latest game")?;
    assert_eq!(saved.modified(), newest.map(|edit| edit.timestamp()));
    let StorageLibraryEnumContent::Notation(game) = saved.record().content() else {
        return Err("wrong saved kind".into());
    };
    assert_eq!(
        game.tree().node(game.tree().root())?.annotations().notes(),
        "newest"
    );
    drop((saved, owner));
    fixture.close().await
}

#[tokio::test]
async fn wall_time_preflight_failure_leaves_content_and_context_unchanged()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let context = owner.snapshot()?.context();
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    assert!(
        fixture
            .notes(&mut owner, "must not be accepted")
            .await
            .is_err()
    );
    assert_eq!(owner.snapshot()?.context(), context);
    assert_eq!(owner.snapshot()?.annotations().notes(), "original");
    assert!(owner.status().accepted_edit().is_none());
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn sql_refusal_retains_accepted_source_and_explicit_retry_saves_newest_original_utc()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let database = fixture._directory.path().join("annotation.sqlite");
    let mut connection = StorageSqliteInspector::open(database)?;
    connection.batch("CREATE TRIGGER refuse_annotation BEFORE UPDATE ON documents BEGIN SELECT RAISE(ABORT, 'controlled refusal'); END;")?;
    fixture
        .notes(&mut owner, "accepted despite SQL refusal")
        .await?;
    let receipt = fixture.checkpoint(&mut owner, false)?;
    owner.observe_save(&receipt).await?;
    assert!(owner.observe_save(&receipt).await.is_err());
    assert!(owner.status().failure().is_some());
    assert!(owner.status().is_dirty());
    assert_eq!(
        owner.snapshot()?.annotations().notes(),
        "accepted despite SQL refusal"
    );
    fixture.wall.value.store(2000, Ordering::SeqCst);
    fixture.notes(&mut owner, "newest after refusal").await?;
    let newest = owner
        .status()
        .accepted_edit()
        .ok_or("missing newest edit")?;
    connection.batch("DROP TRIGGER refuse_annotation;")?;
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    fixture.save(&mut owner, true).await?;
    assert!(owner.status().failure().is_none());
    assert!(!owner.status().is_dirty());
    let recovered = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(DesktopAnnotationOwnerFixture::id(100)?)
        .await?
        .ok_or("missing retry save")?;
    assert_eq!(recovered.modified(), Some(newest.timestamp()));
    drop((connection, recovered, owner));
    fixture.close().await
}

#[tokio::test]
async fn byte_saturation_is_observed_without_losing_edit_and_retry_uses_same_utc()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture
        .notes(&mut owner, "retained through byte refusal")
        .await?;
    let edit = owner.status().accepted_edit();
    let memory = fixture.fill_remaining_memory()?;
    let receipt = fixture.checkpoint(&mut owner, false)?;
    assert!(owner.observe_save(&receipt).await.is_err());
    assert!(!owner.status().has_save());
    assert_eq!(owner.status().accepted_edit(), edit);
    drop(memory);
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    fixture.save(&mut owner, true).await?;
    assert_eq!(owner.status().accepted_edit(), edit);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn cancelled_commit_wait_retains_real_mutation_until_sql_completion_is_observed()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, "commit retained").await?;
    let mut connection =
        StorageSqliteInspector::open(fixture._directory.path().join("annotation.sqlite"))?;
    connection.batch("BEGIN IMMEDIATE;")?;
    let receipt = fixture.checkpoint(&mut owner, false)?;
    owner.observe_save(&receipt).await?;
    let mut waiting = Box::pin(owner.observe_save(&receipt));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert!(owner.status().has_save());
    assert!(matches!(
        owner.observe_save(&fixture.receipt()?).await,
        Err(DesktopAnnotationOwnerError::ForeignReceipt)
    ));
    connection.batch("ROLLBACK;")?;
    owner.observe_save(&receipt).await?;
    assert!(!owner.status().has_save());
    assert!(!owner.status().is_dirty());
    drop((connection, owner));
    fixture.close().await
}

#[tokio::test]
async fn snapshot_and_navigation_remain_serviceable_during_preparation_and_commit()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, "accepted").await?;
    let receipt = fixture.checkpoint(&mut owner, false)?;
    let result = owner.submit(DesktopAnnotationCommands::new(
        fixture.receipt()?,
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::Snapshot,
    ))?;
    assert!(matches!(
        result,
        DesktopAnnotationCommandsEnumAdmission::PendingSnapshot {
            pending: DesktopAnnotationCommandsEnumPending::Preparing { .. },
            ..
        }
    ));
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::Navigate {
                selected: DesktopAnnotationOwnerFixture::node(2)?,
            },
        )
        .await?;
    assert_eq!(
        owner.snapshot()?.context().selected(),
        DesktopAnnotationOwnerFixture::node(2)?
    );
    owner.observe_save(&receipt).await?;
    let root = owner.snapshot()?.root();
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::ChildrenPage {
                parent: root,
                cursor: None,
            },
        )
        .await?;
    owner.observe_save(&receipt).await?;
    assert_eq!(
        owner.snapshot()?.context().selected(),
        DesktopAnnotationOwnerFixture::node(2)?
    );
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn reopened_source_rejects_old_preview_even_when_game_tree_and_storage_match()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let old_context = owner.snapshot()?.context();
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::PreviewVariation(
                GameAnnotationsEnumVariationRequest::Delete {
                    root: DesktopAnnotationOwnerFixture::node(3)?,
                },
            ),
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Variation(preview) = result else {
        return Err("missing preview".into());
    };
    let recovered = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(old_context.game())
        .await?
        .ok_or("missing reopen source")?;
    let storage = recovered.content_revision();
    let receipt = fixture.receipt()?;
    let mut reopened = DesktopAnnotationOwner::open(
        recovered.into_record(),
        storage,
        receipt.clone(),
        fixture.resources(),
    )?;
    reopened.observe_compute(&receipt).await?;
    assert_ne!(reopened.snapshot()?.context().epoch(), old_context.epoch());
    assert!(matches!(
        reopened.submit(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            old_context,
            DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: preview.token()
            }
        )),
        Err(DesktopAnnotationOwnerError::Editor(_))
    ));
    assert!(matches!(
        reopened.submit(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            reopened.snapshot()?.context(),
            DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: preview.token()
            }
        )),
        Err(DesktopAnnotationOwnerError::VariationToken)
    ));
    drop((reopened, owner));
    fixture.close().await
}

#[tokio::test]
async fn cancelled_post_commit_projection_exposes_pending_and_refuses_obsolete_contexts()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture
        .notes(&mut owner, "durable before projection")
        .await?;
    let previous = owner.snapshot()?.context();
    let receipt = fixture.checkpoint(&mut owner, false)?;
    owner.observe_save(&receipt).await?;
    let document = fixture
        .store
        .client()
        .read(previous.game().as_uuid())
        .await?
        .ok_or("missing committed document")?;
    assert_eq!(document.revision(), previous.storage_revision().value() + 1);
    drop(document);
    let lease = fixture.budget.try_lease(1)?;
    let mut waiting = Box::pin(owner.observe_save(&receipt));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    let cached = owner.read_snapshot()?;
    assert_eq!(cached.snapshot().context(), previous);
    assert_eq!(
        cached.pending(),
        Some(DesktopAnnotationCommandsEnumPending::Projecting {
            storage_revision: GameStorageRevision::new(previous.storage_revision().value() + 1),
            covered: GameRevision::ZERO.next()?
        })
    );
    assert!(matches!(
        owner.snapshot(),
        Err(DesktopAnnotationOwnerError::ProjectionPending { .. })
    ));
    assert!(matches!(
        owner.submit(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            previous,
            DesktopAnnotationCommandsEnumAction::Snapshot
        )),
        Err(DesktopAnnotationOwnerError::ProjectionPending { .. })
    ));
    assert!(matches!(
        owner.observe_save(&fixture.receipt()?).await,
        Err(DesktopAnnotationOwnerError::ForeignReceipt)
    ));
    drop(lease);
    owner.observe_save(&receipt).await?;
    assert_eq!(
        owner.snapshot()?.context().storage_revision().value(),
        previous.storage_revision().value() + 1
    );
    assert!(owner.read_snapshot()?.pending().is_none());
    assert!(matches!(
        owner.submit(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            previous,
            DesktopAnnotationCommandsEnumAction::Navigate {
                selected: previous.selected()
            }
        )),
        Err(DesktopAnnotationOwnerError::Editor(_))
    ));
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn promotion_returns_actual_path_and_keeps_previous_mainline_as_an_alternative()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let original_tip = owner.snapshot()?.mainline_tip();
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::PreviewVariation(
                GameAnnotationsEnumVariationRequest::Promote {
                    tip: DesktopAnnotationOwnerFixture::node(5)?,
                },
            ),
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Variation(preview) = result else {
        return Err("missing promotion preview".into());
    };
    assert_eq!(preview.previous_tip(), original_tip);
    assert_eq!(
        preview.resulting_tip(),
        DesktopAnnotationOwnerFixture::node(5)?
    );
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::VariationScope {
                token: preview.token(),
                scope: DesktopAnnotationCommandsEnumScope::SelectedPath,
                offset: 0,
            },
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Scope(path) = result else {
        return Err("missing promoted path".into());
    };
    assert_eq!(
        path.nodes().last().copied(),
        Some(DesktopAnnotationOwnerFixture::node(5)?)
    );
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: preview.token(),
            },
        )
        .await?;
    assert_eq!(
        owner.snapshot()?.mainline_tip(),
        DesktopAnnotationOwnerFixture::node(5)?
    );
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::TreePage { cursor: None },
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Tree(page) = result else {
        return Err("missing promoted tree".into());
    };
    assert!(page.nodes().iter().any(|node| node.id() == original_tip));
    assert!(matches!(
        owner.submit(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            owner.snapshot()?.context(),
            DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: preview.token()
            }
        )),
        Err(DesktopAnnotationOwnerError::VariationToken)
    ));
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn navigation_invalidates_proof_without_assigning_a_content_edit_timestamp()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::PreviewVariation(
                GameAnnotationsEnumVariationRequest::Delete {
                    root: DesktopAnnotationOwnerFixture::node(3)?,
                },
            ),
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Variation(preview) = result else {
        return Err("missing preview".into());
    };
    fixture.wall.unavailable.store(true, Ordering::SeqCst);
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::Navigate {
                selected: DesktopAnnotationOwnerFixture::node(2)?,
            },
        )
        .await?;
    assert!(matches!(
        owner.submit(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            owner.snapshot()?.context(),
            DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: preview.token()
            }
        )),
        Err(DesktopAnnotationOwnerError::Editor(_))
    ));
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 0);
    assert!(owner.status().accepted_edit().is_none());
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn receipt_cancellation_before_cpu_acceptance_cannot_change_the_canonical_record()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let receipt = fixture.receipt()?;
    owner.submit(DesktopAnnotationCommands::new(
        receipt.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "must not be accepted".to_owned(),
            nags: Vec::new(),
        },
    ))?;
    let port = fixture.application.port();
    let quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    port.try_submit(&quit)?;
    assert!(receipt.is_cancellation_requested());
    let rejected = DesktopAnnotationOwner::open(
        DesktopAnnotationOwnerFixture::record(101)?,
        GameStorageRevision::new(1),
        receipt.clone(),
        fixture.resources(),
    )
    .err()
    .ok_or("cancelled open dropped its rejection")?;
    let (record, reason) = rejected.into_parts();
    assert_eq!(record.id(), DesktopAnnotationOwnerFixture::id(101)?);
    assert!(matches!(
        record.content(),
        StorageLibraryEnumContent::Notation(_)
    ));
    assert!(matches!(reason, DesktopAnnotationOwnerError::Cancelled));
    drop(record);
    assert!(owner.observe_compute(&receipt).await.is_err());
    assert_eq!(owner.snapshot()?.annotations().notes(), "original");
    assert!(owner.status().accepted_edit().is_none());
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn closed_storage_service_retains_accepted_record_and_the_primary_save_failure()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let owner = fixture.open(100).await?;
    drop(owner);
    let recovered = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(DesktopAnnotationOwnerFixture::id(100)?)
        .await?
        .ok_or("missing source")?;
    let receipt = fixture.receipt()?;
    let edit_receipt = fixture.receipt()?;
    let save_receipt = fixture.receipt()?;
    let DesktopAnnotationOwnerFixture {
        _directory,
        budget: _budget,
        store,
        executor,
        wall,
        application: _application,
    } = fixture;
    let mut owner = DesktopAnnotationOwner::open(
        recovered.into_record(),
        GameStorageRevision::new(1),
        receipt.clone(),
        DesktopAnnotationOwnerResources {
            executor: Arc::clone(&executor),
            client: store.client(),
            wall_time: wall,
        },
    )?;
    owner.observe_compute(&receipt).await?;
    store.close().await?;
    owner.submit(DesktopAnnotationCommands::new(
        edit_receipt.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "retained after close".to_owned(),
            nags: Vec::new(),
        },
    ))?;
    owner.observe_compute(&edit_receipt).await?;
    owner.submit(DesktopAnnotationCommands::new(
        save_receipt.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::Checkpoint,
    ))?;
    let error = owner
        .observe_save(&save_receipt)
        .await
        .err()
        .ok_or("closed storage admitted a save")?;
    let DesktopAnnotationOwnerError::Retained(error) = error else {
        return Err("missing retained save refusal".into());
    };
    let DesktopAnnotationOwnerError::Storage(error) = error.as_ref() else {
        return Err("missing actual storage refusal".into());
    };
    assert!(matches!(
        error.as_ref(),
        StorageLibraryError::Store(StorageDocumentError::Closed)
    ));
    assert!(owner.status().failure().is_some());
    assert!(owner.status().is_dirty());
    assert_eq!(
        owner.snapshot()?.annotations().notes(),
        "retained after close"
    );
    drop(owner);
    Arc::try_unwrap(executor)
        .map_err(|_executor| "annotation executor still has an owner")?
        .shutdown()
        .await?;
    drop(_directory);
    Ok(())
}

#[tokio::test]
async fn tree_and_proof_pages_account_for_the_whole_subtree_with_at_most_one_hundred_rows()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let start = BoardStart::standard();
    let root = DesktopAnnotationOwnerFixture::node(1)?;
    let mut tree = GameTree::new(root, start.position().clone(), None);
    let mut tip = root;
    for ply in 0..220_u128 {
        let uci = ["g1f3", "g8f6", "f3g1", "f6g8"][usize::try_from(ply % 4)?];
        let movement = tree.node(tip)?.position().parse_uci(uci)?;
        tip = tree.append(
            tip,
            DesktopAnnotationOwnerFixture::node(ply + 2)?,
            movement,
            None,
        )?;
    }
    let game = NotationGame::new(NotationGameParts {
        start,
        tree,
        mainline_tip: tip,
        metadata: GameMetadata::default(),
        result: NotationGameIdentityEnumResult::Unfinished,
        result_header: None,
        time_control: None,
        clocks: Vec::new(),
        warnings: Vec::new(),
    })?;
    let record = StorageLibraryRecord::new(
        DesktopAnnotationOwnerFixture::id(100)?,
        StorageLibraryEnumContent::Notation(Box::new(game)),
    )?;
    let receipt = fixture.receipt()?;
    let mut owner = DesktopAnnotationOwner::open(
        record,
        GameStorageRevision::INITIAL,
        receipt.clone(),
        fixture.resources(),
    )?;
    owner.observe_compute(&receipt).await?;
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::TreePage { cursor: None },
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Tree(page) = result else {
        return Err("missing tree page".into());
    };
    assert_eq!(page.nodes().len(), 100);
    assert!(page.next_cursor().is_some());
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::PreviewVariation(
                GameAnnotationsEnumVariationRequest::Delete {
                    root: DesktopAnnotationOwnerFixture::node(2)?,
                },
            ),
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Variation(preview) = result else {
        return Err("missing full subtree proof".into());
    };
    assert_eq!(preview.deleted_count(), 220);
    let mut offset = 0;
    let mut removed = Vec::new();
    loop {
        let result = fixture
            .execute(
                &mut owner,
                DesktopAnnotationCommandsEnumAction::VariationScope {
                    token: preview.token(),
                    scope: DesktopAnnotationCommandsEnumScope::DeletedNodes,
                    offset,
                },
            )
            .await?;
        let DesktopAnnotationCommandsEnumResult::Scope(page) = result else {
            return Err("missing subtree scope page".into());
        };
        assert!(page.nodes().len() <= 100);
        assert_eq!(page.total(), 220);
        removed.extend_from_slice(page.nodes());
        let Some(next) = page.next() else {
            break;
        };
        offset = next;
    }
    assert_eq!(removed.len(), 220);
    assert_eq!(
        removed
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        220
    );
    fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: preview.token(),
            },
        )
        .await?;
    let result = fixture
        .execute(
            &mut owner,
            DesktopAnnotationCommandsEnumAction::TreePage { cursor: None },
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Tree(page) = result else {
        return Err("missing remaining tree".into());
    };
    assert_eq!(page.nodes().len(), 1);
    assert_eq!(page.nodes()[0].id(), root);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn branch_from_a_foreign_position_is_refused_without_accepting_content_or_utc()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    let record = DesktopAnnotationOwnerFixture::record(101)?;
    let StorageLibraryEnumContent::Notation(game) = record.content() else {
        return Err("expected notation".into());
    };
    let movement = game
        .tree()
        .node(DesktopAnnotationOwnerFixture::node(2)?)?
        .position()
        .parse_uci("e7e5")?;
    let previous = owner.snapshot()?.context();
    assert!(
        fixture
            .execute(
                &mut owner,
                DesktopAnnotationCommandsEnumAction::AppendStudyBranch {
                    node: DesktopAnnotationOwnerFixture::node(90)?,
                    movement
                }
            )
            .await
            .is_err()
    );
    assert_eq!(owner.snapshot()?.context(), previous);
    assert!(owner.status().accepted_edit().is_none());
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn compute_accepted_after_sql_completion_is_reported_pending_until_durable_projection()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let mut owner = fixture.open(100).await?;
    fixture.notes(&mut owner, "first").await?;
    let save = fixture.checkpoint(&mut owner, false)?;
    owner.observe_save(&save).await?;
    let edit = fixture.receipt()?;
    owner.submit(DesktopAnnotationCommands::new(
        edit.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "newer while committing".to_owned(),
            nags: Vec::new(),
        },
    ))?;
    assert!(matches!(
        owner.observe_save(&save).await,
        Err(DesktopAnnotationOwnerError::ComputePending)
    ));
    let result = owner.observe_compute(&edit).await?;
    let DesktopAnnotationCommandsEnumResult::PendingProjection {
        result,
        storage_revision,
        ..
    } = result
    else {
        return Err("accepted edit was falsely reported ready".into());
    };
    assert_eq!(storage_revision, GameStorageRevision::new(2));
    assert!(matches!(
        *result,
        DesktopAnnotationCommandsEnumResult::Edited {
            change: GameAnnotationsEnumChange::Changed,
            ..
        }
    ));
    assert_eq!(
        owner.read_snapshot()?.snapshot().annotations().notes(),
        "newer while committing"
    );
    assert!(matches!(
        owner.snapshot(),
        Err(DesktopAnnotationOwnerError::ProjectionPending { .. })
    ));
    owner.observe_save(&save).await?;
    assert_eq!(
        owner.snapshot()?.annotations().notes(),
        "newer while committing"
    );
    assert!(owner.status().is_dirty());
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn rejected_played_open_returns_the_same_accepted_record_without_a_second_session()
-> DesktopAnnotationOwnerTestResult {
    let fixture = DesktopAnnotationOwnerFixture::new().await?;
    let record = DesktopAnnotationOwnerFixture::played_record(100)?;
    let id = record.id();
    let error = DesktopAnnotationOwner::open(
        record,
        GameStorageRevision::new(1),
        fixture.receipt()?,
        fixture.resources(),
    )
    .err()
    .ok_or("played source was accepted by notation owner")?;
    let (record, reason) = error.into_parts();
    assert_eq!(record.id(), id);
    assert!(matches!(reason, DesktopAnnotationOwnerError::Editor(_)));
    let StorageLibraryEnumContent::Played(session) = record.into_content() else {
        return Err("rejection lost the actual played session".into());
    };
    assert_eq!(session.id(), id);
    drop(session);
    fixture.close().await
}
