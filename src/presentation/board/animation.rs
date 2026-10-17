use std::sync::Arc;

use bevy_egui::egui::Context;
use thiserror::Error;

use crate::board::square::BoardSquare;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::scene::camera::{SceneCamera, SceneCameraError};

#[derive(Debug, Error)]
pub enum PresentationBoardAnimationError {
    #[error("The animation clock failed: {0}")]
    Clock(#[from] ClockDurationError),
    #[error("The animated camera pose is invalid: {0}")]
    Camera(#[from] SceneCameraError),
    #[error("The animation progress exceeds its supported range")]
    Progress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationBoardAnimationEnumOutcome {
    Animating,
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentationBoardAnimationPiece {
    origin: BoardSquare,
    destination: BoardSquare,
    progress: f32,
}

impl PresentationBoardAnimationPiece {
    pub const fn origin(self) -> BoardSquare {
        self.origin
    }

    pub const fn destination(self) -> BoardSquare {
        self.destination
    }

    pub const fn progress(self) -> f32 {
        self.progress
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentationBoardAnimationFrame {
    piece: Option<PresentationBoardAnimationPiece>,
    flip_residual_degrees: f32,
    panel_progress: f32,
    camera: Option<SceneCamera>,
}

impl PresentationBoardAnimationFrame {
    pub const fn piece(self) -> Option<PresentationBoardAnimationPiece> {
        self.piece
    }

    pub const fn flip_residual_degrees(self) -> f32 {
        self.flip_residual_degrees
    }

    pub const fn panel_progress(self) -> f32 {
        self.panel_progress
    }

    pub const fn camera(self) -> Option<SceneCamera> {
        self.camera
    }
}

#[derive(Debug, Clone, Copy)]
struct PresentationBoardAnimationTrack<T> {
    value: T,
    started: ClockInstant,
    duration: ClockDuration,
}

impl<T> PresentationBoardAnimationTrack<T> {
    fn progress(&self, now: ClockInstant) -> Result<f32, PresentationBoardAnimationError> {
        let elapsed = now.elapsed_since(self.started)?;
        let total = self.duration.as_millis();
        let milliseconds = elapsed.as_millis().min(total);
        let linear = f32::from(
            u16::try_from(milliseconds)
                .map_err(|_error| PresentationBoardAnimationError::Progress)?,
        ) / f32::from(
            u16::try_from(total).map_err(|_error| PresentationBoardAnimationError::Progress)?,
        );
        Ok(1.0 - (1.0 - linear).powi(3))
    }

    fn is_finished(&self, now: ClockInstant) -> Result<bool, PresentationBoardAnimationError> {
        Ok(now.elapsed_since(self.started)?.as_nanos() >= self.duration.as_nanos())
    }
}

#[derive(Debug, Clone, Copy)]
struct PresentationBoardAnimationPieceMove {
    origin: BoardSquare,
    destination: BoardSquare,
}

#[derive(Debug, Clone, Copy)]
struct PresentationBoardAnimationFlip {
    from_degrees: f32,
}

#[derive(Debug, Clone, Copy)]
struct PresentationBoardAnimationPanel {
    from: f32,
    to: f32,
}

#[derive(Debug, Clone, Copy)]
struct PresentationBoardAnimationCamera {
    from: SceneCamera,
    to: SceneCamera,
}

pub struct PresentationBoardAnimation {
    clock: Arc<dyn ClockTimeProvider>,
    reduced_motion: bool,
    panel_visible: bool,
    piece: Option<PresentationBoardAnimationTrack<PresentationBoardAnimationPieceMove>>,
    flip: Option<PresentationBoardAnimationTrack<PresentationBoardAnimationFlip>>,
    panel: Option<PresentationBoardAnimationTrack<PresentationBoardAnimationPanel>>,
    camera: Option<PresentationBoardAnimationTrack<PresentationBoardAnimationCamera>>,
}

impl PresentationBoardAnimation {
    pub const PIECE_DURATION: ClockDuration = ClockDuration::from_nanos(180_000_000);
    pub const FLIP_DURATION: ClockDuration = ClockDuration::from_nanos(180_000_000);
    pub const CAMERA_RESET_DURATION: ClockDuration = ClockDuration::from_nanos(180_000_000);
    pub const PANEL_DURATION: ClockDuration = ClockDuration::from_nanos(160_000_000);
    pub const REPAINT_INTERVAL: ClockDuration = ClockDuration::from_nanos(16_000_000);
    const HALF_TURN_DEGREES: f32 = 180.0;
    const FULL_TURN_DEGREES: f32 = 360.0;

    pub fn new(clock: Arc<dyn ClockTimeProvider>, reduced_motion: bool) -> Self {
        Self {
            clock,
            reduced_motion,
            panel_visible: true,
            piece: None,
            flip: None,
            panel: None,
            camera: None,
        }
    }

    pub const fn is_reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    pub const fn is_panel_visible(&self) -> bool {
        self.panel_visible
    }

    pub const fn is_active(&self) -> bool {
        self.piece.is_some() || self.flip.is_some() || self.panel.is_some() || self.camera.is_some()
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.reduced_motion = reduced_motion;
        if reduced_motion {
            self.cancel_all();
        }
    }

    pub fn cancel_all(&mut self) {
        self.piece = None;
        self.flip = None;
        self.panel = None;
        self.camera = None;
    }

    pub fn cancel_obsolete_move(&mut self) {
        self.piece = None;
        self.flip = None;
    }

    pub fn start_piece_move(
        &mut self,
        origin: BoardSquare,
        destination: BoardSquare,
    ) -> Result<PresentationBoardAnimationEnumOutcome, PresentationBoardAnimationError> {
        self.piece = None;
        if self.reduced_motion {
            return Ok(PresentationBoardAnimationEnumOutcome::Settled);
        }
        self.piece = Some(PresentationBoardAnimationTrack {
            value: PresentationBoardAnimationPieceMove {
                origin,
                destination,
            },
            started: self.clock.now()?,
            duration: Self::PIECE_DURATION,
        });
        Ok(PresentationBoardAnimationEnumOutcome::Animating)
    }

    pub fn start_flip(
        &mut self,
    ) -> Result<PresentationBoardAnimationEnumOutcome, PresentationBoardAnimationError> {
        let now = self.clock.now()?;
        let current = self.flip_residual(now)?;
        self.flip = None;
        if self.reduced_motion {
            return Ok(PresentationBoardAnimationEnumOutcome::Settled);
        }
        self.flip = Some(PresentationBoardAnimationTrack {
            value: PresentationBoardAnimationFlip {
                from_degrees: Self::wrap_degrees(current + Self::HALF_TURN_DEGREES),
            },
            started: now,
            duration: Self::FLIP_DURATION,
        });
        Ok(PresentationBoardAnimationEnumOutcome::Animating)
    }

    pub fn start_camera_reset(
        &mut self,
        camera: SceneCamera,
    ) -> Result<(SceneCamera, PresentationBoardAnimationEnumOutcome), PresentationBoardAnimationError>
    {
        let target = camera.reset();
        self.camera = None;
        if self.reduced_motion || camera == target {
            return Ok((target, PresentationBoardAnimationEnumOutcome::Settled));
        }
        self.camera = Some(PresentationBoardAnimationTrack {
            value: PresentationBoardAnimationCamera {
                from: camera,
                to: target,
            },
            started: self.clock.now()?,
            duration: Self::CAMERA_RESET_DURATION,
        });
        Ok((target, PresentationBoardAnimationEnumOutcome::Animating))
    }

    pub fn set_panel_visible(
        &mut self,
        visible: bool,
    ) -> Result<PresentationBoardAnimationEnumOutcome, PresentationBoardAnimationError> {
        let now = self.clock.now()?;
        let from = self.panel_progress(now)?;
        self.panel_visible = visible;
        self.panel = None;
        let to = Self::panel_resting(visible);
        if self.reduced_motion || from == to {
            return Ok(PresentationBoardAnimationEnumOutcome::Settled);
        }
        self.panel = Some(PresentationBoardAnimationTrack {
            value: PresentationBoardAnimationPanel { from, to },
            started: now,
            duration: Self::PANEL_DURATION,
        });
        Ok(PresentationBoardAnimationEnumOutcome::Animating)
    }

    pub fn frame(
        &mut self,
    ) -> Result<PresentationBoardAnimationFrame, PresentationBoardAnimationError> {
        let now = self.clock.now()?;
        let frame = self.sample(now)?;
        self.retire(now)?;
        Ok(frame)
    }

    pub const fn repaint_delay(&self) -> Option<ClockDuration> {
        if self.is_active() {
            Some(Self::REPAINT_INTERVAL)
        } else {
            None
        }
    }

    pub fn request_repaint(&self, context: &Context) {
        if let Some(delay) = self.repaint_delay() {
            context.request_repaint_after(delay.as_duration());
        }
    }

    fn sample(
        &self,
        now: ClockInstant,
    ) -> Result<PresentationBoardAnimationFrame, PresentationBoardAnimationError> {
        let piece = self
            .piece
            .as_ref()
            .map(|track| {
                track
                    .progress(now)
                    .map(|progress| PresentationBoardAnimationPiece {
                        origin: track.value.origin,
                        destination: track.value.destination,
                        progress,
                    })
            })
            .transpose()?;
        let camera = self
            .camera
            .as_ref()
            .map(|track| {
                let progress = track.progress(now)?;
                Self::camera_at(track.value, progress)
            })
            .transpose()?;
        Ok(PresentationBoardAnimationFrame {
            piece,
            flip_residual_degrees: self.flip_residual(now)?,
            panel_progress: self.panel_progress(now)?,
            camera,
        })
    }

    fn retire(&mut self, now: ClockInstant) -> Result<(), PresentationBoardAnimationError> {
        if let Some(track) = &self.piece
            && track.is_finished(now)?
        {
            self.piece = None;
        }
        if let Some(track) = &self.flip
            && track.is_finished(now)?
        {
            self.flip = None;
        }
        if let Some(track) = &self.panel
            && track.is_finished(now)?
        {
            self.panel = None;
        }
        if let Some(track) = &self.camera
            && track.is_finished(now)?
        {
            self.camera = None;
        }
        Ok(())
    }

    fn flip_residual(&self, now: ClockInstant) -> Result<f32, PresentationBoardAnimationError> {
        match &self.flip {
            Some(track) => Ok(track.value.from_degrees * (1.0 - track.progress(now)?)),
            None => Ok(0.0),
        }
    }

    fn panel_progress(&self, now: ClockInstant) -> Result<f32, PresentationBoardAnimationError> {
        match &self.panel {
            Some(track) => {
                Ok(track.value.from + (track.value.to - track.value.from) * track.progress(now)?)
            }
            None => Ok(Self::panel_resting(self.panel_visible)),
        }
    }

    const fn panel_resting(visible: bool) -> f32 {
        if visible { 1.0 } else { 0.0 }
    }

    fn camera_at(
        span: PresentationBoardAnimationCamera,
        progress: f32,
    ) -> Result<SceneCamera, PresentationBoardAnimationError> {
        let yaw_delta = Self::wrap_degrees(span.to.yaw_degrees() - span.from.yaw_degrees());
        Ok(SceneCamera::new(
            span.from.yaw_degrees() + yaw_delta * progress,
            span.from.tilt_degrees()
                + (span.to.tilt_degrees() - span.from.tilt_degrees()) * progress,
            span.from.zoom() + (span.to.zoom() - span.from.zoom()) * progress,
            span.from.orientation(),
        )?)
    }

    fn wrap_degrees(degrees: f32) -> f32 {
        let turn = degrees.rem_euclid(Self::FULL_TURN_DEGREES);
        if turn > Self::HALF_TURN_DEGREES {
            turn - Self::FULL_TURN_DEGREES
        } else {
            turn
        }
    }
}
