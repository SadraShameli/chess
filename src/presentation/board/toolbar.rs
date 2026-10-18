use bevy_egui::egui::{
    self, Align2, Color32, CornerRadius, Id, Margin, Order, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use strum::IntoEnumIterator;

use crate::presentation::board::enums::PresentationBoardEnumCameraAction;
use crate::presentation::board::markings::{PresentationBoardMarkColor, PresentationBoardMarkings};
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationBoardToolbarEnumGlyph {
    Icon(PresentationThemeEnumIcon),
    RotateLeft,
    RotateRight,
    TiltUp,
    TiltDown,
    ZoomOut,
}

impl PresentationBoardToolbarEnumGlyph {
    const GRID: f32 = 24.0;
    const STROKE: f32 = 1.75;

    fn paint(self, painter: &egui::Painter, rect: Rect, color: Color32) {
        let scale = rect.width().min(rect.height()) / Self::GRID;
        let origin = rect.center() - Vec2::splat(Self::GRID * scale / 2.0);
        let at = |x: f32, y: f32| -> Pos2 { origin + Vec2::new(x, y) * scale };
        let stroke = Stroke::new(Self::STROKE * scale.max(0.75), color);
        let line = |points: &[(f32, f32)]| {
            let points: Vec<Pos2> = points.iter().map(|(x, y)| at(*x, *y)).collect();
            painter.add(egui::Shape::line(points, stroke));
        };
        match self {
            Self::Icon(icon) => PresentationThemeIcon::paint(painter, rect, icon, color),
            Self::RotateLeft => {
                line(&[
                    (20.0, 14.0),
                    (20.0, 10.0),
                    (17.0, 6.0),
                    (12.0, 5.0),
                    (7.0, 7.0),
                    (5.0, 11.0),
                ]);
                line(&[(2.0, 8.0), (5.0, 11.0), (8.0, 8.0)]);
                line(&[(4.0, 19.0), (20.0, 19.0)]);
            }
            Self::RotateRight => {
                line(&[
                    (4.0, 14.0),
                    (4.0, 10.0),
                    (7.0, 6.0),
                    (12.0, 5.0),
                    (17.0, 7.0),
                    (19.0, 11.0),
                ]);
                line(&[(22.0, 8.0), (19.0, 11.0), (16.0, 8.0)]);
                line(&[(4.0, 19.0), (20.0, 19.0)]);
            }
            Self::TiltUp => line(&[(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)]),
            Self::TiltDown => line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
            Self::ZoomOut => line(&[(5.0, 12.0), (19.0, 12.0)]),
        }
    }
}

pub struct PresentationBoardToolbar;

impl PresentationBoardToolbar {
    const ACTIONS: [(
        &'static str,
        PresentationBoardEnumCameraAction,
        PresentationBoardToolbarEnumGlyph,
    ); 8] = [
        (
            "Rotate left",
            PresentationBoardEnumCameraAction::RotateLeft,
            PresentationBoardToolbarEnumGlyph::RotateLeft,
        ),
        (
            "Rotate right",
            PresentationBoardEnumCameraAction::RotateRight,
            PresentationBoardToolbarEnumGlyph::RotateRight,
        ),
        (
            "Tilt up",
            PresentationBoardEnumCameraAction::TiltUp,
            PresentationBoardToolbarEnumGlyph::TiltUp,
        ),
        (
            "Tilt down",
            PresentationBoardEnumCameraAction::TiltDown,
            PresentationBoardToolbarEnumGlyph::TiltDown,
        ),
        (
            "Zoom in",
            PresentationBoardEnumCameraAction::ZoomIn,
            PresentationBoardToolbarEnumGlyph::Icon(PresentationThemeEnumIcon::Plus),
        ),
        (
            "Zoom out",
            PresentationBoardEnumCameraAction::ZoomOut,
            PresentationBoardToolbarEnumGlyph::ZoomOut,
        ),
        (
            "Reset view",
            PresentationBoardEnumCameraAction::Reset,
            PresentationBoardToolbarEnumGlyph::Icon(PresentationThemeEnumIcon::ResetCamera),
        ),
        (
            "Flip board",
            PresentationBoardEnumCameraAction::Flip,
            PresentationBoardToolbarEnumGlyph::Icon(PresentationThemeEnumIcon::Flip),
        ),
    ];
    const MARKINGS_LABEL: &'static str = "Board markings";
    const INSET: f32 = 16.0;
    const GAP: f32 = 4.0;
    const POPOVER_WIDTH: f32 = 320.0;

    fn button(
        ui: &mut Ui,
        glyph: PresentationBoardToolbarEnumGlyph,
        label: &str,
        selected: bool,
    ) -> egui::Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let enabled = ui.is_enabled();
        let size = Vec2::splat(PresentationThemeTokens::CONTROL_SM);
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(size, sense);
        let hovered = response.hovered() && enabled;
        let fill = if selected {
            colors.accent_soft
        } else if hovered {
            colors.surface_2
        } else {
            Color32::TRANSPARENT
        };
        let color = if selected {
            colors.accent
        } else if !enabled {
            colors.text_3.gamma_multiply(0.6)
        } else if hovered {
            colors.text
        } else {
            colors.text_2
        };
        ui.painter()
            .rect_filled(rect, PresentationThemeTokens::RADIUS_MD, fill);
        glyph.paint(ui.painter(), rect.shrink(6.0), color);
        let label = label.to_owned();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, label.clone())
        });
        response.on_hover_text(label)
    }

    pub fn show(
        ui: &mut Ui,
        stage: Rect,
        enabled: bool,
        markings: &mut PresentationBoardMarkings,
    ) -> Option<PresentationBoardEnumCameraAction> {
        let colors = PresentationThemeColors::of(ui.ctx());
        let popover_id = Id::new("chess.board.toolbar.markings");
        let mut popover_open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(popover_id))
            .unwrap_or(false);
        let count = Self::ACTIONS.len().saturating_add(1);
        let buttons = f32::from(u16::try_from(count).unwrap_or(u16::MAX));
        let size = Vec2::new(
            buttons * PresentationThemeTokens::CONTROL_SM
                + (buttons - 1.0) * Self::GAP
                + 2.0 * Self::GAP
                + 2.0,
            PresentationThemeTokens::CONTROL_SM + 2.0 * Self::GAP + 2.0,
        );
        let corner = stage.right_bottom() - Vec2::splat(Self::INSET);
        let group = Rect::from_min_max(corner - size, corner);
        let mut selected = None;
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(group)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        egui::Frame::new()
            .fill(colors.surface_1)
            .stroke(Stroke::new(1.0, colors.line))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(4))
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.x = Self::GAP;
                ui.horizontal(|ui| {
                    if Self::button(
                        ui,
                        PresentationBoardToolbarEnumGlyph::Icon(PresentationThemeEnumIcon::Grid),
                        Self::MARKINGS_LABEL,
                        popover_open,
                    )
                    .clicked()
                    {
                        popover_open = !popover_open;
                    }
                    ui.add_enabled_ui(enabled, |ui| {
                        for (label, action, glyph) in Self::ACTIONS {
                            if Self::button(ui, glyph, label, false).clicked() {
                                selected = Some(action);
                            }
                        }
                    });
                });
            });
        if popover_open {
            egui::Area::new(popover_id)
                .order(Order::Middle)
                .fixed_pos(egui::pos2(group.right(), group.top() - Self::GAP * 2.0))
                .pivot(Align2::RIGHT_BOTTOM)
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(colors.surface_2)
                        .stroke(Stroke::new(1.0, colors.line))
                        .corner_radius(CornerRadius::same(PresentationThemeTokens::RADIUS_LG))
                        .shadow(ui.visuals().popup_shadow)
                        .inner_margin(Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_width(Self::POPOVER_WIDTH);
                            PresentationThemeComponents::eyebrow(ui, Self::MARKINGS_LABEL);
                            ui.horizontal_wrapped(|ui| {
                                Self::show_markings(ui, markings);
                            });
                        });
                });
        }
        ui.ctx()
            .data_mut(|data| data.insert_temp(popover_id, popover_open));
        selected
    }

    fn show_markings(ui: &mut Ui, markings: &mut PresentationBoardMarkings) {
        PresentationThemeComponents::field_label(ui, "Marking color");
        let mut color = markings.selected_color();
        for choice in PresentationBoardMarkColor::iter() {
            ui.selectable_value(
                &mut color,
                choice,
                egui::RichText::new(choice.label()).color(choice.color()),
            );
        }
        markings.select_color(color);
        ui.label("Mark from");
        ui.add(
            egui::TextEdit::singleline(markings.entry_origin_mut())
                .id_salt("chess.board.marking_origin")
                .hint_text("e2")
                .desired_width(40.0),
        )
        .widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Marking source square")
        });
        ui.label("to");
        ui.add(
            egui::TextEdit::singleline(markings.entry_destination_mut())
                .id_salt("chess.board.marking_destination")
                .hint_text("e4")
                .desired_width(40.0),
        )
        .widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                "Marking destination square for arrows",
            )
        });
        if PresentationThemeEnumButton::Secondary
            .show_sized(ui, "Mark Circle", PresentationThemeEnumSize::Small)
            .clicked()
        {
            markings.toggle_entry(false);
        }
        if PresentationThemeEnumButton::Secondary
            .show_sized(ui, "Mark Arrow", PresentationThemeEnumSize::Small)
            .clicked()
        {
            markings.toggle_entry(true);
        }
        if markings.is_entry_invalid() {
            ui.label("Enter squares such as e2 and e4, different for arrows");
        }
        let clear = PresentationThemeEnumButton::Ghost.widget(
            ui,
            "Clear Board Markings",
            PresentationThemeEnumSize::Small,
        );
        if ui.add_enabled(markings.has_temporary(), clear).clicked() {
            markings.clear();
        }
        let aids = markings.aids_mut();
        ui.checkbox(&mut aids.legal_dots, "Legal moves");
        ui.checkbox(&mut aids.last_move, "Last move");
        ui.checkbox(&mut aids.check, "Check");
        ui.checkbox(&mut aids.coordinates, "Coordinates");
        ui.checkbox(&mut aids.captured, "Captured");
        if markings.aids().captured {
            let captures = markings.captures();
            let text = if captures.is_empty() {
                "No captures".to_owned()
            } else {
                captures
                    .iter()
                    .map(|capture| {
                        format!(
                            "{} {} x{}",
                            PresentationBoardSemanticGrid::color_name(capture.color),
                            PresentationBoardSemanticGrid::piece_name(capture.kind),
                            capture.count
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            ui.label(format!("Captured: {text}"));
        }
    }
}
