use bevy_egui::egui::{self, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect};

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;
use chess::desktop::dto::DesktopBoardSnapshot;
use chess::presentation::board::input::{
    PresentationBoardInput, PresentationBoardInputEnumGesture,
};
use chess::presentation::board::markings::PresentationBoardMarkColor;
use chess::presentation::board::scene::PresentationBoardScene;
use chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use chess::presentation::board::view::{
    PresentationBoardAction, PresentationBoardView, PresentationBoardViewInput,
};
use chess::scene::camera::SceneCamera;
use chess::scene::enums::SceneEnumLifecycle;
use chess::scene::picking::{ScenePicking, ScenePickingViewport};
use chess::scene::renderer::{SceneRendererError, SceneRendererGeneration};

type PresentationBoardInputResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationBoardInputFixture {
    context: Context,
    view: PresentationBoardView,
    snapshot: DesktopBoardSnapshot,
    scene: PresentationBoardScene,
    screen: Rect,
    image: Rect,
}

impl PresentationBoardInputFixture {
    fn new() -> PresentationBoardInputResult<Self> {
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let image = Rect::from_min_size(egui::pos2(32.0, 32.0), egui::vec2(700.0, 700.0));
        let context = Context::default();
        let scene = Self::scene(image, screen, 1.0, SceneRendererGeneration::initial())?;
        Ok(Self {
            context,
            view: PresentationBoardView::new(),
            snapshot: DesktopBoardSnapshot::from_position(&BoardPosition::standard()),
            scene,
            screen,
            image,
        })
    }

    fn scene(
        image: Rect,
        screen: Rect,
        scale: f32,
        generation: SceneRendererGeneration,
    ) -> PresentationBoardInputResult<PresentationBoardScene> {
        let viewport = ScenePickingViewport::new(image, screen, scale, 1.0)?;
        Ok(PresentationBoardScene::new(
            egui::TextureId::User(1),
            generation,
            SceneEnumLifecycle::Ready,
            viewport,
            SceneCamera::initial(BoardEnumColor::White).fit(viewport.physical_size())?,
        ))
    }

    fn regenerate(&mut self, scale: f32) -> PresentationBoardInputResult {
        let generation = self.scene.generation().next()?;
        self.scene = Self::scene(self.image, self.screen, scale, generation)?;
        self.context.set_pixels_per_point(scale);
        Ok(())
    }

    fn pick(
        scene: &PresentationBoardScene,
        point: Pos2,
    ) -> Result<Option<BoardSquare>, SceneRendererError> {
        let Some(pixel) = scene.viewport().to_physical(point) else {
            return Ok(None);
        };
        Ok(scene.camera().ray(pixel).map(ScenePicking::board_square)?)
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<PresentationBoardAction> {
        let scene = &self.scene;
        let snapshot = &self.snapshot;
        let view = &mut self.view;
        let image = self.image;
        let mut actions = Vec::new();
        let mut output = self.context.run_ui(
            RawInput {
                screen_rect: Some(self.screen),
                events,
                ..Default::default()
            },
            |ui| {
                let mut picker = |point| Self::pick(scene, point);
                if let Ok(frame) = view.show_at(
                    ui,
                    image,
                    PresentationBoardViewInput::new(snapshot, Some(scene), &mut picker),
                ) {
                    actions.extend_from_slice(frame.actions());
                }
            },
        );
        output.textures_delta.clear();
        actions
    }

    fn center(&self, name: &str) -> PresentationBoardInputResult<Pos2> {
        let square: BoardSquare = name.parse()?;
        Ok(
            PresentationBoardSemanticGrid::square_rect(square, &self.scene)
                .ok_or("square off screen")?
                .center(),
        )
    }

    fn press(point: Pos2) -> Vec<Event> {
        vec![
            Event::PointerMoved(point),
            Event::PointerButton {
                pos: point,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ]
    }

    fn release(point: Pos2) -> Vec<Event> {
        vec![
            Event::PointerMoved(point),
            Event::PointerButton {
                pos: point,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ]
    }

    fn click(&mut self, from: Pos2, to: Pos2) -> Vec<PresentationBoardAction> {
        self.frame(Self::press(from));
        self.frame(Self::release(to))
    }
}

#[test]
fn movement_under_five_points_is_a_click_and_larger_movement_is_a_drag()
-> PresentationBoardInputResult {
    let (e2, e4): (BoardSquare, BoardSquare) = ("e2".parse()?, "e4".parse()?);
    let origin = Pos2::new(100.0, 100.0);
    assert_eq!(PresentationBoardInput::DRAG_THRESHOLD_POINTS, 5.0);
    assert!(!PresentationBoardInput::is_drag(
        origin,
        origin + egui::vec2(4.9, 0.0)
    ));
    assert!(PresentationBoardInput::is_drag(
        origin,
        origin + egui::vec2(5.0, 0.0)
    ));
    assert_eq!(
        PresentationBoardInput::classify(e2, e4, Some(origin), Some(origin + egui::vec2(2.0, 0.0))),
        PresentationBoardInputEnumGesture::Click(e2)
    );
    assert_eq!(
        PresentationBoardInput::classify(
            e2,
            e4,
            Some(origin),
            Some(origin + egui::vec2(40.0, 0.0))
        ),
        PresentationBoardInputEnumGesture::Drag {
            origin: e2,
            destination: e4
        }
    );
    Ok(())
}

#[test]
fn a_pointer_drag_between_squares_reports_origin_and_destination() -> PresentationBoardInputResult {
    let mut fixture = PresentationBoardInputFixture::new()?;
    fixture.frame(Vec::new());
    let (from, to) = (fixture.center("e2")?, fixture.center("e4")?);
    let actions = fixture.click(from, to);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].square(), "e4".parse()?);
    assert_eq!(actions[0].origin(), Some("e2".parse()?));
    Ok(())
}

#[test]
fn a_tiny_slip_across_a_square_boundary_stays_a_click_on_the_pressed_square()
-> PresentationBoardInputResult {
    let mut fixture = PresentationBoardInputFixture::new()?;
    fixture.frame(Vec::new());
    let start = fixture.center("e2")?;
    let end = fixture.center("e3")?;
    let steps = 400_u16;
    let mut previous = start;
    let mut crossing = None;
    for step in 1..=steps {
        let point = start + (end - start) * (f32::from(step) / f32::from(steps));
        if PresentationBoardInputFixture::pick(&fixture.scene, point)? == Some("e3".parse()?) {
            crossing = Some((previous, point));
            break;
        }
        previous = point;
    }
    let (before, after) = crossing.ok_or("no boundary between e2 and e3")?;
    assert!(!PresentationBoardInput::is_drag(before, after));
    let actions = fixture.click(before, after);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].square(), "e2".parse()?);
    assert_eq!(actions[0].origin(), None);
    Ok(())
}

#[test]
fn a_geometry_change_mid_gesture_consumes_the_release_until_a_fresh_press()
-> PresentationBoardInputResult {
    let mut fixture = PresentationBoardInputFixture::new()?;
    fixture.frame(Vec::new());
    let selected: BoardSquare = "g1".parse()?;
    fixture.view.set_selected(Some(selected));
    fixture
        .view
        .markings_mut()
        .toggle_circle("c3".parse()?, PresentationBoardMarkColor::Blue);
    let (from, to) = (fixture.center("e2")?, fixture.center("e4")?);
    fixture.frame(PresentationBoardInputFixture::press(from));
    fixture.regenerate(2.0)?;
    fixture.frame(Vec::new());
    let (from, to_after) = (fixture.center("e2")?, fixture.center("e4")?);
    assert!(
        fixture
            .frame(PresentationBoardInputFixture::release(to))
            .is_empty()
    );
    assert_eq!(fixture.view.selected(), Some(selected));
    assert_eq!(fixture.view.markings().marks().len(), 1);
    let actions = fixture.click(from, to_after);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].square(), "e4".parse()?);
    Ok(())
}

#[test]
fn escape_cancels_the_active_pointer_gesture() -> PresentationBoardInputResult {
    let mut fixture = PresentationBoardInputFixture::new()?;
    fixture.frame(Vec::new());
    let (from, to) = (fixture.center("e2")?, fixture.center("e4")?);
    fixture.frame(PresentationBoardInputFixture::press(from));
    fixture.frame(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }]);
    assert!(
        fixture
            .frame(PresentationBoardInputFixture::release(to))
            .is_empty()
    );
    Ok(())
}
