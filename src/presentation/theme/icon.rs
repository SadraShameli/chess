use bevy_egui::egui::{
    self, Color32, CornerRadius, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2,
};
use strum::{EnumIter, IntoStaticStr};

use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, IntoStaticStr)]
pub enum PresentationThemeEnumIcon {
    Games,
    Puzzles,
    Library,
    Study,
    Settings,
    Search,
    Plus,
    Close,
    Grid,
    Flip,
    ResetCamera,
    Sound,
    Pause,
    Play,
    Takeback,
    Resign,
    First,
    Previous,
    Next,
    Last,
    Clock,
    Engine,
    Folder,
    Star,
    Trash,
    Import,
    Export,
    Check,
    Panel,
}

pub struct PresentationThemeIcon;

impl PresentationThemeIcon {
    const STROKE: f32 = 1.75;
    const GRID: f32 = 24.0;

    pub fn paint(painter: &Painter, rect: Rect, icon: PresentationThemeEnumIcon, color: Color32) {
        let scale = rect.width().min(rect.height()) / Self::GRID;
        let origin = rect.center() - Vec2::splat(Self::GRID * scale / 2.0);
        let at = |x: f32, y: f32| -> Pos2 { origin + Vec2::new(x, y) * scale };
        let stroke = Stroke::new(Self::STROKE * scale.max(0.75), color);
        let line = |points: &[(f32, f32)]| {
            let points: Vec<Pos2> = points.iter().map(|(x, y)| at(*x, *y)).collect();
            painter.add(egui::Shape::line(points, stroke));
        };
        let circle = |x: f32, y: f32, r: f32| painter.circle_stroke(at(x, y), r * scale, stroke);
        let rect_at = |x: f32, y: f32, w: f32, h: f32, radius: f32| {
            painter.rect_stroke(
                Rect::from_min_size(at(x, y), Vec2::new(w, h) * scale),
                CornerRadius::same(u8::try_from((radius * scale).round() as i64).unwrap_or(0)),
                stroke,
                egui::StrokeKind::Middle,
            );
        };
        match icon {
            PresentationThemeEnumIcon::Games => {
                rect_at(3.0, 3.0, 18.0, 18.0, 2.0);
                line(&[(3.0, 9.0), (21.0, 9.0)]);
                line(&[(3.0, 15.0), (21.0, 15.0)]);
                line(&[(9.0, 3.0), (9.0, 21.0)]);
                line(&[(15.0, 3.0), (15.0, 21.0)]);
            }
            PresentationThemeEnumIcon::Puzzles => {
                circle(12.0, 12.0, 9.0);
                circle(12.0, 12.0, 5.0);
                circle(12.0, 12.0, 1.0);
            }
            PresentationThemeEnumIcon::Library => {
                rect_at(5.0, 4.0, 4.0, 16.0, 0.0);
                rect_at(10.0, 4.0, 4.0, 16.0, 0.0);
                line(&[
                    (15.5, 4.5),
                    (19.3, 3.5),
                    (22.5, 19.0),
                    (18.7, 20.0),
                    (15.5, 4.5),
                ]);
            }
            PresentationThemeEnumIcon::Study => {
                circle(6.0, 5.0, 2.0);
                circle(6.0, 19.0, 2.0);
                circle(18.0, 9.0, 2.0);
                line(&[(6.0, 7.0), (6.0, 17.0)]);
                line(&[(18.0, 11.0), (17.0, 14.0), (14.0, 16.0), (8.0, 18.0)]);
            }
            PresentationThemeEnumIcon::Settings => {
                line(&[(4.0, 6.0), (14.0, 6.0)]);
                line(&[(18.0, 6.0), (20.0, 6.0)]);
                line(&[(4.0, 12.0), (8.0, 12.0)]);
                line(&[(12.0, 12.0), (20.0, 12.0)]);
                line(&[(4.0, 18.0), (16.0, 18.0)]);
                circle(16.0, 6.0, 2.0);
                circle(10.0, 12.0, 2.0);
                circle(18.0, 18.0, 2.0);
            }
            PresentationThemeEnumIcon::Search => {
                circle(11.0, 11.0, 7.0);
                line(&[(20.0, 20.0), (16.5, 16.5)]);
            }
            PresentationThemeEnumIcon::Plus => {
                line(&[(12.0, 5.0), (12.0, 19.0)]);
                line(&[(5.0, 12.0), (19.0, 12.0)]);
            }
            PresentationThemeEnumIcon::Close => {
                line(&[(6.0, 6.0), (18.0, 18.0)]);
                line(&[(18.0, 6.0), (6.0, 18.0)]);
            }
            PresentationThemeEnumIcon::Grid => {
                rect_at(3.0, 3.0, 7.0, 7.0, 1.0);
                rect_at(14.0, 3.0, 7.0, 7.0, 1.0);
                rect_at(3.0, 14.0, 7.0, 7.0, 1.0);
                rect_at(14.0, 14.0, 7.0, 7.0, 1.0);
            }
            PresentationThemeEnumIcon::Flip => {
                line(&[(7.0, 4.0), (7.0, 20.0)]);
                line(&[(4.0, 17.0), (7.0, 20.0), (10.0, 17.0)]);
                line(&[(17.0, 20.0), (17.0, 4.0)]);
                line(&[(14.0, 7.0), (17.0, 4.0), (20.0, 7.0)]);
            }
            PresentationThemeEnumIcon::ResetCamera => {
                let points: Vec<(f32, f32)> = (0..=24)
                    .map(|step| {
                        let angle = std::f32::consts::PI * (0.75 + 1.5 * step as f32 / 24.0);
                        (12.0 + 8.0 * angle.cos(), 12.0 + 8.0 * angle.sin())
                    })
                    .collect();
                line(&points);
                line(&[(4.0, 4.0), (4.0, 8.0), (8.0, 8.0)]);
            }
            PresentationThemeEnumIcon::Sound => {
                line(&[
                    (4.0, 9.0),
                    (4.0, 15.0),
                    (8.0, 15.0),
                    (13.0, 19.0),
                    (13.0, 5.0),
                    (8.0, 9.0),
                    (4.0, 9.0),
                ]);
                line(&[(17.0, 9.0), (18.5, 12.0), (17.0, 15.0)]);
            }
            PresentationThemeEnumIcon::Pause => {
                line(&[(9.0, 5.0), (9.0, 19.0)]);
                line(&[(15.0, 5.0), (15.0, 19.0)]);
            }
            PresentationThemeEnumIcon::Play => {
                line(&[(7.0, 5.0), (19.0, 12.0), (7.0, 19.0), (7.0, 5.0)]);
            }
            PresentationThemeEnumIcon::Takeback => {
                line(&[(9.0, 14.0), (4.0, 9.0), (9.0, 4.0)]);
                line(&[
                    (4.0, 9.0),
                    (14.0, 9.0),
                    (18.0, 11.0),
                    (20.0, 15.0),
                    (18.0, 19.0),
                    (14.0, 21.0),
                    (11.0, 21.0),
                ]);
            }
            PresentationThemeEnumIcon::Resign => {
                line(&[(5.0, 21.0), (5.0, 4.0)]);
                line(&[
                    (5.0, 4.0),
                    (16.0, 4.0),
                    (14.0, 8.0),
                    (16.0, 12.0),
                    (5.0, 12.0),
                ]);
            }
            PresentationThemeEnumIcon::First => {
                line(&[(6.0, 5.0), (6.0, 19.0)]);
                line(&[(18.0, 6.0), (11.0, 12.0), (18.0, 18.0)]);
            }
            PresentationThemeEnumIcon::Previous => line(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)]),
            PresentationThemeEnumIcon::Next => line(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)]),
            PresentationThemeEnumIcon::Last => {
                line(&[(18.0, 5.0), (18.0, 19.0)]);
                line(&[(6.0, 6.0), (13.0, 12.0), (6.0, 18.0)]);
            }
            PresentationThemeEnumIcon::Clock => {
                circle(12.0, 12.0, 9.0);
                line(&[(12.0, 7.0), (12.0, 12.0), (15.0, 14.0)]);
            }
            PresentationThemeEnumIcon::Engine => {
                rect_at(5.0, 7.0, 14.0, 12.0, 2.0);
                line(&[(12.0, 3.0), (12.0, 7.0)]);
                line(&[(9.0, 12.0), (9.0, 14.0)]);
                line(&[(15.0, 12.0), (15.0, 14.0)]);
            }
            PresentationThemeEnumIcon::Folder => {
                line(&[
                    (3.0, 7.0),
                    (3.0, 19.0),
                    (21.0, 19.0),
                    (21.0, 8.0),
                    (11.0, 8.0),
                    (9.0, 5.0),
                    (3.0, 5.0),
                    (3.0, 7.0),
                ]);
            }
            PresentationThemeEnumIcon::Star => {
                let points: Vec<(f32, f32)> = (0..=10)
                    .map(|step| {
                        let radius = if step % 2 == 0 { 9.0 } else { 4.0 };
                        let angle =
                            -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * step as f32 / 5.0;
                        (12.0 + radius * angle.cos(), 12.5 + radius * angle.sin())
                    })
                    .collect();
                line(&points);
            }
            PresentationThemeEnumIcon::Trash => {
                line(&[(4.0, 7.0), (20.0, 7.0)]);
                line(&[(9.0, 7.0), (9.0, 4.0), (15.0, 4.0), (15.0, 7.0)]);
                line(&[(6.0, 7.0), (7.0, 20.0), (17.0, 20.0), (18.0, 7.0)]);
            }
            PresentationThemeEnumIcon::Import => {
                line(&[(12.0, 3.0), (12.0, 15.0)]);
                line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]);
                line(&[(4.0, 17.0), (4.0, 20.0), (20.0, 20.0), (20.0, 17.0)]);
            }
            PresentationThemeEnumIcon::Export => {
                line(&[(12.0, 15.0), (12.0, 3.0)]);
                line(&[(7.0, 8.0), (12.0, 3.0), (17.0, 8.0)]);
                line(&[(4.0, 17.0), (4.0, 20.0), (20.0, 20.0), (20.0, 17.0)]);
            }
            PresentationThemeEnumIcon::Check => line(&[(5.0, 12.0), (10.0, 17.0), (19.0, 7.0)]),
            PresentationThemeEnumIcon::Panel => {
                rect_at(3.0, 4.0, 18.0, 16.0, 2.0);
                line(&[(15.0, 4.0), (15.0, 20.0)]);
            }
        }
    }

    pub fn show(
        ui: &mut Ui,
        icon: PresentationThemeEnumIcon,
        size: f32,
        color: Color32,
    ) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        Self::paint(ui.painter(), rect, icon, color);
        response
    }

    pub fn button(ui: &mut Ui, icon: PresentationThemeEnumIcon, label: &str) -> Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let size = Vec2::splat(PresentationThemeTokens::CONTROL_SM);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let fill = if response.hovered() {
            colors.surface_2
        } else {
            Color32::TRANSPARENT
        };
        let color = if response.hovered() {
            colors.text
        } else {
            colors.text_2
        };
        ui.painter()
            .rect_filled(rect, PresentationThemeTokens::RADIUS_MD, fill);
        Self::paint(ui.painter(), rect.shrink(6.0), icon, color);
        let label = label.to_owned();
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label.clone())
        });
        response.on_hover_text(label)
    }
}
