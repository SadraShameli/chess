use bevy_egui::egui::{self, Ui};

use crate::board::square::BoardSquare;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{DesktopApplicationPort, DesktopApplicationWakeProvider};
use crate::desktop::editor_commands::DesktopEditorCommandsSnapshot;
use crate::presentation::app::editor::{PresentationAppEditor, PresentationAppEditorError};
use crate::presentation::board::markings::{
    PresentationBoardMarkings, PresentationBoardMarkingsInput,
};
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::board::view::{
    PresentationBoardPickingProvider, PresentationBoardViewError,
};
use crate::presentation::editor::application_port::PresentationEditorApplicationPort;
use crate::presentation::editor::controller::{
    PresentationEditorControllerDrag, PresentationEditorControllerPalette,
};
use crate::scene::editor::SceneEditorBinding;
use crate::scene::enums::SceneEnumLifecycle;
use crate::scene::picking::{ScenePicking, ScenePickingViewport};

pub(super) struct PresentationAppEditorBoardGesture {
    binding: SceneEditorBinding,
    origin: BoardSquare,
    drag: Option<PresentationEditorControllerDrag>,
}

pub(in crate::presentation::app) struct PresentationAppEditorBoardInput<
    'a,
    P: PresentationBoardPickingProvider,
> {
    scene: Option<&'a PresentationBoardScene>,
    picker: &'a mut P,
    markings: &'a mut PresentationBoardMarkings,
    enabled: bool,
}

impl<'a, P: PresentationBoardPickingProvider> PresentationAppEditorBoardInput<'a, P> {
    pub(in crate::presentation::app) fn new(
        scene: Option<&'a PresentationBoardScene>,
        picker: &'a mut P,
        markings: &'a mut PresentationBoardMarkings,
    ) -> Self {
        Self {
            scene,
            picker,
            markings,
            enabled: true,
        }
    }

    pub(in crate::presentation::app) const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl PresentationAppEditor {
    pub(super) fn raw_label(snapshot: &DesktopEditorCommandsSnapshot) -> String {
        let fields = snapshot.draft().fields();
        let rights = fields
            .castling_rights()
            .map(|square| square.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Position editor, {:?}. Variant {:?}; {} to move; castling rook squares [{}]; en passant {:?}; halfmove counter {}; fullmove counter {}. {}",
            snapshot.status(),
            fields.variant(),
            PresentationBoardSemanticGrid::color_name(fields.turn()),
            rights,
            fields.en_passant(),
            fields.halfmoves(),
            fields.fullmoves(),
            if snapshot.is_dirty() {
                "Unsaved changes"
            } else {
                "Saved draft"
            }
        )
    }

    pub(in crate::presentation::app) fn show_board<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
        P: PresentationBoardPickingProvider,
    >(
        &mut self,
        ui: &mut Ui,
        application: &DesktopApplicationPort<C, W>,
        input: PresentationAppEditorBoardInput<'_, P>,
    ) -> Result<ScenePickingViewport, PresentationAppEditorError> {
        let PresentationAppEditorBoardInput {
            scene,
            picker,
            markings,
            enabled,
        } = input;
        let side = ui.available_width().min(ui.available_height()).max(1.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        let viewport =
            ScenePickingViewport::new(rect, ui.clip_rect(), ui.ctx().pixels_per_point(), 1.0)
                .map_err(PresentationBoardViewError::from)?;
        let Some(scene) = scene else {
            self.gesture = None;
            markings.cancel_gesture();
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Preparing 3D editor board",
                egui::FontId::proportional(18.0),
                ui.visuals().weak_text_color(),
            );
            return Ok(viewport);
        };
        let binding = self.binding.filter(|binding| {
            scene.generation() == binding.generation()
                && self
                    .controller
                    .snapshot()
                    .is_some_and(|snapshot| snapshot.context() == binding.context())
        });
        if binding.is_none() {
            self.gesture = None;
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Preparing current 3D editor draft",
                egui::FontId::proportional(18.0),
                ui.visuals().weak_text_color(),
            );
            return Ok(viewport);
        }
        let image_rect = if scene.viewport() == viewport {
            rect
        } else {
            scene.fit(rect)
        };
        ui.painter().image(
            scene.texture(),
            image_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        let input_marks_enabled = enabled;
        let enabled = enabled
            && self.can_input()
            && scene.viewport() == viewport
            && scene.lifecycle() == SceneEnumLifecycle::Ready
            && binding.is_some()
            && !ui.is_sizing_pass();
        if !enabled {
            self.gesture = None;
        }
        markings.paint_overlay(ui, scene);
        let response = ui.interact(
            rect,
            egui::Id::new("chess.editor.image"),
            egui::Sense::click_and_drag(),
        );
        let marks_visible = scene.viewport() == viewport
            && scene.lifecycle() == SceneEnumLifecycle::Ready
            && !ui.is_sizing_pass();
        if marks_visible {
            markings
                .handle_input(
                    ui,
                    PresentationBoardMarkingsInput {
                        enabled: input_marks_enabled,
                        picker: &mut *picker,
                        hovered: response.contains_pointer(),
                        scene,
                        viewport,
                    },
                )
                .map_err(PresentationBoardViewError::from)?;
        } else {
            markings.cancel_gesture();
        }
        let frame = ui.ctx().cumulative_frame_nr();
        let input_available = enabled && self.input_frame != Some(frame);
        let mut operation = None;
        if let Some(snapshot) = self.controller.snapshot() {
            let label = Self::raw_label(snapshot);
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(egui::accesskit::Role::Grid);
                node.set_label(label);
                node.set_row_count(usize::from(ScenePicking::FILES));
                node.set_column_count(usize::from(ScenePicking::FILES));
            });
        }
        let mut board_has_focus = false;
        for square in PresentationBoardSemanticGrid::squares() {
            let Some(bounds) = PresentationBoardSemanticGrid::square_rect(square, scene) else {
                continue;
            };
            let id = Self::square_id(square);
            let square_response = ui.interact(bounds, id, egui::Sense::focusable_noninteractive());
            let piece = self
                .controller
                .snapshot()
                .and_then(|snapshot| snapshot.draft().fields().piece_at(square));
            let label = piece.map_or_else(
                || format!("Editor {square}: empty"),
                |piece| {
                    format!(
                        "Editor {square}: {} {}",
                        PresentationBoardSemanticGrid::color_name(piece.color()),
                        PresentationBoardSemanticGrid::piece_name(piece.kind())
                    )
                },
            );
            ui.ctx().accesskit_node_builder(id, |node| {
                node.set_role(egui::accesskit::Role::GridCell);
                node.set_label(label);
                node.set_row_index(usize::from(u8::from(square) / ScenePicking::FILES));
                node.set_column_index(usize::from(u8::from(square) % ScenePicking::FILES));
                node.add_action(egui::accesskit::Action::Click);
            });
            let activated = ui.ctx().input(|input| {
                input.num_accesskit_action_requests(id, egui::accesskit::Action::Click) > 0
            });
            ui.ctx().input_mut(|input| {
                input.consume_accesskit_action_requests(id, |request| {
                    request.action == egui::accesskit::Action::Click
                })
            });
            if input_available && activated && operation.is_none() {
                operation = Some(self.controller.click_square(square)?);
            }
            if square_response.has_focus() && enabled {
                board_has_focus = true;
                self.controller.focus_square(square);
                ui.ctx().memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        id,
                        egui::EventFilter {
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            ..Default::default()
                        },
                    )
                });
                ui.painter().rect_stroke(
                    bounds.shrink(2.0),
                    3.0,
                    egui::Stroke::new(2.0, ui.visuals().text_color()),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if input_available && operation.is_none() {
            operation = self
                .view
                .keyboard(ui, &mut self.controller, board_has_focus, scene)?;
            if board_has_focus && let Some(square) = self.controller.focused_square() {
                ui.ctx()
                    .memory_mut(|memory| memory.request_focus(Self::square_id(square)));
            }
        }
        let (pressed, released, point, remove, escape) = ui.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_released(egui::PointerButton::Primary),
                input.pointer.interact_pos(),
                input.pointer.button_clicked(egui::PointerButton::Secondary),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if escape {
            self.gesture = None;
        }
        if input_available
            && operation.is_none()
            && let Some(binding) = binding
        {
            let square =
                if let Some(point) = point.filter(|point| viewport.to_physical(*point).is_some()) {
                    picker
                        .pick(point)
                        .map_err(PresentationBoardViewError::from)?
                } else {
                    None
                };
            if pressed
                && response.contains_pointer()
                && let Some(origin) = square
            {
                let drag =
                    if self.controller.snapshot().is_some_and(|snapshot| {
                        snapshot.draft().fields().piece_at(origin).is_some()
                    }) {
                        Some(self.controller.begin_drag(origin)?)
                    } else {
                        None
                    };
                self.gesture = Some(PresentationAppEditorBoardGesture {
                    binding,
                    origin,
                    drag,
                });
                ui.ctx()
                    .memory_mut(|memory| memory.request_focus(Self::square_id(origin)));
            }
            if remove
                && response.contains_pointer()
                && let Some(square) = square
            {
                operation = Some(self.controller.remove_square(square)?);
            }
            let palette = response.dnd_release_payload::<PresentationEditorControllerPalette>();
            if let Some(palette) = palette {
                if self.palette_binding == Some(binding)
                    && let Some(square) = square
                {
                    operation = Some(self.controller.drop_palette_payload(*palette, square)?);
                }
                self.palette_binding = None;
                self.gesture = None;
            } else if released
                && let Some(gesture) = self.gesture.take()
                && gesture.binding == binding
                && let Some(square) = square
            {
                operation = Some(match gesture.drag.filter(|_| gesture.origin != square) {
                    Some(drag) => self.controller.drag_square(drag, square)?,
                    None => self.controller.click_square(square)?,
                });
            }
            if egui::DragAndDrop::has_payload_of_type::<PresentationEditorControllerPalette>(
                ui.ctx(),
            ) && self.palette_binding.is_none()
            {
                self.palette_binding = Some(binding);
            }
        }
        if released {
            self.gesture = None;
            self.palette_binding = None;
        }
        if let Some(operation) = operation {
            self.input_frame = Some(frame);
            let port = PresentationEditorApplicationPort::new(self.connection.owner, application);
            self.submit(operation, &port)?;
        }
        Ok(viewport)
    }

    fn square_id(square: BoardSquare) -> egui::Id {
        egui::Id::new(("chess.editor.square", u8::from(square)))
    }
}
