use std::collections::HashMap;
use std::sync::Arc;

use bevy_egui::egui::{self, Pos2, Rect, Sense, Ui};
use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::square::BoardSquare;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::desktop::identity::DesktopRequestId;
use crate::game::premove::GamePremoveIntent;
use crate::presentation::board::animation::{
    PresentationBoardAnimation, PresentationBoardAnimationError, PresentationBoardAnimationFrame,
};
use crate::presentation::board::enums::{
    PresentationBoardEnumCameraAction, PresentationBoardEnumInputSource,
    PresentationBoardEnumPremoveSquare,
};
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::{
    PresentationBoardSemanticGrid, PresentationBoardSemanticGridFeedback,
};
use crate::presentation::board::transition::PresentationBoardTransition;
use crate::scene::camera::SceneCamera;
use crate::scene::enums::SceneEnumLifecycle;
use crate::scene::picking::{ScenePicking, ScenePickingViewport, ScenePickingViewportError};
use crate::scene::renderer::{SceneRendererError, SceneRendererGeneration};

#[derive(Debug, Error)]
pub enum PresentationBoardViewError {
    #[error(transparent)]
    Viewport(#[from] ScenePickingViewportError),
    #[error(transparent)]
    Picking(#[from] SceneRendererError),
    #[error(transparent)]
    Animation(#[from] PresentationBoardAnimationError),
}

pub trait PresentationBoardPickingProvider {
    fn pick(&mut self, point: Pos2) -> Result<Option<BoardSquare>, SceneRendererError>;
}

impl<F> PresentationBoardPickingProvider for F
where
    F: FnMut(Pos2) -> Result<Option<BoardSquare>, SceneRendererError>,
{
    fn pick(&mut self, point: Pos2) -> Result<Option<BoardSquare>, SceneRendererError> {
        self(point)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationBoardAction {
    identity: DesktopRequestId,
    square: BoardSquare,
    origin: Option<BoardSquare>,
}

impl PresentationBoardAction {
    pub const fn identity(self) -> DesktopRequestId {
        self.identity
    }

    pub const fn square(self) -> BoardSquare {
        self.square
    }

    pub const fn origin(self) -> Option<BoardSquare> {
        self.origin
    }
}

pub struct PresentationBoardViewInput<'a, P: PresentationBoardPickingProvider> {
    snapshot: &'a DesktopBoardSnapshot,
    scene: Option<&'a PresentationBoardScene>,
    picker: &'a mut P,
    enabled: bool,
    pending_move: Option<GamePremoveIntent>,
    render_scale: f32,
}

impl<'a, P: PresentationBoardPickingProvider> PresentationBoardViewInput<'a, P> {
    pub fn new(
        snapshot: &'a DesktopBoardSnapshot,
        scene: Option<&'a PresentationBoardScene>,
        picker: &'a mut P,
    ) -> Self {
        Self {
            snapshot,
            scene,
            picker,
            enabled: true,
            pending_move: None,
            render_scale: 1.0,
        }
    }

    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub const fn with_pending_move(mut self, pending_move: Option<GamePremoveIntent>) -> Self {
        self.pending_move = pending_move;
        self
    }

    pub const fn with_render_scale(mut self, render_scale: f32) -> Self {
        self.render_scale = render_scale;
        self
    }
}

pub struct PresentationBoardViewFrame {
    viewport: ScenePickingViewport,
    actions: Vec<PresentationBoardAction>,
    has_semantic_grid: bool,
    animation: Option<PresentationBoardAnimationFrame>,
}

impl PresentationBoardViewFrame {
    pub const fn animation(&self) -> Option<PresentationBoardAnimationFrame> {
        self.animation
    }

    pub const fn viewport(&self) -> ScenePickingViewport {
        self.viewport
    }

    pub fn actions(&self) -> &[PresentationBoardAction] {
        &self.actions
    }

    pub(crate) const fn has_semantic_grid(&self) -> bool {
        self.has_semantic_grid
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PresentationBoardViewInputKey {
    source: PresentationBoardEnumInputSource,
    square: BoardSquare,
    origin: Option<BoardSquare>,
}

pub struct PresentationBoardView {
    picking: Option<ScenePicking>,
    generation: Option<SceneRendererGeneration>,
    selected: Option<BoardSquare>,
    action_frame: Option<u64>,
    keyboard_frame: Option<u64>,
    retained: HashMap<PresentationBoardViewInputKey, PresentationBoardAction>,
    animation: Option<PresentationBoardAnimation>,
    observed: Option<DesktopBoardSnapshot>,
}

impl Default for PresentationBoardView {
    fn default() -> Self {
        Self::new()
    }
}

impl PresentationBoardView {
    const ID: &'static str = "chess.board.image";

    pub fn new() -> Self {
        Self {
            picking: None,
            generation: None,
            selected: None,
            action_frame: None,
            keyboard_frame: None,
            retained: HashMap::new(),
            animation: None,
            observed: None,
        }
    }

    pub fn with_animation(clock: Arc<dyn ClockTimeProvider>, reduced_motion: bool) -> Self {
        Self {
            animation: Some(PresentationBoardAnimation::new(clock, reduced_motion)),
            ..Self::new()
        }
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        if let Some(animation) = &mut self.animation {
            animation.set_reduced_motion(reduced_motion);
        }
    }

    pub fn camera_action(
        &mut self,
        camera: SceneCamera,
        action: PresentationBoardEnumCameraAction,
    ) -> Result<SceneCamera, PresentationBoardViewError> {
        Ok(match action {
            PresentationBoardEnumCameraAction::RotateLeft => camera.rotate_left(),
            PresentationBoardEnumCameraAction::RotateRight => camera.rotate_right(),
            PresentationBoardEnumCameraAction::TiltUp => camera.tilt_up(),
            PresentationBoardEnumCameraAction::TiltDown => camera.tilt_down(),
            PresentationBoardEnumCameraAction::ZoomIn => camera.zoom_in(),
            PresentationBoardEnumCameraAction::ZoomOut => camera.zoom_out(),
            PresentationBoardEnumCameraAction::Reset => match &mut self.animation {
                Some(animation) => animation.start_camera_reset(camera)?.0,
                None => camera.reset(),
            },
            PresentationBoardEnumCameraAction::Flip => self.auto_flip(camera)?,
        })
    }

    pub fn auto_flip(
        &mut self,
        camera: SceneCamera,
    ) -> Result<SceneCamera, PresentationBoardViewError> {
        if let Some(animation) = &mut self.animation {
            animation.start_flip()?;
        }
        Ok(camera.with_orientation(match camera.orientation() {
            BoardEnumColor::White => BoardEnumColor::Black,
            BoardEnumColor::Black => BoardEnumColor::White,
        }))
    }

    pub fn set_panel_visible(&mut self, visible: bool) -> Result<(), PresentationBoardViewError> {
        if let Some(animation) = &mut self.animation {
            animation.set_panel_visible(visible)?;
        }
        Ok(())
    }

    pub fn panel_progress(&mut self) -> Result<f32, PresentationBoardViewError> {
        Ok(match &mut self.animation {
            Some(animation) => animation.frame()?.panel_progress(),
            None => 1.0,
        })
    }

    fn advance_animation(
        &mut self,
        ui: &Ui,
        snapshot: &DesktopBoardSnapshot,
    ) -> Result<Option<PresentationBoardAnimationFrame>, PresentationBoardViewError> {
        let Some(animation) = &mut self.animation else {
            return Ok(None);
        };
        if let Some(previous) = &self.observed
            && previous.fen() != snapshot.fen()
        {
            match PresentationBoardTransition::piece_move(previous, snapshot) {
                Some((origin, destination)) => {
                    animation.start_piece_move(origin, destination)?;
                }
                None => animation.cancel_obsolete_move(),
            }
        }
        self.observed = Some(snapshot.clone());
        let frame = animation.frame()?;
        animation.request_repaint(ui.ctx());
        Ok(Some(frame))
    }

    pub const fn selected(&self) -> Option<BoardSquare> {
        self.selected
    }

    pub fn set_selected(&mut self, selected: Option<BoardSquare>) {
        if self.selected != selected {
            self.cancel_gesture();
            self.selected = selected;
        }
    }

    pub fn cancel_gesture(&mut self) {
        if let Some(picking) = &mut self.picking {
            picking.cancel();
        }
    }

    fn discard_gesture_input(&mut self, ui: &Ui) {
        self.cancel_gesture();
        if ui
            .ctx()
            .input(|input| input.pointer.button_released(egui::PointerButton::Primary))
            && let Some(generation) = self.generation
            && let Some(picking) = &mut self.picking
        {
            picking.release(generation, None);
        }
    }

    fn synchronize(&mut self, generation: SceneRendererGeneration) {
        match &mut self.picking {
            Some(picking) => picking.invalidate(generation),
            None => self.picking = Some(ScenePicking::new(generation)),
        }
        self.generation = Some(generation);
    }

    fn activate(
        &mut self,
        frame: u64,
        square: BoardSquare,
        source: PresentationBoardEnumInputSource,
        origin: Option<BoardSquare>,
    ) -> Option<PresentationBoardAction> {
        if self.action_frame != Some(frame) {
            self.action_frame = Some(frame);
            self.retained.clear();
        }
        let key = PresentationBoardViewInputKey {
            source,
            square,
            origin,
        };
        if self.retained.contains_key(&key) {
            return None;
        }
        let action = PresentationBoardAction {
            identity: DesktopRequestId::new(),
            square,
            origin,
        };
        self.retained.insert(key, action);
        self.selected = Some(square);
        Some(action)
    }

    pub fn show<P: PresentationBoardPickingProvider>(
        &mut self,
        ui: &mut Ui,
        input: PresentationBoardViewInput<'_, P>,
    ) -> Result<PresentationBoardViewFrame, PresentationBoardViewError> {
        let side = ui.available_width().min(ui.available_height()).max(1.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
        self.show_at(ui, rect, input)
    }

    pub fn show_at<P: PresentationBoardPickingProvider>(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        input: PresentationBoardViewInput<'_, P>,
    ) -> Result<PresentationBoardViewFrame, PresentationBoardViewError> {
        let viewport = ScenePickingViewport::new(
            rect,
            ui.clip_rect(),
            ui.ctx().pixels_per_point(),
            input.render_scale,
        )?;
        let mut output = PresentationBoardViewFrame {
            viewport,
            actions: Vec::new(),
            has_semantic_grid: false,
            animation: self.advance_animation(ui, input.snapshot)?,
        };
        let Some(scene) = input.scene else {
            self.discard_gesture_input(ui);
            ui.painter()
                .rect_filled(rect, 12.0, ui.visuals().window_fill);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Preparing 3D board",
                egui::FontId::proportional(18.0),
                ui.visuals().weak_text_color(),
            );
            return Ok(output);
        };
        self.synchronize(scene.generation());
        ui.painter().image(
            scene.texture(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        if scene.viewport() != viewport || scene.lifecycle() != SceneEnumLifecycle::Ready {
            self.discard_gesture_input(ui);
            return Ok(output);
        }
        if let Some(intent) = input.pending_move {
            Self::show_pending_move(ui, scene, intent);
        }
        if !input.enabled {
            self.discard_gesture_input(ui);
        }
        let response = ui.interact(rect, egui::Id::new(Self::ID), Sense::click_and_drag());
        let frame = ui.ctx().cumulative_frame_nr();
        let (pressed, released, point, escape) = ui.ctx().input(|state| {
            (
                state.pointer.button_pressed(egui::PointerButton::Primary),
                state.pointer.button_released(egui::PointerButton::Primary),
                state.pointer.interact_pos(),
                state.key_pressed(egui::Key::Escape),
            )
        });
        if escape {
            self.cancel_gesture();
        }
        if input.enabled
            && pressed
            && response.contains_pointer()
            && let Some(point) = point
            && viewport.to_physical(point).is_some()
            && let Some(square) = input.picker.pick(point)?
            && let Some(picking) = &mut self.picking
        {
            picking.press(scene.generation(), square);
        }
        if input.enabled && released {
            let square = match point {
                Some(point) if viewport.to_physical(point).is_some() => input.picker.pick(point)?,
                Some(_) | None => None,
            };
            if let Some((origin, destination)) = self
                .picking
                .as_mut()
                .and_then(|picking| picking.release(scene.generation(), square))
                && let Some(action) = self.activate(
                    frame,
                    destination,
                    PresentationBoardEnumInputSource::Pointer,
                    (origin != destination).then_some(origin),
                )
            {
                ui.ctx().memory_mut(|memory| {
                    memory.request_focus(PresentationBoardSemanticGrid::square_id(destination));
                });
                output.actions.push(action);
            }
        }
        let process_keyboard = input.enabled && self.keyboard_frame != Some(frame);
        self.keyboard_frame = Some(frame);
        for action in PresentationBoardSemanticGrid::show(
            ui,
            scene,
            input.snapshot,
            PresentationBoardSemanticGridFeedback::new(self.selected, input.pending_move),
            process_keyboard,
        ) {
            if let Some(action) = self.activate(frame, action.square(), action.source(), None) {
                output.actions.push(action);
            }
        }
        output.has_semantic_grid = true;
        if let Some(square) = self.selected
            && let Some(rect) = PresentationBoardSemanticGrid::square_rect(square, scene)
        {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                3.0,
                egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        Ok(output)
    }

    fn show_pending_move(ui: &Ui, scene: &PresentationBoardScene, intent: GamePremoveIntent) {
        let painter = ui
            .painter()
            .with_clip_rect(scene.viewport().image().intersect(ui.clip_rect()));
        for role in PresentationBoardEnumPremoveSquare::ALL {
            let Some(rect) = PresentationBoardSemanticGrid::square_rect(role.square(intent), scene)
            else {
                continue;
            };
            let color = role.color(ui.visuals());
            painter.rect_stroke(
                rect.shrink(3.0),
                3.0,
                egui::Stroke::new(3.0, color),
                egui::StrokeKind::Inside,
            );
            let galley = painter.layout_no_wrap(
                role.label().to_owned(),
                egui::TextStyle::Small.resolve(ui.style()),
                color,
            );
            let label = Rect::from_min_size(rect.min, galley.size());
            painter.rect_filled(label.expand(2.0), 3.0, ui.visuals().window_fill);
            painter.galley(label.min, galley, color);
        }
    }
}
