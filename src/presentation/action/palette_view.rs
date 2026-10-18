use bevy_egui::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Id, Key, Margin, Modifiers, Order, Sense,
    Stroke, Ui, Vec2,
    text::{LayoutJob, TextFormat},
};

use crate::desktop::preferences::DesktopPreferences;
use crate::presentation::action::palette::{
    PresentationActionPalette, PresentationActionPaletteEntry,
};
use crate::presentation::action::registry::{
    PresentationActionContext, PresentationActionEnumAvailability, PresentationActionEnumId,
    PresentationActionEnumWorkspace, PresentationActionRegistry,
};
use crate::presentation::action::shortcuts::{
    PresentationShortcutEnumPlatform, PresentationShortcuts,
};
use crate::presentation::theme::components::PresentationThemeComponents;
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationActionPaletteEnumChoice {
    Workspace(PresentationActionEnumWorkspace),
    System(PresentationActionEnumId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationActionPaletteEnumGroup {
    Actions,
    System,
}

impl PresentationActionPaletteEnumGroup {
    const fn label(self) -> &'static str {
        match self {
            Self::Actions => "Actions",
            Self::System => "System",
        }
    }
}

#[derive(Debug, Clone)]
struct PresentationActionPaletteItem {
    choice: PresentationActionPaletteEnumChoice,
    group: PresentationActionPaletteEnumGroup,
    icon: PresentationThemeEnumIcon,
    label: &'static str,
    availability: PresentationActionEnumAvailability,
    shortcut: Option<String>,
}

impl PresentationActionPaletteItem {
    fn workspace(entry: PresentationActionPaletteEntry, preferences: DesktopPreferences) -> Self {
        Self {
            choice: PresentationActionPaletteEnumChoice::Workspace(entry.action()),
            group: PresentationActionPaletteEnumGroup::Actions,
            icon: PresentationActionPaletteView::icon(entry.action()),
            label: entry.action().label(),
            availability: entry.availability(),
            shortcut: PresentationActionPaletteView::shortcut_text(preferences, entry.action()),
        }
    }

    const fn system_icon(action: PresentationActionEnumId) -> Option<PresentationThemeEnumIcon> {
        match action {
            PresentationActionEnumId::Refresh => Some(PresentationThemeEnumIcon::ResetCamera),
            PresentationActionEnumId::SelectFile => Some(PresentationThemeEnumIcon::Folder),
            PresentationActionEnumId::Subscribe | PresentationActionEnumId::ConfirmQuit => None,
        }
    }

    fn system(action: PresentationActionEnumId) -> Option<Self> {
        Self::system_icon(action).map(|icon| Self {
            choice: PresentationActionPaletteEnumChoice::System(action),
            group: PresentationActionPaletteEnumGroup::System,
            icon,
            label: action.label(),
            availability: PresentationActionEnumAvailability::Available,
            shortcut: None,
        })
    }
}

#[derive(Debug, Default)]
pub struct PresentationActionPaletteView {
    open: bool,
    query: String,
    selected: usize,
    focus_pending: bool,
}

impl PresentationActionPaletteView {
    const WIDTH: f32 = 640.0;
    const TOP: f32 = 120.0;
    const ROW_HEIGHT: f32 = 40.0;
    const FIELD_HEIGHT: f32 = 56.0;
    const LIST_HEIGHT: f32 = 420.0;
    const GUTTER: f32 = 32.0;

    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        if !self.open {
            self.open = true;
            self.query.clear();
            self.selected = 0;
            self.focus_pending = true;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub const fn icon(action: PresentationActionEnumWorkspace) -> PresentationThemeEnumIcon {
        match action {
            PresentationActionEnumWorkspace::OpenPalette => PresentationThemeEnumIcon::Search,
            PresentationActionEnumWorkspace::NewGame => PresentationThemeEnumIcon::Plus,
            PresentationActionEnumWorkspace::OpenSettings => PresentationThemeEnumIcon::Settings,
            PresentationActionEnumWorkspace::FlipBoard => PresentationThemeEnumIcon::Flip,
            PresentationActionEnumWorkspace::Quit => PresentationThemeEnumIcon::Close,
        }
    }

    pub fn shortcut_text(
        preferences: DesktopPreferences,
        action: PresentationActionEnumWorkspace,
    ) -> Option<String> {
        action.shortcut_action().map(|shortcut| {
            PresentationShortcuts::label(
                PresentationShortcutEnumPlatform::current(),
                PresentationShortcuts::chord(preferences, shortcut),
            )
        })
    }

    pub fn show(
        &mut self,
        context: &egui::Context,
        actions: PresentationActionContext,
        preferences: DesktopPreferences,
    ) -> Option<PresentationActionPaletteEnumChoice> {
        if !self.open {
            return None;
        }
        let colors = PresentationThemeColors::of(context);
        let entries = self.items(actions, preferences);
        let last = entries.len().saturating_sub(1);
        let (down, up, enter) = context.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        if down {
            self.selected = self.selected.saturating_add(1).min(last);
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        self.selected = self.selected.min(last);
        let mut chosen = None;
        if enter
            && let Some(entry) = entries.get(self.selected)
            && entry.availability.is_available()
        {
            chosen = Some(entry.choice);
        }
        let width = Self::WIDTH.min(context.content_rect().width() - Self::GUTTER);
        let id = Id::new("chess.palette");
        let area = egui::Modal::default_area(id)
            .order(Order::Foreground)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, Self::TOP));
        let modal = egui::Modal::new(id)
            .area(area)
            .backdrop_color(colors.scrim)
            .frame(
                Frame::new()
                    .fill(colors.surface_2)
                    .stroke(Stroke::new(1.0, colors.line))
                    .corner_radius(CornerRadius::same(PresentationThemeTokens::RADIUS_XL))
                    .shadow(context.global_style().visuals.window_shadow),
            )
            .show(context, |ui| {
                PresentationThemeComponents::dialog_label(ui, "Command palette");
                ui.set_width(width);
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                self.show_field(ui, width);
                PresentationThemeComponents::divider(ui);
                let picked = self.show_entries(ui, &entries);
                PresentationThemeComponents::divider(ui);
                Self::show_footer(ui);
                picked
            });
        if let Some(picked) = modal.inner {
            chosen = Some(picked);
        }
        if chosen.is_some() || modal.should_close() {
            self.close();
        }
        chosen
    }

    fn show_field(&mut self, ui: &mut Ui, width: f32) {
        let colors = PresentationThemeColors::of(ui.ctx());
        Frame::new()
            .inner_margin(Margin::symmetric(16, 0))
            .show(ui, |ui| {
                ui.set_min_height(Self::FIELD_HEIGHT);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    PresentationThemeIcon::show(
                        ui,
                        PresentationThemeEnumIcon::Search,
                        20.0,
                        colors.text_3,
                    );
                    let field_width = width - 120.0;
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id_salt("chess.palette.query")
                            .frame(Frame::NONE)
                            .background_color(Color32::TRANSPARENT)
                            .font(FontId::proportional(
                                PresentationThemeTypography::INPUT_POINTS,
                            ))
                            .hint_text(
                                egui::RichText::new("Search or run a command").color(colors.text_3),
                            )
                            .desired_width(field_width),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Command")
                    });
                    if response.changed() {
                        self.selected = 0;
                    }
                    if self.focus_pending {
                        response.request_focus();
                        self.focus_pending = false;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        PresentationThemeComponents::kbd(ui, "Esc");
                    });
                });
            });
    }

    fn items(
        &self,
        actions: PresentationActionContext,
        preferences: DesktopPreferences,
    ) -> Vec<PresentationActionPaletteItem> {
        let needle = self.query.trim().to_lowercase();
        PresentationActionPalette::entries(&self.query, actions)
            .into_iter()
            .filter(|entry| entry.action() != PresentationActionEnumWorkspace::OpenPalette)
            .map(|entry| PresentationActionPaletteItem::workspace(entry, preferences))
            .chain(
                PresentationActionRegistry::actions()
                    .iter()
                    .copied()
                    .filter_map(PresentationActionPaletteItem::system)
                    .filter(|item| item.label.to_lowercase().contains(&needle)),
            )
            .collect()
    }

    fn show_entries(
        &mut self,
        ui: &mut Ui,
        entries: &[PresentationActionPaletteItem],
    ) -> Option<PresentationActionPaletteEnumChoice> {
        let mut chosen = None;
        Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(Self::LIST_HEIGHT)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    if entries.is_empty() {
                        Frame::new()
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                PresentationThemeComponents::caption(ui, "No matching commands");
                            });
                    }
                    let mut group = None;
                    for (index, entry) in entries.iter().enumerate() {
                        if group != Some(entry.group) {
                            group = Some(entry.group);
                            let top = if index == 0 { 8 } else { 12 };
                            Frame::new()
                                .inner_margin(Margin {
                                    left: 10,
                                    right: 10,
                                    top,
                                    bottom: 4,
                                })
                                .show(ui, |ui| {
                                    PresentationThemeComponents::eyebrow(ui, entry.group.label());
                                });
                        }
                        let selected = index == self.selected;
                        let response = self.row(ui, entry, selected);
                        if response.hovered() && !selected {
                            self.selected = index;
                        }
                        if response.clicked() && entry.availability.is_available() {
                            chosen = Some(entry.choice);
                        }
                    }
                });
        });
        chosen
    }

    fn row(
        &self,
        ui: &mut Ui,
        entry: &PresentationActionPaletteItem,
        selected: bool,
    ) -> egui::Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let available = entry.availability.is_available();
        let sense = if available {
            Sense::click()
        } else {
            Sense::hover()
        };
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, Self::ROW_HEIGHT), sense);
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, PresentationThemeTokens::RADIUS_MD, colors.accent_soft);
        }
        let icon_color = if selected && available {
            colors.accent
        } else {
            colors.text_3
        };
        let icon_rect = egui::Rect::from_center_size(
            rect.left_center() + Vec2::new(20.0, 0.0),
            Vec2::splat(16.0),
        );
        PresentationThemeIcon::paint(painter, icon_rect, entry.icon, icon_color);
        let galley = painter.layout_job(self.highlight(entry.label, &colors, available));
        painter.galley(
            egui::pos2(rect.left() + 40.0, rect.center().y - galley.size().y / 2.0),
            galley,
            colors.text,
        );
        let right = rect.right() - 10.0;
        match entry.availability {
            PresentationActionEnumAvailability::Available => {
                if let Some(key) = &entry.shortcut {
                    let font = FontId::monospace(PresentationThemeTypography::MICRO_POINTS);
                    let key_galley = painter.layout_no_wrap(key.clone(), font, colors.text_2);
                    let key_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            right - key_galley.size().x - 10.0,
                            rect.center().y - key_galley.size().y / 2.0 - 2.0,
                        ),
                        key_galley.size() + Vec2::new(10.0, 4.0),
                    );
                    painter.rect(
                        key_rect,
                        PresentationThemeTokens::RADIUS_SM,
                        colors.surface_1,
                        Stroke::new(1.0, colors.line),
                        egui::StrokeKind::Inside,
                    );
                    painter.galley(
                        key_rect.min + Vec2::new(5.0, 2.0),
                        key_galley,
                        colors.text_2,
                    );
                }
            }
            PresentationActionEnumAvailability::Unavailable(reason) => {
                painter.text(
                    egui::pos2(right, rect.center().y),
                    Align2::RIGHT_CENTER,
                    reason,
                    FontId::proportional(PresentationThemeTypography::CAPTION_POINTS),
                    colors.text_3,
                );
            }
        }
        let label = entry.label;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, available, selected, label)
        });
        response
    }

    fn highlight(
        &self,
        label: &str,
        colors: &PresentationThemeColors,
        available: bool,
    ) -> LayoutJob {
        let font = FontId::proportional(PresentationThemeTypography::BODY_POINTS);
        let (base, strong) = if available {
            (colors.text_2, colors.text)
        } else {
            (colors.text_3, colors.text_3)
        };
        let mut job = LayoutJob::default();
        let needle = self.query.trim().to_lowercase();
        let found = (!needle.is_empty())
            .then(|| label.to_lowercase().find(&needle))
            .flatten()
            .filter(|start| {
                label.is_char_boundary(*start) && label.is_char_boundary(start + needle.len())
            });
        let format = |color: Color32| TextFormat::simple(font.clone(), color);
        match found {
            Some(start) => {
                let end = start + needle.len();
                job.append(label.get(..start).unwrap_or_default(), 0.0, format(base));
                job.append(
                    label.get(start..end).unwrap_or_default(),
                    0.0,
                    format(strong),
                );
                job.append(label.get(end..).unwrap_or_default(), 0.0, format(base));
            }
            None => job.append(label, 0.0, format(strong)),
        }
        job
    }

    fn show_footer(ui: &mut Ui) {
        Frame::new()
            .inner_margin(Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    PresentationThemeComponents::kbd(ui, "\u{2191}");
                    PresentationThemeComponents::kbd(ui, "\u{2193}");
                    PresentationThemeComponents::caption(ui, "Move");
                    ui.add_space(10.0);
                    PresentationThemeComponents::kbd(ui, "\u{21B5}");
                    PresentationThemeComponents::caption(ui, "Run");
                    ui.add_space(10.0);
                    PresentationThemeComponents::kbd(ui, "Esc");
                    PresentationThemeComponents::caption(ui, "Close");
                });
            });
    }
}
