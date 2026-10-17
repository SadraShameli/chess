use bevy::a11y::ActionRequest;
use bevy::app::{Last, PostUpdate, PreUpdate};
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::prelude::{
    App, IntoScheduleConfigs, MessageWriter, Plugin, Query, Res, ResMut, Resource, With, World,
};
use bevy::window::RequestRedraw;
use bevy_egui::{EguiInput, EguiPostUpdateSet, PrimaryEguiContext};

use crate::presentation::accessibility::{
    PresentationAccessibilityError, PresentationAccessibilityInput,
};
use crate::scene::enums::SceneEnumLifecycle;
use crate::scene::renderer::{SceneRenderer, SceneRendererGeneration};

#[derive(Resource, Default)]
struct DesktopNativeAccessibilityPending {
    requests: MessageCursor<ActionRequest>,
    input: PresentationAccessibilityInput,
    failure: Option<PresentationAccessibilityError>,
}

impl DesktopNativeAccessibilityPending {
    fn record_failure(&mut self, error: PresentationAccessibilityError) {
        if self.failure.is_none() {
            bevy::log::warn!("Native accessibility input was refused: {error}");
            self.failure = Some(error);
        }
    }
}

pub struct DesktopNativeAccessibility;

impl Plugin for DesktopNativeAccessibility {
    fn build(&self, app: &mut App) {
        app.init_resource::<DesktopNativeAccessibilityPending>()
            .add_message::<ActionRequest>()
            .add_message::<RequestRedraw>()
            .add_systems(PreUpdate, Self::collect)
            .add_systems(PostUpdate, Self::deliver.before(EguiPostUpdateSet::EndPass))
            .add_systems(Last, Self::request_pending_frame);
    }
}

impl DesktopNativeAccessibility {
    pub(crate) fn take_failure(world: &mut World) -> Option<PresentationAccessibilityError> {
        world
            .get_resource_mut::<DesktopNativeAccessibilityPending>()
            .and_then(|mut pending| pending.failure.take())
    }

    fn generation(renderer: Option<&SceneRenderer>) -> Option<SceneRendererGeneration> {
        renderer
            .filter(|renderer| renderer.lifecycle() == SceneEnumLifecycle::Ready)
            .map(SceneRenderer::generation)
    }

    fn collect(
        requests: Res<'_, Messages<ActionRequest>>,
        renderer: Option<Res<'_, SceneRenderer>>,
        mut pending: ResMut<'_, DesktopNativeAccessibilityPending>,
    ) {
        let pending = &mut *pending;
        let overflow = pending.requests.len(&requests) > PresentationAccessibilityInput::CAPACITY;
        let generation = Self::generation(renderer.as_deref());
        let mut failure = None;
        for request in pending
            .requests
            .read(&requests)
            .take(PresentationAccessibilityInput::CAPACITY)
        {
            if let Err(error) = pending.input.try_forward(request.0.clone(), generation)
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        pending.requests.clear(&requests);
        if let Some(error) = failure {
            pending.record_failure(error);
        } else if overflow {
            pending.record_failure(PresentationAccessibilityError::Busy);
        }
    }

    fn deliver(
        mut contexts: Query<'_, '_, &mut EguiInput, With<PrimaryEguiContext>>,
        renderer: Option<Res<'_, SceneRenderer>>,
        mut pending: ResMut<'_, DesktopNativeAccessibilityPending>,
    ) {
        let Ok(mut input) = contexts.single_mut() else {
            return;
        };
        pending
            .input
            .drain_into(&mut input, Self::generation(renderer.as_deref()));
    }

    fn request_pending_frame(
        requests: Res<'_, Messages<ActionRequest>>,
        pending: Res<'_, DesktopNativeAccessibilityPending>,
        mut redraw: MessageWriter<'_, RequestRedraw>,
    ) {
        if !pending.requests.is_empty(&requests) {
            redraw.write(RequestRedraw);
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/desktop/native/accessibility.rs"]
mod tests;
