use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use std::time::Duration;

use bevy_egui::egui::{Context, Pos2, RawInput, Rect, ViewportId};

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::dto::DesktopBoardSnapshot;
use chess::desktop::preferences::{DesktopPreferences, DesktopPreferencesEnumMotion};
use chess::presentation::board::animation::{
    PresentationBoardAnimation, PresentationBoardAnimationEnumOutcome,
};
use chess::presentation::board::enums::PresentationBoardEnumCameraAction;
use chess::presentation::board::transition::PresentationBoardTransition;
use chess::presentation::board::view::{PresentationBoardView, PresentationBoardViewInput};
use chess::scene::camera::SceneCamera;

type PresentationBoardAnimationResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const NANOS_PER_MILLI: u64 = 1_000_000;
const TOLERANCE: f32 = 0.001;
const WARMUP_PASSES: usize = 4;

#[derive(Default)]
struct PresentationBoardAnimationClock {
    nanos: AtomicU64,
}

impl PresentationBoardAnimationClock {
    fn advance_millis(&self, millis: u64) {
        self.nanos
            .fetch_add(millis * NANOS_PER_MILLI, Ordering::SeqCst);
    }

    fn rewind_millis(&self, millis: u64) {
        self.nanos
            .fetch_sub(millis * NANOS_PER_MILLI, Ordering::SeqCst);
    }
}

impl ClockTimeProvider for PresentationBoardAnimationClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.nanos.load(Ordering::SeqCst)))
    }
}

struct PresentationBoardAnimationFixture {
    clock: Arc<PresentationBoardAnimationClock>,
    animation: PresentationBoardAnimation,
}

impl PresentationBoardAnimationFixture {
    fn new(reduced_motion: bool) -> Self {
        let clock = Arc::new(PresentationBoardAnimationClock::default());
        clock.advance_millis(1_000);
        let animation = PresentationBoardAnimation::new(clock.clone(), reduced_motion);
        Self { clock, animation }
    }

    fn square(name: &str) -> PresentationBoardAnimationResult<BoardSquare> {
        Ok(name.parse()?)
    }

    fn moved_camera() -> SceneCamera {
        SceneCamera::initial(BoardEnumColor::Black)
            .rotate_right()
            .tilt_up()
            .zoom_out()
    }
}

fn repaint_delay_of(
    animation: &PresentationBoardAnimation,
) -> PresentationBoardAnimationResult<Duration> {
    let context = Context::default();
    for _ in 0..WARMUP_PASSES {
        let mut warmup = context.run_ui(RawInput::default(), |_ui| {});
        warmup.textures_delta.clear();
    }
    let mut output = context.run_ui(RawInput::default(), |ui| {
        animation.request_repaint(ui.ctx());
    });
    output.textures_delta.clear();
    Ok(output
        .viewport_output
        .get(&ViewportId::ROOT)
        .ok_or("missing root viewport")?
        .repaint_delay)
}

fn near(left: f32, right: f32) -> bool {
    (left - right).abs() < TOLERANCE
}

#[test]
fn durations_are_the_approved_typed_constants() {
    assert_eq!(PresentationBoardAnimation::PIECE_DURATION.as_millis(), 180);
    assert_eq!(PresentationBoardAnimation::FLIP_DURATION.as_millis(), 180);
    assert_eq!(PresentationBoardAnimation::PANEL_DURATION.as_millis(), 160);
}

#[test]
fn piece_motion_follows_the_injected_clock_and_retires_at_its_duration()
-> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(false);
    let origin = PresentationBoardAnimationFixture::square("e2")?;
    let destination = PresentationBoardAnimationFixture::square("e4")?;
    assert_eq!(
        fixture.animation.start_piece_move(origin, destination)?,
        PresentationBoardAnimationEnumOutcome::Animating
    );
    let start = fixture.animation.frame()?.piece().ok_or("missing start")?;
    assert!(near(start.progress(), 0.0));
    assert_eq!((start.origin(), start.destination()), (origin, destination));
    fixture.clock.advance_millis(90);
    let middle = fixture.animation.frame()?.piece().ok_or("missing middle")?;
    assert!(near(middle.progress(), 0.875));
    assert!(fixture.animation.is_active());
    fixture.clock.advance_millis(90);
    let end = fixture.animation.frame()?.piece().ok_or("missing end")?;
    assert!(near(end.progress(), 1.0));
    assert!(!fixture.animation.is_active());
    assert!(fixture.animation.frame()?.piece().is_none());
    Ok(())
}

#[test]
fn accepted_move_cancels_the_obsolete_animation_without_waiting_for_it()
-> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(false);
    let first = PresentationBoardAnimationFixture::square("e2")?;
    let second = PresentationBoardAnimationFixture::square("e4")?;
    let third = PresentationBoardAnimationFixture::square("e5")?;
    fixture.animation.start_piece_move(first, second)?;
    fixture.clock.advance_millis(60);
    fixture.animation.start_piece_move(second, third)?;
    let replaced = fixture.animation.frame()?.piece().ok_or("missing piece")?;
    assert_eq!((replaced.origin(), replaced.destination()), (second, third));
    assert!(near(replaced.progress(), 0.0));
    fixture.animation.start_flip()?;
    fixture.animation.cancel_obsolete_move();
    let cancelled = fixture.animation.frame()?;
    assert!(cancelled.piece().is_none());
    assert!(near(cancelled.flip_residual_degrees(), 0.0));
    assert!(!fixture.animation.is_active());
    Ok(())
}

#[test]
fn repaints_are_requested_only_while_an_animation_is_active() -> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(false);
    assert!(fixture.animation.repaint_delay().is_none());
    assert_eq!(repaint_delay_of(&fixture.animation)?, Duration::MAX);
    fixture.animation.start_flip()?;
    assert_eq!(
        fixture.animation.repaint_delay(),
        Some(PresentationBoardAnimation::REPAINT_INTERVAL)
    );
    assert!(
        repaint_delay_of(&fixture.animation)?
            <= PresentationBoardAnimation::REPAINT_INTERVAL.as_duration()
    );
    fixture.clock.advance_millis(180);
    fixture.animation.frame()?;
    assert!(fixture.animation.repaint_delay().is_none());
    assert_eq!(repaint_delay_of(&fixture.animation)?, Duration::MAX);
    Ok(())
}

#[test]
fn flip_turns_a_half_turn_residual_to_zero_and_keeps_camera_offsets_out()
-> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(false);
    let camera = PresentationBoardAnimationFixture::moved_camera();
    fixture.animation.start_flip()?;
    let start = fixture.animation.frame()?;
    assert!(near(start.flip_residual_degrees().abs(), 180.0));
    assert!(start.camera().is_none());
    fixture.clock.advance_millis(90);
    let middle = fixture.animation.frame()?;
    assert!(near(middle.flip_residual_degrees().abs(), 22.5));
    fixture.clock.advance_millis(90);
    assert!(near(
        fixture.animation.frame()?.flip_residual_degrees(),
        0.0
    ));
    assert_eq!(
        camera.with_orientation(BoardEnumColor::White).pose(),
        camera.pose()
    );
    Ok(())
}

#[test]
fn camera_reset_interpolates_pose_but_never_changes_game_orientation()
-> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(false);
    let moved = PresentationBoardAnimationFixture::moved_camera();
    let (target, outcome) = fixture.animation.start_camera_reset(moved)?;
    assert_eq!(outcome, PresentationBoardAnimationEnumOutcome::Animating);
    assert_eq!(target, SceneCamera::initial(BoardEnumColor::Black));
    fixture.clock.advance_millis(90);
    let middle = fixture
        .animation
        .frame()?
        .camera()
        .ok_or("missing camera")?;
    assert_eq!(middle.orientation(), BoardEnumColor::Black);
    assert!(near(middle.yaw_degrees(), 15.0 * (1.0 - 0.875)));
    assert!(near(
        middle.tilt_degrees(),
        moved.tilt_degrees() + (target.tilt_degrees() - moved.tilt_degrees()) * 0.875
    ));
    assert!(near(
        middle.zoom(),
        moved.zoom() + (target.zoom() - moved.zoom()) * 0.875
    ));
    fixture.clock.advance_millis(90);
    assert_eq!(fixture.animation.frame()?.camera(), Some(target));
    assert!(fixture.animation.frame()?.camera().is_none());
    Ok(())
}

#[test]
fn panel_motion_runs_for_160_ms_and_reverses_from_its_current_progress()
-> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(false);
    assert!(near(fixture.animation.frame()?.panel_progress(), 1.0));
    fixture.animation.set_panel_visible(false)?;
    fixture.clock.advance_millis(80);
    assert!(near(fixture.animation.frame()?.panel_progress(), 0.125));
    fixture.animation.set_panel_visible(true)?;
    assert!(near(fixture.animation.frame()?.panel_progress(), 0.125));
    fixture.clock.advance_millis(160);
    assert!(near(fixture.animation.frame()?.panel_progress(), 1.0));
    assert!(!fixture.animation.is_active());
    assert!(fixture.animation.is_panel_visible());
    Ok(())
}

#[test]
fn panel_is_sliding_only_between_its_resting_states() -> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(false);
    assert!(!fixture.animation.frame()?.is_panel_sliding());
    fixture.animation.set_panel_visible(false)?;
    fixture.clock.advance_millis(80);
    assert!(fixture.animation.frame()?.is_panel_sliding());
    fixture.clock.advance_millis(160);
    let settled = fixture.animation.frame()?;
    assert!(!settled.is_panel_sliding());
    assert!(near(settled.panel_progress(), 0.0));
    Ok(())
}

#[test]
fn reduced_motion_applies_final_states_immediately_and_schedules_nothing()
-> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(true);
    let origin = PresentationBoardAnimationFixture::square("e2")?;
    let destination = PresentationBoardAnimationFixture::square("e4")?;
    let moved = PresentationBoardAnimationFixture::moved_camera();
    let settled = PresentationBoardAnimationEnumOutcome::Settled;
    assert_eq!(
        fixture.animation.start_piece_move(origin, destination)?,
        settled
    );
    assert_eq!(fixture.animation.start_flip()?, settled);
    assert_eq!(fixture.animation.set_panel_visible(false)?, settled);
    let (target, outcome) = fixture.animation.start_camera_reset(moved)?;
    assert_eq!(outcome, settled);
    assert_eq!(target, moved.reset());
    let frame = fixture.animation.frame()?;
    assert!(frame.piece().is_none());
    assert!(frame.camera().is_none());
    assert!(near(frame.flip_residual_degrees(), 0.0));
    assert!(near(frame.panel_progress(), 0.0));
    assert!(!fixture.animation.is_panel_visible());
    assert!(fixture.animation.repaint_delay().is_none());
    assert_eq!(repaint_delay_of(&fixture.animation)?, Duration::MAX);
    Ok(())
}

#[test]
fn enabling_reduced_motion_cancels_running_animations_in_one_frame()
-> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(false);
    let origin = PresentationBoardAnimationFixture::square("e2")?;
    let destination = PresentationBoardAnimationFixture::square("e4")?;
    fixture.animation.start_piece_move(origin, destination)?;
    fixture.animation.start_flip()?;
    fixture.animation.set_panel_visible(false)?;
    fixture
        .animation
        .start_camera_reset(PresentationBoardAnimationFixture::moved_camera())?;
    assert!(fixture.animation.is_active());
    fixture.animation.set_reduced_motion(true);
    assert!(fixture.animation.is_reduced_motion());
    assert!(!fixture.animation.is_active());
    let frame = fixture.animation.frame()?;
    assert!(frame.piece().is_none() && frame.camera().is_none());
    assert!(near(frame.panel_progress(), 0.0));
    Ok(())
}

#[test]
fn a_clock_that_moves_backwards_is_a_typed_error() -> PresentationBoardAnimationResult {
    let mut fixture = PresentationBoardAnimationFixture::new(false);
    fixture.animation.start_flip()?;
    fixture.clock.rewind_millis(10);
    assert!(fixture.animation.frame().is_err());
    Ok(())
}

#[test]
fn reduced_motion_preference_is_typed_defaults_off_and_is_listed_for_settings() {
    let mut preferences = DesktopPreferences::default();
    assert_eq!(preferences.motion(), DesktopPreferencesEnumMotion::Full);
    assert!(!preferences.motion().is_reduced());
    assert_eq!(
        DesktopPreferences::motions(),
        [
            DesktopPreferencesEnumMotion::Full,
            DesktopPreferencesEnumMotion::Reduced
        ]
    );
    assert!(preferences.set_motion(DesktopPreferencesEnumMotion::Reduced));
    assert!(!preferences.set_motion(DesktopPreferencesEnumMotion::Reduced));
    assert!(preferences.motion().is_reduced());
    assert_eq!(preferences.motion().label(), "Reduced motion");
}

fn snapshot_after(
    fen: &str,
    uci: &str,
) -> PresentationBoardAnimationResult<(DesktopBoardSnapshot, DesktopBoardSnapshot)> {
    let before = BoardPosition::from_fen(fen, BoardEnumVariant::Standard)?;
    let movement = before.parse_uci(uci)?;
    let after = before.play(movement)?;
    Ok((
        DesktopBoardSnapshot::from_position(&before),
        DesktopBoardSnapshot::from_position(&after),
    ))
}

#[test]
fn transitions_identify_the_visible_mover_for_quiet_capture_castling_and_promotion()
-> PresentationBoardAnimationResult {
    let standard = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    for (fen, uci, origin, destination) in [
        (standard, "e2e4", "e2", "e4"),
        ("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1", "e4d5", "e4", "d5"),
        ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1g1", "e1", "g1"),
        ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1c1", "e1", "c1"),
        ("7k/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8n", "a7", "a8"),
    ] {
        let (before, after) = snapshot_after(fen, uci)?;
        assert_eq!(
            PresentationBoardTransition::piece_move(&before, &after),
            Some((
                PresentationBoardAnimationFixture::square(origin)?,
                PresentationBoardAnimationFixture::square(destination)?
            )),
            "{uci}"
        );
        assert_eq!(
            PresentationBoardTransition::piece_move(&before, &before),
            None
        );
    }
    Ok(())
}

struct PresentationBoardAnimationHarness {
    context: Context,
    view: PresentationBoardView,
}

impl PresentationBoardAnimationHarness {
    fn new(clock: Arc<PresentationBoardAnimationClock>, reduced_motion: bool) -> Self {
        Self {
            context: Context::default(),
            view: PresentationBoardView::with_animation(clock, reduced_motion),
        }
    }

    fn frame(
        &mut self,
        snapshot: &DesktopBoardSnapshot,
    ) -> PresentationBoardAnimationResult<(
        Option<chess::presentation::board::animation::PresentationBoardAnimationFrame>,
        Duration,
    )> {
        let rect = Rect::from_min_size(Pos2::ZERO, bevy_egui::egui::vec2(400.0, 400.0));
        let mut picker = |_point: Pos2| Ok(None);
        let view = &mut self.view;
        let mut failure = None;
        let mut animation = None;
        let mut output = self.context.run_ui(RawInput::default(), |ui| {
            match view.show_at(
                ui,
                rect,
                PresentationBoardViewInput::new(snapshot, None, &mut picker),
            ) {
                Ok(frame) => animation = frame.animation(),
                Err(error) => failure = Some(error),
            }
        });
        output.textures_delta.clear();
        if let Some(error) = failure {
            return Err(error.into());
        }
        let delay = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .ok_or("missing root viewport")?
            .repaint_delay;
        Ok((animation, delay))
    }
}

#[test]
fn view_starts_a_piece_animation_when_the_displayed_position_changes()
-> PresentationBoardAnimationResult {
    let clock = Arc::new(PresentationBoardAnimationClock::default());
    clock.advance_millis(1_000);
    let mut harness = PresentationBoardAnimationHarness::new(clock.clone(), false);
    let (before, after) = snapshot_after(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "e2e4",
    )?;
    for _ in 0..WARMUP_PASSES {
        harness.frame(&before)?;
    }
    let (idle, idle_delay) = harness.frame(&before)?;
    assert!(idle.ok_or("missing frame")?.piece().is_none());
    assert_eq!(idle_delay, Duration::MAX);
    let (started, delay) = harness.frame(&after)?;
    let piece = started
        .ok_or("missing frame")?
        .piece()
        .ok_or("missing piece")?;
    assert_eq!(
        (piece.origin(), piece.destination()),
        (
            PresentationBoardAnimationFixture::square("e2")?,
            PresentationBoardAnimationFixture::square("e4")?
        )
    );
    assert!(delay <= PresentationBoardAnimation::REPAINT_INTERVAL.as_duration());
    clock.advance_millis(180);
    let (finished, _) = harness.frame(&after)?;
    let piece = finished
        .ok_or("missing frame")?
        .piece()
        .ok_or("missing piece")?;
    assert!(near(piece.progress(), 1.0));
    let (settled, settled_delay) = harness.frame(&after)?;
    assert!(settled.ok_or("missing frame")?.piece().is_none());
    assert_eq!(settled_delay, Duration::MAX);
    Ok(())
}

#[test]
fn reduced_motion_view_never_animates_a_changed_position() -> PresentationBoardAnimationResult {
    let clock = Arc::new(PresentationBoardAnimationClock::default());
    let mut harness = PresentationBoardAnimationHarness::new(clock, true);
    let (before, after) = snapshot_after(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "e2e4",
    )?;
    harness.frame(&before)?;
    let (changed, _) = harness.frame(&after)?;
    assert!(changed.ok_or("missing frame")?.piece().is_none());
    Ok(())
}

#[test]
fn camera_actions_start_flip_and_reset_motion_while_keeping_offsets_apart_from_orientation()
-> PresentationBoardAnimationResult {
    let clock = Arc::new(PresentationBoardAnimationClock::default());
    clock.advance_millis(1_000);
    let mut harness = PresentationBoardAnimationHarness::new(clock.clone(), false);
    let snapshot = DesktopBoardSnapshot::from_position(&BoardPosition::standard());
    let moved =
        PresentationBoardAnimationFixture::moved_camera().with_orientation(BoardEnumColor::White);
    let flipped = harness
        .view
        .camera_action(moved, PresentationBoardEnumCameraAction::Flip)?;
    assert_eq!(flipped.orientation(), BoardEnumColor::Black);
    assert_eq!(flipped.pose(), moved.pose());
    let (frame, _) = harness.frame(&snapshot)?;
    let frame = frame.ok_or("missing frame")?;
    assert!(near(frame.flip_residual_degrees().abs(), 180.0));
    let shown = frame.display_camera(flipped);
    assert_eq!(shown.orientation(), BoardEnumColor::Black);
    assert_eq!(shown.tilt_degrees(), flipped.tilt_degrees());
    assert_eq!(shown.zoom(), flipped.zoom());
    clock.advance_millis(180);
    harness.frame(&snapshot)?;
    let (done, _) = harness.frame(&snapshot)?;
    assert!(near(
        done.ok_or("missing frame")?.flip_residual_degrees(),
        0.0
    ));
    let reset = harness
        .view
        .camera_action(moved, PresentationBoardEnumCameraAction::Reset)?;
    assert_eq!(reset, moved.reset());
    let (frame, _) = harness.frame(&snapshot)?;
    let shown = frame.ok_or("missing frame")?.display_camera(reset);
    assert!(near(shown.yaw_degrees(), moved.yaw_degrees()));
    assert_eq!(shown.orientation(), reset.orientation());
    let panel = harness.view.panel_progress()?;
    assert!(near(panel, 1.0));
    harness.view.set_panel_visible(false)?;
    clock.advance_millis(160);
    assert!(near(harness.view.panel_progress()?, 0.0));
    Ok(())
}
