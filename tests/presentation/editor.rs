use bevy_egui::egui;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use crate::board::editor::{
    BoardEditorSessionEnumConfirmation, BoardEditorSessionEnumEdit,
    BoardEditorSessionEnumOperation, BoardEditorSessionEnumSelection, BoardEditorSessionEnumSetup,
};
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::desktop::editor_commands::tests::fixture::{
    DesktopEditorCommandsFixture, DesktopEditorCommandsTestResult,
};
use crate::desktop::editor_commands::{
    DesktopEditorCommandsEnumChoice, DesktopEditorCommandsEnumOperation,
};
use crate::presentation::editor::controller::PresentationEditorController;
use crate::presentation::editor::view::{PresentationEditorView, PresentationEditorViewEnumAction};

#[path = "editor/admission.rs"]
mod admission;
#[path = "editor/draft_list.rs"]
mod draft_list;
#[path = "editor/keyboard.rs"]
mod keyboard;

struct PresentationEditorFixture {
    view: PresentationEditorView,
    controller: PresentationEditorController,
    actions: Vec<PresentationEditorViewEnumAction>,
    needs_fonts: bool,
    board_has_focus: bool,
    keyboard_errors:
        Vec<crate::presentation::editor::controller::PresentationEditorControllerError>,
    scene: crate::presentation::board::scene::PresentationBoardScene,
}

impl PresentationEditorFixture {
    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_fonts {
            ui.ctx()
                .set_fonts(crate::presentation::theme::font::PresentationThemeFont::definitions());
            ui.ctx().request_repaint();
            state.needs_fonts = false;
            return;
        }
        if let Some(action) = state.view.show(ui, &state.controller) {
            state.actions.push(action);
            ui.ctx()
                .request_discard("Verify editor action is submitted once");
        }
        match state.view.keyboard(
            ui,
            &mut state.controller,
            state.board_has_focus,
            &state.scene,
        ) {
            Ok(Some(operation)) => {
                state
                    .actions
                    .push(PresentationEditorViewEnumAction::Command(operation));
                ui.ctx()
                    .request_discard("Verify editor shortcut is submitted once");
            }
            Ok(None) => {}
            Err(error) => state.keyboard_errors.push(error),
        }
    }

    fn harness(
        controller: PresentationEditorController,
    ) -> DesktopEditorCommandsTestResult<Harness<'static, Self>> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1400.0, 1000.0))
            .build_ui_state(
                Self::show,
                Self {
                    view: PresentationEditorView::new(),
                    controller,
                    actions: Vec::new(),
                    needs_fonts: true,
                    board_has_focus: false,
                    keyboard_errors: Vec::new(),
                    scene: Self::scene(BoardEnumColor::White)?,
                },
            );
        harness.run_ok().ok_or("editor did not settle")?;
        Ok(harness)
    }

    fn scene(
        orientation: BoardEnumColor,
    ) -> DesktopEditorCommandsTestResult<crate::presentation::board::scene::PresentationBoardScene>
    {
        let image = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(680.0, 680.0));
        let viewport = crate::scene::picking::ScenePickingViewport::new(image, image, 1.0, 1.0)?;
        Ok(
            crate::presentation::board::scene::PresentationBoardScene::new(
                egui::TextureId::User(1),
                crate::scene::renderer::SceneRendererGeneration::initial(),
                crate::scene::enums::SceneEnumLifecycle::Ready,
                viewport,
                crate::scene::camera::SceneCamera::initial(orientation)
                    .fit(viewport.physical_size())?,
            ),
        )
    }
}

#[tokio::test]
async fn native_palette_and_clear_confirmation_emit_one_typed_action_per_activation()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let snapshot = fixture.snapshot()?;
    fixture.close().await?;
    let mut harness =
        PresentationEditorFixture::harness(PresentationEditorController::new(Some(snapshot)))?;
    harness.get_by_label("White queen").click_accesskit();
    harness.run_ok().ok_or("palette did not settle")?;
    harness.run_steps(4);
    assert_eq!(harness.state().actions.len(), 1);
    assert!(matches!(
        harness.state().actions.first(),
        Some(PresentationEditorViewEnumAction::Command(
            DesktopEditorCommandsEnumOperation::Apply {
                operation: BoardEditorSessionEnumOperation::Select(
                    BoardEditorSessionEnumSelection::Palette {
                        color: BoardEnumColor::White,
                        kind: BoardEnumPieceKind::Queen
                    }
                ),
                ..
            }
        ))
    ));
    harness.state_mut().actions.clear();
    harness.get_by_label("Clear Board").click_accesskit();
    harness
        .run_ok()
        .ok_or("clear confirmation did not settle")?;
    assert!(harness.state().actions.is_empty());
    harness
        .get_by_label("Confirm Clear Board")
        .click_accesskit();
    harness.run_ok().ok_or("confirmed clear did not settle")?;
    assert!(matches!(
        harness.state().actions.first(),
        Some(PresentationEditorViewEnumAction::Command(
            DesktopEditorCommandsEnumOperation::Apply {
                operation: BoardEditorSessionEnumOperation::Replace {
                    setup: BoardEditorSessionEnumSetup::ClearBoard,
                    confirmation: BoardEditorSessionEnumConfirmation::Confirmed
                },
                ..
            }
        ))
    ));
    Ok(())
}

#[tokio::test]
async fn unsaved_leave_exposes_save_discard_stay_and_never_mints_start_game()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    fixture
        .dispatch(DesktopEditorCommandsEnumOperation::Leave {
            expected: fixture.snapshot()?.context(),
        })
        .await?;
    let snapshot = fixture.snapshot()?;
    fixture.close().await?;
    let mut harness =
        PresentationEditorFixture::harness(PresentationEditorController::new(Some(snapshot)))?;
    assert!(harness.query_by_label("Save").is_some());
    assert!(harness.query_by_label("Discard").is_some());
    assert!(harness.query_by_label("Stay").is_some());
    harness.get_by_label("Stay").click_accesskit();
    harness.run_ok().ok_or("Stay did not settle")?;
    harness.run_steps(4);
    assert_eq!(harness.state().actions.len(), 1);
    assert!(matches!(
        harness.state().actions.first(),
        Some(PresentationEditorViewEnumAction::Command(
            DesktopEditorCommandsEnumOperation::Resolve {
                choice: DesktopEditorCommandsEnumChoice::Stay,
                ..
            }
        ))
    ));
    assert!(harness.query_by_label("Start Game").is_none());
    Ok(())
}

#[tokio::test]
async fn raw_invalid_projection_and_typed_board_inputs_use_real_editor_history()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    fixture
        .edit(BoardEditorSessionEnumOperation::Replace {
            setup: BoardEditorSessionEnumSetup::ClearBoard,
            confirmation: BoardEditorSessionEnumConfirmation::Confirmed,
        })
        .await?;
    fixture
        .edit(BoardEditorSessionEnumOperation::Select(
            BoardEditorSessionEnumSelection::Palette {
                color: BoardEnumColor::Black,
                kind: BoardEnumPieceKind::King,
            },
        ))
        .await?;
    let mut controller = PresentationEditorController::new(Some(fixture.snapshot()?));
    let placed = controller.click_square("e8".parse()?)?;
    fixture.dispatch(placed).await?;
    controller.replace_snapshot(Some(fixture.snapshot()?))?;
    assert_eq!(
        controller
            .projection()
            .ok_or("missing raw projection")?
            .fields()
            .pieces()
            .count(),
        1
    );
    assert!(
        !controller
            .projection()
            .ok_or("missing raw projection")?
            .can_play()
    );
    let drag = controller.begin_drag("e8".parse()?)?;
    let dragged = controller.drag_square(drag, "d7".parse()?)?;
    fixture.dispatch(dragged).await?;
    fixture.edit(BoardEditorSessionEnumOperation::Undo).await?;
    let snapshot = fixture.snapshot()?;
    fixture.close().await?;
    assert!(snapshot.draft().fields().piece_at("e8".parse()?).is_some());
    assert!(snapshot.draft().fields().piece_at("d7".parse()?).is_none());
    assert!(snapshot.has_redo());
    Ok(())
}

#[tokio::test]
async fn invalid_findings_are_visible_and_play_is_disabled_without_legal_board_substitution()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    fixture
        .edit(BoardEditorSessionEnumOperation::Edit(
            BoardEditorSessionEnumEdit::Counters {
                halfmoves: -1,
                fullmoves: 0,
            },
        ))
        .await?;
    let snapshot = fixture.snapshot()?;
    fixture.close().await?;
    let mut harness =
        PresentationEditorFixture::harness(PresentationEditorController::new(Some(snapshot)))?;
    assert!(harness.query_by_label("Invalid draft").is_some());
    let play = harness.get_by_label("Play from Position");
    assert!(play.accesskit_node().is_disabled());
    harness.run_steps(4);
    assert!(harness.state().actions.is_empty());
    Ok(())
}
