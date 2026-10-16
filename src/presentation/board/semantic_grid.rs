use bevy::math::Vec3;
use bevy_egui::egui::{self, Id, Key, Rect, Sense, Ui, UiBuilder, accesskit};

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::square::BoardSquare;
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::game::premove::GamePremoveIntent;
use crate::presentation::board::enums::{
    PresentationBoardEnumDirection, PresentationBoardEnumInputSource,
    PresentationBoardEnumPremoveSquare,
};
use crate::presentation::board::scene::PresentationBoardScene;
use crate::scene::picking::ScenePicking;

pub struct PresentationBoardSemanticAction {
    square: BoardSquare,
    source: PresentationBoardEnumInputSource,
}

impl PresentationBoardSemanticAction {
    pub const fn square(&self) -> BoardSquare {
        self.square
    }

    pub const fn source(&self) -> PresentationBoardEnumInputSource {
        self.source
    }
}

pub struct PresentationBoardSemanticGrid;

#[derive(Clone, Copy)]
pub struct PresentationBoardSemanticGridFeedback {
    selected: Option<BoardSquare>,
    pending_move: Option<GamePremoveIntent>,
}

impl PresentationBoardSemanticGridFeedback {
    pub const fn new(
        selected: Option<BoardSquare>,
        pending_move: Option<GamePremoveIntent>,
    ) -> Self {
        Self {
            selected,
            pending_move,
        }
    }
}

impl PresentationBoardSemanticGrid {
    const ID: &'static str = "chess.board.semantic";
    const DIRECTION_ALIGNMENT: f32 = std::f32::consts::FRAC_1_SQRT_2 - 0.0001;

    pub fn squares() -> impl Iterator<Item = BoardSquare> {
        (0..ScenePicking::SQUARES).filter_map(|index| BoardSquare::try_from(index).ok())
    }

    pub fn square_id(square: BoardSquare) -> Id {
        Id::new((Self::ID, u8::from(square)))
    }

    pub fn square_for_accessibility_target(target: accesskit::NodeId) -> Option<BoardSquare> {
        Self::squares().find(|square| Self::square_id(*square).accesskit_id() == target)
    }

    pub const fn color_name(color: BoardEnumColor) -> &'static str {
        match color {
            BoardEnumColor::White => "White",
            BoardEnumColor::Black => "Black",
        }
    }

    pub const fn piece_name(kind: BoardEnumPieceKind) -> &'static str {
        match kind {
            BoardEnumPieceKind::Pawn => "pawn",
            BoardEnumPieceKind::Knight => "knight",
            BoardEnumPieceKind::Bishop => "bishop",
            BoardEnumPieceKind::Rook => "rook",
            BoardEnumPieceKind::Queen => "queen",
            BoardEnumPieceKind::King => "king",
        }
    }

    pub fn label(
        square: BoardSquare,
        snapshot: &DesktopBoardSnapshot,
        feedback: PresentationBoardSemanticGridFeedback,
    ) -> String {
        let mut label = match snapshot
            .pieces()
            .iter()
            .find(|piece| piece.square() == square)
        {
            Some(piece) => format!(
                "{square}: {} {}",
                Self::color_name(piece.color()),
                Self::piece_name(piece.kind())
            ),
            None => format!("{square}: empty"),
        };
        if feedback.selected == Some(square) {
            label.push_str(", selected");
        }
        if feedback.selected.is_some()
            && snapshot.legal_moves().iter().any(|movement| {
                movement.origin() == feedback.selected && movement.destination() == square
            })
        {
            label.push_str(", legal destination");
        }
        if let Some(intent) = feedback.pending_move {
            for role in PresentationBoardEnumPremoveSquare::ALL {
                if role.square(intent) == square {
                    label.push_str(role.semantic_label());
                }
            }
        }
        label
    }

    pub fn square_rect(square: BoardSquare, scene: &PresentationBoardScene) -> Option<Rect> {
        let center = ScenePicking::square_center(square);
        let mut bounds = Rect::NOTHING;
        for offset in [
            Vec3::new(-0.5, 0.0, -0.5),
            Vec3::new(0.5, 0.0, -0.5),
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(-0.5, 0.0, 0.5),
        ] {
            let pixel = scene.camera().project(center + offset)?;
            bounds.extend_with(scene.viewport().to_logical(pixel)?);
        }
        Some(bounds)
    }

    pub fn neighbor(
        square: BoardSquare,
        direction: PresentationBoardEnumDirection,
        scene: &PresentationBoardScene,
    ) -> Option<BoardSquare> {
        let direction = direction.vector();
        let index = u8::from(square);
        let origin = scene
            .camera()
            .project(ScenePicking::square_center(square))?;
        let mut nearest = None;
        let mut best_alignment = Self::DIRECTION_ALIGNMENT;
        for candidate in Self::squares() {
            let next = u8::from(candidate);
            let file_distance = (index % ScenePicking::FILES).abs_diff(next % ScenePicking::FILES);
            let rank_distance = (index / ScenePicking::FILES).abs_diff(next / ScenePicking::FILES);
            if file_distance + rank_distance != 1 {
                continue;
            }
            let point = scene
                .camera()
                .project(ScenePicking::square_center(candidate))?;
            let delta = egui::vec2(point.x - origin.x, point.y - origin.y).normalized();
            let alignment = delta.dot(direction);
            if alignment > best_alignment {
                best_alignment = alignment;
                nearest = Some(candidate);
            }
        }
        nearest
    }

    pub fn show(
        ui: &mut Ui,
        scene: &PresentationBoardScene,
        snapshot: &DesktopBoardSnapshot,
        feedback: PresentationBoardSemanticGridFeedback,
        process_keyboard: bool,
    ) -> Vec<PresentationBoardSemanticAction> {
        let mut actions = Vec::new();
        ui.scope_builder(
            UiBuilder::new()
                .id_salt(Self::ID)
                .max_rect(scene.viewport().image()),
            |ui| {
                ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
                    node.set_role(accesskit::Role::Grid);
                    node.set_label("Chess board");
                    node.set_row_count(usize::from(ScenePicking::FILES));
                    node.set_column_count(usize::from(ScenePicking::FILES));
                });
                for square in Self::squares() {
                    let Some(rect) = Self::square_rect(square, scene) else {
                        continue;
                    };
                    let id = Self::square_id(square);
                    let response = ui.interact(rect, id, Sense::focusable_noninteractive());
                    let label = Self::label(square, snapshot, feedback);
                    ui.ctx().accesskit_node_builder(id, |node| {
                        node.set_role(accesskit::Role::GridCell);
                        node.set_label(label);
                        node.set_row_index(usize::from(u8::from(square) / ScenePicking::FILES));
                        node.set_column_index(usize::from(u8::from(square) % ScenePicking::FILES));
                        node.set_selected(feedback.selected == Some(square));
                        node.add_action(accesskit::Action::Click);
                    });
                    let count = if process_keyboard && response.enabled() {
                        ui.ctx().input(|input| {
                            input.num_accesskit_action_requests(id, accesskit::Action::Click)
                        })
                    } else {
                        0
                    };
                    for occurrence in 0..count {
                        actions.push(PresentationBoardSemanticAction {
                            square,
                            source: PresentationBoardEnumInputSource::Accessibility { occurrence },
                        });
                    }
                    ui.ctx().input_mut(|input| {
                        input.consume_accesskit_action_requests(id, |request| {
                            request.action == accesskit::Action::Click
                        });
                    });
                    if response.has_focus() {
                        ui.ctx().memory_mut(|memory| {
                            memory.set_focus_lock_filter(
                                id,
                                egui::EventFilter {
                                    horizontal_arrows: true,
                                    vertical_arrows: true,
                                    ..Default::default()
                                },
                            );
                        });
                        ui.painter().rect_stroke(
                            rect.shrink(2.0),
                            3.0,
                            egui::Stroke::new(2.0, ui.visuals().text_color()),
                            egui::StrokeKind::Inside,
                        );
                        for direction in PresentationBoardEnumDirection::ALL {
                            if process_keyboard
                                && ui.ctx().input_mut(|input| {
                                    input.consume_key(egui::Modifiers::NONE, direction.key())
                                })
                            {
                                ui.ctx().memory_mut(|memory| {
                                    memory.move_focus(egui::FocusDirection::None);
                                    if let Some(next) = Self::neighbor(square, direction, scene) {
                                        memory.request_focus(Self::square_id(next));
                                    }
                                });
                            }
                        }
                        if process_keyboard
                            && ui.ctx().input_mut(|input| {
                                input.consume_key(egui::Modifiers::NONE, Key::Enter)
                                    || input.consume_key(egui::Modifiers::NONE, Key::Space)
                            })
                        {
                            actions.push(PresentationBoardSemanticAction {
                                square,
                                source: PresentationBoardEnumInputSource::Keyboard,
                            });
                        }
                    }
                }
            },
        );
        actions
    }
}
