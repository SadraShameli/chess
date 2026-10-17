use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy_egui::egui::{self, Context, Event, Key, Modifiers, Pos2, RawInput, Rect, accesskit};

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::application::{
    DesktopApplication, DesktopApplicationWakeError, DesktopApplicationWakeProvider,
};
use chess::desktop::dto::DesktopBoardSnapshot;
use chess::game::premove::GamePremoveIntent;
use chess::presentation::app::PresentationApp;
use chess::presentation::board::animation::{
    PresentationBoardAnimation, PresentationBoardAnimationEnumOutcome,
};
use chess::presentation::board::scene::PresentationBoardScene;
use chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use chess::presentation::board::view::{PresentationBoardView, PresentationBoardViewInput};
use chess::presentation::theme::tokens::PresentationThemeTokens;
use chess::scene::camera::SceneCamera;
use chess::scene::enums::SceneEnumLifecycle;
use chess::scene::picking::{ScenePicking, ScenePickingViewport};
use chess::scene::renderer::SceneRendererGeneration;

type PresentationBoardAccessibilityResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const MIN_LOGICAL_TARGET: f32 = 24.0;
const SCREENS: [(f32, f32); 3] = [(1280.0, 800.0), (1440.0, 900.0), (2560.0, 1440.0)];
const SCALES: [f32; 3] = [1.0, 1.5, 2.0];
const BOARD_INSET: f32 = 32.0;
const PROMOTION_CHOICES: usize = 5;
const TAB_CYCLES: usize = 12;

struct PresentationBoardAccessibilityBoard {
    context: Context,
    view: PresentationBoardView,
    snapshot: DesktopBoardSnapshot,
    scene: PresentationBoardScene,
    screen: Rect,
    image: Rect,
    pending: Option<GamePremoveIntent>,
    tree: HashMap<accesskit::NodeId, accesskit::Node>,
}

impl PresentationBoardAccessibilityBoard {
    fn new(
        orientation: BoardEnumColor,
        screen_size: (f32, f32),
        scale: f32,
        camera: SceneCamera,
    ) -> PresentationBoardAccessibilityResult<Self> {
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(screen_size.0, screen_size.1));
        let side = (screen_size.1 - BOARD_INSET * 2.0).min(screen_size.0 - BOARD_INSET * 2.0);
        let image =
            Rect::from_min_size(egui::pos2(BOARD_INSET, BOARD_INSET), egui::vec2(side, side));
        let viewport = ScenePickingViewport::new(image, screen, scale, 1.0)?;
        let context = Context::default();
        context.enable_accesskit();
        context.set_pixels_per_point(scale);
        Ok(Self {
            context,
            view: PresentationBoardView::new(),
            snapshot: DesktopBoardSnapshot::from_position(&BoardPosition::standard()),
            scene: PresentationBoardScene::new(
                egui::TextureId::User(1),
                SceneRendererGeneration::initial(),
                SceneEnumLifecycle::Ready,
                viewport,
                camera
                    .with_orientation(orientation)
                    .fit(viewport.physical_size())?,
            ),
            screen,
            image,
            pending: None,
            tree: HashMap::new(),
        })
    }

    fn standard(orientation: BoardEnumColor) -> PresentationBoardAccessibilityResult<Self> {
        Self::new(
            orientation,
            SCREENS[0],
            1.0,
            SceneCamera::initial(orientation),
        )
    }

    fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
        let scene = &self.scene;
        let snapshot = &self.snapshot;
        let view = &mut self.view;
        let image = self.image;
        let pending = self.pending;
        let mut output = self.context.run_ui(
            RawInput {
                screen_rect: Some(self.screen),
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
                let _ = view.show_at(
                    ui,
                    image,
                    PresentationBoardViewInput::new(snapshot, Some(scene), &mut picker)
                        .with_pending_move(pending),
                );
            },
        );
        output.textures_delta.clear();
        if let Some(update) = &output.platform_output.accesskit_update {
            self.tree.extend(update.nodes.iter().cloned());
        }
        output
    }

    fn action(square: BoardSquare, action: accesskit::Action) -> Event {
        Event::AccessKitActionRequest(accesskit::ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node: PresentationBoardSemanticGrid::square_id(square).accesskit_id(),
            data: None,
        })
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

    fn node(&self, square: BoardSquare) -> PresentationBoardAccessibilityResult<&accesskit::Node> {
        self.tree
            .get(&PresentationBoardSemanticGrid::square_id(square).accesskit_id())
            .ok_or_else(|| "missing square node".into())
    }

    fn focused(&self) -> Option<BoardSquare> {
        let focused = self.context.memory(|memory| memory.focused())?;
        PresentationBoardSemanticGrid::squares()
            .find(|square| PresentationBoardSemanticGrid::square_id(*square) == focused)
    }
}

#[test]
fn board_is_one_labelled_eight_by_eight_grid_with_row_and_column_positions()
-> PresentationBoardAccessibilityResult {
    let mut board = PresentationBoardAccessibilityBoard::standard(BoardEnumColor::White)?;
    let output = board.frame(Vec::new());
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .ok_or("missing accessibility update")?
        .nodes;
    let grids = nodes
        .iter()
        .filter(|(_, node)| node.role() == accesskit::Role::Grid)
        .collect::<Vec<_>>();
    assert_eq!(grids.len(), 1);
    assert_eq!(grids[0].1.label(), Some("Chess board"));
    assert_eq!(grids[0].1.row_count(), Some(8));
    assert_eq!(grids[0].1.column_count(), Some(8));
    for square in PresentationBoardSemanticGrid::squares() {
        let node = board.node(square)?;
        assert_eq!(node.role(), accesskit::Role::GridCell);
        let index = usize::from(u8::from(square));
        assert_eq!(node.row_index(), Some(index / 8));
        assert_eq!(node.column_index(), Some(index % 8));
    }
    Ok(())
}

#[test]
fn every_label_names_the_square_and_its_piece_or_emptiness_without_color_or_position_cues()
-> PresentationBoardAccessibilityResult {
    let mut board = PresentationBoardAccessibilityBoard::standard(BoardEnumColor::White)?;
    board.frame(Vec::new());
    for (square, expected) in [
        ("e1", "e1: White king"),
        ("d8", "d8: Black queen"),
        ("b1", "b1: White knight"),
        ("a7", "a7: Black pawn"),
        ("e4", "e4: empty"),
    ] {
        let node = board.node(square.parse()?)?;
        assert_eq!(node.label(), Some(expected));
    }
    Ok(())
}

#[test]
fn selection_and_legal_destinations_are_announced_as_text_and_selected_state()
-> PresentationBoardAccessibilityResult {
    let mut board = PresentationBoardAccessibilityBoard::standard(BoardEnumColor::White)?;
    board.frame(Vec::new());
    let origin: BoardSquare = "e2".parse()?;
    board.frame(vec![PresentationBoardAccessibilityBoard::action(
        origin,
        accesskit::Action::Click,
    )]);
    board.frame(Vec::new());
    assert_eq!(board.view.selected(), Some(origin));
    let selected = board.node(origin)?;
    assert_eq!(selected.is_selected(), Some(true));
    assert!(
        selected
            .label()
            .is_some_and(|label| label.ends_with(", selected"))
    );
    for destination in ["e3", "e4"] {
        let node = board.node(destination.parse()?)?;
        assert!(
            node.label()
                .is_some_and(|label| label.ends_with(", legal destination")),
            "{destination}: {:?}",
            node.label()
        );
        assert_ne!(node.is_selected(), Some(true));
    }
    let other = board.node("e5".parse()?)?;
    assert_eq!(other.label(), Some("e5: empty"));
    Ok(())
}

#[test]
fn queued_premove_squares_carry_text_roles_and_visible_text_markers()
-> PresentationBoardAccessibilityResult {
    let mut board = PresentationBoardAccessibilityBoard::standard(BoardEnumColor::White)?;
    board.pending = Some(GamePremoveIntent::new("g1".parse()?, "f3".parse()?, None)?);
    let output = board.frame(Vec::new());
    for (square, role, marker) in [
        ("g1", "queued premove source", "Premove from"),
        ("f3", "queued premove destination", "Premove to"),
    ] {
        let node = board.node(square.parse()?)?;
        assert!(node.label().is_some_and(|label| label.contains(role)));
        assert!(
            output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::epaint::Shape::Text(text) if text.galley.job.text == marker
            )),
            "missing visible {marker} marker"
        );
    }
    Ok(())
}

#[test]
fn focused_square_draws_a_visible_two_point_ring_and_arrows_stop_at_the_board_edge()
-> PresentationBoardAccessibilityResult {
    let mut board = PresentationBoardAccessibilityBoard::standard(BoardEnumColor::White)?;
    board.frame(Vec::new());
    let corner: BoardSquare = "a1".parse()?;
    board.frame(vec![PresentationBoardAccessibilityBoard::action(
        corner,
        accesskit::Action::Focus,
    )]);
    let output = board.frame(Vec::new());
    assert_eq!(board.focused(), Some(corner));
    assert!(
        output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::epaint::Shape::Rect(rect) if (rect.stroke.width - 2.0).abs() < f32::EPSILON
        )),
        "focused square must draw a visible focus ring"
    );
    for key in [Key::ArrowDown, Key::ArrowLeft] {
        board.frame(vec![PresentationBoardAccessibilityBoard::key(key)]);
        assert_eq!(board.focused(), Some(corner), "{key:?} must not drop focus");
    }
    board.frame(vec![PresentationBoardAccessibilityBoard::key(Key::ArrowUp)]);
    assert_eq!(board.focused(), Some("a2".parse()?));
    Ok(())
}

#[test]
fn semantic_squares_stay_inside_the_board_and_meet_the_minimum_target_at_every_layout_scale()
-> PresentationBoardAccessibilityResult {
    assert!(PresentationThemeTokens::MIN_TARGET >= MIN_LOGICAL_TARGET);
    for orientation in [BoardEnumColor::White, BoardEnumColor::Black] {
        for screen in SCREENS {
            for scale in SCALES {
                let board = PresentationBoardAccessibilityBoard::new(
                    orientation,
                    screen,
                    scale,
                    SceneCamera::initial(orientation),
                )?;
                for square in PresentationBoardSemanticGrid::squares() {
                    let rect = PresentationBoardSemanticGrid::square_rect(square, &board.scene)
                        .ok_or("missing projected square")?;
                    assert!(board.image.contains_rect(rect));
                    assert!(
                        rect.width() >= MIN_LOGICAL_TARGET && rect.height() >= MIN_LOGICAL_TARGET,
                        "{orientation:?} {screen:?} x{scale} {square}: {:?}",
                        rect.size()
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn nodes_report_logical_bounds_that_match_the_projected_rectangles_at_every_scale()
-> PresentationBoardAccessibilityResult {
    for scale in SCALES {
        let mut board = PresentationBoardAccessibilityBoard::new(
            BoardEnumColor::White,
            SCREENS[1],
            scale,
            SceneCamera::initial(BoardEnumColor::White),
        )?;
        board.frame(Vec::new());
        board.frame(Vec::new());
        for square in PresentationBoardSemanticGrid::squares() {
            let rect = PresentationBoardSemanticGrid::square_rect(square, &board.scene)
                .ok_or("missing projected square")?;
            let bounds = board
                .node(square)
                .map_err(|error| format!("{error} {square} x{scale}"))?
                .bounds()
                .ok_or("missing node bounds")?;
            assert!(
                (bounds.x0 as f32 - rect.min.x).abs() < 1.0,
                "{square} x{scale}"
            );
            assert!(
                (bounds.y1 as f32 - rect.max.y).abs() < 1.0,
                "{square} x{scale}"
            );
        }
    }
    Ok(())
}

#[test]
fn reduced_motion_camera_reset_and_selection_apply_their_final_state_in_one_frame()
-> PresentationBoardAccessibilityResult {
    let moved = SceneCamera::initial(BoardEnumColor::Black)
        .rotate_right()
        .tilt_up()
        .zoom_out();
    let clock = Arc::new(PresentationBoardAccessibilityClock::default());
    let mut animated = PresentationBoardAnimation::new(clock.clone(), false);
    let (_, outcome) = animated.start_camera_reset(moved)?;
    assert_eq!(outcome, PresentationBoardAnimationEnumOutcome::Animating);
    assert!(animated.repaint_delay().is_some());
    let mut reduced = PresentationBoardAnimation::new(clock, true);
    let (target, outcome) = reduced.start_camera_reset(moved)?;
    assert_eq!(outcome, PresentationBoardAnimationEnumOutcome::Settled);
    assert_eq!(target, SceneCamera::initial(BoardEnumColor::Black));
    assert_eq!(target.orientation(), moved.orientation());
    assert_eq!(
        reduced.start_flip()?,
        PresentationBoardAnimationEnumOutcome::Settled
    );
    assert_eq!(
        reduced.set_panel_visible(false)?,
        PresentationBoardAnimationEnumOutcome::Settled
    );
    let frame = reduced.frame()?;
    assert!(frame.camera().is_none() && frame.piece().is_none());
    assert_eq!(frame.flip_residual_degrees(), 0.0);
    assert_eq!(frame.panel_progress(), 0.0);
    assert!(reduced.repaint_delay().is_none());
    let mut board = PresentationBoardAccessibilityBoard::standard(BoardEnumColor::White)?;
    board.frame(Vec::new());
    let origin: BoardSquare = "e2".parse()?;
    board.frame(vec![PresentationBoardAccessibilityBoard::action(
        origin,
        accesskit::Action::Click,
    )]);
    assert_eq!(board.view.selected(), Some(origin));
    assert!(reduced.repaint_delay().is_none());
    Ok(())
}

#[derive(Default)]
struct PresentationBoardAccessibilityClock {
    nanos: AtomicU64,
}

impl ClockTimeProvider for PresentationBoardAccessibilityClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.nanos.load(Ordering::SeqCst)))
    }
}

struct PresentationBoardAccessibilityWake;

impl DesktopApplicationWakeProvider for PresentationBoardAccessibilityWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

struct PresentationBoardAccessibilityApp {
    context: Context,
    presentation: PresentationApp,
    application:
        DesktopApplication<PresentationBoardAccessibilityClock, PresentationBoardAccessibilityWake>,
    tree: HashMap<accesskit::NodeId, accesskit::Node>,
}

impl PresentationBoardAccessibilityApp {
    fn new() -> Self {
        let context = Context::default();
        context.enable_accesskit();
        let clock = Arc::new(PresentationBoardAccessibilityClock::default());
        Self {
            context,
            presentation: PresentationApp::new(clock.clone()),
            application: DesktopApplication::new(
                clock,
                Arc::new(PresentationBoardAccessibilityWake),
            ),
            tree: HashMap::new(),
        }
    }

    fn frame(
        &mut self,
        scene: Option<&PresentationBoardScene>,
        events: Vec<Event>,
    ) -> PresentationBoardAccessibilityResult<(egui::FullOutput, Option<ScenePickingViewport>)>
    {
        let mut result = None;
        let mut output = self.context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                events,
                ..Default::default()
            },
            |ui| {
                let mut picker = |_point| Ok(None);
                result =
                    Some(
                        self.presentation
                            .show(ui, &self.application.port(), scene, &mut picker),
                    );
            },
        );
        output.textures_delta.clear();
        if let Some(update) = &output.platform_output.accesskit_update {
            self.tree.extend(update.nodes.iter().cloned());
        }
        let frame = result.ok_or("missing presentation frame")??;
        Ok((output, frame.viewport()))
    }

    fn has_role(&self, role: accesskit::Role) -> bool {
        self.tree.values().any(|node| node.role() == role)
    }

    fn count_labels_with_prefix(&self, prefix: &str) -> usize {
        self.tree
            .values()
            .filter(|node| node.label().is_some_and(|label| label.starts_with(prefix)))
            .count()
    }

    fn focused_node(
        &self,
        _output: &egui::FullOutput,
    ) -> PresentationBoardAccessibilityResult<&accesskit::Node> {
        let focused = self
            .context
            .memory(|memory| memory.focused())
            .ok_or("focus was lost")?
            .accesskit_id();
        self.tree
            .get(&focused)
            .ok_or_else(|| "focused node is not exposed".into())
    }

    fn tab(shift: bool) -> Event {
        Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: if shift {
                Modifiers::SHIFT
            } else {
                Modifiers::NONE
            },
        }
    }
}

#[test]
fn quit_dialog_starts_on_the_safe_choice_and_contains_tab_focus_until_escape_closes_it()
-> PresentationBoardAccessibilityResult {
    let mut app = PresentationBoardAccessibilityApp::new();
    app.presentation.request_quit();
    app.frame(None, Vec::new())?;
    let (output, _) = app.frame(None, Vec::new())?;
    assert_eq!(app.focused_node(&output)?.label(), Some("Keep open"));
    for step in 0..TAB_CYCLES {
        let (output, _) = app.frame(
            None,
            vec![PresentationBoardAccessibilityApp::tab(step % 2 == 1)],
        )?;
        let label = app
            .focused_node(&output)
            .map_err(|error| format!("step {step}: {error}"))?
            .label()
            .map(str::to_owned);
        assert!(
            matches!(label.as_deref(), Some("Keep open" | "Confirm quit")),
            "focus escaped the quit dialog: {label:?}"
        );
    }
    app.frame(
        None,
        vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    )?;
    assert!(!app.presentation.is_quit_confirmation_open());
    assert!(app.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn quit_dialog_is_exposed_to_assistive_technology_as_a_dialog()
-> PresentationBoardAccessibilityResult {
    let mut app = PresentationBoardAccessibilityApp::new();
    app.presentation.request_quit();
    app.frame(None, Vec::new())?;
    app.frame(None, Vec::new())?;
    assert!(app.has_role(accesskit::Role::Dialog));
    Ok(())
}

#[test]
fn promotion_dialog_contains_focus_on_its_choices_and_escape_returns_to_the_origin_square()
-> PresentationBoardAccessibilityResult {
    let session = crate::game_transition_fixture::GameTransitionFixture::session_with_start(
        chess::clock::time_control::ClockTimeControl::untimed(),
        None,
        chess::board::start::BoardStart::custom(BoardPosition::from_fen(
            "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            BoardEnumVariant::Standard,
        )?),
    )?;
    let mut app = PresentationBoardAccessibilityApp::new();
    app.presentation.accept_game_snapshot(
        chess::desktop::game_snapshot::DesktopGameSnapshot::capture(
            &session,
            ClockInstant::from_nanos(0),
        )?,
    );
    app.frame(None, Vec::new())?;
    let (_, viewport) = app.frame(None, Vec::new())?;
    let viewport = viewport.ok_or("missing mounted board viewport")?;
    let scene = PresentationBoardScene::new(
        egui::TextureId::User(43),
        SceneRendererGeneration::initial(),
        SceneEnumLifecycle::Ready,
        viewport,
        SceneCamera::initial(BoardEnumColor::White).fit(viewport.physical_size())?,
    );
    app.frame(Some(&scene), Vec::new())?;
    for square in ["a7", "a8"] {
        app.frame(
            Some(&scene),
            vec![PresentationBoardAccessibilityBoard::action(
                square.parse()?,
                accesskit::Action::Click,
            )],
        )?;
    }
    app.frame(Some(&scene), Vec::new())?;
    let (output, _) = app.frame(Some(&scene), Vec::new())?;
    assert!(app.has_role(accesskit::Role::Dialog));
    let choices = app.count_labels_with_prefix("Promote to ");
    assert_eq!(choices + 1, PROMOTION_CHOICES);
    assert_eq!(app.focused_node(&output)?.label(), Some("Promote to Queen"));
    for step in 0..TAB_CYCLES {
        let (output, _) = app.frame(
            Some(&scene),
            vec![PresentationBoardAccessibilityApp::tab(
                step >= TAB_CYCLES / 2,
            )],
        )?;
        let node = app.focused_node(&output)?;
        assert_eq!(node.role(), accesskit::Role::Button);
        assert!(
            node.label().is_some_and(
                |label| label.starts_with("Promote to ") || label == "Cancel Promotion"
            ),
            "focus escaped the promotion dialog: {:?}",
            node.label()
        );
    }
    app.frame(
        Some(&scene),
        vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    )?;
    app.frame(Some(&scene), Vec::new())?;
    assert_eq!(
        app.context.memory(|memory| memory.focused()),
        Some(PresentationBoardSemanticGrid::square_id("a7".parse()?))
    );
    Ok(())
}
