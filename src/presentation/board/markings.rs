use std::collections::HashMap;

use bevy_egui::egui::{self, Color32, Modifiers, Painter, Pos2, Rect, Stroke, Ui};

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::square::BoardSquare;
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::board::transition::PresentationBoardTransition;
use crate::scene::picking::ScenePicking;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PresentationBoardMarkColor {
    #[default]
    Green,
    Red,
    Blue,
    Yellow,
}

impl PresentationBoardMarkColor {
    pub const ALL: [Self; 4] = [Self::Green, Self::Red, Self::Blue, Self::Yellow];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Green => "Green",
            Self::Red => "Red",
            Self::Blue => "Blue",
            Self::Yellow => "Yellow",
        }
    }

    pub const fn color(self) -> Color32 {
        match self {
            Self::Green => Color32::from_rgb(0x15, 0x80, 0x3d),
            Self::Red => Color32::from_rgb(0xdc, 0x26, 0x26),
            Self::Blue => Color32::from_rgb(0x25, 0x63, 0xeb),
            Self::Yellow => Color32::from_rgb(0xca, 0x8a, 0x04),
        }
    }

    pub const fn from_modifiers(modifiers: Modifiers, selected: Self) -> Self {
        match (modifiers.shift, modifiers.alt) {
            (true, true) => Self::Yellow,
            (true, false) => Self::Red,
            (false, true) => Self::Blue,
            (false, false) => selected,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationBoardMark {
    Circle {
        square: BoardSquare,
        color: PresentationBoardMarkColor,
    },
    Arrow {
        origin: BoardSquare,
        destination: BoardSquare,
        color: PresentationBoardMarkColor,
    },
}

impl PresentationBoardMark {
    fn same_shape(self, other: Self) -> bool {
        match (self, other) {
            (Self::Circle { square: left, .. }, Self::Circle { square: right, .. }) => {
                left == right
            }
            (
                Self::Arrow {
                    origin: left_origin,
                    destination: left_destination,
                    ..
                },
                Self::Arrow {
                    origin: right_origin,
                    destination: right_destination,
                    ..
                },
            ) => left_origin == right_origin && left_destination == right_destination,
            _ => false,
        }
    }

    const fn color(self) -> PresentationBoardMarkColor {
        match self {
            Self::Circle { color, .. } | Self::Arrow { color, .. } => color,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationBoardAids {
    pub legal_dots: bool,
    pub last_move: bool,
    pub check: bool,
    pub captured: bool,
    pub coordinates: bool,
}

impl Default for PresentationBoardAids {
    fn default() -> Self {
        Self {
            legal_dots: true,
            last_move: true,
            check: true,
            captured: true,
            coordinates: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationBoardCapture {
    pub color: BoardEnumColor,
    pub kind: BoardEnumPieceKind,
    pub count: u8,
}

type PresentationBoardPieceCounts = HashMap<(BoardEnumColor, BoardEnumPieceKind), u8>;

#[derive(Debug, Clone, Default)]
pub struct PresentationBoardMarkings {
    marks: Vec<PresentationBoardMark>,
    selected_color: PresentationBoardMarkColor,
    aids: PresentationBoardAids,
    last_move: Option<(BoardSquare, BoardSquare)>,
    baseline: Option<PresentationBoardPieceCounts>,
    current: PresentationBoardPieceCounts,
}

impl PresentationBoardMarkings {
    const KINDS: [BoardEnumPieceKind; 6] = [
        BoardEnumPieceKind::Queen,
        BoardEnumPieceKind::Rook,
        BoardEnumPieceKind::Bishop,
        BoardEnumPieceKind::Knight,
        BoardEnumPieceKind::Pawn,
        BoardEnumPieceKind::King,
    ];
    const COLORS: [BoardEnumColor; 2] = [BoardEnumColor::White, BoardEnumColor::Black];

    pub fn marks(&self) -> &[PresentationBoardMark] {
        &self.marks
    }

    pub const fn selected_color(&self) -> PresentationBoardMarkColor {
        self.selected_color
    }

    pub fn select_color(&mut self, color: PresentationBoardMarkColor) {
        self.selected_color = color;
    }

    pub const fn aids(&self) -> PresentationBoardAids {
        self.aids
    }

    pub fn aids_mut(&mut self) -> &mut PresentationBoardAids {
        &mut self.aids
    }

    pub const fn last_move(&self) -> Option<(BoardSquare, BoardSquare)> {
        self.last_move
    }

    pub fn clear(&mut self) {
        self.marks.clear();
    }

    pub fn toggle(&mut self, mark: PresentationBoardMark) {
        match self
            .marks
            .iter()
            .position(|existing| existing.same_shape(mark))
        {
            Some(index) if self.marks[index].color() == mark.color() => {
                self.marks.remove(index);
            }
            Some(index) => self.marks[index] = mark,
            None => self.marks.push(mark),
        }
    }

    pub fn toggle_circle(&mut self, square: BoardSquare, color: PresentationBoardMarkColor) {
        self.toggle(PresentationBoardMark::Circle { square, color });
    }

    pub fn toggle_arrow(
        &mut self,
        origin: BoardSquare,
        destination: BoardSquare,
        color: PresentationBoardMarkColor,
    ) {
        if origin == destination {
            self.toggle_circle(origin, color);
        } else {
            self.toggle(PresentationBoardMark::Arrow {
                origin,
                destination,
                color,
            });
        }
    }

    pub fn observe(
        &mut self,
        previous: Option<&DesktopBoardSnapshot>,
        snapshot: &DesktopBoardSnapshot,
    ) {
        let counts = Self::counts(snapshot);
        let previous_total = previous.map(|previous| Self::counts(previous).values().sum::<u8>());
        let grew = previous_total.is_some_and(|total| counts.values().sum::<u8>() > total);
        if self.baseline.is_none() || grew {
            self.baseline = Some(counts.clone());
            self.last_move = None;
        }
        if let Some(previous) = previous
            && previous.fen() != snapshot.fen()
        {
            self.marks.clear();
            self.last_move = if grew {
                None
            } else {
                PresentationBoardTransition::piece_move(previous, snapshot)
            };
        }
        self.current = counts;
    }

    pub fn captures(&self) -> Vec<PresentationBoardCapture> {
        let Some(baseline) = &self.baseline else {
            return Vec::new();
        };
        Self::COLORS
            .into_iter()
            .flat_map(|color| Self::KINDS.into_iter().map(move |kind| (color, kind)))
            .filter_map(|(color, kind)| {
                let before = baseline.get(&(color, kind)).copied().unwrap_or(0);
                let now = self.current.get(&(color, kind)).copied().unwrap_or(0);
                before
                    .checked_sub(now)
                    .filter(|count| *count > 0)
                    .map(|count| PresentationBoardCapture { color, kind, count })
            })
            .collect()
    }

    fn counts(snapshot: &DesktopBoardSnapshot) -> PresentationBoardPieceCounts {
        let mut counts = HashMap::new();
        for piece in snapshot.pieces() {
            let entry: &mut u8 = counts.entry((piece.color(), piece.kind())).or_default();
            *entry = entry.saturating_add(1);
        }
        counts
    }

    pub fn paint(
        &self,
        ui: &Ui,
        scene: &PresentationBoardScene,
        snapshot: &DesktopBoardSnapshot,
        selected: Option<BoardSquare>,
    ) {
        let painter = ui
            .painter()
            .with_clip_rect(scene.viewport().image().intersect(ui.clip_rect()));
        if self.aids.last_move
            && let Some((origin, destination)) = self.last_move
        {
            for square in [origin, destination] {
                Self::highlight(
                    &painter,
                    scene,
                    square,
                    Color32::from_rgb(0xca, 0x8a, 0x04),
                    4.0,
                );
            }
        }
        if self.aids.check
            && snapshot.is_check()
            && let Some(king) = snapshot.pieces().iter().find(|piece| {
                piece.kind() == BoardEnumPieceKind::King && piece.color() == snapshot.turn()
            })
        {
            Self::highlight(
                &painter,
                scene,
                king.square(),
                Color32::from_rgb(0xdc, 0x26, 0x26),
                6.0,
            );
        }
        if self.aids.legal_dots
            && let Some(selected) = selected
        {
            for movement in snapshot
                .legal_moves()
                .iter()
                .filter(|movement| movement.origin() == Some(selected))
            {
                let destination = movement.destination();
                let Some(rect) = PresentationBoardSemanticGrid::square_rect(destination, scene)
                else {
                    continue;
                };
                let occupied = snapshot
                    .pieces()
                    .iter()
                    .any(|piece| piece.square() == destination);
                let color = Color32::from_rgba_unmultiplied(0x11, 0x15, 0x1a, 0xcc);
                if occupied {
                    painter.circle_stroke(
                        rect.center(),
                        rect.width() * 0.42,
                        Stroke::new(3.0, color),
                    );
                } else {
                    painter.circle_filled(rect.center(), rect.width() * 0.14, color);
                }
            }
        }
        for mark in &self.marks {
            Self::paint_mark(&painter, scene, *mark);
        }
        if self.aids.coordinates {
            Self::paint_coordinates(&painter, scene, ui.visuals().text_color());
        }
    }

    fn highlight(
        painter: &Painter,
        scene: &PresentationBoardScene,
        square: BoardSquare,
        color: Color32,
        width: f32,
    ) {
        if let Some(rect) = PresentationBoardSemanticGrid::square_rect(square, scene) {
            painter.rect_filled(rect, 0.0, color.gamma_multiply(0.25));
            painter.rect_stroke(
                rect.shrink(width / 2.0),
                2.0,
                Stroke::new(width, color),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn paint_mark(painter: &Painter, scene: &PresentationBoardScene, mark: PresentationBoardMark) {
        match mark {
            PresentationBoardMark::Circle { square, color } => {
                if let Some(rect) = PresentationBoardSemanticGrid::square_rect(square, scene) {
                    painter.circle_stroke(
                        rect.center(),
                        rect.width() * 0.46,
                        Stroke::new(4.0, color.color()),
                    );
                }
            }
            PresentationBoardMark::Arrow {
                origin,
                destination,
                color,
            } => {
                let (Some(from), Some(to)) = (
                    PresentationBoardSemanticGrid::square_rect(origin, scene),
                    PresentationBoardSemanticGrid::square_rect(destination, scene),
                ) else {
                    return;
                };
                Self::paint_arrow(painter, from.center(), to.center(), from.width(), color);
            }
        }
    }

    fn paint_arrow(
        painter: &Painter,
        from: Pos2,
        to: Pos2,
        square_width: f32,
        color: PresentationBoardMarkColor,
    ) {
        let direction = to - from;
        let length = direction.length();
        if length <= f32::EPSILON {
            return;
        }
        let unit = direction / length;
        let normal = egui::vec2(-unit.y, unit.x);
        let head = square_width * 0.38;
        let width = square_width * 0.12;
        let base = to - unit * head;
        let tint = color.color().gamma_multiply(0.9);
        painter.line_segment([from, base], Stroke::new(width, tint));
        painter.add(egui::Shape::convex_polygon(
            vec![to, base + normal * head * 0.6, base - normal * head * 0.6],
            tint,
            Stroke::NONE,
        ));
    }

    fn paint_coordinates(painter: &Painter, scene: &PresentationBoardScene, text: Color32) {
        let font = egui::FontId::proportional(12.0);
        let mut files: [Option<Rect>; 8] = [None; 8];
        let mut ranks: [Option<Rect>; 8] = [None; 8];
        for square in PresentationBoardSemanticGrid::squares() {
            let Some(rect) = PresentationBoardSemanticGrid::square_rect(square, scene) else {
                continue;
            };
            let index = usize::from(u8::from(square));
            let file = index % usize::from(ScenePicking::FILES);
            let rank = index / usize::from(ScenePicking::FILES);
            if files[file].is_none_or(|best| rect.center().y > best.center().y) {
                files[file] = Some(rect);
            }
            if ranks[rank].is_none_or(|best| rect.center().x < best.center().x) {
                ranks[rank] = Some(rect);
            }
        }
        for (file, rect) in files.iter().enumerate() {
            if let (Some(rect), Some(letter)) = (rect, u8::try_from(file).ok()) {
                painter.text(
                    rect.left_bottom() + egui::vec2(3.0, -2.0),
                    egui::Align2::LEFT_BOTTOM,
                    char::from(b'a' + letter),
                    font.clone(),
                    text,
                );
            }
        }
        for (rank, rect) in ranks.iter().enumerate() {
            if let (Some(rect), Some(digit)) = (rect, u8::try_from(rank).ok()) {
                painter.text(
                    rect.left_top() + egui::vec2(3.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    char::from(b'1' + digit),
                    font.clone(),
                    text,
                );
            }
        }
    }
}
