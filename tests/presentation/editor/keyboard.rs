use bevy_egui::egui;
use egui_kittest::kittest::Queryable;

use crate::board::editor::{BoardEditorSessionEnumOperation, BoardEditorSessionEnumSelection};
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::desktop::editor_commands::DesktopEditorCommandsEnumOperation;
use crate::desktop::editor_commands::tests::fixture::{
    DesktopEditorCommandsFixture, DesktopEditorCommandsTestResult,
};
use crate::presentation::editor::controller::{
    PresentationEditorController, PresentationEditorControllerEnumKey,
};
use crate::presentation::editor::tests::PresentationEditorFixture;
use crate::presentation::editor::view::PresentationEditorViewEnumAction;

#[tokio::test]
async fn focused_editor_shortcuts_emit_once_and_setup_confirmation_suppresses_letters()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let snapshot = fixture.snapshot()?;
    fixture.close().await?;
    let mut harness =
        PresentationEditorFixture::harness(PresentationEditorController::new(Some(snapshot)))?;
    harness.state_mut().board_has_focus = true;
    harness.key_press(egui::Key::Q);
    harness.run_ok().ok_or("queen shortcut did not settle")?;
    harness.run_steps(4);
    assert_eq!(harness.state().actions.len(), 1);
    assert!(matches!(
        harness.state().actions.first(),
        Some(PresentationEditorViewEnumAction::Command(
            DesktopEditorCommandsEnumOperation::Apply {
                operation: BoardEditorSessionEnumOperation::Select(
                    BoardEditorSessionEnumSelection::Palette {
                        kind: BoardEnumPieceKind::Queen,
                        color: BoardEnumColor::White
                    }
                ),
                ..
            }
        ))
    ));
    harness.state_mut().actions.clear();
    harness.get_by_label("Clear Board").click_accesskit();
    harness.run_ok().ok_or("confirmation did not settle")?;
    harness.key_press(egui::Key::N);
    harness.run_ok().ok_or("modal key did not settle")?;
    assert!(harness.state().actions.is_empty());
    assert!(harness.state().keyboard_errors.is_empty());
    Ok(())
}

#[tokio::test]
async fn placement_removal_undo_and_redo_use_the_editor_session_and_reject_stale_palette_payload()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let mut controller = PresentationEditorController::new(Some(fixture.snapshot()?));
    let old_drag = controller.begin_drag("e2".parse()?)?;
    let old_palette =
        crate::presentation::editor::controller::PresentationEditorControllerPalette::new(
            fixture.snapshot()?.context(),
            BoardEnumColor::White,
            BoardEnumPieceKind::Knight,
        );
    fixture
        .dispatch(controller.key(PresentationEditorControllerEnumKey::Piece(
            BoardEnumPieceKind::Knight,
        ))?)
        .await?;
    controller.replace_snapshot(Some(fixture.snapshot()?))?;
    controller.focus_square("c3".parse()?);
    fixture
        .dispatch(controller.key(PresentationEditorControllerEnumKey::Place)?)
        .await?;
    controller.replace_snapshot(Some(fixture.snapshot()?))?;
    let stale = controller.drop_palette_payload(old_palette, "d3".parse()?);
    let stale_drag = controller.drag_square(old_drag, "e4".parse()?);
    fixture
        .dispatch(controller.key(PresentationEditorControllerEnumKey::Remove)?)
        .await?;
    controller.replace_snapshot(Some(fixture.snapshot()?))?;
    fixture
        .dispatch(controller.key(PresentationEditorControllerEnumKey::Undo)?)
        .await?;
    let restored = fixture.snapshot()?;
    controller.replace_snapshot(Some(restored.clone()))?;
    fixture
        .dispatch(controller.key(PresentationEditorControllerEnumKey::Redo)?)
        .await?;
    let removed = fixture.snapshot()?;
    fixture.close().await?;
    assert!(stale.is_err());
    assert!(stale_drag.is_err());
    assert!(restored.draft().fields().piece_at("c3".parse()?).is_some());
    assert!(removed.draft().fields().piece_at("c3".parse()?).is_none());
    Ok(())
}

#[tokio::test]
async fn editor_arrow_focus_uses_the_actual_scene_projection_for_each_orientation()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let snapshot = fixture.snapshot()?;
    fixture.close().await?;
    for (orientation, expected) in [(BoardEnumColor::White, "e3"), (BoardEnumColor::Black, "e1")] {
        let mut controller = PresentationEditorController::new(Some(snapshot.clone()));
        controller.focus_square("e2".parse()?);
        let mut harness = PresentationEditorFixture::harness(controller)?;
        harness.state_mut().scene = PresentationEditorFixture::scene(orientation)?;
        harness.state_mut().board_has_focus = true;
        harness.key_press(egui::Key::ArrowUp);
        harness.run_ok().ok_or("arrow focus did not settle")?;
        harness.run_steps(4);
        assert_eq!(
            harness.state().controller.focused_square(),
            Some(expected.parse()?)
        );
        assert!(harness.state().actions.is_empty());
        assert!(harness.state().keyboard_errors.is_empty());
    }
    Ok(())
}
