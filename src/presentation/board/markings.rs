use std::collections::HashMap;

use bevy_egui::egui::{self, Color32, Modifiers, Painter, Pos2, Rect, Stroke, Ui};

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::square::BoardSquare;
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::game::enums::GameEnumMarkingColor;
use crate::game::move_node::GameMoveNodeMarking;
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::board::transition::PresentationBoardTransition;
use crate::presentation::board::view::PresentationBoardPickingProvider;
use crate::scene::picking::{ScenePicking, ScenePickingViewport};
use crate::scene::renderer::{SceneRendererError, SceneRendererGeneration};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    strum::IntoStaticStr,
    strum::Display,
    strum::EnumIter,
)]
pub enum PresentationBoardMarkColor {
    #[default]
    #[strum(serialize = "Green")]
    Green,
    #[strum(serialize = "Red")]
    Red,
    #[strum(serialize = "Blue")]
    Blue,
    #[strum(serialize = "Yellow")]
    Yellow,
}

impl PresentationBoardMarkColor {
    pub fn label(self) -> &'static str {
        self.into()
    }

    pub const fn color(self) -> Color32 {
        match self {
            Self::Green => Color32::from_rgb(0x15, 0x80, 0x3d),
            Self::Red => Color32::from_rgb(0xdc, 0x26, 0x26),
            Self::Blue => Color32::from_rgb(0x25, 0x63, 0xeb),
            Self::Yellow => Color32::from_rgb(0xca, 0x8a, 0x04),
        }
    }

    pub const fn game_color(self) -> GameEnumMarkingColor {
        match self {
            Self::Green => GameEnumMarkingColor::Green,
            Self::Red => GameEnumMarkingColor::Red,
            Self::Blue => GameEnumMarkingColor::Blue,
            Self::Yellow => GameEnumMarkingColor::Yellow,
        }
    }

    pub const fn from_game_color(color: GameEnumMarkingColor) -> Self {
        match color {
            GameEnumMarkingColor::Green => Self::Green,
            GameEnumMarkingColor::Red => Self::Red,
            GameEnumMarkingColor::Blue => Self::Blue,
            GameEnumMarkingColor::Yellow => Self::Yellow,
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

    pub fn from_game(marking: GameMoveNodeMarking) -> Self {
        let color = PresentationBoardMarkColor::from_game_color(marking.color());
        match marking.destination() {
            Some(destination) => Self::Arrow {
                origin: marking.origin(),
                destination,
                color,
            },
            None => Self::Circle {
                square: marking.origin(),
                color,
            },
        }
    }

    pub fn to_game(self) -> Option<GameMoveNodeMarking> {
        match self {
            Self::Circle { square, color } => {
                Some(GameMoveNodeMarking::circle(color.game_color(), square))
            }
            Self::Arrow {
                origin,
                destination,
                color,
            } => GameMoveNodeMarking::arrow(color.game_color(), origin, destination).ok(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationBoardMarkRequest {
    Toggle(PresentationBoardMark),
    ClearTemporary,
}

#[derive(Debug, Clone, Default)]
struct PresentationBoardExternalMarks {
    saved: Vec<PresentationBoardMark>,
    temporary: Vec<PresentationBoardMark>,
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
    external: Option<PresentationBoardExternalMarks>,
    requests: Vec<PresentationBoardMarkRequest>,
    gesture: Option<(
        BoardSquare,
        SceneRendererGeneration,
        PresentationBoardMarkColor,
    )>,
    entry_origin: String,
    entry_destination: String,
    entry_invalid: bool,
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
    const DASH_SEGMENTS: u16 = 48;
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
        self.requests
            .push(PresentationBoardMarkRequest::ClearTemporary);
    }

    pub fn has_temporary(&self) -> bool {
        match &self.external {
            Some(external) => !external.temporary.is_empty(),
            None => !self.marks.is_empty(),
        }
    }

    pub fn set_external(
        &mut self,
        marks: Option<(Vec<PresentationBoardMark>, Vec<PresentationBoardMark>)>,
    ) {
        self.external =
            marks.map(|(saved, temporary)| PresentationBoardExternalMarks { saved, temporary });
    }

    pub fn take_requests(&mut self) -> Vec<PresentationBoardMarkRequest> {
        std::mem::take(&mut self.requests)
    }

    pub fn reset_baseline(&mut self) {
        self.baseline = None;
        self.last_move = None;
        self.marks.clear();
        self.gesture = None;
    }

    pub fn cancel_gesture(&mut self) {
        self.gesture = None;
    }

    pub fn entry_origin_mut(&mut self) -> &mut String {
        &mut self.entry_origin
    }

    pub fn entry_destination_mut(&mut self) -> &mut String {
        &mut self.entry_destination
    }

    pub const fn is_entry_invalid(&self) -> bool {
        self.entry_invalid
    }

    pub fn toggle_entry(&mut self, arrow: bool) {
        let color = self.selected_color;
        let origin = self.entry_origin.trim().parse::<BoardSquare>();
        let destination = self.entry_destination.trim().parse::<BoardSquare>();
        self.entry_invalid = false;
        match (origin, destination, arrow) {
            (Ok(origin), _, false) => self.toggle_circle(origin, color),
            (Ok(origin), Ok(destination), true) if origin != destination => {
                self.toggle_arrow(origin, destination, color)
            }
            _ => self.entry_invalid = true,
        }
    }

    pub fn handle_input<P: PresentationBoardPickingProvider>(
        &mut self,
        ui: &Ui,
        enabled: bool,
        picker: &mut P,
        hovered: bool,
        scene: &PresentationBoardScene,
        viewport: ScenePickingViewport,
    ) -> Result<(), SceneRendererError> {
        let (pressed, released, point, modifiers, escape) = ui.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input
                    .pointer
                    .button_released(egui::PointerButton::Secondary),
                input.pointer.interact_pos(),
                input.modifiers,
                input.key_pressed(egui::Key::Escape),
            )
        });
        if escape || !enabled {
            self.gesture = None;
            return Ok(());
        }
        if !pressed && !released {
            return Ok(());
        }
        let square = match point {
            Some(point) if viewport.to_physical(point).is_some() => picker.pick(point)?,
            Some(_) | None => None,
        };
        if pressed
            && hovered
            && let Some(square) = square
        {
            let color = PresentationBoardMarkColor::from_modifiers(modifiers, self.selected_color);
            self.gesture = Some((square, scene.generation(), color));
        }
        if released
            && let Some((origin, generation, color)) = self.gesture.take()
            && generation == scene.generation()
            && let Some(destination) = square
        {
            self.toggle_arrow(origin, destination, color);
        }
        Ok(())
    }

    pub fn toggle(&mut self, mark: PresentationBoardMark) {
        self.requests
            .push(PresentationBoardMarkRequest::Toggle(mark));
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
        if self.baseline.is_none() {
            self.baseline = Some(counts.clone());
        }
        if let Some(previous) = previous
            && previous.fen() != snapshot.fen()
        {
            self.marks.clear();
            self.last_move = PresentationBoardTransition::piece_move(previous, snapshot);
        }
        self.current = counts;
    }

    pub fn captures(&self) -> Vec<PresentationBoardCapture> {
        let Some(baseline) = &self.baseline else {
            return Vec::new();
        };
        let mut captures = Vec::new();
        for color in Self::COLORS {
            let count = |counts: &PresentationBoardPieceCounts, kind| {
                counts.get(&(color, kind)).copied().unwrap_or(0)
            };
            let promoted: u8 = Self::KINDS
                .into_iter()
                .filter(|kind| *kind != BoardEnumPieceKind::Pawn)
                .map(|kind| count(&self.current, kind).saturating_sub(count(baseline, kind)))
                .sum();
            for kind in Self::KINDS {
                let mut lost = count(baseline, kind).saturating_sub(count(&self.current, kind));
                if kind == BoardEnumPieceKind::Pawn {
                    lost = lost.saturating_sub(promoted);
                }
                if lost > 0 {
                    captures.push(PresentationBoardCapture {
                        color,
                        kind,
                        count: lost,
                    });
                }
            }
        }
        captures
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
        self.paint_overlay(ui, scene);
    }

    pub fn paint_overlay(&self, ui: &Ui, scene: &PresentationBoardScene) {
        let painter = ui
            .painter()
            .with_clip_rect(scene.viewport().image().intersect(ui.clip_rect()));
        match &self.external {
            Some(external) => {
                for mark in &external.saved {
                    Self::paint_mark(&painter, scene, *mark, true);
                }
                for mark in &external.temporary {
                    Self::paint_mark(&painter, scene, *mark, false);
                }
            }
            None => {
                for mark in &self.marks {
                    Self::paint_mark(&painter, scene, *mark, false);
                }
            }
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

    fn paint_mark(
        painter: &Painter,
        scene: &PresentationBoardScene,
        mark: PresentationBoardMark,
        saved: bool,
    ) {
        match mark {
            PresentationBoardMark::Circle { square, color } => {
                if let Some(rect) = PresentationBoardSemanticGrid::square_rect(square, scene) {
                    let radius = rect.width() * 0.46;
                    if saved {
                        let points: Vec<Pos2> = (0..=Self::DASH_SEGMENTS)
                            .map(|step| {
                                let angle = std::f32::consts::TAU * f32::from(step)
                                    / f32::from(Self::DASH_SEGMENTS);
                                rect.center() + egui::vec2(angle.cos(), angle.sin()) * radius
                            })
                            .collect();
                        painter.extend(egui::Shape::dashed_line(
                            &points,
                            Stroke::new(4.0, color.color()),
                            6.0,
                            4.0,
                        ));
                    } else {
                        painter.circle_stroke(
                            rect.center(),
                            radius,
                            Stroke::new(4.0, color.color()),
                        );
                    }
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
                Self::paint_arrow(
                    painter,
                    from.center(),
                    to.center(),
                    from.width(),
                    color,
                    saved,
                );
            }
        }
    }

    fn paint_arrow(
        painter: &Painter,
        from: Pos2,
        to: Pos2,
        square_width: f32,
        color: PresentationBoardMarkColor,
        saved: bool,
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
        if saved {
            painter.extend(egui::Shape::dashed_line(
                &[from, base],
                Stroke::new(width, tint),
                width * 2.0,
                width,
            ));
        } else {
            painter.line_segment([from, base], Stroke::new(width, tint));
        }
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
