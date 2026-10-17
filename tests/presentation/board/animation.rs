use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use std::time::Duration;

use bevy_egui::egui::{Context, RawInput, ViewportId};

use chess::board::enums::BoardEnumColor;
use chess::board::square::BoardSquare;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::preferences::{DesktopPreferences, DesktopPreferencesEnumMotion};
use chess::presentation::board::animation::{
    PresentationBoardAnimation, PresentationBoardAnimationEnumOutcome,
};
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
