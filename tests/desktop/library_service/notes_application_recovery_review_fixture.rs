use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::OwnedSemaphorePermit;

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumPending,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::game_bridge::DesktopGameBridgeProvider;
use crate::desktop::game_editor::{
    DesktopGameEditorContext, DesktopGameEditorEnumSource, DesktopGameEditorPublication,
    DesktopGameEditorPublicationError,
};
use crate::desktop::game_supervisor::{
    DesktopGameSupervisorEditorReceipt, DesktopGameSupervisorError,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::tests::notes_application_recovery_review::application_fixture::DesktopLibraryServiceOpenApplicationRecoveryReviewFixture;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceEnumSaveReceipt,
    DesktopLibraryServiceError, DesktopLibraryServiceOperation, DesktopLibraryServiceReceipt,
};
use crate::desktop::messages::{DesktopCommand, DesktopMessagesEnumResult, DesktopRequest};
use crate::game::enums::GameRevision;
use crate::game::storage_repository::GameStorageRevision;
use crate::presentation::analysis::annotations::PresentationAnalysisAnnotations;
use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorIntent;
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::library_record::StorageLibraryEnumContent;
use crate::storage::library_repository::StorageLibraryRepository;

#[derive(Clone, Copy)]
pub(super) enum DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase {
    PresentationNotationSql,
    NotationSql,
    PlayedPublication,
}

impl DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase {
    fn is_notation(self) -> bool {
        matches!(self, Self::PresentationNotationSql | Self::NotationSql)
    }
}

#[derive(Debug)]
struct DesktopLibraryServiceNotesApplicationRecoveryReviewSaved {
    context: DesktopGameEditorContext,
    notes: String,
    receipt: DesktopLibraryServiceEnumSaveReceipt,
}

impl DesktopLibraryServiceNotesApplicationRecoveryReviewSaved {
    fn capture(result: &DesktopLibraryServiceEnumResult) -> Result<Self, String> {
        match result {
            DesktopLibraryServiceEnumResult::Saved { snapshot, receipt } => Ok(Self {
                context: snapshot.context(),
                notes: snapshot.annotations().notes().to_owned(),
                receipt: receipt.clone(),
            }),
            result @ (DesktopLibraryServiceEnumResult::Query(_)
            | DesktopLibraryServiceEnumResult::Metadata(_)
            | DesktopLibraryServiceEnumResult::Organized(_)
            | DesktopLibraryServiceEnumResult::Import(_)
            | DesktopLibraryServiceEnumResult::Exported(_)
            | DesktopLibraryServiceEnumResult::TrashPlan(_)
            | DesktopLibraryServiceEnumResult::Trashed(_)
            | DesktopLibraryServiceEnumResult::Restored(_)
            | DesktopLibraryServiceEnumResult::Opened(_)
            | DesktopLibraryServiceEnumResult::Editor(_)
            | DesktopLibraryServiceEnumResult::PendingSnapshot { .. }) => {
                Err(format!("expected actual Saved result, received {result:?}"))
            }
        }
    }

    fn service(receipt: &DesktopLibraryServiceReceipt) -> Result<Self, String> {
        match receipt.try_result() {
            Some(Ok(result)) => Self::capture(&result),
            result => Err(format!("service did not acknowledge Saved: {result:?}")),
        }
    }

    fn agrees(&self, other: &Self) -> bool {
        self.context == other.context
            && self.notes == other.notes
            && match (&self.receipt, &other.receipt) {
                (
                    DesktopLibraryServiceEnumSaveReceipt::Played(left),
                    DesktopLibraryServiceEnumSaveReceipt::Played(right),
                ) => left == right,
                (
                    DesktopLibraryServiceEnumSaveReceipt::Notation {
                        receipt: left,
                        covered: left_covered,
                    },
                    DesktopLibraryServiceEnumSaveReceipt::Notation {
                        receipt: right,
                        covered: right_covered,
                    },
                ) => Arc::ptr_eq(left, right) && left_covered == right_covered,
                (
                    DesktopLibraryServiceEnumSaveReceipt::Played(_),
                    DesktopLibraryServiceEnumSaveReceipt::Notation { .. },
                )
                | (
                    DesktopLibraryServiceEnumSaveReceipt::Notation { .. },
                    DesktopLibraryServiceEnumSaveReceipt::Played(_),
                ) => false,
            }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct DesktopLibraryServiceNotesApplicationRecoveryReviewDurable {
    revision: GameStorageRevision,
    notes: String,
    modified: Option<i64>,
}

struct DesktopLibraryServiceNotesApplicationRecoveryReviewPanel {
    intent: Result<Option<PresentationAnalysisEditorIntent>, String>,
    admitted: bool,
    attached: bool,
    completed: bool,
    clean: bool,
}

struct DesktopLibraryServiceNotesApplicationRecoveryReviewRetained {
    original_request: DesktopRequest,
    original: DesktopApplicationReceipt,
    service: DesktopLibraryServiceReceipt,
    error: Arc<DesktopLibraryServiceError>,
    lower: Option<DesktopGameSupervisorEditorReceipt>,
}

struct DesktopLibraryServiceNotesApplicationRecoveryReviewAttempt {
    certified: DesktopGameEditorContext,
    accepted: DesktopGameEditorContext,
    correlated: bool,
    fault_observed: bool,
    cached_before: bool,
    reads_accepted: usize,
    durable_failed: DesktopLibraryServiceNotesApplicationRecoveryReviewDurable,
    retry_id: DesktopRequestId,
    distinct: bool,
    application_saved: Result<DesktopLibraryServiceNotesApplicationRecoveryReviewSaved, String>,
    service_saved: Result<DesktopLibraryServiceNotesApplicationRecoveryReviewSaved, String>,
    original_saved_before_cleanup:
        Result<DesktopLibraryServiceNotesApplicationRecoveryReviewSaved, String>,
    panel: Option<DesktopLibraryServiceNotesApplicationRecoveryReviewPanel>,
}

pub(super) struct DesktopLibraryServiceNotesApplicationRecoveryReviewObservation {
    kind: DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase,
    before: DesktopGameEditorContext,
    attempt: DesktopLibraryServiceNotesApplicationRecoveryReviewAttempt,
    original_immutable: bool,
    cached_after: bool,
    lower_reconciled: bool,
    original_saved: Result<DesktopLibraryServiceNotesApplicationRecoveryReviewSaved, String>,
    cleanup_saved: Option<DesktopLibraryServiceNotesApplicationRecoveryReviewSaved>,
    durable_before: DesktopLibraryServiceNotesApplicationRecoveryReviewDurable,
    durable_after: DesktopLibraryServiceNotesApplicationRecoveryReviewDurable,
    current_after: DesktopGameEditorContext,
    reads_before: usize,
    reads_after: usize,
    released: Option<(usize, usize, usize)>,
}

impl DesktopLibraryServiceOpenApplicationRecoveryReviewFixture {
    fn notes_review_request(
        &self,
        action: DesktopLibraryServiceEnumAction,
    ) -> DesktopLibraryServiceTestResult<DesktopRequest> {
        Ok(self.application.request(DesktopCommand::library(
            DesktopLibraryServiceOperation::new(self.library.workspace, action),
        ))?)
    }

    async fn notes_review_execute(
        &mut self,
        action: DesktopLibraryServiceEnumAction,
    ) -> DesktopLibraryServiceTestResult<DesktopApplicationReceipt> {
        let request = self.notes_review_request(action)?;
        let application = self.application.try_submit(&request)?;
        self.drive_until(|| application.response().is_some())
            .await?;
        drop(self.observed.try_recv()?);
        Ok(application)
    }

    async fn notes_review_open(
        &mut self,
        before: DesktopGameEditorContext,
    ) -> DesktopLibraryServiceTestResult<DesktopGameEditorPublication> {
        let application = self
            .notes_review_execute(DesktopLibraryServiceEnumAction::Open {
                game: before.game(),
                storage_revision: before.storage_revision(),
            })
            .await?;
        match DesktopLibraryServiceNotesApplicationRecoveryReviewObservation::value(&application)? {
            DesktopLibraryServiceEnumResult::Opened(snapshot)
            | DesktopLibraryServiceEnumResult::PendingSnapshot { snapshot, .. } => {
                Ok(snapshot.clone())
            }
            result @ (DesktopLibraryServiceEnumResult::Query(_)
            | DesktopLibraryServiceEnumResult::Metadata(_)
            | DesktopLibraryServiceEnumResult::Organized(_)
            | DesktopLibraryServiceEnumResult::Import(_)
            | DesktopLibraryServiceEnumResult::Exported(_)
            | DesktopLibraryServiceEnumResult::TrashPlan(_)
            | DesktopLibraryServiceEnumResult::Trashed(_)
            | DesktopLibraryServiceEnumResult::Restored(_)
            | DesktopLibraryServiceEnumResult::Editor(_)
            | DesktopLibraryServiceEnumResult::Saved { .. }) => {
                Err(format!("Open did not return retained source: {result:?}").into())
            }
        }
    }

    async fn notes_review_durable(
        &self,
        expected: DesktopGameEditorContext,
    ) -> DesktopLibraryServiceTestResult<DesktopLibraryServiceNotesApplicationRecoveryReviewDurable>
    {
        let recovered =
            StorageLibraryRepository::new(self.library.store.client(), &self.library.executor)
                .load(expected.game())
                .await?
                .ok_or("missing actual SQLite source")?;
        let tree = match recovered.record().content() {
            StorageLibraryEnumContent::Played(session) => session.tree(),
            StorageLibraryEnumContent::Notation(game) => game.tree(),
        };
        Ok(DesktopLibraryServiceNotesApplicationRecoveryReviewDurable {
            revision: recovered.content_revision(),
            notes: tree
                .node(expected.selected())?
                .annotations()
                .notes()
                .to_owned(),
            modified: recovered.modified().map(|timestamp| timestamp.value()),
        })
    }

    async fn notes_review_cached(
        &mut self,
        request: &DesktopRequest,
        original: &DesktopApplicationReceipt,
        error: &Arc<DesktopLibraryServiceError>,
    ) -> DesktopLibraryServiceTestResult<bool> {
        let dispatches = self.dispatches.load(Ordering::SeqCst);
        let cached = self.application.try_submit(request)?;
        self.drive_until(|| cached.response().is_some()).await?;
        Ok(cached.shares_outcome(original)
            && self.dispatches.load(Ordering::SeqCst) == dispatches
            && self.observed.is_empty()
            && DesktopLibraryServiceNotesApplicationRecoveryReviewObservation::same_failure(
                &cached, error,
            ))
    }
}

impl DesktopLibraryServiceNotesApplicationRecoveryReviewObservation {
    const GAME: u128 = 4600;
    const NOTES: &str = "one accepted edit recovered by a fresh application action";
    const ACCEPTED_UTC: i64 = 1000;
    const RETRY_UTC: i64 = 9000;
    const SQL_FAILURE: &str = "controlled fresh application notes refusal";
    const INSTALL_TRIGGER: &str = "CREATE TRIGGER notes_application_recovery_refusal BEFORE UPDATE ON library_entries BEGIN SELECT RAISE(ABORT, 'controlled fresh application notes refusal'); END;";
    const REMOVE_TRIGGER: &str = "DROP TRIGGER IF EXISTS notes_application_recovery_refusal;";

    fn value(
        application: &DesktopApplicationReceipt,
    ) -> Result<&DesktopLibraryServiceEnumResult, String> {
        match application.response() {
            Some(DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::Library(value),
                revision,
            }) if *revision == value.revision() => Ok(value.as_ref()),
            response => Err(format!(
                "application did not deliver Library value: {response:?}"
            )),
        }
    }

    fn source_revision(context: DesktopGameEditorContext) -> GameRevision {
        match context.source() {
            DesktopGameEditorEnumSource::Played { revision }
            | DesktopGameEditorEnumSource::Notation { revision } => revision,
        }
    }

    fn same_failure(
        application: &DesktopApplicationReceipt,
        expected: &Arc<DesktopLibraryServiceError>,
    ) -> bool {
        matches!(application.response(), Some(DesktopMessagesEnumResult::LibraryFailure(error))
            if Arc::ptr_eq(error, expected))
    }

    pub(super) async fn capture(
        kind: DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase,
    ) -> DesktopLibraryServiceTestResult<Self> {
        let mut fixture = DesktopLibraryServiceOpenApplicationRecoveryReviewFixture::new().await?;
        let (game, revision) = if kind.is_notation() {
            (
                DesktopLibraryServiceFixture::id(Self::GAME)?,
                fixture.library.seed(Self::GAME, true).await?,
            )
        } else {
            let session = DesktopLibraryServiceOpenApplicationRecoveryReviewFixture::played()?;
            (session.id(), fixture.library.seed_played(&session).await?)
        };
        let opening = fixture
            .notes_review_execute(DesktopLibraryServiceEnumAction::Open {
                game,
                storage_revision: revision,
            })
            .await?;
        let DesktopLibraryServiceEnumResult::Opened(publication) = Self::value(&opening)? else {
            return Err("initial actual application did not open source".into());
        };
        let before = publication.context();
        let durable_before = fixture.notes_review_durable(before).await?;
        let reads_before = fixture.library.wall.calls.load(Ordering::SeqCst);
        let mut pressure = Vec::new();
        let attempt = Self::attempt(&mut fixture, kind, publication, &mut pressure).await;
        drop(pressure);
        let removed = if kind.is_notation() {
            fixture.library.sql(Self::REMOVE_TRIGGER).await
        } else {
            Ok(())
        };
        let cleanup = async {
            removed?;
            if attempt
                .as_ref()
                .is_ok_and(|(attempt, _)| attempt.application_saved.is_ok())
            {
                return Ok(None);
            }
            let retained = fixture.notes_review_open(before).await?;
            let expected = if kind.is_notation() {
                retained.context()
            } else {
                before
            };
            drop(retained);
            let application = fixture
                .notes_review_execute(DesktopLibraryServiceEnumAction::Editor {
                    expected,
                    action: DesktopAnnotationCommandsEnumAction::RetrySave,
                })
                .await?;
            Ok::<_, Box<dyn std::error::Error>>(Some(
                DesktopLibraryServiceNotesApplicationRecoveryReviewSaved::capture(Self::value(
                    &application,
                )?)?,
            ))
        }
        .await;
        let observation = async {
            let (attempt, retained) = attempt?;
            let cleanup_saved = cleanup?;
            let original_immutable = Self::same_failure(&retained.original, &retained.error);
            let cached_after = fixture
                .notes_review_cached(&retained.original_request, &retained.original, &retained.error)
                .await?;
            let original_saved =
                DesktopLibraryServiceNotesApplicationRecoveryReviewSaved::service(&retained.service);
            let lower_reconciled = retained.lower.as_ref().is_none_or(|lower| {
                lower.pending().is_none()
                    && lower.storage_receipt().is_some_and(|receipt| {
                        matches!(&original_saved, Ok(saved)
                            if matches!(&saved.receipt, DesktopLibraryServiceEnumSaveReceipt::Played(actual)
                                if *actual == receipt))
                    })
            });
            let current_after = fixture.notes_review_open(before).await?.context();
            let durable_after = fixture.notes_review_durable(before).await?;
            let reads_after = fixture.library.wall.calls.load(Ordering::SeqCst);
            Ok::<_, Box<dyn std::error::Error>>(Self {
                kind,
                before,
                attempt,
                original_immutable,
                cached_after,
                lower_reconciled,
                original_saved,
                cleanup_saved,
                durable_before,
                durable_after,
                current_after,
                reads_before,
                reads_after,
                released: None,
            })
        }
        .await;
        drop(opening);
        let released = fixture.close().await?;
        let mut observation = observation?;
        observation.released = Some(released);
        Ok(observation)
    }

    async fn attempt(
        fixture: &mut DesktopLibraryServiceOpenApplicationRecoveryReviewFixture,
        kind: DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase,
        publication: &DesktopGameEditorPublication,
        pressure: &mut Vec<OwnedSemaphorePermit>,
    ) -> DesktopLibraryServiceTestResult<(
        DesktopLibraryServiceNotesApplicationRecoveryReviewAttempt,
        DesktopLibraryServiceNotesApplicationRecoveryReviewRetained,
    )> {
        let before = publication.context();
        let original_request =
            fixture.notes_review_request(DesktopLibraryServiceEnumAction::Editor {
                expected: before,
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: Self::NOTES.to_owned(),
                    nags: Vec::new(),
                },
            })?;
        let mut panel = if matches!(
            kind,
            DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase::PresentationNotationSql
        ) {
            let mut panel = PresentationAnalysisAnnotations::new();
            panel.synchronize(publication, None);
            panel.set_notes(before, Self::NOTES.to_owned())?;
            let intent = panel
                .save_notes(original_request.request_id())?
                .ok_or("missing original presentation save intent")?;
            Some((panel, intent))
        } else {
            None
        };
        let positive_pressure = if kind.is_notation() {
            fixture.library.sql(Self::INSTALL_TRIGGER).await?;
            false
        } else {
            *pressure = fixture.library.fill_memory()?;
            pressure
                .iter()
                .map(OwnedSemaphorePermit::num_permits)
                .sum::<usize>()
                > 0
        };
        let original = fixture.application.try_submit(&original_request)?;
        if let Some((panel, intent)) = &mut panel {
            intent.clone().admit(original.clone())?;
            panel.attach_receipt(intent, original.clone())?;
        }
        fixture
            .drive_until(|| original.response().is_some())
            .await?;
        let service = fixture.observed.try_recv()?;
        let error = match original.response() {
            Some(DesktopMessagesEnumResult::LibraryFailure(error)) => Arc::clone(error),
            response => {
                return Err(
                    format!("missing original actual application failure: {response:?}").into(),
                );
            }
        };
        let certified = match error.disposition() {
            DesktopLibraryServiceEnumDisposition::Retained {
                expected: Some(expected),
                ..
            } => expected,
            disposition @ (DesktopLibraryServiceEnumDisposition::Refused
            | DesktopLibraryServiceEnumDisposition::Retained { .. }) => {
                return Err(format!("missing certified retained binding: {disposition:?}").into());
            }
        };
        let correlated = service.application_receipt().shares_outcome(&original)
            && service.request_id() == original_request.request_id()
            && matches!(service.try_result(), Some(Err(ref service_error)) if Arc::ptr_eq(service_error, &error))
            && error.disposition()
                == (DesktopLibraryServiceEnumDisposition::Retained {
                    request: original.request_id(),
                    expected: Some(before),
                });
        let lower = if kind.is_notation() {
            None
        } else {
            Some(
                fixture
                    .library
                    .service
                    .played
                    .get(&original.request_id())
                    .map(|played| played.receipt.clone())
                    .ok_or("missing original retained played receipt")?,
            )
        };
        let (accepted, fault_observed) = if let Some(lower) = &lower {
            let Some(DesktopAnnotationCommandsEnumPending::Publishing { context }) =
                lower.pending()
            else {
                return Err("played notes did not retain receiving publication".into());
            };
            let fault = positive_pressure
                && lower.application_receipt().shares_outcome(&original)
                && lower.storage_receipt().is_none()
                && matches!(lower.try_result(), Some(Err(error))
                    if matches!(error.as_ref(), DesktopGameSupervisorError::EditorPublicationPending { context: actual, source }
                        if *actual == context && matches!(source.as_ref(), DesktopGameEditorPublicationError::Memory(StorageDocumentError::Saturated))));
            let snapshots = fixture.library.games.snapshots();
            fixture
                .drive_until(|| {
                    snapshots.borrow().iter().any(|snapshot| {
                        snapshot.id() == before.game()
                            && snapshot.revision() == Self::source_revision(context)
                    })
                })
                .await?;
            let actual = fixture.snapshot(before.game())?;
            (
                context,
                fault && actual.revision() == Self::source_revision(context),
            )
        } else {
            let retained = fixture.notes_review_open(before).await?;
            (
                retained.context(),
                error.to_string().contains(Self::SQL_FAILURE),
            )
        };
        let reads_accepted = fixture.library.wall.calls.load(Ordering::SeqCst);
        let cached_before = fixture
            .notes_review_cached(&original_request, &original, &error)
            .await?;
        if let Some((panel, _)) = &mut panel {
            panel.observe_service_error(&original, before, &error)?;
        }
        pressure.clear();
        if kind.is_notation() {
            fixture.library.sql(Self::REMOVE_TRIGGER).await?;
        }
        let durable_failed = fixture.notes_review_durable(before).await?;
        fixture
            .library
            .wall
            .millis
            .store(Self::RETRY_UTC, Ordering::SeqCst);
        let retry_request =
            fixture.notes_review_request(DesktopLibraryServiceEnumAction::Editor {
                expected: certified,
                action: DesktopAnnotationCommandsEnumAction::RetrySave,
            })?;
        let panel_intent = panel.as_mut().map(|(panel, _)| {
            panel
                .retry_save(retry_request.request_id())
                .map_err(|error| error.to_string())
        });
        let retry = fixture.application.try_submit(&retry_request)?;
        let mut panel_observation = panel_intent.map(|intent| {
            let admitted = intent
                .as_ref()
                .ok()
                .and_then(|intent| intent.as_ref())
                .is_some_and(|intent| intent.clone().admit(retry.clone()).is_ok());
            let attached = match (&mut panel, &intent) {
                (Some((panel, _)), Ok(Some(intent))) => {
                    panel.attach_receipt(intent, retry.clone()).is_ok()
                }
                _ => false,
            };
            DesktopLibraryServiceNotesApplicationRecoveryReviewPanel {
                intent,
                admitted,
                attached,
                completed: false,
                clean: false,
            }
        });
        fixture.drive_until(|| retry.response().is_some()).await?;
        let retry_service = fixture.observed.try_recv()?;
        let distinct = retry.request_id() == retry_request.request_id()
            && retry.request_id() != original.request_id()
            && !retry.shares_outcome(&original)
            && retry_service.application_receipt().shares_outcome(&retry);
        if let (Some((panel, _)), Some(observation)) = (&mut panel, &mut panel_observation) {
            if let Ok(result) = Self::value(&retry) {
                observation.completed = observation.attached
                    && panel
                        .observe_service_result(&retry, certified, result)
                        .is_ok()
                    && panel.pending_intent().is_none();
            }
            observation.clean = !panel.has_notes_changes() && panel.notes() == Some(Self::NOTES);
        }
        let original_saved_before_cleanup =
            DesktopLibraryServiceNotesApplicationRecoveryReviewSaved::service(&service);
        Ok((
            DesktopLibraryServiceNotesApplicationRecoveryReviewAttempt {
                certified,
                accepted,
                correlated,
                fault_observed,
                cached_before,
                reads_accepted,
                durable_failed,
                retry_id: retry.request_id(),
                distinct,
                application_saved: Self::value(&retry)
                    .and_then(DesktopLibraryServiceNotesApplicationRecoveryReviewSaved::capture),
                service_saved: DesktopLibraryServiceNotesApplicationRecoveryReviewSaved::service(
                    &retry_service,
                ),
                original_saved_before_cleanup,
                panel: panel_observation,
            },
            DesktopLibraryServiceNotesApplicationRecoveryReviewRetained {
                original_request,
                original,
                service,
                error,
                lower,
            },
        ))
    }

    pub(super) fn assert_fresh_panel_intent(&self) -> DesktopLibraryServiceTestResult {
        let panel = self
            .attempt
            .panel
            .as_ref()
            .ok_or("presentation case did not exercise panel")?;
        let intent = panel
            .intent
            .as_ref()
            .map_err(|error| std::io::Error::other(error.clone()))?
            .as_ref()
            .ok_or("retained save did not emit a retry intent")?;
        assert_eq!(
            intent.request_id(),
            self.attempt.retry_id,
            "retained save must bind the fresh actual application request instead of returning the original SaveNotes intent"
        );
        assert_eq!(intent.expected(), self.attempt.certified);
        assert_eq!(
            intent.action(),
            &DesktopAnnotationCommandsEnumAction::RetrySave
        );
        assert!(
            panel.admitted && panel.attached,
            "fresh actual receipt must bind the dedicated retry intent"
        );
        assert!(
            panel.completed && panel.clean,
            "actual Saved receipt must finish the retained draft without another edit"
        );
        Ok(())
    }

    pub(super) fn assert_recovered(self) -> DesktopLibraryServiceTestResult {
        assert_eq!(
            self.released,
            Some((1, StorageDocumentStore::MEMORY_BUDGET, 1))
        );
        assert!(self.attempt.fault_observed && self.attempt.correlated);
        assert!(
            self.attempt.cached_before && self.cached_after && self.original_immutable,
            "original terminal application failure must stay immutable and resubmission must not dispatch again"
        );
        assert!(self.attempt.distinct && self.lower_reconciled);
        assert_eq!(self.attempt.certified, self.before);
        assert_eq!(
            Self::source_revision(self.attempt.accepted),
            Self::source_revision(self.before).next()?
        );
        assert_eq!(
            self.attempt.accepted.generation(),
            self.before.generation().next()?
        );
        assert_eq!(self.attempt.accepted.game(), self.before.game());
        assert_eq!(self.attempt.accepted.epoch(), self.before.epoch());
        assert_eq!(self.attempt.accepted.selected(), self.before.selected());
        assert_eq!(
            self.attempt.accepted.storage_revision(),
            self.before.storage_revision()
        );
        assert_eq!(self.durable_before, self.attempt.durable_failed);
        assert_eq!(
            (
                self.reads_before,
                self.attempt.reads_accepted,
                self.reads_after
            ),
            (0, 1, 1)
        );
        assert_eq!(self.durable_after.notes, Self::NOTES);
        assert_eq!(self.durable_after.modified, Some(Self::ACCEPTED_UTC));
        assert_eq!(
            self.durable_after.revision.value(),
            self.before.storage_revision().value() + 1
        );
        assert_eq!(self.current_after.source(), self.attempt.accepted.source());
        assert_eq!(
            self.current_after.generation(),
            self.attempt.accepted.generation()
        );
        assert_eq!(self.current_after.epoch(), self.before.epoch());
        assert_eq!(self.current_after.selected(), self.before.selected());
        assert_eq!(
            self.current_after.storage_revision(),
            self.durable_after.revision
        );
        let original = self.original_saved.map_err(std::io::Error::other)?;
        assert_eq!(original.context, self.current_after);
        if let Some(cleanup) = self.cleanup_saved {
            assert!(
                original.agrees(&cleanup),
                "baseline cleanup must save and reconcile the original retained source"
            );
        }
        assert!(
            self.attempt.application_saved.is_ok(),
            "fresh actual application RetrySave using the original retained error binding must return Saved before cleanup: {:?}",
            self.attempt.application_saved
        );
        let application = self
            .attempt
            .application_saved
            .map_err(std::io::Error::other)?;
        let service = self.attempt.service_saved.map_err(std::io::Error::other)?;
        let retained = self
            .attempt
            .original_saved_before_cleanup
            .map_err(std::io::Error::other)?;
        assert!(
            application.agrees(&service)
                && application.agrees(&retained)
                && application.agrees(&original),
            "new actual application request must receive the same actual Saved receipt as its service and original retained operation before cleanup"
        );
        match (self.kind.is_notation(), &application.receipt) {
            (true, DesktopLibraryServiceEnumSaveReceipt::Notation { receipt, covered }) => {
                assert_eq!(receipt.id(), self.before.game());
                assert_eq!(receipt.content_revision(), self.durable_after.revision);
                assert_eq!(*covered, Self::source_revision(self.attempt.accepted));
            }
            (false, DesktopLibraryServiceEnumSaveReceipt::Played(receipt)) => {
                assert_eq!(receipt.game(), self.before.game());
                assert_eq!(
                    receipt.game_revision(),
                    Self::source_revision(self.attempt.accepted)
                );
                assert_eq!(receipt.storage_revision(), self.durable_after.revision);
            }
            _ => return Err("actual Saved receipt has the wrong source kind".into()),
        }
        Ok(())
    }
}
