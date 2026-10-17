#[path = "app_library/fixture.rs"]
mod fixture;

use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction;
use crate::desktop::application::DesktopApplication;
use crate::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot, DesktopEnumValue};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestClock, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceError,
};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
use crate::presentation::analysis::annotations::PresentationAnalysisAnnotationsEnumGuardChoice;
use crate::presentation::app::PresentationAppEnumWorkspace;
use crate::presentation::app::library::PresentationAppLibraryEnumNavigation;
use crate::storage::library_query::StorageLibraryQueryOptions;
use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};
use fixture::PresentationAppLibraryFixture;
use std::num::NonZeroU32;
use std::sync::Arc;

#[tokio::test]
async fn native_controls_open_once_and_show_the_real_notes_guard_across_egui_passes()
-> DesktopLibraryServiceTestResult {
    let fixture = PresentationAppLibraryFixture::new().await?;
    fixture.source.seed(100, true).await?;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .build_ui_state(
            |ui, (fixture, failures): &mut (PresentationAppLibraryFixture, Vec<String>)| {
                let before = fixture
                    .app
                    .library
                    .as_ref()
                    .map(|library| library.pending.len());
                let mut picker = |_point| Ok(None);
                if let Err(error) =
                    fixture
                        .app
                        .show(ui, &fixture.application.port(), None, &mut picker)
                {
                    failures.push(error.to_string());
                }
                if before
                    != fixture
                        .app
                        .library
                        .as_ref()
                        .map(|library| library.pending.len())
                {
                    ui.ctx()
                        .request_discard("Exercise repeated native Library passes");
                }
            },
            (fixture, Vec::new()),
        );
    harness.run_steps(3);
    let initial = harness
        .state()
        .0
        .app
        .library
        .as_ref()
        .ok_or("missing Library")?
        .pending
        .len();
    harness.state_mut().0.drain().await?;
    harness.run_steps(3);
    harness.get_by_label("Open saved game").click();
    harness.run_steps(3);
    let opens = harness
        .state()
        .0
        .app
        .library
        .as_ref()
        .ok_or("missing Library")?
        .pending
        .len();
    harness.state_mut().0.finish().await?;
    harness.run_steps(3);
    harness
        .get_by_role_and_label(
            egui::accesskit::Role::MultilineTextInput,
            "Plain text notes",
        )
        .focus();
    harness.run_steps(1);
    harness
        .get_by_role_and_label(
            egui::accesskit::Role::MultilineTextInput,
            "Plain text notes",
        )
        .type_text(" guarded draft");
    harness.run_steps(2);
    harness.get_by_label("Play").click();
    harness.run_steps(3);
    let guarded = harness
        .query_all_by_label("Save your notes before leaving?")
        .count();
    let before_stay = harness.state().0.app.workspace();
    harness.get_by_label("Stay").click();
    harness.run_steps(3);
    let draft = harness
        .state()
        .0
        .app
        .library
        .as_ref()
        .ok_or("missing Library")?
        .annotations
        .notes()
        .map(str::to_owned);
    let (fixture, failures) = harness.into_state();
    fixture.close().await?;
    assert!(failures.is_empty(), "native UI failures: {failures:?}");
    assert_eq!(initial, 3);
    assert_eq!(opens, 1);
    assert_eq!(guarded, 1);
    assert_eq!(before_stay, PresentationAppEnumWorkspace::Library);
    assert!(draft.is_some_and(|draft| draft.contains("guarded draft")));
    Ok(())
}

#[tokio::test]
async fn mounted_library_uses_real_pages_and_exact_opened_board() -> DesktopLibraryServiceTestResult
{
    let mut fixture = PresentationAppLibraryFixture::new().await?;
    fixture.source.seed(100, true).await?;
    fixture.app.accept_snapshot(DesktopBootstrapSnapshot::new(
        &crate::board::position::BoardPosition::from_fen(
            "7k/8/8/8/8/8/8/7K w - - 0 1",
            crate::board::enums::BoardEnumVariant::Standard,
        )?,
        NonZeroU32::MIN,
        DesktopEnumEngineSnapshot::Unavailable {
            message: "fixture".into(),
        },
    ));
    fixture.app.select_workspace(
        PresentationAppEnumWorkspace::Library,
        &fixture.application.port(),
    )?;
    assert!(fixture.app.board_snapshot().is_none());
    let output = fixture.frame()?;
    assert!(PresentationAppLibraryFixture::has_text(&output, "Library"));
    fixture.frame()?;
    assert_eq!(
        fixture
            .app
            .library
            .as_ref()
            .ok_or("missing library")?
            .pending
            .len(),
        3
    );
    fixture.drain().await?;
    fixture.open(100).await?;
    let library = fixture.app.library.as_ref().ok_or("missing library")?;
    let publication = library.publication.as_ref().ok_or("missing publication")?;
    assert_eq!(
        fixture.app.board_snapshot().ok_or("missing board")?.fen(),
        publication.board().fen()
    );
    assert!(fixture.app.game_snapshot().is_none());
    assert!(
        library
            .panel
            .controller()
            .page()
            .ok_or("missing query page")?
            .entries()
            .len()
            <= 100
    );
    fixture.assert_scene()?;
    fixture.close().await
}

#[tokio::test]
async fn dirty_open_waits_for_durable_save_and_stay_abandons_frozen_intent()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationAppLibraryFixture::new().await?;
    fixture.source.seed(100, true).await?;
    fixture.source.seed(101, true).await?;
    fixture.query().await?;
    fixture.open(100).await?;
    fixture.notes("my draft")?;
    fixture.request_open(101)?;
    assert!(fixture.application.try_next()?.is_none());
    assert!(fixture.library()?.annotations.has_guard());
    fixture.resolve(PresentationAnalysisAnnotationsEnumGuardChoice::Save)?;
    assert!(fixture.library()?.annotations.has_guard());
    assert_eq!(
        fixture
            .library()?
            .publication
            .as_ref()
            .ok_or("missing publication")?
            .context()
            .game(),
        DesktopLibraryServiceFixture::id(100)?
    );
    fixture.finish().await?;
    assert!(!fixture.library()?.annotations.has_guard());
    let opened = fixture.finish().await?;
    assert!(matches!(opened, DesktopLibraryServiceEnumResult::Opened(_)));
    fixture.notes("stay draft")?;
    fixture.request_open(100)?;
    fixture.resolve(PresentationAnalysisAnnotationsEnumGuardChoice::Stay)?;
    assert!(fixture.library()?.navigation.is_none());
    assert!(!fixture.library()?.panel.controller().is_open_pending());
    assert_eq!(fixture.library()?.annotations.notes(), Some("stay draft"));
    assert!(fixture.application.try_next()?.is_none());
    fixture.close().await
}

#[tokio::test]
async fn guarded_workspace_new_game_game_and_quit_leave_only_after_discard()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationAppLibraryFixture::new().await?;
    fixture.source.seed(100, true).await?;
    fixture.query().await?;
    fixture.open(100).await?;
    for target in [
        PresentationAppLibraryEnumNavigation::Workspace(PresentationAppEnumWorkspace::Games),
        PresentationAppLibraryEnumNavigation::NewGame,
        PresentationAppLibraryEnumNavigation::Game(DesktopLibraryServiceFixture::id(200)?),
        PresentationAppLibraryEnumNavigation::Quit,
    ] {
        fixture.notes("protected")?;
        assert!(fixture.library()?.request_navigation(target)?.is_none());
        assert!(fixture.library()?.annotations.has_guard());
        fixture.resolve(PresentationAnalysisAnnotationsEnumGuardChoice::Stay)?;
        assert_eq!(fixture.library()?.annotations.notes(), Some("protected"));
    }
    fixture.app.select_workspace(
        PresentationAppEnumWorkspace::Games,
        &fixture.application.port(),
    )?;
    assert_eq!(
        fixture.app.workspace(),
        PresentationAppEnumWorkspace::Library
    );
    fixture.resolve(PresentationAnalysisAnnotationsEnumGuardChoice::Discard)?;
    assert_eq!(fixture.app.workspace(), PresentationAppEnumWorkspace::Games);
    fixture.close().await
}

#[tokio::test]
async fn busy_retains_the_prepared_request_and_foreign_receipts_cannot_finish_it()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationAppLibraryFixture::new().await?;
    let port = fixture.application.port();
    for _ in 0..DesktopApplication::<DesktopLibraryServiceTestClock, DesktopNativeEventLoopWake>::ORDINARY_CAPACITY {
        port.try_submit(&port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?)?;
    }
    let intent = fixture
        .library()?
        .panel
        .controller_mut()
        .query(
            DesktopRequestId::new(),
            StorageLibraryQueryOptions::default(),
        )?
        .ok_or("missing query")?;
    fixture.library()?.dispatch_panel(intent.clone(), &port)?;
    let original = fixture
        .library()?
        .panel
        .controller()
        .retained_request(&intent)
        .ok_or("missing retained request")?
        .request_id();
    fixture.app.synchronize_library(&port)?;
    assert_eq!(
        fixture
            .library()?
            .panel
            .controller()
            .retained_request(&intent)
            .ok_or("missing retained request")?
            .request_id(),
        original
    );
    while let Some(work) = fixture.application.try_next()? {
        fixture.application.complete_value(
            &work,
            DesktopEnumValue::Bootstrap(DesktopBootstrapSnapshot::new(
                &crate::board::position::BoardPosition::standard(),
                NonZeroU32::MIN,
                DesktopEnumEngineSnapshot::Unavailable {
                    message: "fixture".into(),
                },
            )),
            None,
        )?;
    }
    fixture.app.synchronize_library(&port)?;
    let other = DesktopApplication::new(
        Arc::new(DesktopLibraryServiceTestClock),
        Arc::new(DesktopNativeEventLoopWake::new()),
    );
    let other_port = other.port();
    let foreign = other_port.try_submit(
        &other_port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?,
    )?;
    assert!(
        fixture
            .library()?
            .panel
            .controller_mut()
            .observe_error(
                &foreign,
                intent.operation(),
                Arc::new(DesktopLibraryServiceError::Busy)
            )
            .is_err()
    );
    fixture.drain().await?;
    fixture.close().await
}

#[tokio::test]
async fn save_failure_keeps_guard_draft_and_exact_unadmitted_open_revision()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationAppLibraryFixture::new().await?;
    let revision = fixture.source.seed(100, true).await?;
    fixture.source.seed(101, true).await?;
    fixture.query().await?;
    fixture.open(100).await?;
    fixture.notes("retained notes")?;
    fixture.request_open(101)?;
    fixture.source.sql("CREATE TRIGGER fail_ui_save BEFORE UPDATE ON library_entries BEGIN SELECT RAISE(ABORT, 'ui save blocked'); END;").await?;
    fixture.resolve(PresentationAnalysisAnnotationsEnumGuardChoice::Save)?;
    fixture.finish_failure().await?;
    let guarded = fixture.library()?.annotations.has_guard();
    let notes = fixture.library()?.annotations.notes().map(str::to_owned);
    let navigation = fixture.library()?.navigation.is_some();
    let unadmitted = fixture.application.try_next()?.is_none();
    fixture.source.sql("DROP TRIGGER fail_ui_save;").await?;
    let retained = fixture
        .source
        .execute(
            crate::desktop::library_service::DesktopLibraryServiceEnumAction::Open {
                game: DesktopLibraryServiceFixture::id(100)?,
                storage_revision: revision,
            },
        )
        .await?;
    let expected = retained
        .editor_context()
        .ok_or("missing accepted source during fixture recovery")?;
    let recovered = fixture
        .source
        .execute(
            crate::desktop::library_service::DesktopLibraryServiceEnumAction::Editor {
                expected,
                action: DesktopAnnotationCommandsEnumAction::RetrySave,
            },
        )
        .await?;
    fixture.close().await?;
    assert!(guarded);
    assert_eq!(notes.as_deref(), Some("retained notes"));
    assert!(navigation);
    assert!(unadmitted);
    assert!(matches!(
        recovered,
        DesktopLibraryServiceEnumResult::Saved { .. }
    ));
    Ok(())
}

#[tokio::test]
async fn legal_study_branch_and_tree_navigation_use_actual_service_contexts()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationAppLibraryFixture::new().await?;
    fixture.source.seed(100, true).await?;
    fixture.query().await?;
    fixture.open(100).await?;
    let movement = fixture
        .library()?
        .publication
        .as_ref()
        .ok_or("missing publication")?
        .board()
        .legal_moves()
        .first()
        .ok_or("missing legal move")?
        .movement();
    let port = fixture.application.port();
    let node = crate::game::enums::GameMoveId::try_from(uuid::Uuid::from_u128(500))?;
    fixture.library()?.append_move(node, movement, &port)?;
    assert!(matches!(
        fixture.finish().await?,
        DesktopLibraryServiceEnumResult::Saved { .. }
    ));
    fixture.library()?.load_tree(false, &port)?;
    fixture.finish().await?;
    let library = fixture.library()?;
    let root = library
        .publication
        .as_ref()
        .ok_or("missing publication")?
        .root();
    assert!(
        library
            .tree
            .tree_page()
            .ok_or("missing real tree page")?
            .nodes()
            .len()
            <= 100
    );
    let effect = library.annotations.request_switch(DesktopRequestId::new(), crate::presentation::analysis::annotations::PresentationAnalysisAnnotationsEnumDestination::Navigate(root))?.ok_or("missing navigation")?;
    library.annotation_effect(effect, &port)?;
    fixture.finish().await?;
    assert_eq!(
        fixture
            .library()?
            .publication
            .as_ref()
            .ok_or("missing publication")?
            .context()
            .selected(),
        root
    );
    assert!(
        !matches!(fixture.application.try_next()?.map(|work| work.request().command().operation().clone()), Some(DesktopEnumCommand::Library(operation)) if matches!(operation.action(), crate::desktop::library_service::DesktopLibraryServiceEnumAction::Editor { action: DesktopAnnotationCommandsEnumAction::RetrySave, .. }))
    );
    fixture.close().await
}

#[tokio::test]
async fn real_played_save_receipt_finishes_without_a_second_checkpoint()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationAppLibraryFixture::new().await?;
    let mut session = DesktopLibraryServiceFixture::played(100)?;
    session.begin_analysis(
        session.revision(),
        crate::clock::duration::ClockInstant::from_nanos(0),
    )?;
    fixture.source.seed_played(&session).await?;
    fixture.query().await?;
    fixture.open(100).await?;
    fixture.notes("saved played notes")?;
    let port = fixture.application.port();
    fixture.library()?.save_notes(false, &port)?;
    let result = fixture.finish().await?;
    let played = matches!(
        &result,
        DesktopLibraryServiceEnumResult::Saved {
            receipt: crate::desktop::library_service::DesktopLibraryServiceEnumSaveReceipt::Played(
                _
            ),
            ..
        }
    );
    let clean = !fixture.library()?.annotations.has_notes_changes();
    let idle = fixture.application.try_next()?.is_none() && !fixture.app.has_pending_work();
    drop(result);
    fixture.close().await?;
    assert!(played && clean && idle);
    Ok(())
}

#[tokio::test]
async fn changed_live_source_retains_old_notes_and_refuses_stale_editor_input()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationAppLibraryFixture::new().await?;
    let mut session = DesktopLibraryServiceFixture::played(100)?;
    session.begin_analysis(
        session.revision(),
        crate::clock::duration::ClockInstant::from_nanos(0),
    )?;
    fixture.source.seed_played(&session).await?;
    fixture.query().await?;
    fixture.open(100).await?;
    fixture.notes("old draft")?;
    session.save_notes_and_nags(
        session.revision(),
        session.view(),
        "new source notes".into(),
        Vec::new(),
    )?;
    fixture
        .app
        .accept_game_snapshot(crate::desktop::game_snapshot::DesktopGameSnapshot::capture(
            &session,
            crate::clock::duration::ClockInstant::from_nanos(50),
        )?);
    let port = fixture.application.port();
    let refused = fixture.library()?.save_notes(false, &port).is_err();
    let draft = fixture.library()?.annotations.notes().map(str::to_owned);
    let no_request = fixture.application.try_next()?.is_none();
    fixture.close().await?;
    assert!(refused && no_request);
    assert_eq!(draft.as_deref(), Some("old draft"));
    Ok(())
}

#[tokio::test]
async fn certified_native_open_retry_keeps_its_fresh_request_during_busy_admission()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationAppLibraryFixture::new().await?;
    let mut session = DesktopLibraryServiceFixture::played(100)?;
    session.begin_analysis(
        session.revision(),
        crate::clock::duration::ClockInstant::from_nanos(50),
    )?;
    let revision = fixture.source.seed_played(&session).await?;
    fixture
        .source
        .registry
        .restore(crate::desktop::game_registry::DesktopGameRegistryRestored {
            session,
            storage_revision: revision,
        })
        .await?;
    fixture.frame()?;
    fixture.drain().await?;
    let port = fixture.application.port();
    let open = fixture
        .library()?
        .panel
        .controller_mut()
        .open_saved(
            DesktopRequestId::new(),
            DesktopLibraryServiceFixture::id(100)?,
        )?
        .ok_or("missing Open intent")?;
    let pressure = fixture.source.fill_memory()?;
    fixture.library()?.dispatch_panel(open.clone(), &port)?;
    fixture.finish_failure().await?;
    let old = fixture
        .library()?
        .panel
        .controller()
        .retained_receipt(&open)
        .ok_or("missing original application receipt")?
        .clone();
    let output = fixture.frame()?;
    let visible = PresentationAppLibraryFixture::has_text(&output, "Retry opening");
    for _ in 0..DesktopApplication::<DesktopLibraryServiceTestClock, DesktopNativeEventLoopWake>::ORDINARY_CAPACITY {
        port.try_submit(&port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?)?;
    }
    let retry = fixture
        .library()?
        .panel
        .controller_mut()
        .retry_open(DesktopRequestId::new())?
        .ok_or("missing certified retry")?;
    fixture.library()?.dispatch_panel(retry.clone(), &port)?;
    let prepared = fixture
        .library()?
        .panel
        .controller()
        .retained_request(&retry)
        .ok_or("missing prepared retry")?
        .request_id();
    fixture.app.synchronize_library(&port)?;
    let stable = fixture
        .library()?
        .panel
        .controller()
        .retained_request(&retry)
        .is_some_and(|request| request.request_id() == prepared);
    let retained_origin = fixture
        .library()?
        .panel
        .controller()
        .retained_receipt(&open)
        .is_some_and(|receipt| receipt.shares_outcome(&old));
    drop(pressure);
    while let Some(work) = fixture.application.try_next()? {
        fixture.application.complete_value(
            &work,
            DesktopEnumValue::Bootstrap(DesktopBootstrapSnapshot::new(
                &crate::board::position::BoardPosition::standard(),
                NonZeroU32::MIN,
                DesktopEnumEngineSnapshot::Unavailable {
                    message: "admission pressure".into(),
                },
            )),
            None,
        )?;
    }
    fixture.app.synchronize_library(&port)?;
    let result = fixture.finish().await?;
    let opened = matches!(&result, DesktopLibraryServiceEnumResult::Opened(_));
    let distinct = prepared != old.request_id();
    let original_failed = matches!(
        old.response(),
        Some(crate::desktop::messages::DesktopMessagesEnumResult::LibraryFailure(_))
    );
    let idle = !fixture.app.has_pending_work() && fixture.application.try_next()?.is_none();
    drop((old, result));
    fixture.close().await?;
    assert!(visible && stable && retained_origin && opened && distinct && original_failed && idle);
    Ok(())
}

#[tokio::test]
async fn file_selection_round_trip_sends_file_select_then_dispatches_import_with_the_handle()
-> DesktopLibraryServiceTestResult {
    use crate::clock::duration::ClockInstant;
    use crate::desktop::file_handle::{
        DesktopFileEnumPurpose, DesktopFileHandleRegistry, DesktopFileHandleRequest,
    };
    use crate::desktop::library_service::{
        DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumImportAction,
    };
    use crate::desktop::messages::DesktopMessagesEntityId;
    use crate::presentation::library::view::PresentationLibraryEnumFileRequest;

    let mut fixture = PresentationAppLibraryFixture::new().await?;
    let port = fixture.application.port();
    fixture
        .library()?
        .select_file(PresentationLibraryEnumFileRequest::ImportSource, &port)?;
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing file selection work")?;
    let asked = matches!(
        work.request().command().operation(),
        DesktopEnumCommand::FileSelect {
            owner,
            purpose: DesktopFileEnumPurpose::ReadFile
        } if *owner == fixture.source.workspace
    );
    assert!(asked);
    assert!(
        fixture
            .library()?
            .select_file(PresentationLibraryEnumFileRequest::ImportSource, &port)
            .is_err()
    );
    fixture.application.begin_commit(&work)?;
    let source = fixture.source.directory.path().join("games.pgn");
    std::fs::write(&source, "1. e4 *\n")?;
    let mut registry = DesktopFileHandleRegistry::new();
    let handle = registry
        .register_selection(
            source,
            DesktopFileHandleRequest::new(
                DesktopMessagesEntityId::new().file_handle_owner(),
                DesktopFileEnumPurpose::ReadFile,
                ClockInstant::from_nanos(1),
            ),
            &fixture.source.executor,
        )
        .await?;
    fixture
        .application
        .complete_value(&work, DesktopEnumValue::FileSelect(Some(handle)), None)?;
    fixture.app.synchronize_library(&port)?;
    let import = fixture
        .application
        .try_next()?
        .ok_or("missing import work after file selection")?;
    let prepared = matches!(
        import.request().command().operation(),
        DesktopEnumCommand::Library(operation) if matches!(
            operation.action(),
            DesktopLibraryServiceEnumAction::Import(
                DesktopLibraryServiceEnumImportAction::Prepare(file)
            ) if file.handle() == handle
        )
    );
    assert!(prepared);
    Ok(())
}
