#[path = "coordinator_editor/fixture.rs"]
mod fixture;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chess::board::editor::{
    BoardEditorDraft, BoardEditorSessionEnumEdit, BoardEditorSessionEnumOperation,
};
use chess::board::enums::BoardEnumColor;
use chess::clock::duration::ClockInstant;
use chess::desktop::application::DesktopApplicationError;
use chess::desktop::application::DesktopApplicationReceipt;
use chess::desktop::dto::DesktopEnumValue;
use chess::desktop::editor_commands::{
    DesktopEditorCommandsEnumChoice, DesktopEditorCommandsEnumOperation,
    DesktopEditorCommandsEnumPrompt, DesktopEditorCommandsEnumResult, DesktopEditorCommandsError,
    DesktopEditorCommandsSnapshot,
};
use chess::desktop::error::DesktopEnumFailureCode;
use chess::desktop::identity::DesktopRevision;
use chess::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
};
use chess::storage::editor_repository::{
    StorageEditorRepositoryEnumMutation, StorageEditorRepositoryRevision,
};

use fixture::{
    DesktopCoordinatorEditorFixture, DesktopCoordinatorEditorFixtureDriver,
    DesktopCoordinatorEditorTestResult,
};

enum DesktopCoordinatorEditorRecoveryEnumDestination {
    Leave,
    MissingRead,
}

#[tokio::test]
async fn a_never_admitted_busy_editor_ticket_can_retry_after_newer_results_retire()
-> DesktopCoordinatorEditorTestResult {
    use chess::presentation::editor::application_port::PresentationEditorApplicationPort;
    use chess::presentation::editor::controller::PresentationEditorController;

    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let mut occupied = Vec::new();
    for _ in 0..256 {
        occupied.push(fixture.submit(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?);
    }
    let port = PresentationEditorApplicationPort::new(fixture.owner, &fixture.port);
    let mut controller = PresentationEditorController::new(Some(initial.clone()));
    let busy = controller
        .submit(
            DesktopEditorCommandsEnumOperation::Apply {
                expected: initial.context(),
                operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Turn(
                    BoardEnumColor::Black,
                )),
            },
            &port,
        )
        .is_err();
    let original = controller
        .pending_request()
        .ok_or("missing unadmitted request")?
        .request_id();
    let unadmitted = controller.receipt().is_none();
    let last = occupied.last().ok_or("missing full queue")?;
    driver.response(&fixture, last).await?;
    let newer = fixture.submit(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
    driver.response(&fixture, &newer).await?;
    fixture.clock.set(600_000_000_100);
    let retirement = fixture.submit(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
    driver.response(&fixture, &retirement).await?;
    let retry = controller.retry(&port);
    let same_request = controller
        .pending_request()
        .is_some_and(|request| request.request_id() == original);
    let applied = if retry.is_ok() {
        let receipt = controller
            .receipt()
            .ok_or("missing accepted retry")?
            .clone();
        let response = driver.response(&fixture, &receipt).await?;
        let actual = DesktopCoordinatorEditorFixture::snapshot(response)?;
        controller.accept(receipt.request_id())?;
        controller.snapshot().is_some_and(|snapshot| {
            snapshot.context() == actual.context()
                && snapshot.draft().fields().turn() == BoardEnumColor::Black
        })
    } else {
        false
    };
    drop(controller);
    fixture.close(driver, None).await?;
    assert!(busy && unadmitted && same_request);
    assert!(
        retry.is_ok(),
        "a never-admitted request became permanently unresubmittable: {retry:?}"
    );
    assert!(applied);
    Ok(())
}

#[tokio::test]
async fn controller_cannot_install_an_unpublished_intermediate_editor_snapshot()
-> DesktopCoordinatorEditorTestResult {
    use chess::presentation::editor::application_port::PresentationEditorApplicationPort;
    use chess::presentation::editor::controller::PresentationEditorController;

    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let port = PresentationEditorApplicationPort::new(fixture.owner, &fixture.port);
    let mut controller = PresentationEditorController::new(Some(initial.clone()));
    controller.submit(DesktopEditorCommandsEnumOperation::Snapshot, &port)?;
    let receipt = controller
        .receipt()
        .ok_or("missing snapshot receipt")?
        .clone();
    let premature = controller.accept(receipt.request_id());
    let before_publication = controller.snapshot().map(|snapshot| snapshot.context());
    driver.response(&fixture, &receipt).await?;
    drop(controller);
    fixture.close(driver, None).await?;
    assert!(
        premature.is_err(),
        "controller accepted a value with no actual application publication"
    );
    assert_eq!(before_publication, Some(initial.context()));
    Ok(())
}

#[tokio::test]
async fn controller_uses_the_actual_retained_receipt_payload_after_an_editor_edit()
-> DesktopCoordinatorEditorTestResult {
    use chess::presentation::editor::application_port::PresentationEditorApplicationPort;
    use chess::presentation::editor::controller::PresentationEditorController;

    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let port = PresentationEditorApplicationPort::new(fixture.owner, &fixture.port);
    let mut controller = PresentationEditorController::new(Some(initial.clone()));
    controller.submit(
        DesktopEditorCommandsEnumOperation::Apply {
            expected: initial.context(),
            operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Turn(
                BoardEnumColor::Black,
            )),
        },
        &port,
    )?;
    let receipt = controller.receipt().ok_or("missing edit receipt")?.clone();
    let actual =
        DesktopCoordinatorEditorFixture::snapshot(driver.response(&fixture, &receipt).await?)?;
    controller.accept(receipt.request_id())?;
    let installed = controller.snapshot().map(|snapshot| snapshot.context());
    drop(controller);
    fixture.close(driver, None).await?;
    assert_eq!(
        installed,
        Some(actual.context()),
        "controller substituted an older genuine snapshot for the actual completed edit"
    );
    Ok(())
}

struct DesktopCoordinatorEditorFailedTransition {
    submitted: DesktopEditorCommandsSnapshot,
    application: DesktopApplicationReceipt,
    failure: Arc<DesktopEditorCommandsError>,
}

impl DesktopCoordinatorEditorFixture {
    async fn failed_transition(
        &self,
        driver: &mut DesktopCoordinatorEditorFixtureDriver,
        destination: DesktopCoordinatorEditorRecoveryEnumDestination,
    ) -> DesktopCoordinatorEditorTestResult<DesktopCoordinatorEditorFailedTransition> {
        let initial = self.create(driver).await?;
        let seed = self.submit(DesktopCommand::editor(
            self.owner,
            DesktopEditorCommandsEnumOperation::Save {
                expected: initial.context(),
            },
        ))?;
        driver.response(self, &seed).await?;
        let changed = self.submit(DesktopCommand::editor(
            self.owner,
            DesktopEditorCommandsEnumOperation::Apply {
                expected: initial.context(),
                operation: BoardEditorSessionEnumOperation::Edit(
                    BoardEditorSessionEnumEdit::Counters {
                        halfmoves: 17,
                        fullmoves: 83,
                    },
                ),
            },
        ))?;
        let submitted = Self::snapshot(driver.response(self, &changed).await?)?;
        let destination = match destination {
            DesktopCoordinatorEditorRecoveryEnumDestination::Leave => {
                DesktopEditorCommandsEnumOperation::Leave {
                    expected: submitted.context(),
                }
            }
            DesktopCoordinatorEditorRecoveryEnumDestination::MissingRead => {
                DesktopEditorCommandsEnumOperation::Read {
                    opening: Self::opening(2)?,
                    expected: Some(submitted.context()),
                }
            }
        };
        let prompt = self.submit(DesktopCommand::editor(self.owner, destination))?;
        driver.response(self, &prompt).await?;
        let connection = rusqlite::Connection::open(&self.path)?;
        connection.execute_batch("CREATE TRIGGER fail_editor_transition AFTER UPDATE ON editor_drafts BEGIN SELECT RAISE(ROLLBACK, 'editor transition rollback'); END")?;
        let application = self.submit(DesktopCommand::editor(
            self.owner,
            DesktopEditorCommandsEnumOperation::Resolve {
                expected: submitted.context(),
                choice: DesktopEditorCommandsEnumChoice::Save,
            },
        ))?;
        let result = driver.response(self, &application).await?;
        connection.execute_batch("DROP TRIGGER fail_editor_transition")?;
        drop(connection);
        let DesktopMessagesEnumResult::EditorFailure(failure) = result else {
            return Err("the original transition save did not fail".into());
        };
        Ok(DesktopCoordinatorEditorFailedTransition {
            submitted,
            application,
            failure,
        })
    }
}

#[tokio::test]
async fn canceled_fresh_retry_observes_save_without_executing_the_original_leave()
-> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let original = fixture
        .failed_transition(
            &mut driver,
            DesktopCoordinatorEditorRecoveryEnumDestination::Leave,
        )
        .await?;
    let blocker = rusqlite::Connection::open(&fixture.path)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    let retry = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Save {
            expected: original.submitted.context(),
        },
    ))?;
    driver.poll_pending().await?;
    let stop = fixture.submit(DesktopCommand::new(
        DesktopEnumCommand::Stop {
            owner: fixture.owner,
        },
        None,
    ))?;
    driver.poll_pending().await?;
    let canceled = retry.is_cancellation_requested();
    let original_canceled = original.application.is_cancellation_requested();
    blocker.execute_batch("ROLLBACK")?;
    drop(blocker);
    let response = driver.response(&fixture, &retry).await?;
    let stopped = driver.response(&fixture, &stop).await?;
    let snapshot = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Snapshot,
    ))?;
    let snapshot = driver.response(&fixture, &snapshot).await?;
    let recovered = fixture
        .repository
        .read(original.submitted.context().draft())
        .await?;
    let durable = recovered.as_ref().map(|record| record.draft().clone());
    drop(recovered);
    let original_preserved = matches!(original.application.response(), Some(DesktopMessagesEnumResult::EditorFailure(error)) if Arc::ptr_eq(error, &original.failure));
    fixture.close(driver, None).await?;
    assert!(canceled);
    assert!(!original_canceled);
    assert!(original_preserved);
    assert_eq!(durable.as_ref(), Some(original.submitted.draft()));
    assert!(matches!(
        stopped,
        DesktopMessagesEnumResult::ReconciliationRequired
    ));
    assert!(
        matches!(response, DesktopMessagesEnumResult::Value { value: DesktopEnumValue::Editor(value), .. }
        if matches!(*value, DesktopEditorCommandsEnumResult::Written { snapshot: Some(ref snapshot), .. }
            if snapshot.context() == original.submitted.context() && snapshot.prompt() == Some(DesktopEditorCommandsEnumPrompt::Leave) && !snapshot.is_dirty())),
        "canceling the actual fresh retry still executed the original Leave destination"
    );
    assert!(
        matches!(snapshot, DesktopMessagesEnumResult::Value { value: DesktopEnumValue::Editor(value), .. }
        if matches!(*value, DesktopEditorCommandsEnumResult::Snapshot(Some(ref snapshot)) if snapshot.context() == original.submitted.context()))
    );
    Ok(())
}

#[tokio::test]
async fn retry_quit_observes_a_previously_failed_save_without_reading_its_destination()
-> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let original = fixture
        .failed_transition(
            &mut driver,
            DesktopCoordinatorEditorRecoveryEnumDestination::MissingRead,
        )
        .await?;
    let quit = fixture.submit(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    driver.poll_pending().await?;
    let original_canceled = original.application.is_cancellation_requested();
    fixture.port.retry_quit()?;
    driver.poll_pending().await?;
    let recovered = fixture
        .repository
        .read(original.submitted.context().draft())
        .await?;
    let durable = recovered.as_ref().map(|record| record.draft().clone());
    drop(recovered);
    let shutdown = Arc::clone(&fixture.shutdown);
    let finished = driver.retire_after_storage_barrier().await?;
    let quit_result = quit.response().cloned();
    let original_preserved = matches!(original.application.response(), Some(DesktopMessagesEnumResult::EditorFailure(error)) if Arc::ptr_eq(error, &original.failure));
    fixture.close_storage().await?;
    assert!(!original_canceled);
    assert!(original_preserved);
    assert_eq!(durable.as_ref(), Some(original.submitted.draft()));
    assert!(
        finished,
        "shutdown tried to execute the old pending destination after saving"
    );
    assert_eq!(shutdown.load(Ordering::Acquire), 1);
    assert!(matches!(
        quit_result,
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    Ok(())
}

#[tokio::test]
async fn retry_quit_recovers_canceled_editor_write_after_confirmed_rollback()
-> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let seed = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Save {
            expected: initial.context(),
        },
    ))?;
    driver.response(&fixture, &seed).await?;
    let changed = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Apply {
            expected: initial.context(),
            operation: BoardEditorSessionEnumOperation::Edit(
                BoardEditorSessionEnumEdit::Counters {
                    halfmoves: 17,
                    fullmoves: 83,
                },
            ),
        },
    ))?;
    let submitted =
        DesktopCoordinatorEditorFixture::snapshot(driver.response(&fixture, &changed).await?)?;
    let blocker = rusqlite::Connection::open(&fixture.path)?;
    blocker.execute_batch(
        "CREATE TRIGGER fail_editor_update AFTER UPDATE ON editor_drafts BEGIN SELECT RAISE(ROLLBACK, 'editor quit rollback'); END; BEGIN IMMEDIATE",
    )?;
    let save = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Save {
            expected: submitted.context(),
        },
    ))?;
    driver.poll_pending().await?;
    let quit = fixture.submit(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    driver.poll_pending().await?;
    let canceled = save.is_cancellation_requested();
    let shutdown_before_failure = fixture.shutdown.load(Ordering::Acquire);
    blocker.execute_batch("ROLLBACK")?;
    let failure = driver.response(&fixture, &save).await?;
    blocker.execute_batch("DROP TRIGGER fail_editor_update")?;
    drop(blocker);
    let before_retry = fixture
        .repository
        .read(initial.context().draft())
        .await?
        .ok_or("seed disappeared after rollback")?;
    let durable_before_retry = before_retry.draft().clone();
    let revision_before_retry = before_retry.storage_revision();
    drop(before_retry);
    fixture.port.retry_quit()?;
    driver.poll_pending().await?;
    let after_retry = fixture.repository.read(initial.context().draft()).await?;
    let recovered = after_retry
        .as_ref()
        .map(|recovered| (recovered.draft().clone(), recovered.storage_revision()));
    drop(after_retry);
    let shutdown = Arc::clone(&fixture.shutdown);
    let finished = driver.retire_after_storage_barrier().await?;
    let quit_response = quit.response().cloned();
    let original_response_preserved = matches!(
        (save.response(), &failure),
        (Some(DesktopMessagesEnumResult::EditorFailure(actual)), DesktopMessagesEnumResult::EditorFailure(original))
            if Arc::ptr_eq(actual, original)
    );
    fixture.close_storage().await?;
    assert!(canceled);
    assert_eq!(shutdown_before_failure, 0);
    assert!(
        matches!(failure, DesktopMessagesEnumResult::EditorFailure(error)
        if matches!(error.as_ref(), DesktopEditorCommandsError::WriteFailed(_)))
    );
    assert_eq!(&durable_before_retry, initial.draft());
    assert!(original_response_preserved);
    assert!(
        finished,
        "Retry Quit left the confirmed rolled-back editor write permanently retained"
    );
    assert_eq!(shutdown.load(Ordering::Acquire), 1);
    assert!(matches!(
        quit_response,
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    assert_eq!(
        recovered,
        Some((
            submitted.draft().clone(),
            StorageEditorRepositoryRevision::new(revision_before_retry.value() + 1)
        ))
    );
    Ok(())
}

#[tokio::test]
async fn routed_typed_failure_retires_only_its_controller_request_and_allows_authoritative_refresh()
-> DesktopCoordinatorEditorTestResult {
    use chess::presentation::editor::application_port::PresentationEditorApplicationPort;
    use chess::presentation::editor::controller::PresentationEditorController;

    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let change = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Apply {
            expected: initial.context(),
            operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Turn(
                BoardEnumColor::Black,
            )),
        },
    ))?;
    let changed =
        DesktopCoordinatorEditorFixture::snapshot(driver.response(&fixture, &change).await?)?;
    let (typed, acknowledged, pending, refreshed) = {
        let port = PresentationEditorApplicationPort::new(fixture.owner, &fixture.port);
        let mut controller = PresentationEditorController::new(Some(initial.clone()));
        controller.submit(
            DesktopEditorCommandsEnumOperation::Save {
                expected: initial.context(),
            },
            &port,
        )?;
        let receipt = controller
            .receipt()
            .ok_or("missing routed controller receipt")?
            .clone();
        let response = driver.response(&fixture, &receipt).await?;
        let typed = matches!(response, DesktopMessagesEnumResult::EditorFailure(error) if matches!(error.as_ref(), DesktopEditorCommandsError::StaleContext));
        let acknowledged = controller.acknowledge_failure(receipt.request_id()).is_ok();
        let pending = controller.is_pending();
        let refreshed = if acknowledged {
            controller.submit(DesktopEditorCommandsEnumOperation::Snapshot, &port)?;
            let refresh = controller
                .receipt()
                .ok_or("missing refresh receipt")?
                .clone();
            let DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::Editor(_),
                ..
            } = driver.response(&fixture, &refresh).await?
            else {
                return Err("missing authoritative editor refresh".into());
            };
            controller.accept(refresh.request_id())?;
            controller.snapshot().map(|snapshot| snapshot.context())
        } else {
            None
        };
        (typed, acknowledged, pending, refreshed)
    };
    fixture.close(driver, None).await?;
    assert!(typed);
    assert!(
        acknowledged,
        "the controller refused its actual routed typed error"
    );
    assert!(!pending);
    assert_eq!(refreshed, Some(changed.context()));
    Ok(())
}

#[tokio::test]
async fn initial_create_uses_revision_zero_and_the_stable_workspace_owner()
-> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    fixture.clock.set(123);
    let opening = DesktopCoordinatorEditorFixture::opening(1)?;
    let request = fixture.request(DesktopEditorCommandsEnumOperation::Create {
        opening,
        selected: None,
        expected: None,
    })?;
    let expected_revision = request.command().expected_revision();
    let mutation_owner = request.command().operation().mutation_entity();
    let receipt = fixture.port.try_submit(&request)?;
    let response = driver.response(&fixture, &receipt).await?;
    let revision = if let DesktopMessagesEnumResult::Value { revision, .. } = &response {
        *revision
    } else {
        return Err("Create did not publish a value".into());
    };
    let snapshot = DesktopCoordinatorEditorFixture::snapshot(response)?;
    let owner = fixture.owner;
    let received_at = receipt.received_at();
    let quit = fixture.close(driver, None).await?;
    assert_eq!(expected_revision, Some(DesktopRevision::ZERO));
    assert_eq!(revision, Some(DesktopRevision::ZERO));
    assert_eq!(mutation_owner, Some(owner));
    assert_ne!(owner, DesktopMessagesEntityId::for_editor(opening.draft()));
    assert_eq!(snapshot.context().draft(), opening.draft());
    assert_eq!(snapshot.context().session(), opening.session());
    assert_eq!(
        DesktopRevision::for_editor(snapshot.context().revision()),
        DesktopRevision::ZERO
    );
    assert_eq!(received_at, ClockInstant::from_nanos(123));
    assert!(matches!(quit, DesktopMessagesEnumResult::QuitReady));
    Ok(())
}

#[tokio::test]
async fn mismatched_native_revision_and_wrong_workspace_are_refused_before_mutation()
-> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let operation = DesktopEditorCommandsEnumOperation::Save {
        expected: initial.context(),
    };
    let stale = fixture.submit(DesktopCommand::new(
        DesktopEnumCommand::Editor {
            owner: fixture.owner,
            operation: operation.clone(),
        },
        Some(DesktopRevision::ZERO.next()?),
    ))?;
    let stale = driver.response(&fixture, &stale).await?;
    let wrong = fixture.submit(DesktopCommand::editor(
        DesktopMessagesEntityId::new(),
        operation,
    ))?;
    let wrong = driver.response(&fixture, &wrong).await?;
    let snapshot = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Snapshot,
    ))?;
    let snapshot =
        DesktopCoordinatorEditorFixture::snapshot(driver.response(&fixture, &snapshot).await?)?;
    let entries = fixture.repository.list(None).await?.entries().to_vec();
    fixture.close(driver, None).await?;
    assert!(
        matches!(stale, DesktopMessagesEnumResult::Failure(error) if matches!(error.code(), DesktopEnumFailureCode::StaleRevision))
    );
    assert!(
        matches!(wrong, DesktopMessagesEnumResult::Failure(error) if matches!(error.code(), DesktopEnumFailureCode::InvalidInput))
    );
    assert_eq!(snapshot.context(), initial.context());
    assert!(entries.is_empty());
    Ok(())
}

#[tokio::test]
async fn stale_domain_context_keeps_its_typed_failure_and_current_draft()
-> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let operation = DesktopEditorCommandsEnumOperation::Apply {
        expected: initial.context(),
        operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Turn(
            BoardEnumColor::Black,
        )),
    };
    let changed = fixture.submit(DesktopCommand::editor(fixture.owner, operation.clone()))?;
    let changed =
        DesktopCoordinatorEditorFixture::snapshot(driver.response(&fixture, &changed).await?)?;
    let stale = fixture.submit(DesktopCommand::editor(fixture.owner, operation))?;
    let stale = driver.response(&fixture, &stale).await?;
    let snapshot = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Snapshot,
    ))?;
    let snapshot =
        DesktopCoordinatorEditorFixture::snapshot(driver.response(&fixture, &snapshot).await?)?;
    fixture.close(driver, None).await?;
    assert!(
        matches!(stale, DesktopMessagesEnumResult::EditorFailure(error) if matches!(error.as_ref(), DesktopEditorCommandsError::StaleContext))
    );
    assert_ne!(changed.context().revision(), initial.context().revision());
    assert_eq!(snapshot.context(), changed.context());
    assert_eq!(snapshot.draft().fields().turn(), BoardEnumColor::Black);
    Ok(())
}

#[tokio::test]
async fn save_waits_for_sqlite_ack_once_while_bootstrap_remains_responsive()
-> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let blocker = rusqlite::Connection::open(&fixture.path)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    let request = fixture.request(DesktopEditorCommandsEnumOperation::Save {
        expected: initial.context(),
    })?;
    let save = fixture.port.try_submit(&request)?;
    let duplicate = fixture.port.try_submit(&request.clone())?;
    driver.poll_pending().await?;
    let before_ack = save.response();
    let competing = fixture.port.request(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Leave {
            expected: initial.context(),
        },
    ))?;
    let refused = fixture.port.try_submit(&competing);
    let bootstrap = fixture.submit(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
    driver.poll_pending().await?;
    let bootstrap = bootstrap.response();
    let still_pending = save.response().is_none();
    blocker.execute_batch("ROLLBACK")?;
    drop(blocker);
    let result = driver.response(&fixture, &save).await?;
    let repeated = fixture.port.try_submit(&request)?;
    let recovered = fixture
        .repository
        .read(initial.context().draft())
        .await?
        .ok_or("missing actual save")?;
    let storage_revision = recovered.storage_revision();
    let saved_draft = recovered.draft().clone();
    drop(recovered);
    fixture.close(driver, None).await?;
    assert!(before_ack.is_none());
    assert!(still_pending);
    assert!(matches!(refused, Err(DesktopApplicationError::EntityBusy)));
    assert!(matches!(
        bootstrap,
        Some(DesktopMessagesEnumResult::Value {
            value: DesktopEnumValue::Bootstrap(_),
            ..
        })
    ));
    assert!(save.shares_outcome(&duplicate));
    assert!(save.shares_outcome(&repeated));
    assert_eq!(save.request_id(), request.request_id());
    assert_eq!(storage_revision.value(), 1);
    assert_eq!(&saved_draft, initial.draft());
    assert!(
        matches!(result, DesktopMessagesEnumResult::Value { value: DesktopEnumValue::Editor(value), .. } if matches!(*value, DesktopEditorCommandsEnumResult::Written { receipt, snapshot: Some(ref snapshot) } if receipt.id() == initial.context().draft() && receipt.storage_revision() == storage_revision && !snapshot.is_dirty()))
    );
    Ok(())
}

#[tokio::test]
async fn canceled_accepted_save_is_observed_with_its_original_acknowledgement()
-> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let blocker = rusqlite::Connection::open(&fixture.path)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    let request = fixture.request(DesktopEditorCommandsEnumOperation::Save {
        expected: initial.context(),
    })?;
    let save = fixture.port.try_submit(&request)?;
    driver.poll_pending().await?;
    let stop = fixture.submit(DesktopCommand::new(
        DesktopEnumCommand::Stop {
            owner: fixture.owner,
        },
        None,
    ))?;
    driver.poll_pending().await?;
    let canceled = save.is_cancellation_requested();
    let waiting = save.response().is_none() && stop.response().is_none();
    blocker.execute_batch("ROLLBACK")?;
    drop(blocker);
    let saved = driver.response(&fixture, &save).await?;
    let stopped = driver.response(&fixture, &stop).await?;
    let snapshot = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Snapshot,
    ))?;
    let snapshot =
        DesktopCoordinatorEditorFixture::snapshot(driver.response(&fixture, &snapshot).await?)?;
    let recovered = fixture
        .repository
        .read(initial.context().draft())
        .await?
        .ok_or("canceled accepted save disappeared")?;
    let storage_revision = recovered.storage_revision();
    drop(recovered);
    fixture.close(driver, None).await?;
    assert!(canceled);
    assert!(waiting);
    assert!(matches!(
        stopped,
        DesktopMessagesEnumResult::ReconciliationRequired
    ));
    assert_eq!(snapshot.context(), initial.context());
    assert_eq!(snapshot.storage_revision(), storage_revision);
    assert!(!snapshot.is_dirty());
    assert!(
        matches!(saved, DesktopMessagesEnumResult::Value { value: DesktopEnumValue::Editor(value), .. } if matches!(*value, DesktopEditorCommandsEnumResult::Written { receipt, .. } if receipt.id() == initial.context().draft() && matches!(receipt.mutation(), StorageEditorRepositoryEnumMutation::Saved { draft_revision, .. } if draft_revision == initial.context().revision())))
    );
    Ok(())
}

#[tokio::test]
async fn pending_save_prevents_quit_cleanup_until_the_actual_write_is_observed()
-> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let initial = fixture.create(&mut driver).await?;
    let blocker = rusqlite::Connection::open(&fixture.path)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    let save = fixture.submit(DesktopCommand::editor(
        fixture.owner,
        DesktopEditorCommandsEnumOperation::Save {
            expected: initial.context(),
        },
    ))?;
    driver.poll_pending().await?;
    let quit = fixture.submit(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    driver.poll_pending().await?;
    let waiting = quit.response().is_none() && save.response().is_none();
    let shutdown_before_ack = fixture.shutdown.load(Ordering::Acquire);
    let shutdown = Arc::clone(&fixture.shutdown);
    blocker.execute_batch("ROLLBACK")?;
    drop(blocker);
    let quit_result = fixture.close(driver, Some(quit)).await?;
    let saved = save.response().cloned();
    assert!(waiting);
    assert_eq!(shutdown_before_ack, 0);
    assert_eq!(shutdown.load(Ordering::Acquire), 1);
    assert!(matches!(quit_result, DesktopMessagesEnumResult::QuitReady));
    assert!(
        matches!(saved, Some(DesktopMessagesEnumResult::Value { value: DesktopEnumValue::Editor(value), .. }) if matches!(*value, DesktopEditorCommandsEnumResult::Written { receipt, .. } if receipt.id() == initial.context().draft()))
    );
    Ok(())
}

async fn cancellation_after_storage_wait(
    operation: DesktopEditorCommandsEnumOperation,
) -> DesktopCoordinatorEditorTestResult {
    let (fixture, mut driver) = DesktopCoordinatorEditorFixture::new().await?;
    let draft = BoardEditorDraft::new(DesktopCoordinatorEditorFixture::id(1)?, None);
    let blocker = rusqlite::Connection::open(&fixture.path)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    let mut external_write = fixture
        .repository
        .save(&draft, StorageEditorRepositoryRevision::ABSENT)?;
    let request = fixture.request(operation)?;
    let awaited = fixture.port.try_submit(&request)?;
    driver.poll_pending().await?;
    let before_cancel = awaited.response();
    let quit = fixture.submit(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    driver.poll_pending().await?;
    let canceled = awaited.is_cancellation_requested();
    let before_result = awaited.response();
    blocker.execute_batch("ROLLBACK")?;
    drop(blocker);
    external_write.wait().await?;
    fixture.close(driver, Some(quit)).await?;
    let result = awaited.response().cloned();
    assert!(before_cancel.is_none());
    assert!(canceled);
    assert!(before_result.is_none());
    assert!(
        matches!(result, Some(DesktopMessagesEnumResult::EditorFailure(error)) if matches!(error.as_ref(), DesktopEditorCommandsError::Cancelled))
    );
    Ok(())
}

#[tokio::test]
async fn list_checks_actual_receipt_cancellation_after_the_awaited_page()
-> DesktopCoordinatorEditorTestResult {
    cancellation_after_storage_wait(DesktopEditorCommandsEnumOperation::List { after: None }).await
}

#[tokio::test]
async fn read_checks_actual_receipt_cancellation_after_the_awaited_row()
-> DesktopCoordinatorEditorTestResult {
    cancellation_after_storage_wait(DesktopEditorCommandsEnumOperation::Read {
        opening: DesktopCoordinatorEditorFixture::opening(1)?,
        expected: None,
    })
    .await
}
