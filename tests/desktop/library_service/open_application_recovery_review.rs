use crate::desktop::library_service::tests::application_recovery_fixture as fixture;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::clock::duration::ClockInstant;
use crate::clock::state::ClockStateView;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumPending,
    DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::game_editor::{
    DesktopGameEditorContext, DesktopGameEditorEnumSource, DesktopGameEditorEpoch,
    DesktopGameEditorPublication, DesktopGameEditorPublicationError,
    DesktopGameEditorSnapshotEnumState,
};
use crate::desktop::game_registry::DesktopGameRegistryRestored;
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::game_supervisor::DesktopGameSupervisorError;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::tests::open_application_recovery_review::fixture::DesktopLibraryServiceOpenApplicationRecoveryReviewFixture;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceError, DesktopLibraryServiceOperation,
    DesktopLibraryServiceRequest,
};
use crate::desktop::messages::{DesktopCommand, DesktopMessagesEnumResult};
use crate::game::enums::{GameEnumPhase, GameMoveId, GameRevision};
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::library_record::StorageLibraryEnumContent;
use crate::storage::library_repository::StorageLibraryRepository;

#[derive(Debug, PartialEq, Eq)]
struct DesktopLibraryServiceOpenApplicationRecoveryReviewPublication {
    context: DesktopGameEditorContext,
    notes: String,
    state: DesktopGameEditorSnapshotEnumState,
}

impl DesktopLibraryServiceOpenApplicationRecoveryReviewPublication {
    fn capture(snapshot: &DesktopGameEditorPublication) -> Self {
        Self {
            context: snapshot.context(),
            notes: snapshot.annotations().notes().to_owned(),
            state: snapshot.state().clone(),
        }
    }

    fn service(
        result: Option<Result<DesktopLibraryServiceEnumResult, Arc<DesktopLibraryServiceError>>>,
    ) -> Result<Self, String> {
        match result {
            Some(Ok(DesktopLibraryServiceEnumResult::Opened(snapshot))) => {
                Ok(Self::capture(&snapshot))
            }
            result => Err(format!("expected recovered Opened publication: {result:?}")),
        }
    }

    fn application(receipt: &DesktopApplicationReceipt) -> Result<Self, String> {
        match receipt.response() {
            Some(DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::Library(value),
                revision,
            }) if *revision == value.revision() => match value.as_ref() {
                DesktopLibraryServiceEnumResult::Opened(snapshot) => Ok(Self::capture(snapshot)),
                value @ (DesktopLibraryServiceEnumResult::Query(_)
                | DesktopLibraryServiceEnumResult::Metadata(_)
                | DesktopLibraryServiceEnumResult::Organized(_)
                | DesktopLibraryServiceEnumResult::TrashPlan(_)
                | DesktopLibraryServiceEnumResult::Trashed(_)
                | DesktopLibraryServiceEnumResult::Restored(_)
                | DesktopLibraryServiceEnumResult::Editor(_)
                | DesktopLibraryServiceEnumResult::PendingSnapshot { .. }
                | DesktopLibraryServiceEnumResult::Saved { .. }) => Err(format!(
                    "fresh application did not recover Opened: {value:?}"
                )),
            },
            response => Err(format!(
                "fresh application did not receive publication: {response:?}"
            )),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct DesktopLibraryServiceOpenApplicationRecoveryReviewSource {
    revision: GameRevision,
    phase: GameEnumPhase,
    live: GameMoveId,
    selected: GameMoveId,
    clock: ClockStateView,
}

impl DesktopLibraryServiceOpenApplicationRecoveryReviewSource {
    fn capture(snapshot: &DesktopGameSnapshot) -> DesktopLibraryServiceTestResult<Self> {
        Ok(Self {
            revision: snapshot.revision(),
            phase: snapshot.phase(),
            live: snapshot.live(),
            selected: snapshot.view(),
            clock: snapshot.clock().view(ClockInstant::from_nanos(50))?,
        })
    }
}

#[tokio::test]
async fn failed_played_open_exposes_original_context_for_fresh_application_retry_without_replaying_open()
-> DesktopLibraryServiceTestResult {
    let mut old = DesktopLibraryServiceFixture::new().await?;
    let old_session = DesktopLibraryServiceOpenApplicationRecoveryReviewFixture::played()?;
    let old_revision = old.seed_played(&old_session).await?;
    let old_publication = old.open(100, old_revision).await?;
    let old_context = old_publication.context();
    drop((old_publication, old_session));
    old.close().await?;

    let mut fixture = DesktopLibraryServiceOpenApplicationRecoveryReviewFixture::new().await?;
    let session = DesktopLibraryServiceOpenApplicationRecoveryReviewFixture::played()?;
    let game = session.id();
    let revision = fixture.library.seed_played(&session).await?;
    drop(session);
    let recovered =
        StorageLibraryRepository::new(fixture.library.store.client(), &fixture.library.executor)
            .load(game)
            .await?
            .ok_or("missing played source in SQLite")?;
    let StorageLibraryEnumContent::Played(session) = recovered.into_record().into_content() else {
        return Err("SQLite source is not a played game".into());
    };
    fixture
        .library
        .registry
        .restore(DesktopGameRegistryRestored {
            session: *session,
            storage_revision: revision,
        })
        .await?;
    let before = DesktopLibraryServiceOpenApplicationRecoveryReviewSource::capture(
        &fixture.snapshot(game)?,
    )?;
    let stored = fixture
        .library
        .store
        .client()
        .read(game.as_uuid())
        .await?
        .ok_or("missing seeded SQLite document")?;
    let durable_before = (stored.revision(), stored.body().to_owned());
    drop(stored);
    let reads_before = fixture.library.wall.calls.load(Ordering::SeqCst);
    let pressure = fixture.library.fill_memory()?;
    let positive_pressure = pressure
        .iter()
        .map(tokio::sync::OwnedSemaphorePermit::num_permits)
        .sum::<usize>()
        > 0;
    let operation = DesktopLibraryServiceOperation::new(
        fixture.library.workspace,
        DesktopLibraryServiceEnumAction::Open {
            game,
            storage_revision: revision,
        },
    );
    let request = fixture
        .application
        .request(DesktopCommand::library(operation.clone()))?;
    let original = fixture.application.try_submit(&request)?;
    let original_id = original.request_id();
    fixture
        .drive_until(|| original.response().is_some())
        .await?;
    let original_service = fixture.observed.try_recv()?;
    let lower = fixture
        .library
        .service
        .played
        .get(&original.request_id())
        .map(|played| played.receipt.clone())
        .ok_or("initial Open did not retain its actual lower receipt")?;
    let actual_context = match lower.pending() {
        Some(DesktopAnnotationCommandsEnumPending::Publishing { context }) => context,
        pending => {
            return Err(format!("initial Open did not retain Publishing: {pending:?}").into());
        }
    };
    let receiving_refusal = matches!(lower.try_result(), Some(Err(error))
        if matches!(error.as_ref(), DesktopGameSupervisorError::EditorPublicationPending { context, source }
            if *context == actual_context && matches!(source.as_ref(), DesktopGameEditorPublicationError::Memory(StorageDocumentError::Saturated))));
    let first_error = match original.response() {
        Some(DesktopMessagesEnumResult::LibraryFailure(error)) => Some(Arc::clone(error)),
        _ => None,
    };
    let disposition = first_error.as_ref().map(|error| error.disposition());
    let certified = match disposition {
        Some(DesktopLibraryServiceEnumDisposition::Retained { expected, .. }) => expected,
        _ => None,
    };
    let service_correlated = original_service
        .application_receipt()
        .shares_outcome(&original)
        && lower.application_receipt().shares_outcome(&original)
        && original_service.request_id() == request.request_id();
    let failure_forwarded = matches!(original_service.try_result(), Some(Err(error))
        if first_error.as_ref().is_some_and(|first| Arc::ptr_eq(first, &error)));
    let cached = fixture.application.try_submit(&request)?;
    fixture.drive_until(|| cached.response().is_some()).await?;
    let cached_before = cached.shares_outcome(&original)
        && fixture.dispatches.load(Ordering::SeqCst) == 1
        && fixture.observed.is_empty();

    let foreign_request = fixture.application.request(DesktopCommand::library(
        DesktopLibraryServiceOperation::new(
            fixture.library.workspace,
            DesktopLibraryServiceEnumAction::Editor {
                expected: old_context,
                action: DesktopAnnotationCommandsEnumAction::RetrySave,
            },
        ),
    ))?;
    let foreign = fixture.application.try_submit(&foreign_request)?;
    fixture.drive_until(|| foreign.response().is_some()).await?;
    let foreign_service = fixture.observed.try_recv()?;
    let foreign_refused = matches!(foreign.response(), Some(DesktopMessagesEnumResult::LibraryFailure(error))
        if error.disposition() == DesktopLibraryServiceEnumDisposition::Refused)
        && matches!(foreign_service.try_result(), Some(Err(error)) if error.disposition() == DesktopLibraryServiceEnumDisposition::Refused);
    let owner_preserved = lower.pending()
        == Some(DesktopAnnotationCommandsEnumPending::Publishing {
            context: actual_context,
        })
        && fixture.library.service.played.len() == 1
        && fixture
            .library
            .service
            .played
            .get(&original.request_id())
            .is_some_and(|owner| {
                owner
                    .receipt
                    .application_receipt()
                    .shares_outcome(&original)
            })
        && matches!(original_service.try_result(), Some(Err(error)) if first_error.as_ref().is_some_and(|first| Arc::ptr_eq(first, &error)));
    let reads_accepted = fixture.library.wall.calls.load(Ordering::SeqCst);
    let source_accepted = DesktopLibraryServiceOpenApplicationRecoveryReviewSource::capture(
        &fixture.snapshot(game)?,
    )?;
    drop(pressure);
    fixture.library.wall.millis.store(9000, Ordering::SeqCst);

    let retry = if let Some(expected) = certified {
        let retry_request = fixture.application.request(DesktopCommand::library(
            DesktopLibraryServiceOperation::new(
                fixture.library.workspace,
                DesktopLibraryServiceEnumAction::Editor {
                    expected,
                    action: DesktopAnnotationCommandsEnumAction::RetrySave,
                },
            ),
        ))?;
        let retry_application = fixture.application.try_submit(&retry_request)?;
        fixture
            .drive_until(|| retry_application.response().is_some())
            .await?;
        let retry_service = fixture.observed.try_recv()?;
        let distinct = retry_application.request_id() != original.request_id()
            && retry_request.request_id() == retry_application.request_id()
            && !retry_application.shares_outcome(&original)
            && retry_service
                .application_receipt()
                .shares_outcome(&retry_application)
            && lower.application_receipt().shares_outcome(&original);
        let application_result =
            DesktopLibraryServiceOpenApplicationRecoveryReviewPublication::application(
                &retry_application,
            );
        let service_result = DesktopLibraryServiceOpenApplicationRecoveryReviewPublication::service(
            retry_service.try_result(),
        );
        let original_result =
            DesktopLibraryServiceOpenApplicationRecoveryReviewPublication::service(
                original_service.try_result(),
            );
        let lower_recovered = lower.pending().is_none()
            && matches!(lower.try_result(), Some(Ok(DesktopAnnotationCommandsEnumResult::Snapshot(snapshot))) if snapshot.context() == expected);
        drop((retry_application, retry_service, retry_request));
        Some((
            distinct,
            application_result,
            service_result,
            original_result,
            lower_recovered,
        ))
    } else {
        None
    };
    let source_after_retry = DesktopLibraryServiceOpenApplicationRecoveryReviewSource::capture(
        &fixture.snapshot(game)?,
    )?;
    let no_save_before_cleanup = lower.storage_receipt().is_none();
    if lower.pending().is_some() {
        let cleanup = fixture
            .library
            .port
            .try_submit(DesktopLibraryServiceRequest::new(
                original.clone(),
                operation.clone(),
            ))?;
        fixture
            .drive_until(|| cleanup.try_result().is_some())
            .await?;
        let cleanup_result = cleanup.try_result();
        drop((cleanup, cleanup_result));
    }
    let lower_result = match lower.try_result() {
        Some(Ok(DesktopAnnotationCommandsEnumResult::Snapshot(snapshot))) => {
            Ok(DesktopLibraryServiceOpenApplicationRecoveryReviewPublication::capture(&snapshot))
        }
        result => Err(format!(
            "original lower publication did not recover: {result:?}"
        )),
    };
    let original_after_cleanup =
        DesktopLibraryServiceOpenApplicationRecoveryReviewPublication::service(
            original_service.try_result(),
        );
    let no_save_after_cleanup = lower.storage_receipt().is_none() && lower.pending().is_none();
    let cached_again = fixture.application.try_submit(&request)?;
    fixture
        .drive_until(|| cached_again.response().is_some())
        .await?;
    let cached_after = cached_again.shares_outcome(&original)
        && matches!(cached_again.response(), Some(DesktopMessagesEnumResult::LibraryFailure(error))
            if first_error.as_ref().is_some_and(|first| Arc::ptr_eq(first, error)))
        && fixture.dispatches.load(Ordering::SeqCst) == if certified.is_some() { 3 } else { 2 }
        && fixture.observed.is_empty();
    let stored = fixture
        .library
        .store
        .client()
        .read(game.as_uuid())
        .await?
        .ok_or("missing recovered SQLite document")?;
    let durable_after = (stored.revision(), stored.body().to_owned());
    drop(stored);
    let loaded =
        StorageLibraryRepository::new(fixture.library.store.client(), &fixture.library.executor)
            .load(game)
            .await?
            .ok_or("missing recovered library entry")?;
    let durable_revision = loaded.content_revision();
    let modified = loaded.modified().map(|timestamp| timestamp.value());
    drop(loaded);
    let reads_after = fixture.library.wall.calls.load(Ordering::SeqCst);
    let factories = fixture.library.factories.load(Ordering::SeqCst);
    let source_after_cleanup = DesktopLibraryServiceOpenApplicationRecoveryReviewSource::capture(
        &fixture.snapshot(game)?,
    )?;
    drop((
        lower,
        original_service,
        original,
        cached,
        cached_again,
        request,
        foreign,
        foreign_service,
        foreign_request,
        first_error,
        operation,
    ));
    let (shutdowns, memory, capacity) = fixture.close().await?;

    assert_eq!(shutdowns, 1);
    assert_eq!(memory, StorageDocumentStore::MEMORY_BUDGET);
    assert_eq!(capacity, 1);
    assert!(positive_pressure && receiving_refusal);
    assert!(service_correlated && failure_forwarded);
    assert!(
        cached_before && cached_after,
        "original application request must keep its cached terminal failure without dispatch"
    );
    assert!(
        foreign_refused && owner_preserved,
        "old actual epoch must not replace the original Open owner or consume its watcher"
    );
    assert_eq!(old_context.game(), actual_context.game());
    assert_eq!(old_context.source(), actual_context.source());
    assert_eq!(old_context.selected(), actual_context.selected());
    assert_eq!(
        old_context.storage_revision(),
        actual_context.storage_revision()
    );
    assert_eq!(old_context.generation(), actual_context.generation());
    assert_ne!(old_context.epoch(), actual_context.epoch());
    assert_eq!(
        actual_context.epoch(),
        DesktopGameEditorEpoch::from_request_id(original_id)
    );
    assert_eq!(
        actual_context.source(),
        DesktopGameEditorEnumSource::Played {
            revision: before.revision
        }
    );
    assert_eq!(before, source_accepted);
    assert_eq!(before, source_after_retry);
    assert_eq!(before, source_after_cleanup);
    assert_eq!((reads_before, reads_accepted, reads_after), (0, 0, 0));
    assert_eq!(factories, 1);
    assert!(no_save_before_cleanup && no_save_after_cleanup);
    assert_eq!(durable_before, durable_after);
    assert_eq!(durable_revision, revision);
    assert_eq!(modified, Some(10));
    assert!(
        lower_result.is_ok(),
        "supported original service cleanup failed: {lower_result:?}"
    );
    assert_eq!(original_after_cleanup, lower_result);
    assert_eq!(
        certified,
        Some(actual_context),
        "retained Open must expose the exact original lower Publishing context before a fresh UI request can retry; observed {disposition:?}"
    );
    assert_eq!(
        disposition,
        Some(DesktopLibraryServiceEnumDisposition::Retained {
            request: original_id,
            expected: Some(actual_context),
        })
    );
    let (distinct, application_result, service_result, original_result, lower_recovered) =
        retry.ok_or("certified Open continuation did not exercise fresh application retry")?;
    assert!(distinct);
    assert!(
        lower_recovered,
        "fresh application retry must finish the original lower publication before cleanup"
    );
    assert!(
        application_result.is_ok(),
        "fresh application retry did not recover Opened before cleanup: {application_result:?}"
    );
    assert_eq!(application_result, lower_result);
    assert_eq!(service_result, lower_result);
    assert_eq!(original_result, lower_result);
    Ok(())
}
