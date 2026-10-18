use bevy_egui::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, Response, RichText, Sense, Stroke,
    Ui, Vec2, WidgetText,
};

use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationThemeEnumButton {
    Primary,
    Secondary,
    Ghost,
    Danger,
    DangerSolid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationThemeEnumSize {
    Small,
    Medium,
    Large,
}

impl PresentationThemeEnumSize {
    const fn height(self) -> f32 {
        match self {
            Self::Small => PresentationThemeTokens::CONTROL_SM,
            Self::Medium => PresentationThemeTokens::CONTROL_MD,
            Self::Large => PresentationThemeTokens::CONTROL_LG,
        }
    }

    const fn points(self) -> f32 {
        match self {
            Self::Small => PresentationThemeTypography::CAPTION_POINTS,
            Self::Medium => PresentationThemeTypography::UI_POINTS,
            Self::Large => PresentationThemeTypography::BODY_POINTS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationThemeEnumTone {
    Neutral,
    Accent,
    Run,
    Low,
    Error,
    Unsaved,
}

impl PresentationThemeEnumTone {
    const fn colors(self, colors: &PresentationThemeColors) -> (Color32, Color32) {
        match self {
            Self::Neutral => (colors.text_2, colors.surface_3),
            Self::Accent => (colors.accent, colors.accent_soft),
            Self::Run => (colors.run, colors.run_soft),
            Self::Low => (colors.low, colors.low_soft),
            Self::Error => (colors.err, colors.err_soft),
            Self::Unsaved => (colors.unsaved, colors.unsaved_soft),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationThemeEnumClock {
    Running,
    Low,
    Idle,
}

impl PresentationThemeEnumButton {
    pub fn widget<'a>(
        self,
        ui: &Ui,
        label: impl Into<String>,
        size: PresentationThemeEnumSize,
    ) -> egui::Button<'a> {
        let colors = PresentationThemeColors::of(ui.ctx());
        let (fill, text, stroke) = match self {
            Self::Primary => (colors.accent, colors.on_accent, Color32::TRANSPARENT),
            Self::Secondary => (colors.surface_2, colors.text, colors.line),
            Self::Ghost => (Color32::TRANSPARENT, colors.text_2, Color32::TRANSPARENT),
            Self::Danger => (colors.surface_2, colors.err, colors.line),
            Self::DangerSolid => (colors.err, colors.on_err, Color32::TRANSPARENT),
        };
        let mut rich = RichText::new(label.into()).size(size.points()).color(text);
        if matches!(self, Self::Primary | Self::DangerSolid) {
            rich = rich.strong();
        }
        egui::Button::new(rich)
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke))
            .corner_radius(CornerRadius::same(PresentationThemeTokens::RADIUS_MD))
            .min_size(Vec2::new(size.height(), size.height()))
    }

    pub fn show(self, ui: &mut Ui, label: impl Into<String>) -> Response {
        let button = self.widget(ui, label, PresentationThemeEnumSize::Medium);
        ui.add(button)
    }

    pub fn show_sized(
        self,
        ui: &mut Ui,
        label: impl Into<String>,
        size: PresentationThemeEnumSize,
    ) -> Response {
        let button = self.widget(ui, label, size);
        ui.add(button)
    }
}

pub struct PresentationThemeComponents;

impl PresentationThemeComponents {
    pub fn rail_item(
        ui: &mut Ui,
        icon: PresentationThemeEnumIcon,
        label: &str,
        selected: bool,
        enabled: bool,
    ) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let (rect, response) = ui.allocate_exact_size(Vec2::new(60.0, 56.0), sense);
        let (fill, text) = if selected {
            (colors.accent_soft, colors.accent)
        } else if response.hovered() && enabled {
            (colors.surface_2, colors.text)
        } else {
            (Color32::TRANSPARENT, colors.text_3)
        };
        let text = if enabled {
            text
        } else {
            text.gamma_multiply(0.45)
        };
        let painter = ui.painter();
        painter.rect_filled(rect, PresentationThemeTokens::RADIUS_LG, fill);
        let icon_rect = egui::Rect::from_center_size(
            rect.center() - Vec2::new(0.0, 8.0),
            Vec2::splat(20.0),
        );
        PresentationThemeIcon::paint(painter, icon_rect, icon, text);
        painter.text(
            rect.center() + Vec2::new(0.0, 13.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(PresentationThemeTypography::MICRO_POINTS),
            text,
        );
        let label = label.to_owned();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, label.clone())
        });
        response
    }

    pub fn tab(ui: &mut Ui, label: impl Into<WidgetText>, selected: bool) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let stroke = if selected {
            Stroke::new(1.0, colors.line)
        } else {
            Stroke::NONE
        };
        ui.add(
            egui::Button::selectable(selected, label)
                .fill(if selected {
                    colors.surface_2
                } else {
                    Color32::TRANSPARENT
                })
                .stroke(stroke)
                .corner_radius(CornerRadius::same(PresentationThemeTokens::RADIUS_MD))
                .min_size(Vec2::new(0.0, PresentationThemeTokens::CONTROL_MD)),
        )
    }

    pub fn eyebrow(ui: &mut Ui, text: &str) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.label(
            RichText::new(text.to_uppercase())
                .size(PresentationThemeTypography::MICRO_POINTS)
                .strong()
                .extra_letter_spacing(0.9)
                .color(colors.text_3),
        )
    }

    pub fn subhead(ui: &mut Ui, text: impl Into<String>) -> Response {
        ui.label(
            RichText::new(text)
                .size(PresentationThemeTypography::SUBHEAD_POINTS)
                .strong(),
        )
    }

    pub fn title(ui: &mut Ui, text: impl Into<String>) -> Response {
        ui.label(
            RichText::new(text)
                .size(PresentationThemeTypography::TITLE_POINTS)
                .strong(),
        )
    }

    pub fn caption(ui: &mut Ui, text: impl Into<String>) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.label(
            RichText::new(text)
                .size(PresentationThemeTypography::CAPTION_POINTS)
                .color(colors.text_3),
        )
    }

    pub fn pill(ui: &mut Ui, text: impl Into<String>, tone: PresentationThemeEnumTone) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let (fg, bg) = tone.colors(&colors);
        Frame::new()
            .fill(bg)
            .corner_radius(CornerRadius::same(u8::MAX))
            .inner_margin(Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(text)
                        .size(PresentationThemeTypography::MICRO_POINTS)
                        .strong()
                        .color(fg),
                )
            })
            .inner
    }

    pub fn kbd(ui: &mut Ui, key: &str) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        Frame::new()
            .fill(colors.surface_1)
            .stroke(Stroke::new(1.0, colors.line))
            .corner_radius(CornerRadius::same(PresentationThemeTokens::RADIUS_SM))
            .inner_margin(Margin::symmetric(5, 2))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(key)
                        .size(PresentationThemeTypography::MICRO_POINTS)
                        .monospace()
                        .color(colors.text_2),
                )
            })
            .inner
    }

    pub fn clock(
        ui: &mut Ui,
        time: &str,
        state: PresentationThemeEnumClock,
        remaining: Option<f32>,
    ) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let color = match state {
            PresentationThemeEnumClock::Running => colors.text,
            PresentationThemeEnumClock::Low => colors.low,
            PresentationThemeEnumClock::Idle => colors.text_3,
        };
        ui.with_layout(Layout::top_down(Align::Max), |ui| {
            let response = ui.label(
                RichText::new(time)
                    .size(PresentationThemeTypography::CLOCK_POINTS)
                    .strong()
                    .color(color),
            );
            if state != PresentationThemeEnumClock::Idle
                && let Some(share) = remaining
            {
                let width = response.rect.width().max(104.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 2.0), Sense::hover());
                ui.painter().rect_filled(rect, 1.0, colors.line);
                let filled = rect.width() * share.clamp(0.0, 1.0);
                let bar =
                    egui::Rect::from_min_max(egui::pos2(rect.max.x - filled, rect.min.y), rect.max);
                ui.painter().rect_filled(bar, 1.0, color);
            }
            response
        })
        .inner
    }

    pub fn modal(context: &egui::Context, id: egui::Id) -> egui::Modal {
        let colors = PresentationThemeColors::of(context);
        egui::Modal::new(id).backdrop_color(colors.scrim).frame(
            Frame::new()
                .fill(colors.surface_2)
                .stroke(Stroke::new(1.0, colors.line))
                .corner_radius(CornerRadius::same(PresentationThemeTokens::RADIUS_XL))
                .shadow(context.global_style().visuals.window_shadow)
                .inner_margin(Margin::same(20)),
        )
    }

    pub fn panel() -> Frame {
        Frame::new().inner_margin(Margin::same(16))
    }

    pub fn card(ui: &Ui) -> Frame {
        let colors = PresentationThemeColors::of(ui.ctx());
        Frame::new()
            .fill(colors.surface_1)
            .stroke(Stroke::new(1.0, colors.line))
            .corner_radius(CornerRadius::same(PresentationThemeTokens::RADIUS_LG))
            .inner_margin(Margin::same(16))
    }

    pub fn bar(ui: &Ui, fill: Color32) -> Frame {
        let colors = PresentationThemeColors::of(ui.ctx());
        Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, colors.line))
            .inner_margin(Margin::symmetric(12, 8))
    }

    pub fn segmented<T: Copy + PartialEq>(
        ui: &mut Ui,
        value: &mut T,
        options: &[(T, &str)],
        wide: bool,
    ) -> bool {
        let colors = PresentationThemeColors::of(ui.ctx());
        let mut changed = false;
        Frame::new()
            .fill(colors.surface_1)
            .stroke(Stroke::new(1.0, colors.line))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(2))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.horizontal(|ui| {
                    let count = options.len().max(1) as f32;
                    let width = if wide {
                        (ui.available_width() - 2.0 * (count - 1.0)) / count
                    } else {
                        0.0
                    };
                    for (option, label) in options {
                        let on = *value == *option;
                        let text = RichText::new(*label)
                            .size(PresentationThemeTypography::CAPTION_POINTS)
                            .color(if on { colors.text } else { colors.text_2 });
                        let response = ui.add(
                            egui::Button::selectable(on, text)
                                .fill(if on {
                                    colors.surface_3
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .stroke(Stroke::NONE)
                                .corner_radius(CornerRadius::same(
                                    PresentationThemeTokens::RADIUS_MD,
                                ))
                                .min_size(Vec2::new(width, PresentationThemeTokens::CONTROL_SM)),
                        );
                        if response.clicked() && !on {
                            *value = *option;
                            changed = true;
                        }
                    }
                });
            });
        changed
    }

    pub fn list_row(
        ui: &mut Ui,
        label: &str,
        meta: Option<&str>,
        selected: bool,
    ) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
        let fill = if selected {
            colors.accent_soft
        } else if response.hovered() {
            colors.surface_2
        } else {
            Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, PresentationThemeTokens::RADIUS_MD, fill);
        let font = egui::FontId::proportional(PresentationThemeTypography::UI_POINTS);
        ui.painter().text(
            rect.left_center() + Vec2::new(10.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            font,
            colors.text,
        );
        if let Some(meta) = meta {
            ui.painter().text(
                rect.right_center() - Vec2::new(10.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                meta,
                egui::FontId::proportional(PresentationThemeTypography::CAPTION_POINTS),
                colors.text_3,
            );
        }
        let label = label.to_owned();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label.clone())
        });
        response
    }

    pub fn field_label(ui: &mut Ui, text: &str) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.label(
            RichText::new(text)
                .size(PresentationThemeTypography::CAPTION_POINTS)
                .color(colors.text_2),
        )
    }

    pub fn input(ui: &mut Ui, text: &mut String, hint: &str) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(RichText::new(hint).color(colors.text_3))
                .background_color(colors.bg)
                .margin(Margin::symmetric(10, 7))
                .desired_width(f32::INFINITY),
        )
    }

    pub fn divider(ui: &mut Ui) {
        let colors = PresentationThemeColors::of(ui.ctx());
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            Stroke::new(1.0, colors.line),
        );
    }
}
