#[path = "../presentation/app_editor/fixture.rs"]
mod application_fixture;
#[path = "editor/fixture.rs"]
mod fixture;

use application_fixture::{PresentationAppEditorFixture, PresentationAppEditorTestResult};
use chess::board::editor::{BoardEditorSessionEnumEdit, BoardEditorSessionEnumOperation};
use chess::desktop::editor_commands::DesktopEditorCommandsEnumOperation;
use chess::presentation::app::editor::PresentationAppEditorEnumIntent;
use fixture::SceneEditorFixture;

struct SceneEditorMountFixture {
    application: PresentationAppEditorFixture,
    scene: SceneEditorFixture,
    renderer: chess::scene::renderer::SceneRenderer,
    textures: bevy_egui::EguiUserTextures,
    failures: Vec<String>,
}

impl SceneEditorMountFixture {
    fn frame(&mut self, ui: &mut bevy_egui::egui::Ui) -> PresentationAppEditorTestResult {
        use chess::board::enums::BoardEnumColor;
        use chess::presentation::board::scene::PresentationBoardScene;
        use chess::scene::camera::SceneCamera;
        let camera = SceneCamera::initial(BoardEnumColor::White);
        if let Some(snapshot) = self.application.app.editor_snapshot() {
            self.renderer
                .apply_editor(&mut self.scene.world, snapshot)?;
        }
        self.renderer.mark_ready(self.renderer.generation())?;
        self.application
            .app
            .bind_editor_scene(self.renderer.editor_binding());
        let scene =
            PresentationBoardScene::from_renderer(&self.renderer, &mut self.textures, camera)?;
        let mut picker = |_point| Ok(None);
        let frame =
            self.application
                .app
                .show(ui, &self.application.port, Some(&scene), &mut picker)?;
        if let Some(viewport) = frame.viewport()
            && viewport != self.renderer.viewport()
        {
            self.renderer
                .update_view(&mut self.scene.world, viewport, camera)?;
            ui.ctx().request_discard("Use actual editor scene geometry");
        }
        Ok(())
    }

    async fn close(mut self) -> PresentationAppEditorTestResult<Vec<String>> {
        self.renderer.despawn(&mut self.scene.world)?;
        drop((self.scene, self.textures));
        self.application.close().await?;
        Ok(self.failures)
    }
}

#[tokio::test]
async fn mounted_raw_scene_accessibility_uses_controller_and_blocks_setup_modal_input()
-> PresentationAppEditorTestResult {
    use bevy_egui::egui;
    use egui_kittest::{Harness, kittest::Queryable};
    let mut application = PresentationAppEditorFixture::new().await?;
    application
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &application.port)?;
    application.settle().await?;
    let mut scene = SceneEditorFixture::new()?;
    let renderer = scene.spawn()?;
    let state = SceneEditorMountFixture {
        application,
        scene,
        renderer,
        textures: bevy_egui::EguiUserTextures::default(),
        failures: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1600.0, 1800.0))
        .build_ui_state(
            |ui, state: &mut SceneEditorMountFixture| {
                if let Err(error) = state.frame(ui) {
                    state.failures.push(error.to_string());
                }
            },
            state,
        );
    harness.run_steps(3);
    harness.state_mut().application.settle().await?;
    harness.run_steps(3);
    let before = harness
        .state()
        .application
        .app
        .editor_snapshot()
        .ok_or("missing mounted draft")?
        .clone();
    harness.get_by_label("Clear Board").click_accesskit();
    harness.run_steps(3);
    harness
        .get_by_label("Editor e1: White king")
        .click_accesskit();
    harness.run_steps(3);
    let blocked = !harness
        .state()
        .application
        .app
        .editor_controller()
        .ok_or("missing mounted controller")?
        .is_pending();
    let after_blocked = harness
        .state()
        .application
        .app
        .editor_snapshot()
        .ok_or("lost blocked draft")?
        .clone();
    harness
        .get_by_label("Cancel Setup Replacement")
        .click_accesskit();
    harness.run_steps(3);
    harness.get_by_label("White queen").click_accesskit();
    harness.run_steps(3);
    harness.state_mut().application.settle().await?;
    harness.run_steps(3);
    harness
        .get_by_label("Editor e1: White king")
        .click_accesskit();
    harness.run_steps(3);
    harness.state_mut().application.settle().await?;
    harness.run_steps(3);
    let raw = harness
        .state()
        .application
        .app
        .editor_snapshot()
        .ok_or("missing raw replacement")?
        .clone();
    let semantic = harness.query_all_by_label("Editor e1: White queen").count();
    let rendered = harness.state().renderer.piece_root("e1".parse()?).is_some();
    let failures = harness.into_state().close().await?;
    assert!(failures.is_empty(), "{failures:?}");
    assert!(blocked);
    assert_eq!(before.draft(), after_blocked.draft());
    assert_eq!(semantic, 1);
    assert!(rendered);
    assert!(raw.draft().validate().is_err());
    assert_eq!(
        raw.draft()
            .fields()
            .piece_at("e1".parse()?)
            .map(|piece| piece.kind()),
        Some(chess::board::enums::BoardEnumPieceKind::Queen)
    );
    Ok(())
}

#[tokio::test]
async fn actual_scene_renders_raw_invalid_draft_and_invalidates_generation_on_field_edit()
-> PresentationAppEditorTestResult {
    let mut application = PresentationAppEditorFixture::new().await?;
    application
        .app
        .editor_action(PresentationAppEditorEnumIntent::Create, &application.port)?;
    application.settle().await?;
    let context = application
        .app
        .editor_snapshot()
        .ok_or("missing draft")?
        .context();
    application.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Apply {
            expected: context,
            operation: BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Remove {
                square: "e1".parse()?,
            }),
        }),
        &application.port,
    )?;
    application.settle().await?;
    let raw = application
        .app
        .editor_snapshot()
        .ok_or("missing raw draft")?
        .clone();
    let mut fixture = SceneEditorFixture::new()?;
    let mut renderer = fixture.spawn()?;
    let changed = renderer.apply_editor(&mut fixture.world, &raw)?;
    let binding = renderer.editor_binding().ok_or("missing editor binding")?;
    let absent_king = renderer.piece_root("e1".parse()?).is_none();
    let retained_rook = renderer.piece_root("a1".parse()?).is_some();
    application.app.editor_action(
        PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::Apply {
            expected: raw.context(),
            operation: BoardEditorSessionEnumOperation::Edit(
                BoardEditorSessionEnumEdit::Counters {
                    halfmoves: -1,
                    fullmoves: 0,
                },
            ),
        }),
        &application.port,
    )?;
    application.settle().await?;
    let next = application
        .app
        .editor_snapshot()
        .ok_or("missing field edit")?
        .clone();
    let field_changed = renderer.apply_editor(&mut fixture.world, &next)?;
    let next_binding = renderer
        .editor_binding()
        .ok_or("missing replacement binding")?;
    let stale = renderer.mark_ready(binding.generation()).is_err();
    renderer.despawn(&mut fixture.world)?;
    drop(fixture);
    application.close().await?;
    assert!(raw.draft().validate().is_err());
    assert!(changed && field_changed);
    assert!(absent_king && retained_rook);
    assert_eq!(binding.context(), raw.context());
    assert_eq!(next_binding.context(), next.context());
    assert_ne!(binding.generation(), next_binding.generation());
    assert!(stale);
    Ok(())
}
