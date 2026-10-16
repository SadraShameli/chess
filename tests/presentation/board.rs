use bevy_egui::egui::{
    self, Context, Event, Key, Modifiers, Pos2, RawInput, Rect, TextureId, accesskit,
};

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;
use chess::desktop::dto::DesktopBoardSnapshot;
use chess::presentation::accessibility::{
    PresentationAccessibilityError, PresentationAccessibilityInput,
};
use chess::presentation::board::scene::PresentationBoardScene;
use chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use chess::presentation::board::view::{
    PresentationBoardAction, PresentationBoardView, PresentationBoardViewInput,
};
use chess::scene::camera::SceneCamera;
use chess::scene::enums::SceneEnumLifecycle;
use chess::scene::picking::{ScenePicking, ScenePickingViewport};
use chess::scene::renderer::SceneRendererGeneration;

type PresentationBoardTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationBoardFixture {
    context: Context,
    view: PresentationBoardView,
    snapshot: DesktopBoardSnapshot,
    scene: PresentationBoardScene,
    actions: Vec<PresentationBoardAction>,
}

impl PresentationBoardFixture {
    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0))
    }

    fn image() -> Rect {
        Rect::from_min_size(egui::pos2(32.0, 32.0), egui::vec2(680.0, 680.0))
    }

    fn new(orientation: BoardEnumColor) -> PresentationBoardTestResult<Self> {
        let context = Context::default();
        context.enable_accesskit();
        let viewport = ScenePickingViewport::new(Self::image(), Self::screen(), 1.0, 1.0)?;
        let scene = PresentationBoardScene::new(
            TextureId::User(1),
            SceneRendererGeneration::initial(),
            SceneEnumLifecycle::Ready,
            viewport,
            SceneCamera::initial(orientation).fit(viewport.physical_size())?,
        );
        Ok(Self {
            context,
            view: PresentationBoardView::new(),
            snapshot: DesktopBoardSnapshot::from_position(&BoardPosition::standard()),
            scene,
            actions: Vec::new(),
        })
    }

    fn request(square: BoardSquare, action: accesskit::Action) -> accesskit::ActionRequest {
        accesskit::ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node: PresentationBoardSemanticGrid::square_id(square).accesskit_id(),
            data: None,
        }
    }

    fn point(&self, square: BoardSquare) -> PresentationBoardTestResult<Pos2> {
        self.scene
            .viewport()
            .to_logical(
                self.scene
                    .camera()
                    .project(ScenePicking::square_center(square))
                    .ok_or("square must project into image")?,
            )
            .ok_or_else(|| "square must be inside clip".into())
    }

    fn frame(
        &mut self,
        events: Vec<Event>,
        repeat: bool,
    ) -> PresentationBoardTestResult<egui::FullOutput> {
        let mut result = Ok(());
        let scene = &self.scene;
        let snapshot = &self.snapshot;
        let view = &mut self.view;
        let actions = &mut self.actions;
        let mut output = self.context.run_ui(
            RawInput {
                screen_rect: Some(Self::screen()),
                events,
                ..Default::default()
            },
            |ui| {
                let mut picker = |point| {
                    let Some(pixel) = scene.viewport().to_physical(point) else {
                        return Ok(None);
                    };
                    scene
                        .camera()
                        .ray(pixel)
                        .map(ScenePicking::board_square)
                        .map_err(chess::scene::renderer::SceneRendererError::from)
                };
                match view.show_at(
                    ui,
                    Self::image(),
                    PresentationBoardViewInput::new(snapshot, Some(scene), &mut picker),
                ) {
                    Ok(frame) => actions.extend_from_slice(frame.actions()),
                    Err(error) => result = Err(error),
                }
                if repeat && ui.ctx().current_pass_index() == 0 {
                    ui.ctx()
                        .request_discard("exercise repeated native presentation pass");
                }
            },
        );
        output.textures_delta.clear();
        result?;
        Ok(output)
    }

    fn pointer(point: Pos2, pressed: bool) -> Vec<Event> {
        vec![
            Event::PointerMoved(point),
            Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            },
        ]
    }

    fn key(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }
}

#[test]
fn semantic_board_exposes_all_named_visible_focusable_squares_with_stable_ids()
-> PresentationBoardTestResult {
    let mut fixture = PresentationBoardFixture::new(BoardEnumColor::White)?;
    let output = fixture.frame(Vec::new(), false)?;
    let update = output
        .platform_output
        .accesskit_update
        .ok_or("missing accessibility update")?;
    let cells = update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == accesskit::Role::GridCell)
        .collect::<Vec<_>>();
    assert_eq!(cells.len(), usize::from(ScenePicking::SQUARES));
    for square in PresentationBoardSemanticGrid::squares() {
        let id = PresentationBoardSemanticGrid::square_id(square).accesskit_id();
        let (_, node) = cells
            .iter()
            .copied()
            .find(|(candidate, _)| *candidate == id)
            .ok_or("missing square node")?;
        assert!(node.supports_action(accesskit::Action::Focus));
        assert!(node.supports_action(accesskit::Action::Click));
        assert!(node.bounds().is_some());
        assert_eq!(
            node.label(),
            Some(PresentationBoardSemanticGrid::label(square, &fixture.snapshot, None).as_str())
        );
        assert_eq!(
            PresentationBoardSemanticGrid::square_for_accessibility_target(id),
            Some(square)
        );
    }
    assert!(fixture.actions.is_empty());
    Ok(())
}

#[test]
fn pointer_drag_preserves_its_origin_and_destination_across_repeated_passes()
-> PresentationBoardTestResult {
    let mut fixture = PresentationBoardFixture::new(BoardEnumColor::White)?;
    fixture.frame(Vec::new(), false)?;
    let origin: BoardSquare = "e2".parse()?;
    let destination: BoardSquare = "e4".parse()?;
    let start = fixture.point(origin)?;
    let end = fixture.point(destination)?;
    fixture.frame(
        vec![
            Event::PointerMoved(start),
            Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        false,
    )?;
    fixture.frame(
        vec![
            Event::PointerMoved(end),
            Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        true,
    )?;
    assert_eq!(fixture.actions.len(), 1);
    assert_eq!(fixture.actions[0].origin(), Some(origin));
    assert_eq!(fixture.actions[0].square(), destination);
    Ok(())
}

#[test]
fn native_click_is_delivered_once_across_multiple_egui_passes_without_mutating_chess()
-> PresentationBoardTestResult {
    let mut fixture = PresentationBoardFixture::new(BoardEnumColor::White)?;
    fixture.frame(Vec::new(), false)?;
    let square: BoardSquare = "e2".parse()?;
    let original = fixture.snapshot.fen().to_owned();
    let mut queue = PresentationAccessibilityInput::new();
    queue.try_forward(
        PresentationBoardFixture::request(square, accesskit::Action::Click),
        Some(fixture.scene.generation()),
    )?;
    let mut input = RawInput::default();
    let delivery = queue.drain_into(&mut input, Some(fixture.scene.generation()));
    assert_eq!(delivery.delivered(), 1);
    let output = fixture.frame(input.events, true)?;
    assert_eq!(output.platform_output.num_completed_passes, 2);
    assert_eq!(fixture.actions.len(), 1);
    assert_eq!(fixture.actions[0].square(), square);
    assert_eq!(fixture.view.selected(), Some(square));
    assert_eq!(fixture.snapshot.fen(), original);
    fixture.frame(Vec::new(), false)?;
    assert_eq!(fixture.actions.len(), 1);
    Ok(())
}

#[test]
fn keyboard_focus_tracks_visual_orientation_and_enter_activates_once() -> PresentationBoardTestResult
{
    for (orientation, expected) in [(BoardEnumColor::White, "e3"), (BoardEnumColor::Black, "e1")] {
        let mut fixture = PresentationBoardFixture::new(orientation)?;
        fixture.frame(Vec::new(), false)?;
        let origin: BoardSquare = "e2".parse()?;
        fixture.frame(
            vec![Event::AccessKitActionRequest(
                PresentationBoardFixture::request(origin, accesskit::Action::Focus),
            )],
            false,
        )?;
        fixture.frame(vec![PresentationBoardFixture::key(Key::ArrowUp)], false)?;
        let expected: BoardSquare = expected.parse()?;
        assert!(
            fixture.context.memory(
                |memory| memory.has_focus(PresentationBoardSemanticGrid::square_id(expected))
            ),
            "{orientation:?}: focused {:?}, expected {expected}",
            fixture
                .context
                .memory(|memory| memory.focused())
                .and_then(|id| PresentationBoardSemanticGrid::squares()
                    .find(|square| PresentationBoardSemanticGrid::square_id(*square) == id))
        );
        fixture.frame(vec![PresentationBoardFixture::key(Key::Enter)], true)?;
        assert_eq!(fixture.actions.len(), 1);
        assert_eq!(fixture.actions[0].square(), expected);
    }
    Ok(())
}

#[test]
fn accessibility_ingress_is_bounded_generation_checked_and_consumed_without_replay()
-> PresentationBoardTestResult {
    let fixture = PresentationBoardFixture::new(BoardEnumColor::White)?;
    let square: BoardSquare = "e2".parse()?;
    let request = PresentationBoardFixture::request(square, accesskit::Action::Click);
    let mut queue = PresentationAccessibilityInput::new();
    assert!(matches!(
        queue.try_forward(request.clone(), None),
        Err(PresentationAccessibilityError::MissingGeneration)
    ));
    for _ in 0..PresentationAccessibilityInput::CAPACITY {
        queue.try_forward(request.clone(), Some(fixture.scene.generation()))?;
    }
    assert!(matches!(
        queue.try_forward(request.clone(), Some(fixture.scene.generation())),
        Err(PresentationAccessibilityError::Busy)
    ));
    let mut input = RawInput::default();
    let delivery = queue.drain_into(&mut input, Some(fixture.scene.generation().next()?));
    assert_eq!(delivery.stale(), PresentationAccessibilityInput::CAPACITY);
    assert_eq!(delivery.delivered(), 0);
    assert!(input.events.is_empty());
    assert_eq!(
        queue
            .drain_into(&mut input, Some(fixture.scene.generation()))
            .delivered(),
        0
    );
    queue.try_forward(request, Some(fixture.scene.generation()))?;
    assert_eq!(
        queue
            .drain_into(&mut input, Some(fixture.scene.generation()))
            .delivered(),
        1
    );
    assert_eq!(
        queue
            .drain_into(&mut input, Some(fixture.scene.generation()))
            .delivered(),
        0
    );
    assert_eq!(input.events.len(), 1);
    Ok(())
}

#[test]
fn changed_scene_consumes_pending_pointer_release_then_accepts_a_fresh_press()
-> PresentationBoardTestResult {
    let mut fixture = PresentationBoardFixture::new(BoardEnumColor::White)?;
    fixture.frame(Vec::new(), false)?;
    let origin = fixture.point("e2".parse()?)?;
    let target = fixture.point("e4".parse()?)?;
    fixture.frame(PresentationBoardFixture::pointer(origin, true), false)?;
    fixture.scene = PresentationBoardScene::new(
        fixture.scene.texture(),
        fixture.scene.generation().next()?,
        SceneEnumLifecycle::Ready,
        fixture.scene.viewport(),
        fixture.scene.camera().clone(),
    );
    fixture.frame(PresentationBoardFixture::pointer(target, false), true)?;
    assert!(fixture.actions.is_empty());
    assert_eq!(fixture.view.selected(), None);
    fixture.frame(PresentationBoardFixture::pointer(target, true), false)?;
    fixture.frame(PresentationBoardFixture::pointer(target, false), false)?;
    assert_eq!(fixture.actions.len(), 1);
    assert_eq!(fixture.actions[0].square(), "e4".parse()?);
    Ok(())
}

#[test]
fn every_square_has_a_projected_semantic_target_across_camera_bounds() -> PresentationBoardTestResult
{
    let viewport = ScenePickingViewport::new(
        PresentationBoardFixture::image(),
        PresentationBoardFixture::screen(),
        2.0,
        0.75,
    )?;
    for orientation in [BoardEnumColor::White, BoardEnumColor::Black] {
        for yaw in [0.0, 45.0, 90.0, 180.0, 270.0] {
            for tilt in [0.0, 20.0, 45.0] {
                let scene = PresentationBoardScene::new(
                    TextureId::User(1),
                    SceneRendererGeneration::initial(),
                    SceneEnumLifecycle::Ready,
                    viewport,
                    SceneCamera::new(yaw, tilt, SceneCamera::MAX_ZOOM, orientation)?
                        .fit(viewport.physical_size())?,
                );
                for square in PresentationBoardSemanticGrid::squares() {
                    let rect = PresentationBoardSemanticGrid::square_rect(square, &scene)
                        .ok_or("missing projected square")?;
                    assert!(rect.is_finite() && rect.is_positive());
                    assert!(PresentationBoardFixture::image().contains_rect(rect));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn native_selection_sync_clears_stale_highlights_and_cancels_a_pending_drag()
-> PresentationBoardTestResult {
    let mut fixture = PresentationBoardFixture::new(BoardEnumColor::White)?;
    fixture.frame(Vec::new(), false)?;
    let origin: BoardSquare = "e2".parse()?;
    let destination: BoardSquare = "e4".parse()?;
    fixture.view.set_selected(Some(origin));
    let start = fixture.point(origin)?;
    let end = fixture.point(destination)?;
    fixture.frame(PresentationBoardFixture::pointer(start, true), false)?;
    fixture.view.set_selected(None);
    let output = fixture.frame(PresentationBoardFixture::pointer(end, false), true)?;
    assert!(fixture.actions.is_empty());
    assert_eq!(fixture.view.selected(), None);
    let update = output
        .platform_output
        .accesskit_update
        .ok_or("missing cleared board semantics")?;
    assert!(
        update
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == accesskit::Role::GridCell)
            .all(|(_, node)| node
                .label()
                .is_none_or(|label| !label.contains(", selected")))
    );
    let selected: BoardSquare = "d2".parse()?;
    fixture.view.set_selected(Some(selected));
    let output = fixture.frame(Vec::new(), false)?;
    assert!(fixture.actions.is_empty());
    let update = output
        .platform_output
        .accesskit_update
        .ok_or("missing synchronized selection semantics")?;
    let (_, node) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == PresentationBoardSemanticGrid::square_id(selected).accesskit_id())
        .ok_or("missing selected square")?;
    assert!(
        node.label()
            .is_some_and(|label| label.contains(", selected"))
    );
    Ok(())
}
