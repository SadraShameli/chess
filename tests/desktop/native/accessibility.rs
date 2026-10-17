use bevy::a11y::ActionRequest;
use bevy::app::{PostUpdate, Update};
use bevy::prelude::{
    App, Entity, IntoScheduleConfigs, MessageWriter, Query, ResMut, Resource, With, World,
};
use bevy::window::RequestRedraw;
use bevy_egui::{
    EguiContext, EguiFullOutput, EguiInput, EguiPostUpdateSet, EguiPrimaryContextPass,
    PrimaryEguiContext, egui, run_egui_context_pass_loop_system,
};

use crate::board::enums::BoardEnumColor;
use crate::desktop::native::accessibility::DesktopNativeAccessibility;
use crate::presentation::accessibility::{
    PresentationAccessibilityError, PresentationAccessibilityInput,
};
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::scene::camera::SceneCamera;
use crate::scene::renderer::fixture::SceneRendererFixture;
use crate::scene::renderer::{SceneRenderer, SceneRendererError};

type DesktopNativeAccessibilityTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Resource, Default)]
struct DesktopNativeAccessibilityTestView {
    button: Option<egui::Id>,
    activations: usize,
    forwarded: Vec<usize>,
    deferred: Option<egui::accesskit::ActionRequest>,
    camera: Option<SceneCamera>,
}

struct DesktopNativeAccessibilityFixture {
    app: App,
    context: Entity,
    button: egui::Id,
}

impl DesktopNativeAccessibilityFixture {
    fn new() -> DesktopNativeAccessibilityTestResult<Self> {
        let mut app = App::new();
        app.add_plugins(DesktopNativeAccessibility)
            .init_resource::<DesktopNativeAccessibilityTestView>()
            .add_systems(EguiPrimaryContextPass, Self::show)
            .add_systems(Update, Self::change_camera)
            .add_systems(
                PostUpdate,
                Self::observe
                    .after(DesktopNativeAccessibility::deliver)
                    .before(EguiPostUpdateSet::EndPass),
            )
            .add_systems(
                PostUpdate,
                run_egui_context_pass_loop_system.in_set(EguiPostUpdateSet::EndPass),
            )
            .add_systems(
                PostUpdate,
                (Self::finish_outputs, Self::publish_deferred)
                    .chain()
                    .after(EguiPostUpdateSet::EndPass),
            );
        let mut context = EguiContext::default();
        context.get_mut().enable_accesskit();
        let context = app
            .world_mut()
            .spawn((
                context,
                PrimaryEguiContext,
                EguiInput(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 480.0),
                    )),
                    ..Default::default()
                }),
            ))
            .id();
        app.update();
        let button = app
            .world()
            .resource::<DesktopNativeAccessibilityTestView>()
            .button
            .ok_or("the native fixture did not mount its button")?;
        Ok(Self {
            app,
            context,
            button,
        })
    }

    fn show(
        mut contexts: Query<'_, '_, &mut EguiContext, With<PrimaryEguiContext>>,
        mut view: ResMut<'_, DesktopNativeAccessibilityTestView>,
    ) {
        let Ok(mut context) = contexts.single_mut() else {
            return;
        };
        let context = context.get_mut();
        let mut ui = egui::Ui::new(
            context.clone(),
            egui::Id::new("native-accessibility-fixture"),
            egui::UiBuilder::new().max_rect(context.content_rect()),
        );
        let response = ui.button("Activate native action");
        view.button = Some(response.id);
        if response.clicked() {
            view.activations += 1;
        }
    }

    fn observe(
        contexts: Query<'_, '_, &EguiInput, With<PrimaryEguiContext>>,
        mut view: ResMut<'_, DesktopNativeAccessibilityTestView>,
    ) {
        if let Ok(input) = contexts.single() {
            view.forwarded.push(
                input
                    .events
                    .iter()
                    .filter(|event| matches!(event, egui::Event::AccessKitActionRequest(_)))
                    .count(),
            );
        }
    }

    fn publish_deferred(
        mut view: ResMut<'_, DesktopNativeAccessibilityTestView>,
        mut messages: MessageWriter<'_, ActionRequest>,
    ) {
        if let Some(request) = view.deferred.take() {
            messages.write(ActionRequest(request));
        }
    }

    fn finish_outputs(mut outputs: Query<'_, '_, &mut EguiFullOutput>) {
        for mut output in &mut outputs {
            if let Some(output) = &mut output.0 {
                output.textures_delta.clear();
            }
        }
    }

    fn change_camera(world: &mut World) -> bevy::prelude::Result {
        let Some(camera) = world
            .resource_mut::<DesktopNativeAccessibilityTestView>()
            .camera
            .take()
        else {
            return Ok(());
        };
        let mut renderer = world
            .remove_resource::<SceneRenderer>()
            .ok_or(SceneRendererError::MissingResource("SceneRenderer"))?;
        let viewport = renderer.viewport();
        let result = renderer.update_view(world, viewport, camera);
        world.insert_resource(renderer);
        result?;
        Ok(())
    }

    fn request(target: egui::Id) -> egui::accesskit::ActionRequest {
        egui::accesskit::ActionRequest {
            action: egui::accesskit::Action::Click,
            target_tree: egui::accesskit::TreeId::ROOT,
            target_node: target.accesskit_id(),
            data: None,
        }
    }

    fn send(&mut self, target: egui::Id) {
        self.app
            .world_mut()
            .write_message(ActionRequest(Self::request(target)));
    }

    fn finish(self) -> (usize, Vec<usize>) {
        let view = self
            .app
            .world()
            .resource::<DesktopNativeAccessibilityTestView>();
        let observed = (view.activations, view.forwarded.clone());
        drop(self);
        observed
    }
}

#[test]
fn native_action_reaches_the_mounted_egui_widget_once_without_replay()
-> DesktopNativeAccessibilityTestResult {
    let mut fixture = DesktopNativeAccessibilityFixture::new()?;
    fixture.send(fixture.button);
    fixture.app.update();
    fixture.app.update();
    let failure = DesktopNativeAccessibility::take_failure(fixture.app.world_mut());
    let (activations, forwarded) = fixture.finish();
    assert!(failure.is_none());
    assert_eq!(activations, 1);
    assert_eq!(forwarded, [0, 1, 0]);
    Ok(())
}

#[test]
fn actions_published_after_egui_request_a_frame_and_arrive_on_the_next_pass()
-> DesktopNativeAccessibilityTestResult {
    let mut fixture = DesktopNativeAccessibilityFixture::new()?;
    fixture
        .app
        .world_mut()
        .resource_mut::<DesktopNativeAccessibilityTestView>()
        .deferred = Some(DesktopNativeAccessibilityFixture::request(fixture.button));
    fixture.app.update();
    let redraws = fixture
        .app
        .world_mut()
        .resource_mut::<bevy::ecs::message::Messages<RequestRedraw>>()
        .drain()
        .count();
    fixture.app.update();
    fixture.app.update();
    let (activations, forwarded) = fixture.finish();
    assert_eq!(redraws, 1);
    assert_eq!(activations, 1);
    assert_eq!(forwarded, [0, 0, 1, 0]);
    Ok(())
}

#[test]
fn native_input_saturation_is_bounded_and_reports_a_typed_refusal()
-> DesktopNativeAccessibilityTestResult {
    let mut fixture = DesktopNativeAccessibilityFixture::new()?;
    for _ in 0..=PresentationAccessibilityInput::CAPACITY {
        fixture.send(fixture.button);
    }
    fixture.app.update();
    let failure = DesktopNativeAccessibility::take_failure(fixture.app.world_mut());
    fixture.app.update();
    let (_, forwarded) = fixture.finish();
    assert!(matches!(
        failure,
        Some(PresentationAccessibilityError::Busy)
    ));
    assert_eq!(forwarded, [0, PresentationAccessibilityInput::CAPACITY, 0]);
    Ok(())
}

#[test]
fn missing_board_generation_refuses_that_action_and_preserves_other_native_controls()
-> DesktopNativeAccessibilityTestResult {
    let mut fixture = DesktopNativeAccessibilityFixture::new()?;
    let square = "e2".parse()?;
    fixture.send(PresentationBoardSemanticGrid::square_id(square));
    fixture.send(fixture.button);
    fixture.app.update();
    let failure = DesktopNativeAccessibility::take_failure(fixture.app.world_mut());
    let (activations, forwarded) = fixture.finish();
    assert!(matches!(
        failure,
        Some(PresentationAccessibilityError::MissingGeneration)
    ));
    assert_eq!(activations, 1);
    assert_eq!(forwarded, [0, 1]);
    Ok(())
}

#[test]
fn pending_native_actions_survive_until_the_primary_context_is_available()
-> DesktopNativeAccessibilityTestResult {
    let mut fixture = DesktopNativeAccessibilityFixture::new()?;
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.context)
        .remove::<PrimaryEguiContext>();
    fixture.send(fixture.button);
    fixture.app.update();
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.context)
        .insert(PrimaryEguiContext);
    fixture.app.update();
    fixture.app.update();
    let (activations, forwarded) = fixture.finish();
    assert_eq!(activations, 1);
    assert_eq!(forwarded, [0, 1, 0]);
    Ok(())
}

#[test]
fn actual_scene_generation_changes_reject_queued_board_actions_before_the_egui_pass()
-> DesktopNativeAccessibilityTestResult {
    let mut fixture = DesktopNativeAccessibilityFixture::new()?;
    let renderer = SceneRendererFixture::spawn(fixture.app.world_mut())?;
    let original = renderer.generation();
    fixture.app.world_mut().insert_resource(renderer);
    let square = "e2".parse()?;
    let target = PresentationBoardSemanticGrid::square_id(square);
    fixture.send(target);
    fixture.app.update();
    fixture
        .app
        .world_mut()
        .resource_mut::<DesktopNativeAccessibilityTestView>()
        .camera = Some(SceneCamera::initial(BoardEnumColor::Black));
    fixture.send(target);
    fixture.app.update();
    let changed = fixture.app.world().resource::<SceneRenderer>().generation();
    fixture.send(target);
    fixture.app.update();
    let failure = DesktopNativeAccessibility::take_failure(fixture.app.world_mut());
    let renderer = fixture
        .app
        .world_mut()
        .remove_resource::<SceneRenderer>()
        .ok_or("missing renderer cleanup owner")?;
    renderer.despawn(fixture.app.world_mut())?;
    let (activations, forwarded) = fixture.finish();
    assert!(failure.is_none());
    assert_ne!(changed, original);
    assert_eq!(activations, 0);
    assert_eq!(forwarded, [0, 1, 0, 1]);
    Ok(())
}
