use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::clock::duration::ClockInstant;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumPending,
};
use crate::desktop::game_bridge::DesktopGameBridgeProvider;
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorContext,
    DesktopGameEditorEnumSource, DesktopGameEditorEpoch, DesktopGameEditorPublicationError,
};
use crate::desktop::game_supervisor::DesktopGameSupervisorError;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceEnumSaveReceipt,
    DesktopLibraryServiceError, DesktopLibraryServiceOperation, DesktopLibraryServiceRequest,
};
use crate::game::enums::GameRevision;
use crate::game::storage_repository::{GameStorageReceipt, GameStorageRevision};
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::library_record::StorageLibraryEnumContent;
use crate::storage::library_repository::StorageLibraryRepository;

#[derive(Clone, Copy)]
pub(super) enum DesktopLibraryServicePlayedRecoveryReviewEnumRetry {
    FreshRetrySave,
    CancelledOriginal,
    Original,
    ForeignBindingThenRetry,
    RepeatedFreshRetrySave,
}

#[derive(Debug, PartialEq, Eq)]
struct DesktopLibraryServicePlayedRecoveryReviewSaved {
    context: DesktopGameEditorContext,
    receipt: GameStorageReceipt,
    notes: String,
}

impl DesktopLibraryServicePlayedRecoveryReviewSaved {
    fn observe(
        result: Option<Result<DesktopLibraryServiceEnumResult, Arc<DesktopLibraryServiceError>>>,
    ) -> Result<Self, String> {
        match result {
            Some(Ok(DesktopLibraryServiceEnumResult::Saved {
                snapshot,
                receipt: DesktopLibraryServiceEnumSaveReceipt::Played(receipt),
            })) => Ok(Self {
                context: snapshot.context(),
                receipt,
                notes: snapshot.annotations().notes().to_owned(),
            }),
            result => Err(format!(
                "expected actual played save acknowledgement: {result:?}"
            )),
        }
    }
}

pub(super) struct DesktopLibraryServicePlayedRecoveryReviewObservation {
    pressure: bool,
    publishing: bool,
    original_correlated: bool,
    retry_correlated: bool,
    independent_retry: bool,
    cancellation_requested: bool,
    retained_disposition: bool,
    foreign_refusals: bool,
    retry_aliases_reconciled: bool,
    before: DesktopGameEditorContext,
    accepted: DesktopGameEditorContext,
    status_revision: GameRevision,
    reads_before: usize,
    reads_accepted: usize,
    reads_after: usize,
    original: Result<DesktopLibraryServicePlayedRecoveryReviewSaved, String>,
    retry: Result<DesktopLibraryServicePlayedRecoveryReviewSaved, String>,
    canonical: GameStorageReceipt,
    durable_revision: GameRevision,
    durable_storage_revision: GameStorageRevision,
    durable_notes: String,
    modified: i64,
    released_memory: usize,
    released_capacity: usize,
}

impl DesktopLibraryServicePlayedRecoveryReviewObservation {
    const NOTES: &str = "one accepted played edit awaiting publication";
    const OWNERSHIP_TIMEOUT: Duration = Duration::from_secs(3);

    pub(super) async fn capture(
        kind: DesktopLibraryServicePlayedRecoveryReviewEnumRetry,
    ) -> DesktopLibraryServiceTestResult<Self> {
        let mut fixture = DesktopLibraryServiceFixture::new().await?;
        let mut session = DesktopLibraryServiceFixture::played(100)?;
        session.begin_analysis(session.revision(), ClockInstant::from_nanos(50))?;
        let revision = fixture.seed_played(&session).await?;
        let opened = fixture.open(100, revision).await?;
        let before = opened.context();
        let mut foreign_contexts = Vec::new();
        if matches!(
            kind,
            DesktopLibraryServicePlayedRecoveryReviewEnumRetry::ForeignBindingThenRetry
        ) {
            let foreign_epoch =
                DesktopGameEditorEpoch::from_request_id(fixture.receipt()?.request_id());
            for binding in [
                DesktopGameEditorBinding::new(revision, foreign_epoch, before.generation()),
                DesktopGameEditorBinding::new(
                    revision,
                    before.epoch(),
                    before.generation().next()?,
                ),
                DesktopGameEditorBinding::new(
                    GameStorageRevision::new(revision.value() + 1),
                    before.epoch(),
                    before.generation(),
                ),
            ] {
                foreign_contexts.push(DesktopGameEditor::played(&session, binding)?.context());
            }
            session.save_notes_and_nags(
                session.revision(),
                session.view(),
                "foreign local revision".to_owned(),
                Vec::new(),
            )?;
            foreign_contexts.push(
                DesktopGameEditor::played(
                    &session,
                    DesktopGameEditorBinding::new(revision, before.epoch(), before.generation()),
                )?
                .context(),
            );
            let other = DesktopLibraryServiceFixture::played(101)?;
            foreign_contexts.push(
                DesktopGameEditor::played(
                    &other,
                    DesktopGameEditorBinding::new(revision, before.epoch(), before.generation()),
                )?
                .context(),
            );
            drop(other);
        }
        drop(session);
        let reads_before = fixture.wall.calls.load(Ordering::SeqCst);
        let memory = fixture.fill_memory()?;
        let application = fixture.receipt()?;
        let action = DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: Self::NOTES.to_owned(),
            nags: Vec::new(),
        };
        let operation = DesktopLibraryServiceOperation::new(
            fixture.workspace,
            DesktopLibraryServiceEnumAction::Editor {
                expected: before,
                action: action.clone(),
            },
        );
        let mut original = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
            application.clone(),
            operation.clone(),
        ))?;
        let first_result = fixture.drive(&mut original).await;
        let retained_disposition = matches!(original.try_result(), Some(Err(error))
        if error.disposition() == (DesktopLibraryServiceEnumDisposition::Retained {
            request: application.request_id(), expected: Some(before),
        }));
        let canonical = fixture
            .service
            .played
            .get(&application.request_id())
            .map(|played| played.receipt.clone())
            .ok_or("missing retained original supervisor receipt")?;
        let Some(Err(error)) = canonical.try_result() else {
            return Err("played source did not reach publication refusal".into());
        };
        let DesktopGameSupervisorError::EditorPublicationPending { context, source } =
            error.as_ref()
        else {
            return Err(format!("unexpected played refusal: {error}").into());
        };
        let accepted = *context;
        let pressure = matches!(
            source.as_ref(),
            DesktopGameEditorPublicationError::Memory(StorageDocumentError::Saturated)
        );
        drop(error);
        let publishing = matches!(
            canonical.pending(),
            Some(DesktopAnnotationCommandsEnumPending::Publishing { context }) if context == accepted
        ) && canonical.storage_receipt().is_none();
        let original_correlated = canonical.application_receipt().shares_outcome(&application)
            && original.application_receipt().shares_outcome(&application);
        let DesktopGameEditorEnumSource::Played {
            revision: accepted_revision,
        } = accepted.source()
        else {
            return Err("publication refusal lost played identity".into());
        };
        let mut statuses = fixture.games.snapshots();
        let status = async {
            loop {
                if let Some(snapshot) = statuses
                    .borrow()
                    .iter()
                    .find(|snapshot| snapshot.id() == before.game())
                    .filter(|snapshot| snapshot.revision() == accepted_revision)
                {
                    return Ok::<_, Box<dyn std::error::Error>>(snapshot.revision());
                }
                statuses.changed().await?;
            }
        };
        let status_revision = tokio::time::timeout(Self::OWNERSHIP_TIMEOUT, async {
            tokio::select! {
                result = status => result,
                result = Box::pin(fixture.registry.run()) => Err(format!("registry ended before source observation: {result:?}").into()),
            }
        })
        .await??;
        let reads_accepted = fixture.wall.calls.load(Ordering::SeqCst);
        let mut retry_aliases = Vec::new();
        if matches!(
            kind,
            DesktopLibraryServicePlayedRecoveryReviewEnumRetry::RepeatedFreshRetrySave
        ) {
            for _ in 0..2 {
                let mut alias = fixture.submit(DesktopLibraryServiceEnumAction::Editor {
                    expected: before,
                    action: DesktopAnnotationCommandsEnumAction::RetrySave,
                })?;
                let result = fixture.drive(&mut alias).await;
                retry_aliases.push(alias);
                drop(result);
            }
        }
        let mut foreign_refusals = true;
        for expected in foreign_contexts {
            let mut foreign = fixture.submit(DesktopLibraryServiceEnumAction::Editor {
                expected,
                action: DesktopAnnotationCommandsEnumAction::RetrySave,
            })?;
            let response = fixture.drive(&mut foreign).await;
            foreign_refusals &= matches!(foreign.try_result(), Some(Err(error))
                if error.disposition() == DesktopLibraryServiceEnumDisposition::Refused);
            foreign_refusals &= canonical.pending().is_some();
            drop((response, foreign));
        }
        if matches!(
            kind,
            DesktopLibraryServicePlayedRecoveryReviewEnumRetry::ForeignBindingThenRetry
        ) {
            let cancelled = fixture.receipt()?;
            cancelled.cancellation().request();
            let mut refused = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
                cancelled.clone(),
                DesktopLibraryServiceOperation::new(
                    fixture.workspace,
                    DesktopLibraryServiceEnumAction::Editor {
                        expected: before,
                        action: DesktopAnnotationCommandsEnumAction::RetrySave,
                    },
                ),
            ))?;
            let response = fixture.drive(&mut refused).await;
            foreign_refusals &= matches!(refused.try_result(), Some(Err(error))
                if matches!(error.as_ref(), DesktopLibraryServiceError::Cancelled)
                    && error.disposition() == DesktopLibraryServiceEnumDisposition::Refused);
            foreign_refusals &= canonical.pending().is_some();
            drop((cancelled, refused, response));
        }
        if matches!(
            kind,
            DesktopLibraryServicePlayedRecoveryReviewEnumRetry::CancelledOriginal
        ) {
            application.cancellation().request();
        }
        let cancellation_requested = application.is_cancellation_requested();
        drop(memory);
        fixture.wall.millis.store(9000, Ordering::SeqCst);
        let retry_application = match kind {
            DesktopLibraryServicePlayedRecoveryReviewEnumRetry::FreshRetrySave
            | DesktopLibraryServicePlayedRecoveryReviewEnumRetry::ForeignBindingThenRetry
            | DesktopLibraryServicePlayedRecoveryReviewEnumRetry::RepeatedFreshRetrySave => {
                fixture.receipt()?
            }
            DesktopLibraryServicePlayedRecoveryReviewEnumRetry::CancelledOriginal
            | DesktopLibraryServicePlayedRecoveryReviewEnumRetry::Original => application.clone(),
        };
        let independent_retry = retry_application.request_id() != application.request_id();
        let retry_operation = match kind {
            DesktopLibraryServicePlayedRecoveryReviewEnumRetry::FreshRetrySave
            | DesktopLibraryServicePlayedRecoveryReviewEnumRetry::ForeignBindingThenRetry
            | DesktopLibraryServicePlayedRecoveryReviewEnumRetry::RepeatedFreshRetrySave => {
                DesktopLibraryServiceOperation::new(
                    fixture.workspace,
                    DesktopLibraryServiceEnumAction::Editor {
                        expected: before,
                        action: DesktopAnnotationCommandsEnumAction::RetrySave,
                    },
                )
            }
            DesktopLibraryServicePlayedRecoveryReviewEnumRetry::CancelledOriginal
            | DesktopLibraryServicePlayedRecoveryReviewEnumRetry::Original => operation.clone(),
        };
        let mut retry = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
            retry_application.clone(),
            retry_operation,
        ))?;
        let retry_correlated = retry
            .application_receipt()
            .shares_outcome(&retry_application);
        let retry_response = fixture.drive(&mut retry).await;
        let original_result =
            DesktopLibraryServicePlayedRecoveryReviewSaved::observe(original.try_result());
        let retry_result =
            DesktopLibraryServicePlayedRecoveryReviewSaved::observe(retry.try_result());
        let retry_aliases_reconciled = retry_aliases.iter().all(|alias| {
            DesktopLibraryServicePlayedRecoveryReviewSaved::observe(alias.try_result())
                == retry_result
        });
        if canonical.pending().is_some() {
            let mut continuation =
                fixture
                    .games
                    .try_submit_editor(DesktopAnnotationCommands::new(
                        application.clone(),
                        before,
                        action,
                    ))?;
            tokio::time::timeout(Self::OWNERSHIP_TIMEOUT, async {
                tokio::select! {
                    result = continuation.wait() => result.map(drop).map_err(|error| -> Box<dyn std::error::Error> { error.into() }),
                    result = Box::pin(fixture.registry.run()) => Err(format!("registry ended before original cleanup: {result:?}").into()),
                }
            })
            .await??;
            drop(continuation);
        }
        let receipt = canonical
            .storage_receipt()
            .ok_or("original edit cleanup has no actual storage acknowledgement")?;
        let loaded = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
            .load(before.game())
            .await?
            .ok_or("original played record disappeared")?;
        let StorageLibraryEnumContent::Played(durable) = loaded.record().content() else {
            return Err("saved played edit changed content kind".into());
        };
        let durable_revision = durable.revision();
        let durable_storage_revision = loaded.content_revision();
        let durable_notes = durable
            .tree()
            .node(before.selected())?
            .annotations()
            .notes()
            .to_owned();
        let modified = loaded.modified().ok_or("saved edit has no UTC")?.value();
        let reads_after = fixture.wall.calls.load(Ordering::SeqCst);
        drop((
            loaded,
            canonical,
            opened,
            original,
            retry,
            statuses,
            first_result,
            retry_response,
            retry_aliases,
        ));
        let memory_observer = fixture.store.client();
        let budget = fixture.budget.clone();
        drop((application, retry_application, operation));
        fixture.close().await?;
        let released_memory = memory_observer
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?
            .num_permits();
        let released_capacity = budget.available();
        drop((memory_observer, budget));
        Ok(Self {
            pressure,
            publishing,
            original_correlated,
            retry_correlated,
            independent_retry,
            cancellation_requested,
            retained_disposition,
            foreign_refusals,
            retry_aliases_reconciled,
            before,
            accepted,
            status_revision,
            reads_before,
            reads_accepted,
            reads_after,
            original: original_result,
            retry: retry_result,
            canonical: receipt,
            durable_revision,
            durable_storage_revision,
            durable_notes,
            modified,
            released_memory,
            released_capacity,
        })
    }

    pub(super) fn assert_recovered(self) -> DesktopLibraryServiceTestResult {
        assert!(self.pressure && self.publishing);
        assert!(self.original_correlated && self.retry_correlated);
        assert!(
            self.retained_disposition && self.foreign_refusals && self.retry_aliases_reconciled
        );
        let DesktopGameEditorEnumSource::Played { revision: before } = self.before.source() else {
            return Err("opening did not expose played revision".into());
        };
        let accepted = before.next()?;
        assert_eq!(self.status_revision, accepted);
        assert_eq!(
            self.accepted.source(),
            DesktopGameEditorEnumSource::Played { revision: accepted }
        );
        assert_eq!(self.accepted.generation(), self.before.generation().next()?);
        assert_eq!(self.reads_accepted, self.reads_before + 1);
        assert_eq!(self.reads_after, self.reads_accepted);
        assert_eq!(self.durable_revision, accepted);
        assert_eq!(
            self.durable_storage_revision.value(),
            self.before.storage_revision().value() + 1
        );
        assert_eq!(self.durable_notes, Self::NOTES);
        assert_eq!(self.modified, 1000);
        assert_eq!(self.canonical.game_revision(), accepted);
        assert_eq!(
            self.canonical.storage_revision(),
            self.durable_storage_revision
        );
        assert_eq!(self.released_memory, StorageDocumentStore::MEMORY_BUDGET);
        assert_eq!(self.released_capacity, 1);
        assert!(
            self.retry.is_ok(),
            "service retry did not recover accepted publication before cleanup: {:?}",
            self.retry
        );
        assert!(
            self.original.is_ok(),
            "original service receipt was not reconciled before cleanup: {:?}",
            self.original
        );
        let original = self.original.map_err(std::io::Error::other)?;
        let retry = self.retry.map_err(std::io::Error::other)?;
        assert_eq!(original, retry);
        assert_eq!(original.receipt, self.canonical);
        assert_eq!(original.context.source(), self.accepted.source());
        assert_eq!(original.context.generation(), self.accepted.generation());
        assert_eq!(original.context.epoch(), self.before.epoch());
        assert_eq!(
            original.context.storage_revision(),
            self.durable_storage_revision
        );
        assert_eq!(original.notes, Self::NOTES);
        Ok(())
    }
}

#[tokio::test]
async fn fresh_retry_save_recovers_original_played_publication_and_reconciles_both_service_receipts()
-> DesktopLibraryServiceTestResult {
    let observation = DesktopLibraryServicePlayedRecoveryReviewObservation::capture(
        DesktopLibraryServicePlayedRecoveryReviewEnumRetry::FreshRetrySave,
    )
    .await?;
    assert!(observation.independent_retry);
    assert!(!observation.cancellation_requested);
    observation.assert_recovered()
}

#[tokio::test]
async fn cancelled_original_service_receipt_recovers_already_accepted_played_publication()
-> DesktopLibraryServiceTestResult {
    let observation = DesktopLibraryServicePlayedRecoveryReviewObservation::capture(
        DesktopLibraryServicePlayedRecoveryReviewEnumRetry::CancelledOriginal,
    )
    .await?;
    assert!(!observation.independent_retry);
    assert!(observation.cancellation_requested);
    observation.assert_recovered()
}

#[tokio::test]
async fn foreign_retry_bindings_and_cancelled_fresh_retry_preserve_the_original_played_continuation()
-> DesktopLibraryServiceTestResult {
    DesktopLibraryServicePlayedRecoveryReviewObservation::capture(
        DesktopLibraryServicePlayedRecoveryReviewEnumRetry::ForeignBindingThenRetry,
    )
    .await?
    .assert_recovered()
}

#[tokio::test]
async fn repeated_failed_retry_save_receipts_reconcile_with_the_original_played_save()
-> DesktopLibraryServiceTestResult {
    DesktopLibraryServicePlayedRecoveryReviewObservation::capture(
        DesktopLibraryServicePlayedRecoveryReviewEnumRetry::RepeatedFreshRetrySave,
    )
    .await?
    .assert_recovered()
}
