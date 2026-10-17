#[path = "library_service/admission_memory_review.rs"]
mod admission_memory_review;
#[path = "library_service/annotation_checkpoint_regressions.rs"]
mod annotation_checkpoint_regressions;
#[path = "library_service/editor.rs"]
mod editor;
#[path = "library_service/fixture.rs"]
pub(crate) mod fixture;
#[path = "library_service/pagination.rs"]
mod pagination;
#[path = "library_service/played_recovery_review.rs"]
mod played_recovery_review;
#[path = "library_service/recovery.rs"]
mod recovery;

use std::future::{Future, poll_fn};
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Duration;

use tokio::time::Instant;

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::game_editor::DesktopGameEditorEnumSource;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumResult,
    DesktopLibraryServiceEnumSaveReceipt, DesktopLibraryServiceError,
    DesktopLibraryServiceOperation, DesktopLibraryServiceRequest,
};
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataEnumResponse,
};
use crate::storage::library_query::StorageLibraryQueryOptions;
use crate::storage::library_repository::StorageLibraryRepository;

#[tokio::test]
async fn query_and_metadata_use_canonical_storage_pages() -> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    fixture.seed(100, true).await?;
    let DesktopLibraryServiceEnumResult::Query(page) = fixture
        .execute(DesktopLibraryServiceEnumAction::Query {
            options: StorageLibraryQueryOptions::default(),
            cursor: None,
        })
        .await?
    else {
        return Err("missing query page".into());
    };
    assert_eq!(page.entries().len(), 1);
    assert_eq!(
        page.entries()[0].id(),
        DesktopLibraryServiceFixture::id(100)?
    );
    let DesktopLibraryServiceEnumResult::Metadata(StorageLibraryMetadataEnumResponse::Tags(tags)) =
        fixture
            .execute(DesktopLibraryServiceEnumAction::Metadata(
                StorageLibraryMetadataEnumRequest::Tags { cursor: None },
            ))
            .await?
    else {
        return Err("missing metadata page".into());
    };
    assert_eq!(tags.catalog_revision(), page.catalog_revision());
    assert!(tags.entries().is_empty());
    drop((tags, page));
    fixture.close().await
}

#[tokio::test]
async fn notation_save_returns_real_receipt_and_preserves_original_utc_for_unchanged_edit()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    assert!(matches!(
        opened.context().source(),
        DesktopGameEditorEnumSource::Notation { .. }
    ));
    let action = DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
        notes: "accepted notes".to_owned(),
        nags: Vec::new(),
    };
    let DesktopLibraryServiceEnumResult::Saved {
        snapshot,
        receipt: DesktopLibraryServiceEnumSaveReceipt::Notation { receipt, covered },
    } = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: opened.context(),
            action: action.clone(),
        })
        .await?
    else {
        return Err("missing durable notation result".into());
    };
    assert_eq!(receipt.id(), opened.context().game());
    assert_eq!(
        receipt.content_revision(),
        snapshot.context().storage_revision()
    );
    assert_eq!(covered.value(), 1);
    assert_eq!(snapshot.annotations().notes(), "accepted notes");
    let calls = fixture.wall.calls.load(Ordering::SeqCst);
    fixture.wall.millis.store(9000, Ordering::SeqCst);
    assert!(matches!(
        fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: snapshot.context(),
                action
            })
            .await?,
        DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Edited { .. })
    ));
    let observed_calls = fixture.wall.calls.load(Ordering::SeqCst);
    let loaded = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(opened.context().game())
        .await?
        .ok_or("missing saved source")?;
    assert_eq!(
        loaded.modified().ok_or("missing modified UTC")?.value(),
        1000
    );
    assert_eq!(loaded.content_revision(), receipt.content_revision());
    drop((loaded, receipt, opened, snapshot));
    fixture.close().await?;
    assert_eq!(observed_calls, calls);
    Ok(())
}

#[tokio::test]
async fn both_open_kinds_reuse_the_authoritative_epoch_and_reject_stale_contexts()
-> DesktopLibraryServiceTestResult {
    for notation in [true, false] {
        let mut fixture = DesktopLibraryServiceFixture::new().await?;
        let revision = if notation {
            fixture.seed(100, true).await?
        } else {
            let mut session = DesktopLibraryServiceFixture::played(100)?;
            session.begin_analysis(
                session.revision(),
                crate::clock::duration::ClockInstant::from_nanos(0),
            )?;
            fixture.seed_played(&session).await?
        };
        let first = fixture.open(100, revision).await?;
        let second = fixture.open(100, revision).await?;
        assert_eq!(first.context(), second.context());
        let DesktopLibraryServiceEnumResult::Saved { snapshot, .. } = fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: first.context(),
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "new accepted source".to_owned(),
                    nags: Vec::new(),
                },
            })
            .await?
        else {
            return Err("missing saved editor source".into());
        };
        assert!(
            fixture
                .execute(DesktopLibraryServiceEnumAction::Editor {
                    expected: first.context(),
                    action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                        notes: "stale overwrite".to_owned(),
                        nags: Vec::new()
                    }
                })
                .await
                .is_err()
        );
        let reopened = fixture.open(100, revision).await?;
        assert_eq!(reopened.context(), snapshot.context());
        assert_eq!(reopened.annotations().notes(), "new accepted source");
        assert_eq!(
            fixture.factories.load(Ordering::SeqCst),
            usize::from(!notation)
        );
        drop((first, second, snapshot, reopened));
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn abandoned_wait_retains_admission_and_finishes_the_original_edit()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    let lease = fixture.budget.try_lease(1)?;
    let application = fixture.receipt()?;
    let operation = DesktopLibraryServiceOperation::new(
        fixture.workspace,
        DesktopLibraryServiceEnumAction::Editor {
            expected: opened.context(),
            action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "retained edit".to_owned(),
                nags: Vec::new(),
            },
        },
    );
    let mut reply = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
        application.clone(),
        operation.clone(),
    ))?;
    let mut waiting = Box::pin(fixture.drive(&mut reply));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    drop(reply);
    drop(lease);
    let mut retained = fixture
        .port
        .try_submit(DesktopLibraryServiceRequest::new(application, operation))?;
    let DesktopLibraryServiceEnumResult::Saved { snapshot, .. } =
        fixture.drive(&mut retained).await?
    else {
        return Err("lost abandoned edit".into());
    };
    assert_eq!(snapshot.annotations().notes(), "retained edit");
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 1);
    drop((snapshot, opened, retained));
    fixture.close().await
}

#[tokio::test]
async fn workspace_and_closing_rules_reject_fresh_work_but_allow_snapshot()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    let foreign = DesktopLibraryServiceOperation::new(
        DesktopMessagesEntityId::new(),
        DesktopLibraryServiceEnumAction::Open {
            game: opened.context().game(),
            storage_revision: revision,
        },
    );
    assert!(matches!(
        fixture.port.try_submit(DesktopLibraryServiceRequest::new(
            fixture.receipt()?,
            foreign
        )),
        Err(DesktopLibraryServiceError::ForeignWorkspace)
    ));
    fixture.port.begin_closing();
    assert!(
        fixture
            .submit(DesktopLibraryServiceEnumAction::Open {
                game: opened.context().game(),
                storage_revision: revision
            })
            .is_err()
    );
    assert!(matches!(
        fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: opened.context(),
                action: DesktopAnnotationCommandsEnumAction::Snapshot
            })
            .await?,
        DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Snapshot(_))
    ));
    drop(opened);
    fixture.close().await
}

#[tokio::test]
async fn deadline_keeps_pending_load_owned_until_cleanup_is_observed()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let lease = fixture.budget.try_lease(1)?;
    let mut reply = fixture.submit(DesktopLibraryServiceEnumAction::Open {
        game: DesktopLibraryServiceFixture::id(100)?,
        storage_revision: revision,
    })?;
    let mut waiting = Box::pin(fixture.drive(&mut reply));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert!(
        fixture
            .service
            .shutdown_retained(Instant::now())
            .await
            .is_err()
    );
    assert!(!fixture.service.is_shutdown_complete());
    drop(lease);
    fixture
        .service
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(fixture.service.is_shutdown_complete());
    drop(reply);
    fixture.close().await
}
#[path = "library_service/open_application_recovery_review_fixture.rs"]
pub(crate) mod application_recovery_fixture;
#[path = "library_service/notes_application_recovery_review.rs"]
mod notes_application_recovery_review;
#[path = "library_service/open_application_recovery_review.rs"]
mod open_application_recovery_review;
