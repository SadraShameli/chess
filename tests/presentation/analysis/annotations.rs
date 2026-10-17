use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};

#[path = "annotation_service_refusal_review.rs"]
mod annotation_service_refusal_review;

#[path = "annotation_service_results.rs"]
mod annotation_service_results;

use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAdmission;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumPending,
};
use crate::desktop::annotation_owner::DesktopAnnotationOwnerEnumSaveProgress;
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::game_editor::{
    DesktopGameEditorBinding, DesktopGameEditorEpoch, DesktopGameEditorGeneration,
};
use crate::desktop::preferences::DesktopPreferencesEnumTheme;
use crate::game::enums::GameEnumMarkingColor;
use crate::game::move_node::{GameMoveNodeMarking, GameMoveNodeNag};
use crate::game::storage_repository::GameStorageRevision;
use crate::presentation::analysis::annotations::{
    PresentationAnalysisAnnotations, PresentationAnalysisAnnotationsEnumDestination,
    PresentationAnalysisAnnotationsEnumEffect, PresentationAnalysisAnnotationsEnumGuardChoice,
    PresentationAnalysisAnnotationsEnumMarkingInput,
};
use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorError;
use crate::presentation::analysis::editor_intent::tests::{
    PresentationAnalysisEditorTestFixture, PresentationAnalysisEditorTestResult,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;

struct PresentationAnalysisAnnotationsTestUi {
    panel: PresentationAnalysisAnnotations,
    request: crate::desktop::identity::DesktopRequestId,
    effects: Vec<PresentationAnalysisAnnotationsEnumEffect>,
    failures: Vec<String>,
    needs_theme: bool,
}

#[tokio::test]
async fn real_checkpoint_failure_retains_draft_and_edit_identity_for_explicit_retry()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "{old} 1. e4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    panel.set_notes(expected, "keep on failure".to_owned())?;
    let receipt = fixture.receipt()?;
    let intent = panel
        .save_notes(receipt.request_id())?
        .ok_or("missing save")?;
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.observe_result(&receipt, expected, &result)?;
    let checkpoint = fixture.receipt()?;
    let command = panel
        .prepare_checkpoint(checkpoint.request_id())?
        .ok_or("missing checkpoint")?;
    panel.attach_checkpoint(receipt.request_id(), expected, checkpoint.clone())?;
    owner.submit(command.admit(checkpoint.clone())?)?;
    let closed = fixture.close_storage().await?;
    let error = owner
        .observe_save(&checkpoint)
        .await
        .err()
        .ok_or("closed storage accepted save")?;
    let failure = DesktopFailure::new(
        DesktopEnumFailureCode::FileUnavailable,
        error.to_string(),
        true,
    );
    panel.observe_checkpoint_failure(&checkpoint, &failure)?;
    assert!(panel.failure().is_some_and(DesktopFailure::is_retryable));
    assert_eq!(panel.notes(), Some("keep on failure"));
    assert_eq!(
        panel
            .pending_intent()
            .ok_or("lost edit intent")?
            .request_id(),
        receipt.request_id()
    );
    let retry_request = PresentationAnalysisEditorTestFixture::request(2001)?;
    let retry = panel.retry_save(retry_request)?.ok_or("missing retry")?;
    assert_eq!(retry.expected(), owner.snapshot()?.context());
    assert!(matches!(
        retry.action(),
        DesktopAnnotationCommandsEnumAction::RetrySave
    ));
    assert!(panel.retry_save(retry_request)?.is_none());
    drop((panel, owner));
    closed.close().await
}

impl PresentationAnalysisAnnotationsTestUi {
    fn harness(
        panel: PresentationAnalysisAnnotations,
        request: crate::desktop::identity::DesktopRequestId,
    ) -> Harness<'static, Self> {
        Harness::builder()
            .with_size(egui::vec2(900.0, 800.0))
            .build_ui_state(
                Self::show,
                Self {
                    panel,
                    request,
                    effects: Vec::new(),
                    failures: Vec::new(),
                    needs_theme: true,
                },
            )
    }

    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_theme {
            PresentationThemeTokens::apply(ui.ctx(), DesktopPreferencesEnumTheme::Dark);
            state.needs_theme = false;
            ui.ctx().request_repaint();
            return;
        }
        match state.panel.show(ui, state.request) {
            Ok(Some(effect)) => {
                state.effects.push(effect);
                ui.ctx()
                    .request_discard("Exercise repeated annotation passes");
            }
            Ok(None) => {}
            Err(error) => state.failures.push(error.to_string()),
        }
    }
}

#[tokio::test]
async fn native_guard_traps_keyboard_focus_and_escape_stays_with_the_draft()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let owner = fixture.open(100, "{old} 1. e4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    panel.set_notes(owner.snapshot()?.context(), "keep".to_owned())?;
    panel.request_switch(
        PresentationAnalysisEditorTestFixture::request(1001)?,
        PresentationAnalysisAnnotationsEnumDestination::Leave(
            PresentationAnalysisEditorTestFixture::request(1002)?,
        ),
    )?;
    let mut harness = PresentationAnalysisAnnotationsTestUi::harness(
        panel,
        PresentationAnalysisEditorTestFixture::request(1003)?,
    );
    harness.run_ok().ok_or("guard did not settle")?;
    assert!(harness.get_by_label("Stay").is_focused());
    for label in ["Discard", "Save", "Stay"] {
        harness.key_press(egui::Key::Tab);
        harness.run_ok().ok_or("guard focus did not settle")?;
        assert!(harness.get_by_label(label).is_focused());
    }
    harness.key_press(egui::Key::Escape);
    harness
        .run_ok()
        .ok_or("guard cancellation did not settle")?;
    assert!(!harness.state().panel.has_guard());
    assert_eq!(harness.state().panel.notes(), Some("keep"));
    assert!(harness.state().effects.is_empty());
    assert!(harness.state().failures.is_empty());
    drop((harness, owner));
    fixture.close().await
}

#[tokio::test]
async fn guard_retains_an_authoritative_board_move_for_discard_continuation()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let owner = fixture.open(100, "{old} 1. e4 *").await?;
    let publication = owner.snapshot()?;
    let movement = publication
        .board()
        .legal_moves()
        .first()
        .ok_or("missing board move")?
        .movement();
    let node = PresentationAnalysisEditorTestFixture::node(1001)?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(publication, None);
    panel.set_notes(publication.context(), "draft".to_owned())?;
    panel.request_switch(
        PresentationAnalysisEditorTestFixture::request(1002)?,
        PresentationAnalysisAnnotationsEnumDestination::AppendStudyBranch { node, movement },
    )?;
    let effect = panel
        .resolve_guard(
            PresentationAnalysisAnnotationsEnumGuardChoice::Discard,
            PresentationAnalysisEditorTestFixture::request(1003)?,
        )?
        .ok_or("missing continuation")?;
    let PresentationAnalysisAnnotationsEnumEffect::Command(intent) = effect else {
        return Err("wrong continuation".into());
    };
    assert_eq!(intent.expected(), publication.context());
    assert!(
        matches!(intent.action(), DesktopAnnotationCommandsEnumAction::AppendStudyBranch { node: retained_node, movement: retained_move } if *retained_node == node && *retained_move == movement)
    );
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn canonical_refusal_retains_the_original_save_intent_notes_and_guard()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "{old} 1. e4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    panel.set_notes(expected, "invalid\0notes".to_owned())?;
    let navigation = PresentationAnalysisEditorTestFixture::request(1001)?;
    panel.request_switch(
        navigation,
        PresentationAnalysisAnnotationsEnumDestination::Leave(
            PresentationAnalysisEditorTestFixture::request(1002)?,
        ),
    )?;
    let receipt = fixture.receipt()?;
    let effect = panel
        .resolve_guard(
            PresentationAnalysisAnnotationsEnumGuardChoice::Save,
            receipt.request_id(),
        )?
        .ok_or("missing save")?;
    let PresentationAnalysisAnnotationsEnumEffect::Command(intent) = effect else {
        return Err("wrong save".into());
    };
    panel.attach_receipt(&intent, receipt.clone())?;
    assert!(matches!(
        owner.submit(intent.admit(receipt.clone())?)?,
        DesktopAnnotationCommandsEnumAdmission::ComputePending
    ));
    let refusal = owner
        .observe_compute(&receipt)
        .await
        .err()
        .ok_or("invalid notes were accepted")?;
    let failure = DesktopFailure::new(
        DesktopEnumFailureCode::InvalidInput,
        refusal.to_string(),
        false,
    );
    panel.observe_failure(&receipt, expected, &failure)?;
    assert_eq!(panel.notes(), Some("invalid\0notes"));
    assert_eq!(
        panel.pending_intent().ok_or("lost intent")?.request_id(),
        receipt.request_id()
    );
    assert!(panel.has_guard());
    assert_eq!(owner.snapshot()?.annotations().notes(), "old");
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn repeated_publication_observation_keeps_one_selected_field_copy_and_shared_backing()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let owner = fixture.open(100, "{selected text} 1. e4 *").await?;
    let publication = owner.snapshot()?.clone();
    assert!(std::ptr::eq(publication.board(), owner.snapshot()?.board()));
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(&publication, None);
    let pointer = panel.notes().ok_or("missing notes")?.as_ptr();
    for _ in 0..100 {
        assert!(!panel.synchronize(&publication, None));
        assert_eq!(panel.notes().ok_or("missing notes")?.as_ptr(), pointer);
    }
    drop((panel, publication, owner));
    fixture.close().await
}

#[tokio::test]
async fn every_storage_epoch_generation_and_source_revision_change_preserves_a_dirty_binding()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let record = PresentationAnalysisEditorTestFixture::record(100, "{old} *")?;
    let root = PresentationAnalysisEditorTestFixture::node(1)?;
    let epoch = DesktopGameEditorEpoch::from_request_id(
        PresentationAnalysisEditorTestFixture::request(1001)?,
    );
    let binding = DesktopGameEditorBinding::new(
        GameStorageRevision::new(1),
        epoch,
        DesktopGameEditorGeneration::ZERO,
    );
    let original = fixture.publish(&record, root, binding)?;
    for changed_binding in [
        DesktopGameEditorBinding::new(
            GameStorageRevision::new(2),
            epoch,
            DesktopGameEditorGeneration::ZERO,
        ),
        DesktopGameEditorBinding::new(
            GameStorageRevision::new(1),
            DesktopGameEditorEpoch::from_request_id(
                PresentationAnalysisEditorTestFixture::request(1002)?,
            ),
            DesktopGameEditorGeneration::ZERO,
        ),
        DesktopGameEditorBinding::new(
            GameStorageRevision::new(1),
            epoch,
            DesktopGameEditorGeneration::new(1),
        ),
    ] {
        let mut panel = PresentationAnalysisAnnotations::new();
        panel.synchronize(&original, None);
        panel.set_notes(original.context(), "retain".to_owned())?;
        panel.synchronize(&fixture.publish(&record, root, changed_binding)?, None);
        assert!(panel.is_conflicted());
        assert_eq!(panel.context(), Some(original.context()));
        assert_eq!(panel.notes(), Some("retain"));
    }
    let editor = crate::desktop::game_editor::DesktopGameEditor::notation(
        &record,
        crate::game::enums::GameRevision::ZERO.next()?,
        root,
        binding,
    )?;
    let changed = editor.publish(editor.context(), &fixture.store.client())?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(&original, None);
    panel.set_notes(original.context(), "retain".to_owned())?;
    panel.synchronize(&changed, None);
    assert!(panel.is_conflicted());
    assert_eq!(panel.context(), Some(original.context()));
    drop((panel, changed, original));
    fixture.close().await
}

#[tokio::test]
async fn independent_source_during_save_does_not_consume_draft_or_release_navigation()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "{old} 1. e4 *").await?;
    let foreign = fixture.open(101, "{other} 1. d4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    panel.set_notes(expected, "retained".to_owned())?;
    panel.request_switch(
        PresentationAnalysisEditorTestFixture::request(1001)?,
        PresentationAnalysisAnnotationsEnumDestination::Leave(
            PresentationAnalysisEditorTestFixture::request(1002)?,
        ),
    )?;
    let receipt = fixture.receipt()?;
    let effect = panel
        .resolve_guard(
            PresentationAnalysisAnnotationsEnumGuardChoice::Save,
            receipt.request_id(),
        )?
        .ok_or("missing save")?;
    let PresentationAnalysisAnnotationsEnumEffect::Command(intent) = effect else {
        return Err("wrong effect".into());
    };
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.synchronize(foreign.snapshot()?, None);
    assert!(panel.observe_result(&receipt, expected, &result).is_err());
    assert_eq!(panel.notes(), Some("retained"));
    assert_eq!(panel.context(), Some(expected));
    assert!(panel.has_guard());
    assert!(panel.is_conflicted());
    drop((panel, owner, foreign));
    fixture.close().await
}

#[tokio::test]
async fn guard_stay_and_discard_preserve_then_restore_the_bound_plain_text()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let owner = fixture.open(100, "{<b>plain</b>} 1. e4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    let context = panel.context().ok_or("missing context")?;
    panel.set_notes(context, "**literal**".to_owned())?;
    let target = PresentationAnalysisAnnotationsEnumDestination::Navigate(
        PresentationAnalysisEditorTestFixture::node(2)?,
    );
    let navigation = PresentationAnalysisEditorTestFixture::request(1001)?;
    assert!(panel.request_switch(navigation, target)?.is_none());
    assert!(panel.request_switch(navigation, target)?.is_none());
    panel.resolve_guard(
        PresentationAnalysisAnnotationsEnumGuardChoice::Stay,
        PresentationAnalysisEditorTestFixture::request(1002)?,
    )?;
    assert_eq!(panel.notes(), Some("**literal**"));
    assert_eq!(panel.context(), Some(context));
    panel.request_switch(navigation, target)?;
    let effect = panel
        .resolve_guard(
            PresentationAnalysisAnnotationsEnumGuardChoice::Discard,
            PresentationAnalysisEditorTestFixture::request(1003)?,
        )?
        .ok_or("missing continuation")?;
    let PresentationAnalysisAnnotationsEnumEffect::Command(intent) = effect else {
        return Err("wrong continuation".into());
    };
    assert_eq!(intent.request_id(), navigation);
    assert_eq!(intent.expected(), context);
    assert!(
        matches!(intent.action(), DesktopAnnotationCommandsEnumAction::Navigate { selected } if *selected == PresentationAnalysisEditorTestFixture::node(2)?)
    );
    assert_eq!(panel.notes(), Some("<b>plain</b>"));
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn save_guard_waits_for_correlated_edit_and_durable_acknowledgment()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "{old} 1. e4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = panel.context().ok_or("missing context")?;
    panel.set_notes(expected, "saved".to_owned())?;
    let navigation = PresentationAnalysisEditorTestFixture::request(1001)?;
    panel.request_switch(
        navigation,
        PresentationAnalysisAnnotationsEnumDestination::Navigate(
            PresentationAnalysisEditorTestFixture::node(2)?,
        ),
    )?;
    let receipt = fixture.receipt()?;
    let effect = panel
        .resolve_guard(
            PresentationAnalysisAnnotationsEnumGuardChoice::Save,
            receipt.request_id(),
        )?
        .ok_or("missing save")?;
    let PresentationAnalysisAnnotationsEnumEffect::Command(intent) = effect else {
        return Err("wrong save".into());
    };
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    let foreign = fixture.receipt()?;
    assert!(matches!(
        panel.observe_result(&foreign, expected, &result),
        Err(PresentationAnalysisEditorError::ForeignReceipt)
    ));
    panel.observe_result(&receipt, expected, &result)?;
    assert!(panel.has_guard());
    let checkpoint = fixture.checkpoint(&mut owner)?;
    panel
        .prepare_checkpoint(checkpoint.request_id())?
        .ok_or("missing checkpoint intent")?;
    panel.attach_checkpoint(receipt.request_id(), expected, checkpoint.clone())?;
    let committing = owner.observe_save(&checkpoint).await?;
    assert!(
        panel
            .observe_save(&checkpoint, &committing, owner.snapshot()?)?
            .is_none()
    );
    let saved = owner.observe_save(&checkpoint).await?;
    assert!(matches!(
        saved,
        DesktopAnnotationOwnerEnumSaveProgress::Saved { .. }
    ));
    let effect = panel
        .observe_save(&checkpoint, &saved, owner.snapshot()?)?
        .ok_or("missing durable continuation")?;
    let PresentationAnalysisAnnotationsEnumEffect::Command(continuation) = effect else {
        return Err("wrong continuation".into());
    };
    assert_eq!(continuation.request_id(), navigation);
    assert_eq!(continuation.expected(), owner.snapshot()?.context());
    assert!(!panel.has_notes_changes());
    assert!(!panel.has_guard());
    assert!(
        panel
            .observe_save(&checkpoint, &saved, owner.snapshot()?)
            .is_err()
    );
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn completion_never_consumes_a_newer_draft_or_an_independent_publication()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "{old} 1. e4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = panel.context().ok_or("missing context")?;
    panel.set_notes(expected, "first".to_owned())?;
    let receipt = fixture.receipt()?;
    let intent = panel
        .save_notes(receipt.request_id())?
        .ok_or("missing save")?;
    assert!(
        panel
            .request_switch(
                PresentationAnalysisEditorTestFixture::request(4001)?,
                PresentationAnalysisAnnotationsEnumDestination::Leave(
                    PresentationAnalysisEditorTestFixture::request(4002)?
                )
            )?
            .is_none()
    );
    assert!(panel.has_guard());
    panel.resolve_guard(
        PresentationAnalysisAnnotationsEnumGuardChoice::Stay,
        PresentationAnalysisEditorTestFixture::request(4003)?,
    )?;
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.observe_result(&receipt, expected, &result)?;
    panel.set_notes(expected, "newer".to_owned())?;
    let checkpoint = fixture.checkpoint(&mut owner)?;
    panel
        .prepare_checkpoint(checkpoint.request_id())?
        .ok_or("missing checkpoint intent")?;
    panel.attach_checkpoint(receipt.request_id(), expected, checkpoint.clone())?;
    owner.observe_save(&checkpoint).await?;
    let saved = owner.observe_save(&checkpoint).await?;
    assert!(
        panel
            .observe_save(&checkpoint, &saved, owner.snapshot()?)?
            .is_none()
    );
    assert_eq!(panel.notes(), Some("newer"));
    assert_eq!(panel.context(), Some(expected));
    assert!(panel.is_conflicted());
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn editing_then_reverting_to_the_submitted_text_still_retains_the_newer_draft()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "{old} 1. e4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    panel.set_notes(expected, "submitted".to_owned())?;
    let receipt = fixture.receipt()?;
    let intent = panel
        .save_notes(receipt.request_id())?
        .ok_or("missing save")?;
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.observe_result(&receipt, expected, &result)?;
    panel.set_notes(expected, "new edit".to_owned())?;
    panel.set_notes(expected, "submitted".to_owned())?;
    let checkpoint = fixture.checkpoint(&mut owner)?;
    panel
        .prepare_checkpoint(checkpoint.request_id())?
        .ok_or("missing checkpoint intent")?;
    panel.attach_checkpoint(receipt.request_id(), expected, checkpoint.clone())?;
    owner.observe_save(&checkpoint).await?;
    let saved = owner.observe_save(&checkpoint).await?;
    assert!(
        panel
            .observe_save(&checkpoint, &saved, owner.snapshot()?)?
            .is_none()
    );
    assert_eq!(panel.context(), Some(expected));
    assert_eq!(panel.notes(), Some("submitted"));
    assert!(panel.is_conflicted());
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn notes_nags_and_markings_have_separate_saves_and_pending_refusal()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let owner = fixture.open(100, "{old} 1. e4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = panel.context().ok_or("missing context")?;
    panel.set_notes(expected, "notes".to_owned())?;
    panel.set_nags(expected, vec![GameMoveNodeNag::new(14)])?;
    let marking = GameMoveNodeMarking::circle(GameEnumMarkingColor::Green, "e4".parse()?);
    panel.marking_input(
        expected,
        PresentationAnalysisAnnotationsEnumMarkingInput::Toggle(marking),
    )?;
    let request = PresentationAnalysisEditorTestFixture::request(1001)?;
    panel.synchronize(
        owner.snapshot()?,
        Some(DesktopAnnotationCommandsEnumPending::Computing),
    );
    assert!(matches!(
        panel.save_markings(request),
        Err(PresentationAnalysisEditorError::SourcePending)
    ));
    panel.synchronize(owner.snapshot()?, None);
    let intent = panel.save_markings(request)?.ok_or("missing markings")?;
    assert!(
        matches!(intent.action(), DesktopAnnotationCommandsEnumAction::SaveMarkings { markings } if markings == &vec![marking])
    );
    assert!(panel.has_notes_changes());
    assert!(panel.save_markings(request)?.is_none());
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn equal_fen_nodes_and_games_clear_temporary_drawings_without_rebasing_notes()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let text = "1. Nf3 (1. Nc3 Nc6 2. Nf3 Nf6) Nf6 2. Nc3 Nc6 *";
    let record = PresentationAnalysisEditorTestFixture::record(100, text)?;
    let binding = DesktopGameEditorBinding::new(
        GameStorageRevision::new(1),
        DesktopGameEditorEpoch::from_request_id(PresentationAnalysisEditorTestFixture::request(
            500,
        )?),
        DesktopGameEditorGeneration::ZERO,
    );
    let root = fixture.publish(
        &record,
        PresentationAnalysisEditorTestFixture::node(6)?,
        binding,
    )?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(&root, None);
    let expected = root.context();
    panel.set_notes(expected, "keep".to_owned())?;
    panel.marking_input(
        expected,
        PresentationAnalysisAnnotationsEnumMarkingInput::Toggle(GameMoveNodeMarking::circle(
            GameEnumMarkingColor::Red,
            "a1".parse()?,
        )),
    )?;
    let other = fixture.publish(
        &record,
        PresentationAnalysisEditorTestFixture::node(9)?,
        binding,
    )?;
    assert_eq!(root.board().fen(), other.board().fen());
    assert_ne!(root.context().selected(), other.context().selected());
    panel.synchronize(&other, None);
    assert!(
        panel
            .markings()
            .ok_or("missing markings")?
            .temporary()
            .is_empty()
    );
    assert_eq!(panel.notes(), Some("keep"));
    assert!(panel.is_conflicted());
    assert!(matches!(
        panel.save_notes(PresentationAnalysisEditorTestFixture::request(1001)?),
        Err(PresentationAnalysisEditorError::Conflict)
    ));
    let foreign = PresentationAnalysisEditorTestFixture::record(101, text)?;
    let foreign_publication = fixture.publish(
        &foreign,
        PresentationAnalysisEditorTestFixture::node(6)?,
        binding,
    )?;
    assert_eq!(root.board().fen(), foreign_publication.board().fen());
    panel.synchronize(&foreign_publication, None);
    assert!(
        panel
            .marking_input(
                expected,
                PresentationAnalysisAnnotationsEnumMarkingInput::ClearTemporary
            )
            .is_err()
    );
    drop((panel, root, other, foreign_publication));
    fixture.close().await
}

#[tokio::test]
async fn native_save_button_emits_once_across_repeated_egui_passes()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let owner = fixture.open(100, "{old} 1. e4 *").await?;
    let mut panel = PresentationAnalysisAnnotations::new();
    panel.synchronize(owner.snapshot()?, None);
    panel.set_notes(owner.snapshot()?.context(), "<b>literal</b>".to_owned())?;
    let request = PresentationAnalysisEditorTestFixture::request(1001)?;
    let mut harness = PresentationAnalysisAnnotationsTestUi::harness(panel, request);
    harness.run();
    harness.get_by_label("Save Notes and NAGs").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().effects.len(), 1);
    assert!(harness.state().failures.is_empty());
    drop((harness, owner));
    fixture.close().await
}

#[path = "annotation_guard_review.rs"]
mod annotation_guard_review;
