use std::future::{Future, poll_fn};
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Duration;

use tokio::time::Instant;

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumPending,
};
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceEnumSaveReceipt,
    DesktopLibraryServiceError, DesktopLibraryServiceOperation, DesktopLibraryServicePort,
    DesktopLibraryServiceRequest,
};
use crate::storage::library_query::StorageLibraryQueryOptions;
use crate::storage::library_repository::StorageLibraryRepository;

#[tokio::test]
async fn failed_save_retains_newest_source_and_retry_keeps_accepted_utc()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    fixture.sql("CREATE TRIGGER service_refusal BEFORE UPDATE ON library_entries BEGIN SELECT RAISE(ABORT, 'controlled service refusal'); END;").await?;
    assert!(
        fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: opened.context(),
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "first accepted source".to_owned(),
                    nags: Vec::new()
                }
            })
            .await
            .is_err()
    );
    let latest = fixture.open(100, revision).await?;
    assert_eq!(latest.annotations().notes(), "first accepted source");
    fixture.wall.millis.store(2000, Ordering::SeqCst);
    assert!(
        fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: latest.context(),
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "newest accepted source".to_owned(),
                    nags: Vec::new()
                }
            })
            .await
            .is_err()
    );
    fixture.sql("DROP TRIGGER service_refusal;").await?;
    fixture.wall.millis.store(9000, Ordering::SeqCst);
    let DesktopLibraryServiceEnumResult::PendingSnapshot {
        snapshot: pending, ..
    } = fixture
        .execute(DesktopLibraryServiceEnumAction::Open {
            game: opened.context().game(),
            storage_revision: revision,
        })
        .await?
    else {
        return Err("missing retained newest source".into());
    };
    let DesktopLibraryServiceEnumResult::Saved {
        snapshot,
        receipt: DesktopLibraryServiceEnumSaveReceipt::Notation { receipt, .. },
    } = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: pending.context(),
            action: DesktopAnnotationCommandsEnumAction::RetrySave,
        })
        .await?
    else {
        return Err("missing retry receipt".into());
    };
    assert_eq!(snapshot.annotations().notes(), "newest accepted source");
    let loaded = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(opened.context().game())
        .await?
        .ok_or("missing retry document")?;
    assert_eq!(loaded.content_revision(), receipt.content_revision());
    assert_eq!(
        loaded.modified().ok_or("missing modified UTC")?.value(),
        2000
    );
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 2);
    drop((loaded, receipt, opened, latest, pending, snapshot));
    fixture.close().await
}

#[tokio::test]
async fn publication_retry_does_not_replay_mutation_or_wall_clock()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    let memory = fixture.fill_memory()?;
    let application = fixture.receipt()?;
    let operation = DesktopLibraryServiceOperation::new(
        fixture.workspace,
        DesktopLibraryServiceEnumAction::Editor {
            expected: opened.context(),
            action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "one accepted edit".to_owned(),
                nags: Vec::new(),
            },
        },
    );
    let mut failed = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
        application.clone(),
        operation.clone(),
    ))?;
    assert!(fixture.drive(&mut failed).await.is_err());
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 1);
    drop(memory);
    fixture.wall.millis.store(9000, Ordering::SeqCst);
    let mut retry = fixture
        .port
        .try_submit(DesktopLibraryServiceRequest::new(application, operation))?;
    let DesktopLibraryServiceEnumResult::Saved { snapshot, .. } = fixture.drive(&mut retry).await?
    else {
        return Err("missing publication recovery".into());
    };
    assert_eq!(snapshot.annotations().notes(), "one accepted edit");
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 1);
    drop((opened, snapshot, failed, retry));
    fixture.close().await
}

#[tokio::test]
async fn pending_read_observes_the_owned_source_while_compute_waits_for_capacity()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    let lease = fixture.budget.try_lease(1)?;
    let mut edit = fixture.submit(DesktopLibraryServiceEnumAction::Editor {
        expected: opened.context(),
        action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "pending source".to_owned(),
            nags: Vec::new(),
        },
    })?;
    let mut running = Box::pin(fixture.drive(&mut edit));
    poll_fn(|context| {
        assert!(running.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(running);
    assert!(matches!(
        fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: opened.context(),
                action: DesktopAnnotationCommandsEnumAction::Snapshot
            })
            .await?,
        DesktopLibraryServiceEnumResult::PendingSnapshot {
            pending: DesktopAnnotationCommandsEnumPending::Computing,
            ..
        }
    ));
    drop(lease);
    assert!(matches!(
        fixture.drive(&mut edit).await?,
        DesktopLibraryServiceEnumResult::Saved { .. }
    ));
    drop((opened, edit));
    fixture.close().await
}

#[tokio::test]
async fn admitted_operations_are_bounded_before_any_future_is_polled()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let extra = DesktopLibraryServiceFixture::new().await?;
    let mut replies = Vec::new();
    for _ in 0..DesktopLibraryServicePort::CAPACITY {
        replies.push(fixture.submit(DesktopLibraryServiceEnumAction::Query {
            options: StorageLibraryQueryOptions::default(),
            cursor: None,
        })?);
    }
    assert!(matches!(
        fixture.port.try_submit(DesktopLibraryServiceRequest::new(
            extra.receipt()?,
            DesktopLibraryServiceOperation::new(
                fixture.workspace,
                DesktopLibraryServiceEnumAction::Query {
                    options: StorageLibraryQueryOptions::default(),
                    cursor: None
                }
            )
        )),
        Err(DesktopLibraryServiceError::Busy)
    ));
    drop(replies);
    extra.close().await?;
    fixture.close().await
}

#[tokio::test]
async fn shutdown_control_returns_the_actor_after_deadline_and_can_resume_cleanup()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let lease = fixture.budget.try_lease(1)?;
    let reply = fixture.submit(DesktopLibraryServiceEnumAction::Open {
        game: DesktopLibraryServiceFixture::id(100)?,
        storage_revision: revision,
    })?;
    let mut running = Box::pin(fixture.service.run());
    poll_fn(|context| {
        assert!(running.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(running);
    let mut shutdown = fixture.port.try_shutdown(Instant::now())?;
    assert!(matches!(
        fixture.service.run().await,
        Err(DesktopLibraryServiceError::Retained(_))
    ));
    assert!(shutdown.wait().await.is_err());
    assert!(!fixture.service.is_shutdown_complete());
    drop(lease);
    let mut retry = fixture
        .port
        .try_shutdown(Instant::now() + Duration::from_secs(3))?;
    fixture.service.run().await?;
    retry.wait().await?;
    assert!(fixture.service.is_shutdown_complete());
    drop((reply, shutdown, retry));
    fixture.close().await
}

#[tokio::test]
async fn full_source_memory_is_retained_until_observed_notation_cleanup()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    drop(opened);
    let retained = fixture.available_memory()?;
    fixture
        .service
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    let cleaned = fixture.available_memory()?;
    assert!(cleaned > retained);
    fixture.close().await
}

#[tokio::test]
async fn cancelled_unaccepted_editor_requests_certify_refusal_without_edits_or_utc_reads()
-> DesktopLibraryServiceTestResult {
    for notation in [false, true] {
        let mut fixture = DesktopLibraryServiceFixture::new().await?;
        let revision = fixture.seed(100, notation).await?;
        let opened = fixture.open(100, revision).await?;
        let before = opened.context();
        let reads = fixture.wall.calls.load(Ordering::SeqCst);
        let application = fixture.receipt()?;
        application.cancellation().request();
        let mut receipt = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
            application.clone(),
            DesktopLibraryServiceOperation::new(
                fixture.workspace,
                DesktopLibraryServiceEnumAction::Editor {
                    expected: before,
                    action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                        notes: "must remain unaccepted".to_owned(),
                        nags: Vec::new(),
                    },
                },
            ),
        ))?;
        let response = fixture.drive(&mut receipt).await;
        let refused = matches!(receipt.try_result(), Some(Err(error))
            if error.disposition() == DesktopLibraryServiceEnumDisposition::Refused);
        let snapshot = fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: before,
                action: DesktopAnnotationCommandsEnumAction::Snapshot,
            })
            .await?;
        let unchanged = snapshot.editor_context() == Some(before);
        let reads_after = fixture.wall.calls.load(Ordering::SeqCst);
        let loaded = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
            .load(before.game())
            .await?
            .ok_or("cancelled request lost stored source")?;
        let stored_revision = loaded.content_revision();
        let modified = loaded.modified().ok_or("missing source UTC")?.value();
        drop((loaded, response, receipt, snapshot, opened, application));
        fixture.close().await?;
        assert!(refused && unchanged);
        assert_eq!(reads_after, reads);
        assert_eq!(stored_revision, revision);
        assert_eq!(modified, 10);
    }
    Ok(())
}

#[tokio::test]
async fn failed_notation_save_and_cleanup_keep_the_original_disposition_until_real_retry_acknowledgement()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    let before = opened.context();
    fixture.sql("CREATE TRIGGER disposition_refusal BEFORE UPDATE ON library_entries BEGIN SELECT RAISE(ABORT, 'controlled disposition refusal'); END;").await?;
    let application = fixture.receipt()?;
    let mut original = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
        application.clone(),
        DesktopLibraryServiceOperation::new(
            fixture.workspace,
            DesktopLibraryServiceEnumAction::Editor {
                expected: before,
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "accepted source retains its exact receipt".to_owned(),
                    nags: Vec::new(),
                },
            },
        ),
    ))?;
    let failed = fixture.drive(&mut original).await;
    let expected = DesktopLibraryServiceEnumDisposition::Retained {
        request: application.request_id(),
        expected: Some(before),
    };
    let save_retained =
        matches!(original.try_result(), Some(Err(error)) if error.disposition() == expected);
    let latest = fixture.open(100, revision).await?;
    fixture.wall.millis.store(-1, Ordering::SeqCst);
    let mut refused = fixture.submit(DesktopLibraryServiceEnumAction::Editor {
        expected: latest.context(),
        action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "unaccepted draft cannot inherit the older continuation".to_owned(),
            nags: Vec::new(),
        },
    })?;
    let refusal = fixture.drive(&mut refused).await;
    let draft_refused = matches!(refused.try_result(), Some(Err(error))
        if error.disposition() == DesktopLibraryServiceEnumDisposition::Refused);
    fixture.wall.millis.store(9000, Ordering::SeqCst);
    let cleanup = fixture
        .service
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await;
    let cleanup_retained = matches!(&cleanup, Err(error) if error.disposition() == expected);
    let stopped = fixture.service.is_shutdown_complete();
    fixture.sql("DROP TRIGGER disposition_refusal;").await?;
    let retried = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: latest.context(),
            action: DesktopAnnotationCommandsEnumAction::RetrySave,
        })
        .await?;
    let reconciled = matches!(original.try_result(), Some(Ok(DesktopLibraryServiceEnumResult::Saved { snapshot, .. }))
        if snapshot.context() == retried.editor_context().ok_or("retry lost source context")?
            && snapshot.annotations().notes() == "accepted source retains its exact receipt");
    let reads = fixture.wall.calls.load(Ordering::SeqCst);
    drop((
        opened,
        application,
        original,
        failed,
        latest,
        cleanup,
        retried,
        refused,
        refusal,
    ));
    fixture.close().await?;
    assert!(save_retained && cleanup_retained && reconciled && draft_refused);
    assert!(!stopped);
    assert_eq!(reads, 2);
    Ok(())
}
