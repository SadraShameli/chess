use std::sync::atomic::Ordering;

use crate::clock::duration::ClockInstant;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::tests::played_recovery_review::{
    DesktopLibraryServicePlayedRecoveryReviewEnumRetry,
    DesktopLibraryServicePlayedRecoveryReviewObservation,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumResult, DesktopLibraryServiceError,
    DesktopLibraryServiceOperation, DesktopLibraryServicePort, DesktopLibraryServiceRequest,
};
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::game_document::StorageGameDocumentError;
use crate::storage::library_repository::{StorageLibraryError, StorageLibraryRepository};

#[derive(Debug)]
struct DesktopLibraryServiceAdmissionMemoryReviewRetained {
    commands: usize,
    note_bytes: usize,
    slots: usize,
    memory: usize,
}

impl DesktopLibraryServiceAdmissionMemoryReviewRetained {
    fn observe(fixture: &DesktopLibraryServiceFixture) -> DesktopLibraryServiceTestResult<Self> {
        let replies = fixture
            .port
            .replies
            .try_lock()
            .map_err(|_error| "retained request observation is unavailable")?;
        let note_bytes = replies
            .iter()
            .filter_map(|reply| {
                if let DesktopLibraryServiceEnumAction::Editor {
                    action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { notes, .. },
                    ..
                } = reply.operation.action()
                {
                    Some(notes.len())
                } else {
                    None
                }
            })
            .sum();
        Ok(Self {
            commands: replies.len(),
            note_bytes,
            slots: fixture.port.admission.available_permits(),
            memory: fixture.available_memory()?,
        })
    }
}

#[tokio::test]
async fn over_total_budget_notes_are_refused_before_admission_without_retained_payload_growth()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let mut session = DesktopLibraryServiceFixture::played(100)?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(50))?;
    let revision = fixture.seed_played(&session).await?;
    drop(session);
    let opened = fixture.open(100, revision).await?;
    let before = opened.context();
    let document = fixture
        .store
        .client()
        .read(before.game().as_uuid())
        .await?
        .ok_or("missing seeded document")?;
    let body_before = document.body().to_owned();
    let revision_before = document.revision();
    drop(document);
    let retained_before = DesktopLibraryServiceAdmissionMemoryReviewRetained::observe(&fixture)?;
    let reads_before = fixture.wall.calls.load(Ordering::SeqCst);
    let mut attempts = Vec::new();
    for attempt in 0..3 {
        let application = fixture.receipt()?;
        let action = DesktopLibraryServiceEnumAction::Editor {
            expected: before,
            action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: match attempt {
                    0 => "x".repeat(StorageDocumentStore::MEMORY_BUDGET + 1),
                    1 => "é".repeat(StorageDocumentStore::MEMORY_BUDGET / 2 + 1),
                    _ => String::with_capacity(StorageDocumentStore::MEMORY_BUDGET + 1),
                },
                nags: Vec::new(),
            },
        };
        let operation = DesktopLibraryServiceOperation::new(fixture.workspace, action);
        let preflight = operation.validate_payload();
        let typed_preflight = matches!(&preflight, Err(DesktopLibraryServiceError::Library(error))
            if matches!(error.as_ref(), StorageLibraryError::Store(StorageDocumentError::DocumentTooLarge)));
        let admission = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
            application.clone(),
            operation,
        ));
        let queued = DesktopLibraryServiceAdmissionMemoryReviewRetained::observe(&fixture)?;
        let mut refused = false;
        let mut typed_size_refusal = false;
        let mut cancelled = false;
        match admission {
            Err(error) => {
                refused = true;
                typed_size_refusal = matches!(
                    &error,
                    DesktopLibraryServiceError::Library(error) if matches!(
                        error.as_ref(),
                        StorageLibraryError::Store(StorageDocumentError::DocumentTooLarge)
                            | StorageLibraryError::Codec(StorageGameDocumentError::TooLarge
                                | StorageGameDocumentError::TextTooLarge)
                    )
                );
            }
            Ok(mut receipt) => {
                application.cancellation().request();
                let response = fixture.drive(&mut receipt).await;
                cancelled = matches!(
                    receipt.try_result(),
                    Some(Err(error)) if matches!(error.as_ref(), DesktopLibraryServiceError::Cancelled)
                );
                drop((response, receipt));
            }
        }
        let completed = DesktopLibraryServiceAdmissionMemoryReviewRetained::observe(&fixture)?;
        attempts.push((
            refused,
            typed_size_refusal,
            typed_preflight,
            cancelled,
            queued,
            completed,
        ));
        drop(application);
    }
    let reads_after = fixture.wall.calls.load(Ordering::SeqCst);
    let source = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: before,
            action: DesktopAnnotationCommandsEnumAction::Snapshot,
        })
        .await?;
    let DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Snapshot(
        source,
    )) = source
    else {
        return Err("unchanged played owner did not publish its source".into());
    };
    let source_context = source.context();
    let source_notes = source.annotations().notes().to_owned();
    let loaded = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(before.game())
        .await?
        .ok_or("admission changed the stored game identity")?;
    let modified = loaded.modified().ok_or("seeded record has no UTC")?.value();
    let storage_revision = loaded.content_revision();
    drop(loaded);
    let document = fixture
        .store
        .client()
        .read(before.game().as_uuid())
        .await?
        .ok_or("admission removed the seeded document")?;
    let body_after = document.body().to_owned();
    let revision_after = document.revision();
    drop((document, source, opened));
    let memory_observer = fixture.store.client();
    let budget = fixture.budget.clone();
    fixture.close().await?;
    let released = memory_observer
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?
        .num_permits();
    let capacity = budget.available();
    drop((memory_observer, budget));
    assert_eq!(source_context, before);
    assert!(source_notes.is_empty());
    assert_eq!(reads_after, reads_before);
    assert_eq!(body_after, body_before);
    assert_eq!(revision_after, revision_before);
    assert_eq!(storage_revision, revision);
    assert_eq!(modified, 10);
    assert_eq!(released, StorageDocumentStore::MEMORY_BUDGET);
    assert_eq!(capacity, 1);
    assert_eq!(attempts.len(), 3);
    for (refused, typed_size_refusal, typed_preflight, cancelled, queued, completed) in attempts {
        assert!(typed_preflight);
        assert!(
            refused || cancelled,
            "unexpected admission was not cancelled before source execution"
        );
        assert_eq!(queued.memory, retained_before.memory);
        assert_eq!(completed.memory, retained_before.memory);
        assert!(
            refused && typed_size_refusal,
            "over-total-budget notes entered the service transport instead of receiving a typed size refusal: queued={queued:?}, completed={completed:?}"
        );
        assert_eq!(queued.commands, retained_before.commands);
        assert_eq!(queued.note_bytes, retained_before.note_bytes);
        assert_eq!(completed.note_bytes, retained_before.note_bytes);
        assert_eq!(queued.slots, DesktopLibraryServicePort::CAPACITY);
        assert_eq!(completed.slots, retained_before.slots);
    }
    Ok(())
}

#[tokio::test]
async fn valid_sized_played_notes_under_temporary_memory_pressure_remain_recoverable()
-> DesktopLibraryServiceTestResult {
    DesktopLibraryServicePlayedRecoveryReviewObservation::capture(
        DesktopLibraryServicePlayedRecoveryReviewEnumRetry::Original,
    )
    .await?
    .assert_recovered()
}

#[tokio::test]
async fn individually_valid_queued_payloads_share_a_finite_transport_byte_budget()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    let before = opened.context();
    let reads = fixture.wall.calls.load(Ordering::SeqCst);
    let memory = fixture.available_memory()?;
    let mut receipts = Vec::new();
    let mut bounded = false;
    for _ in 0..DesktopLibraryServicePort::CAPACITY {
        let application = fixture.receipt()?;
        let admitted = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
            application.clone(),
            DesktopLibraryServiceOperation::new(
                fixture.workspace,
                DesktopLibraryServiceEnumAction::Editor {
                    expected: before,
                    action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                        notes: "x".repeat(1024 * 1024),
                        nags: Vec::new(),
                    },
                },
            ),
        ));
        match admitted {
            Ok(receipt) => {
                application.cancellation().request();
                receipts.push(receipt);
            }
            Err(DesktopLibraryServiceError::Busy) => {
                bounded = true;
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let count = receipts.len();
    let queued = DesktopLibraryServiceAdmissionMemoryReviewRetained::observe(&fixture)?;
    let mut cancellations = true;
    for receipt in &mut receipts {
        let result = fixture.drive(receipt).await;
        cancellations &= receipt.application_receipt().is_cancellation_requested()
            && matches!(receipt.try_result(), Some(Err(error))
                if matches!(error.disposition(), crate::desktop::library_service::DesktopLibraryServiceEnumDisposition::Refused));
        drop(result);
    }
    let snapshot = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: before,
            action: DesktopAnnotationCommandsEnumAction::Snapshot,
        })
        .await?;
    let unchanged = snapshot.editor_context() == Some(before);
    let reads_after = fixture.wall.calls.load(Ordering::SeqCst);
    drop((opened, snapshot, receipts));
    fixture.close().await?;
    assert!(
        bounded && cancellations && unchanged,
        "bounded={bounded}, cancellations={cancellations}, unchanged={unchanged}, count={count}, queued={queued:?}"
    );
    assert!(count > 0 && count < DesktopLibraryServicePort::CAPACITY);
    assert!(queued.note_bytes <= StorageDocumentStore::MEMORY_BUDGET);
    assert_eq!(queued.memory, memory);
    assert_eq!(reads_after, reads);
    Ok(())
}
