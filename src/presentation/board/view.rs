use std::collections::HashMap;

use bevy_egui::egui::{self, Pos2, Rect, Sense, Ui};
use thiserror::Error;

use crate::board::square::BoardSquare;
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::desktop::identity::DesktopRequestId;
use crate::presentation::board::enums::PresentationBoardEnumInputSource;
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::scene::enums::SceneEnumLifecycle;
use crate::scene::picking::{ScenePicking, ScenePickingViewport, ScenePickingViewportError};
use crate::scene::renderer::{SceneRendererError, SceneRendererGeneration};

#[derive(Debug, Error)]
pub enum PresentationBoardViewError {
    #[error(transparent)]
    Viewport(#[from] ScenePickingViewportError),
    #[error(transparent)]
    Picking(#[from] SceneRendererError),
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
}

impl PresentationBoardAction {
    pub const fn identity(self) -> DesktopRequestId {
        self.identity
    }

    pub const fn square(self) -> BoardSquare {
        self.square
    }
}

pub struct PresentationBoardViewInput<'a, P: PresentationBoardPickingProvider> {
    snapshot: &'a DesktopBoardSnapshot,
    scene: Option<&'a PresentationBoardScene>,
    picker: &'a mut P,
    enabled: bool,
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
            render_scale: 1.0,
        }
    }

    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
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
}

impl PresentationBoardViewFrame {
    pub const fn viewport(&self) -> ScenePickingViewport {
        self.viewport
    }

    pub fn actions(&self) -> &[PresentationBoardAction] {
        &self.actions
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PresentationBoardViewInputKey {
    source: PresentationBoardEnumInputSource,
    square: BoardSquare,
}

pub struct PresentationBoardView {
    picking: Option<ScenePicking>,
    generation: Option<SceneRendererGeneration>,
    selected: Option<BoardSquare>,
    action_frame: Option<u64>,
    keyboard_frame: Option<u64>,
    retained: HashMap<PresentationBoardViewInputKey, PresentationBoardAction>,
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
        }
    }

    pub const fn selected(&self) -> Option<BoardSquare> {
        self.selected
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
    ) -> Option<PresentationBoardAction> {
        if self.action_frame != Some(frame) {
            self.action_frame = Some(frame);
            self.retained.clear();
        }
        let key = PresentationBoardViewInputKey { source, square };
        if self.retained.contains_key(&key) {
            return None;
        }
        let action = PresentationBoardAction {
            identity: DesktopRequestId::new(),
            square,
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
        if scene.viewport() != viewport
            || scene.lifecycle() != SceneEnumLifecycle::Ready
            || !input.enabled
        {
            self.discard_gesture_input(ui);
            return Ok(output);
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
        if pressed
            && response.contains_pointer()
            && let Some(point) = point
            && viewport.to_physical(point).is_some()
            && let Some(square) = input.picker.pick(point)?
            && let Some(picking) = &mut self.picking
        {
            picking.press(scene.generation(), square);
        }
        if released {
            let square = match point {
                Some(point) if viewport.to_physical(point).is_some() => input.picker.pick(point)?,
                Some(_) | None => None,
            };
            if let Some((_, destination)) = self
                .picking
                .as_mut()
                .and_then(|picking| picking.release(scene.generation(), square))
                && let Some(action) = self.activate(
                    frame,
                    destination,
                    PresentationBoardEnumInputSource::Pointer,
                )
            {
                ui.ctx().memory_mut(|memory| {
                    memory.request_focus(PresentationBoardSemanticGrid::square_id(destination));
                });
                output.actions.push(action);
            }
        }
        let process_keyboard = self.keyboard_frame != Some(frame);
        self.keyboard_frame = Some(frame);
        for action in PresentationBoardSemanticGrid::show(
            ui,
            scene,
            input.snapshot,
            self.selected,
            process_keyboard,
        ) {
            if let Some(action) = self.activate(frame, action.square(), action.source()) {
                output.actions.push(action);
            }
        }
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
}
