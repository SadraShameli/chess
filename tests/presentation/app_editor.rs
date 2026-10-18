#[path = "app_editor/fixture.rs"]
mod fixture;

use chess::board::editor::{BoardEditorSessionEnumEdit, BoardEditorSessionEnumOperation};
use chess::desktop::editor_commands::{
    DesktopEditorCommandsEnumChoice, DesktopEditorCommandsEnumOperation,
};
use chess::presentation::app::PresentationAppEnumWorkspace;
use chess::presentation::app::editor::PresentationAppEditorEnumIntent;
use fixture::{PresentationAppEditorFixture, PresentationAppEditorTestResult};

#[tokio::test]
async fn fen_transfer_resolves_real_prompt_and_saves_prior_invalid_draft_separately()
-> PresentationAppEditorTestResult {
    use chess::board::enums::BoardEnumVariant;
    let mut fixture = PresentationAppEditorFixture::new().await?;
    fixture
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &fixture.port)?;
    fixture.settle().await?;
    let expected = fixture
        .app
        .editor_snapshot()
        .ok_or("missing draft")?
        .context();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Apply {
            expected,
            operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Remove {
                square: "e1".parse()?,
            }),
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let prior = fixture
        .app
        .editor_snapshot()
        .ok_or("missing invalid draft")?
        .clone();
    let fen = "7k/8/8/8/8/8/8/K7 w - - 0 1";
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::ImportFen {
            fen: fen.into(),
            variant: BoardEnumVariant::Standard,
        },
        &fixture.port,
    )?;
    let ticket = fixture
        .app
        .editor_controller()
        .and_then(|controller| controller.pending_request())
        .ok_or("missing FEN ticket")?
        .request_id();
    fixture.settle().await?;
    let prompted = fixture
        .app
        .editor_snapshot()
        .ok_or("lost prior draft")?
        .clone();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Resolve {
            expected: prompted.context(),
            choice: DesktopEditorCommandsEnumChoice::Save,
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let imported = fixture
        .app
        .editor_snapshot()
        .ok_or("missing separate imported draft")?
        .clone();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::ExportFen {
            expected: imported.context(),
        }),
        &fixture.port,
    )?;
    let export = fixture
        .app
        .editor_controller()
        .and_then(|controller| controller.receipt())
        .ok_or("missing actual export receipt")?
        .clone();
    fixture.observe(&export).await?;
    let exported = if let Some(chess::desktop::messages::DesktopMessagesEnumResult::Value {
        value: chess::desktop::dto::DesktopEnumValue::Editor(result),
        ..
    }) = export.response()
        && let chess::desktop::editor_commands::DesktopEditorCommandsEnumResult::Fen {
            context,
            fen,
        } = result.as_ref()
    {
        Some((*context, fen.clone()))
    } else {
        None
    };
    fixture.settle().await?;
    let saved = fixture
        .repository
        .read(prior.context().draft())
        .await?
        .ok_or("prior invalid draft was not saved")?;
    let listed = fixture.app.editor_page().is_some_and(|page| {
        page.entries()
            .iter()
            .any(|entry| entry.id() == prior.context().draft())
    });
    Box::pin(fixture.close()).await?;
    assert_eq!(prompted.draft(), prior.draft());
    assert!(prompted.prompt().is_some());
    assert_eq!(imported.context().session(), ticket);
    assert_ne!(imported.context().draft(), prior.context().draft());
    assert_eq!(imported.draft().validate()?.fen(), fen);
    assert_eq!(saved.draft().fields(), prior.draft().fields());
    assert!(saved.draft().validate().is_err());
    assert!(listed);
    assert_eq!(exported, Some((imported.context(), fen.to_owned())));
    Ok(())
}

#[tokio::test]
async fn generic_owner_refusal_stays_distinct_and_failed_refresh_does_not_spin()
-> PresentationAppEditorTestResult {
    use chess::board::editor::{BoardEditorDraftError, BoardEditorDraftId};
    use chess::desktop::messages::DesktopMessagesEntityId;
    use chess::presentation::app::editor::{
        PresentationAppEditorConnection, PresentationAppEditorDraftIdProvider,
        PresentationAppEditorError,
    };
    struct PresentationAppEditorWrongOwnerDraftId;
    impl PresentationAppEditorDraftIdProvider for PresentationAppEditorWrongOwnerDraftId {
        fn next_id(&self) -> Result<BoardEditorDraftId, BoardEditorDraftError> {
            BoardEditorDraftId::try_from(uuid::Uuid::from_u128(501))
        }
    }
    let mut fixture = PresentationAppEditorFixture::new().await?;
    fixture
        .app
        .mount_editor(PresentationAppEditorConnection::new(
            DesktopMessagesEntityId::new(),
            std::sync::Arc::new(PresentationAppEditorWrongOwnerDraftId),
        ));
    fixture
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &fixture.port)?;
    fixture.settle().await?;
    let generic = matches!(
        fixture.app.editor_failure(),
        Some(PresentationAppEditorError::Failure(_))
    );
    let pending = fixture.app.has_pending_work();
    let snapshot = fixture.app.editor_snapshot().cloned();
    Box::pin(fixture.close()).await?;
    assert!(generic);
    assert!(!pending);
    assert!(snapshot.is_none());
    Ok(())
}

#[tokio::test]
async fn new_draft_copies_selected_played_position_without_changing_history_clock_or_outcome()
-> PresentationAppEditorTestResult {
    use chess::clock::duration::ClockInstant;
    use chess::clock::time_control::ClockTimeControl;
    use chess::desktop::game_snapshot::DesktopGameSnapshot;
    let mut fixture = PresentationAppEditorFixture::new().await?;
    let mut session = crate::game_transition_fixture::GameTransitionFixture::session(
        ClockTimeControl::untimed(),
        None,
    )?;
    crate::game_transition_fixture::GameTransitionFixture::play(&mut session, 2, "e2e4", 0)?;
    let played = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(100))?;
    fixture.app.accept_game_snapshot(played.clone());
    fixture
        .app
        .select_workspace(PresentationAppEnumWorkspace::Editor, &fixture.port)?;
    fixture
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &fixture.port)?;
    fixture.settle().await?;
    let copied = fixture
        .app
        .editor_snapshot()
        .ok_or("missing copied position")?
        .clone();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Apply {
            expected: copied.context(),
            operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Remove {
                square: "e1".parse()?,
            }),
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    fixture
        .app
        .select_workspace(PresentationAppEnumWorkspace::Games, &fixture.port)?;
    fixture.settle().await?;
    let expected = fixture
        .app
        .editor_snapshot()
        .ok_or("missing leave guard")?
        .context();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Resolve {
            expected,
            choice: DesktopEditorCommandsEnumChoice::Discard,
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let retained = fixture
        .app
        .game_snapshot()
        .ok_or("source game disappeared")?
        .clone();
    Box::pin(fixture.close()).await?;
    assert_eq!(copied.draft().validate()?.fen(), played.board().fen());
    assert_eq!(retained.id(), played.id());
    assert_eq!(retained.revision(), played.revision());
    assert_eq!(retained.live(), played.live());
    assert_eq!(retained.view(), played.view());
    assert_eq!(retained.moves().len(), played.moves().len());
    assert_eq!(retained.board().fen(), played.board().fen());
    assert_eq!(retained.clock(), played.clock());
    assert_eq!(retained.outcome(), played.outcome());
    assert_eq!(retained.phase(), played.phase());
    Ok(())
}

#[tokio::test]
async fn typed_stale_failure_refreshes_actual_snapshot_before_further_edits()
-> PresentationAppEditorTestResult {
    let mut fixture = PresentationAppEditorFixture::new().await?;
    fixture
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &fixture.port)?;
    fixture.settle().await?;
    let initial = fixture
        .app
        .editor_snapshot()
        .ok_or("missing draft")?
        .clone();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Apply {
            expected: initial.context(),
            operation: BoardEditorSessionEnumOperation::Edit(
                BoardEditorSessionEnumEdit::Counters {
                    halfmoves: 9,
                    fullmoves: 37,
                },
            ),
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let changed = fixture.app.editor_snapshot().ok_or("missing edit")?.clone();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Validate {
            expected: initial.context(),
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let typed = matches!(
        fixture.app.editor_failure(),
        Some(chess::presentation::app::editor::PresentationAppEditorError::Command(_))
    );
    let authoritative = fixture
        .app
        .editor_snapshot()
        .ok_or("missing refreshed owner")?
        .clone();
    let pending = fixture
        .app
        .editor_controller()
        .ok_or("missing controller")?
        .is_pending();
    Box::pin(fixture.close()).await?;
    assert!(typed);
    assert!(!pending);
    assert_eq!(authoritative.context(), changed.context());
    assert_eq!(authoritative.draft(), changed.draft());
    Ok(())
}

#[tokio::test]
async fn busy_retry_reuses_actual_opening_request_and_its_session()
-> PresentationAppEditorTestResult {
    use chess::desktop::messages::{DesktopCommand, DesktopEnumCommand};
    let mut fixture = PresentationAppEditorFixture::new().await?;
    fixture
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &fixture.port)?;
    let command = fixture
        .app
        .editor_controller()
        .and_then(|controller| controller.pending_request())
        .ok_or("missing opening request")?
        .command()
        .operation();
    let DesktopEnumCommand::Editor { owner, .. } = command else {
        return Err("wrong opening command".into());
    };
    let owner = *owner;
    fixture.settle().await?;
    let initial = fixture
        .app
        .editor_snapshot()
        .ok_or("missing initial draft")?
        .clone();
    let save = fixture.port.request(DesktopCommand::editor(
        owner,
        DesktopEditorCommandsEnumOperation::Save {
            expected: initial.context(),
        },
    ))?;
    let receipt = fixture.port.try_submit(&save)?;
    fixture
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &fixture.port)?;
    let pending = fixture
        .app
        .editor_controller()
        .and_then(|controller| controller.pending_request())
        .ok_or("Busy lost opening request")?
        .request_id();
    let unadmitted = fixture
        .app
        .editor_controller()
        .and_then(|controller| controller.receipt())
        .is_none();
    fixture.observe(&receipt).await?;
    fixture.app.retry_editor_action(&fixture.port)?;
    let admitted = fixture
        .app
        .editor_controller()
        .and_then(|controller| controller.receipt())
        .ok_or("retry has no receipt")?
        .request_id();
    fixture.settle().await?;
    let next = fixture
        .app
        .editor_snapshot()
        .ok_or("retry did not open draft")?
        .clone();
    Box::pin(fixture.close()).await?;
    assert!(unadmitted);
    assert_eq!(admitted, pending);
    assert_eq!(next.context().session(), pending);
    assert_ne!(next.context().draft(), initial.context().draft());
    Ok(())
}

#[tokio::test]
async fn real_headless_workspace_mounts_setup_confirmation_and_preserves_invalid_draft()
-> PresentationAppEditorTestResult {
    use bevy_egui::egui;
    use egui_kittest::{Harness, kittest::Queryable};
    let mut fixture = PresentationAppEditorFixture::new().await?;
    fixture
        .app
        .select_workspace(PresentationAppEnumWorkspace::Editor, &fixture.port)?;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1600.0, 1800.0))
        .build_ui_state(
            |ui, (fixture, failures): &mut (PresentationAppEditorFixture, Vec<String>)| {
                let mut picker = |_point| Ok(None);
                if let Err(error) = fixture.app.show(ui, &fixture.port, None, &mut picker) {
                    failures.push(error.to_string());
                }
            },
            (fixture, Vec::new()),
        );
    harness.run_steps(3);
    harness.state_mut().0.settle().await?;
    harness.run_steps(3);
    harness.get_by_label("New Draft").click_accesskit();
    harness.run_steps(3);
    harness.state_mut().0.settle().await?;
    harness.run_steps(3);
    harness.get_by_label("Clear Board").click_accesskit();
    harness.run_steps(3);
    let confirming = harness.query_all_by_label("Confirm Clear Board").count();
    harness
        .get_by_label("Confirm Clear Board")
        .click_accesskit();
    harness.run_steps(3);
    harness.state_mut().0.settle().await?;
    harness.run_steps(3);
    let raw = harness
        .state()
        .0
        .app
        .editor_snapshot()
        .ok_or("missing raw workspace draft")?
        .clone();
    let validation = harness.query_all_by_label("Invalid draft").count();
    let (fixture, failures) = harness.into_state();
    Box::pin(fixture.close()).await?;
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(confirming, 1);
    assert_eq!(validation, 1);
    assert_eq!(raw.draft().fields().pieces().count(), 0);
    assert!(raw.draft().validate().is_err());
    Ok(())
}

#[tokio::test]
async fn mounted_opening_uses_actual_ticket_and_invalid_draft_survives_save_reopen()
-> PresentationAppEditorTestResult {
    let mut fixture = PresentationAppEditorFixture::new().await?;
    fixture
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &fixture.port)?;
    let request = fixture
        .app
        .editor_controller()
        .and_then(|controller| controller.pending_request())
        .ok_or("missing request")?;
    let ticket = request.request_id();
    fixture.settle().await?;
    let initial = fixture
        .app
        .editor_snapshot()
        .ok_or("missing snapshot")?
        .clone();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Apply {
            expected: initial.context(),
            operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Remove {
                square: "e1".parse()?,
            }),
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let invalid = fixture
        .app
        .editor_snapshot()
        .ok_or("missing invalid draft")?
        .clone();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Save {
            expected: invalid.context(),
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let stored = fixture
        .repository
        .read(invalid.context().draft())
        .await?
        .ok_or("draft was not stored")?;
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Read(invalid.context().draft()),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let reopened = fixture
        .app
        .editor_snapshot()
        .ok_or("draft was not reopened")?
        .clone();
    Box::pin(fixture.close()).await?;
    assert_eq!(initial.context().session(), ticket);
    assert!(invalid.draft().validate().is_err());
    assert_eq!(stored.draft().fields(), invalid.draft().fields());
    assert_eq!(reopened.draft().fields(), invalid.draft().fields());
    assert!(!reopened.has_undo());
    assert!(!reopened.has_redo());
    Ok(())
}

#[tokio::test]
async fn mounted_navigation_stay_retains_draft_and_discard_releases_exact_destination()
-> PresentationAppEditorTestResult {
    let mut fixture = PresentationAppEditorFixture::new().await?;
    fixture
        .app
        .select_workspace(PresentationAppEnumWorkspace::Editor, &fixture.port)?;
    fixture
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &fixture.port)?;
    fixture.settle().await?;
    let initial = fixture
        .app
        .editor_snapshot()
        .ok_or("missing draft")?
        .clone();
    fixture
        .app
        .select_workspace(PresentationAppEnumWorkspace::Games, &fixture.port)?;
    fixture.settle().await?;
    let prompted = fixture
        .app
        .editor_snapshot()
        .ok_or("lost guarded draft")?
        .clone();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Resolve {
            expected: prompted.context(),
            choice: DesktopEditorCommandsEnumChoice::Stay,
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let stayed = fixture.app.workspace();
    let retained = fixture
        .app
        .editor_snapshot()
        .ok_or("lost retained draft")?
        .clone();
    fixture
        .app
        .select_workspace(PresentationAppEnumWorkspace::Games, &fixture.port)?;
    fixture.settle().await?;
    let expected = fixture
        .app
        .editor_snapshot()
        .ok_or("missing second prompt")?
        .context();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Resolve {
            expected,
            choice: DesktopEditorCommandsEnumChoice::Discard,
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let left = fixture.app.workspace();
    let closed = fixture.app.editor_snapshot().is_none();
    Box::pin(fixture.close()).await?;
    assert!(prompted.prompt().is_some());
    assert_eq!(stayed, PresentationAppEnumWorkspace::Editor);
    assert_eq!(retained.draft(), initial.draft());
    assert_eq!(left, PresentationAppEnumWorkspace::Games);
    assert!(closed);
    Ok(())
}

#[tokio::test]
async fn play_prefills_new_game_without_creating_or_replacing_the_retained_draft()
-> PresentationAppEditorTestResult {
    let mut fixture = PresentationAppEditorFixture::new().await?;
    fixture
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &fixture.port)?;
    fixture.settle().await?;
    let initial = fixture
        .app
        .editor_snapshot()
        .ok_or("missing draft")?
        .clone();
    fixture.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Play {
            expected: initial.context(),
        }),
        &fixture.port,
    )?;
    fixture.settle().await?;
    let prefill = fixture
        .app
        .new_game_configuration()
        .ok_or("missing New Game prefill")??
        .start()
        .clone();
    let retained = fixture
        .app
        .editor_snapshot()
        .ok_or("Play lost draft")?
        .clone();
    let game = fixture.app.game_snapshot().cloned();
    Box::pin(fixture.close()).await?;
    assert_eq!(
        prefill,
        chess::game::configuration::GameConfigurationEnumStart::Custom(initial.draft().validate()?)
    );
    assert_eq!(retained.draft(), initial.draft());
    assert!(game.is_none());
    Ok(())
}
