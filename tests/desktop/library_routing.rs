use chess::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction;
use chess::desktop::application::DesktopApplicationError;
use chess::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use chess::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceOperation,
};
use chess::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
};

#[path = "coordinator_library.rs"]
mod coordinator;
#[path = "coordinator_library/fixture.rs"]
mod fixture;

use self::fixture::{DesktopLibraryRoutingFixture, DesktopLibraryRoutingTestResult};

#[test]
fn oversized_library_payload_is_refused_before_application_retention()
-> DesktopLibraryRoutingTestResult {
    let application = DesktopLibraryRoutingFixture::application();
    let port = application.port();
    let request = port.request(DesktopCommand::library(
        DesktopLibraryServiceOperation::new(
            DesktopMessagesEntityId::new(),
            DesktopLibraryServiceEnumAction::Editor {
                expected: DesktopLibraryRoutingFixture::context(1)?,
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "x".repeat(
                        chess::storage::document_store::StorageDocumentStore::MEMORY_BUDGET + 1,
                    ),
                    nags: Vec::new(),
                },
            },
        ),
    ))?;
    let admitted = port.try_submit(&request);
    let refused = admitted.is_err();
    let quit =
        port.try_submit(&port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?)?;
    let work = application.try_next()?.ok_or("missing reserved cleanup")?;
    application.complete_control(&work)?;
    let closed = matches!(quit.response(), Some(DesktopMessagesEnumResult::QuitReady));
    let drained = application.try_next()?.is_none();
    drop((admitted, request, quit, work, port, application));
    assert!(closed && drained);
    assert!(
        refused,
        "an over-budget Library payload entered application retention"
    );
    Ok(())
}

#[test]
fn canonical_library_classification_keeps_editor_actions_in_ordinary_admission()
-> DesktopLibraryRoutingTestResult {
    let workspace = DesktopMessagesEntityId::new();
    let expected = DesktopLibraryRoutingFixture::context(1)?;
    let operations = [
        DesktopLibraryRoutingFixture::query(workspace),
        DesktopLibraryRoutingFixture::open(workspace, expected.game()),
        DesktopLibraryServiceOperation::new(
            workspace,
            DesktopLibraryServiceEnumAction::Editor {
                expected,
                action: DesktopAnnotationCommandsEnumAction::Snapshot,
            },
        ),
        DesktopLibraryServiceOperation::new(
            workspace,
            DesktopLibraryServiceEnumAction::Editor {
                expected,
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "retained notes".into(),
                    nags: Vec::new(),
                },
            },
        ),
        DesktopLibraryServiceOperation::new(
            workspace,
            DesktopLibraryServiceEnumAction::Editor {
                expected,
                action: DesktopAnnotationCommandsEnumAction::RetrySave,
            },
        ),
    ];
    for operation in operations {
        let revision = operation.expected_revision();
        let owner = operation.mutation_entity();
        let closing = operation.is_available_while_closing();
        let command = DesktopCommand::library(operation);
        assert_eq!(command.expected_revision(), revision);
        assert_eq!(command.operation().mutation_entity(), owner);
        assert_eq!(command.operation().is_available_while_closing(), closing);
        assert!(!command.operation().is_control());
    }
    Ok(())
}

#[test]
fn application_deduplicates_actual_receipt_and_excludes_competing_game_edits()
-> DesktopLibraryRoutingTestResult {
    let application = DesktopLibraryRoutingFixture::application();
    let port = application.port();
    let workspace = DesktopMessagesEntityId::new();
    let game = DesktopLibraryRoutingFixture::game(1)?;
    let request = port.request(DesktopCommand::library(DesktopLibraryRoutingFixture::open(
        workspace, game,
    )))?;
    let receipt = port.try_submit(&request)?;
    assert!(receipt.shares_outcome(&port.try_submit(&request)?));
    let editor = port.request(DesktopCommand::library(
        DesktopLibraryServiceOperation::new(
            DesktopMessagesEntityId::new(),
            DesktopLibraryServiceEnumAction::Editor {
                expected: DesktopLibraryRoutingFixture::context(1)?,
                action: DesktopAnnotationCommandsEnumAction::Navigate {
                    selected: DesktopLibraryRoutingFixture::context(1)?.selected(),
                },
            },
        ),
    ))?;
    assert!(matches!(
        port.try_submit(&editor),
        Err(DesktopApplicationError::EntityBusy)
    ));
    let work = application.try_next()?.ok_or("missing library work")?;
    assert!(receipt.shares_outcome(work.receipt()));
    assert!(application.try_next()?.is_none());
    application.begin_commit(&work)?;
    application.complete_failure(
        &work,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "provider refused before acceptance".into(),
            true,
        ),
    )?;
    assert!(receipt.shares_outcome(&port.try_submit(&request)?));
    assert!(port.try_submit(&editor).is_ok());
    Ok(())
}

#[test]
fn library_reads_use_existing_ordinary_bound_and_quit_cancels_queued_receipts()
-> DesktopLibraryRoutingTestResult {
    let application = DesktopLibraryRoutingFixture::application();
    let port = application.port();
    let workspace = DesktopMessagesEntityId::new();
    let mut receipts = Vec::new();
    let capacity = chess::desktop::application::DesktopApplication::<
        fixture::DesktopLibraryRoutingClock,
        fixture::DesktopLibraryRoutingWake,
    >::ORDINARY_CAPACITY;
    let mut saturated = false;
    for _ in 0..=capacity {
        let request = port.request(DesktopCommand::library(
            DesktopLibraryRoutingFixture::query(workspace),
        ))?;
        match port.try_submit(&request) {
            Ok(receipt) => receipts.push(receipt),
            Err(DesktopApplicationError::Busy) => {
                saturated = true;
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let quit =
        port.try_submit(&port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?)?;
    let work = application.try_next()?.ok_or("missing reserved quit")?;
    application.complete_control(&work)?;
    assert!(saturated);
    assert_eq!(receipts.len(), capacity);
    assert!(receipts.iter().all(|receipt| matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::Cancelled)
    )));
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    Ok(())
}

#[test]
fn library_cleanup_actions_remain_ordinary_while_closing() -> DesktopLibraryRoutingTestResult {
    let application = DesktopLibraryRoutingFixture::application();
    let port = application.port();
    let workspace = DesktopMessagesEntityId::new();
    port.try_submit(&port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?)?;
    let query = port.request(DesktopCommand::library(
        DesktopLibraryRoutingFixture::query(workspace),
    ))?;
    assert!(matches!(
        port.try_submit(&query),
        Err(DesktopApplicationError::Closing)
    ));
    let expected = DesktopLibraryRoutingFixture::context(1)?;
    let cleanup = DesktopLibraryServiceOperation::new(
        workspace,
        DesktopLibraryServiceEnumAction::Editor {
            expected,
            action: DesktopAnnotationCommandsEnumAction::RetrySave,
        },
    );
    let receipt = port.try_submit(&port.request(DesktopCommand::library(cleanup))?)?;
    assert!(receipt.deadline().is_none());
    Ok(())
}
