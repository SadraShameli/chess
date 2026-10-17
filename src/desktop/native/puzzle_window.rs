use std::num::NonZeroU64;
use std::sync::Arc;

use bevy::ecs::entity::Entity;
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::ecs::world::World;
use bevy::window::{AppLifecycle, Window, WindowFocused, WindowOccluded};
use thiserror::Error;
use tokio::sync::watch;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{DesktopApplicationPort, DesktopApplicationWakeProvider};
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::desktop::native::lifecycle::{
    DesktopNativeLifecycle, DesktopNativeLifecycleEnumSignal,
    DesktopNativeLifecycleEnumWindowEvent, DesktopNativeLifecyclePuzzleWindowId,
};
use crate::desktop::puzzle_bridge::DesktopPuzzleBridgeEnumHealth;
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceError,
    DesktopPuzzleServiceNativeProgress, DesktopPuzzleServiceProvider, DesktopPuzzleServiceReceipt,
    DesktopPuzzleServiceState,
};
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::presentation::app::{
    PresentationApp, PresentationAppEnumWorkspace, PresentationAppError,
};
use crate::presentation::puzzle::controller::PresentationPuzzleController;
use crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus;
use crate::puzzle::run::PuzzleRunEnumStatus;
use crate::puzzle::session::{PuzzleSessionContext, PuzzleSessionId};
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};

#[cfg(test)]
#[path = "../../../tests/desktop/native/puzzle_window.rs"]
mod tests;

#[derive(Debug, Error)]
pub enum DesktopNativePuzzleWindowError {
    #[error("The native puzzle window entity {0:?} has no nonzero lifecycle identity")]
    WindowIdentity(Entity),
    #[error("The bound native puzzle window {0:?} is unavailable")]
    MissingWindow(Entity),
}

pub struct DesktopNativePuzzleWindowFrame<'a> {
    pub world: &'a World,
    pub graphics_ready: bool,
    pub rendered_board: Option<&'a DesktopBoardSnapshot>,
}

impl DesktopNativePuzzleWindowFrame<'_> {
    fn board_readiness(
        &self,
        controller: Option<&PresentationPuzzleController>,
        input_ready: bool,
    ) -> PuzzleTimerEnumReadiness {
        if !self.graphics_ready || !input_ready {
            return PuzzleTimerEnumReadiness::BoardNotReady;
        }
        let Some(controller) = controller else {
            return PuzzleTimerEnumReadiness::BoardNotReady;
        };
        if matches!(
            controller.snapshot().status(),
            PuzzleRunEnumStatus::Paused { .. }
        ) {
            return PuzzleTimerEnumReadiness::Ready;
        }
        match (controller.board(), self.rendered_board) {
            (Some(current), Some(rendered))
                if current.fen() == rendered.fen() && current.variant() == rendered.variant() =>
            {
                PuzzleTimerEnumReadiness::Ready
            }
            (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) | (None, None) => {
                PuzzleTimerEnumReadiness::BoardNotReady
            }
        }
    }
}

#[derive(PartialEq, Eq)]
enum DesktopNativePuzzleWindowEnumServiceHealth {
    Starting,
    Ready,
    SaveBlocked(PuzzleCheckpointEnumStatus),
    ShuttingDown,
    ShutdownComplete,
    Failed,
}

impl DesktopNativePuzzleWindowEnumServiceHealth {
    fn from_health(health: &DesktopPuzzleBridgeEnumHealth) -> Self {
        match health {
            DesktopPuzzleBridgeEnumHealth::Starting => Self::Starting,
            DesktopPuzzleBridgeEnumHealth::Ready => Self::Ready,
            DesktopPuzzleBridgeEnumHealth::SaveBlocked { status, .. } => Self::SaveBlocked(*status),
            DesktopPuzzleBridgeEnumHealth::ShuttingDown => Self::ShuttingDown,
            DesktopPuzzleBridgeEnumHealth::ShutdownComplete => Self::ShutdownComplete,
            DesktopPuzzleBridgeEnumHealth::Failed(_) => Self::Failed,
        }
    }
}

struct DesktopNativePuzzleWindowServiceProgress {
    selected: Option<PuzzleSessionId>,
    activity: DesktopPuzzleServiceEnumActivity,
    health: Option<DesktopNativePuzzleWindowEnumServiceHealth>,
    snapshot: Option<Arc<DesktopPuzzleSnapshot>>,
    native_admission: Option<DesktopPuzzleServiceNativeProgress>,
}

impl DesktopNativePuzzleWindowServiceProgress {
    fn new(state: &DesktopPuzzleServiceState) -> Self {
        Self {
            selected: state.selected(),
            activity: state.activity(),
            health: state
                .health()
                .map(DesktopNativePuzzleWindowEnumServiceHealth::from_health),
            snapshot: state.snapshot().cloned(),
            native_admission: state.native_admission_progress().cloned(),
        }
    }

    fn matches(&self, state: &DesktopPuzzleServiceState) -> bool {
        self.selected == state.selected()
            && self.activity == state.activity()
            && self.health
                == state
                    .health()
                    .map(DesktopNativePuzzleWindowEnumServiceHealth::from_health)
            && match (self.snapshot.as_ref(), state.snapshot()) {
                (Some(previous), Some(current)) => Arc::ptr_eq(previous, current),
                (None, None) => true,
                (Some(_), None) | (None, Some(_)) => false,
            }
            && match (
                self.native_admission.as_ref(),
                state.native_admission_progress(),
            ) {
                (Some(previous), Some(current)) => previous.matches(current),
                (None, None) => true,
                (Some(_), None) | (None, Some(_)) => false,
            }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DesktopNativePuzzleWindowInterruption {
    session: PuzzleSessionId,
    reason: PuzzleTimerEnumPauseReason,
}

struct DesktopNativePuzzleWindowActive {
    interruption: DesktopNativePuzzleWindowInterruption,
    receipt: DesktopPuzzleServiceReceipt,
}

#[derive(Default)]
struct DesktopNativePuzzleWindowInterruptions {
    active: Option<DesktopNativePuzzleWindowActive>,
    queued: Option<DesktopNativePuzzleWindowInterruption>,
    retry_blocked: bool,
    concealed: Option<PuzzleSessionId>,
}

impl DesktopNativePuzzleWindowInterruptions {
    fn signal<P: DesktopPuzzleServiceProvider>(
        &mut self,
        provider: &P,
        lifecycle: &DesktopNativeLifecycle,
        signal: DesktopNativeLifecycleEnumSignal,
    ) {
        if let Some(reason) = lifecycle.puzzle_interruption(signal) {
            self.capture(provider, reason);
        }
    }

    fn capture<P: DesktopPuzzleServiceProvider>(
        &mut self,
        provider: &P,
        reason: PuzzleTimerEnumPauseReason,
    ) {
        let Some(session) = provider.state().borrow().selected() else {
            return;
        };
        self.concealed = Some(session);
        self.retry_blocked = false;
        let interruption = DesktopNativePuzzleWindowInterruption { session, reason };
        if self.queued == Some(interruption)
            || self.queued.is_none()
                && self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.interruption == interruption)
        {
            return;
        }
        self.queued = Some(interruption);
    }

    fn observe_concealment(&mut self, state: &DesktopPuzzleServiceState) {
        let Some(session) = self.concealed else {
            return;
        };
        if state.selected() != Some(session)
            || state.snapshot().is_some_and(|snapshot| {
                snapshot.context().session_id() == session
                    && matches!(
                        snapshot.status(),
                        PuzzleRunEnumStatus::Paused { .. }
                            | PuzzleRunEnumStatus::Exhausted
                            | PuzzleRunEnumStatus::Expired(_)
                            | PuzzleRunEnumStatus::Ended(_)
                    )
            })
        {
            self.concealed = None;
        }
    }

    fn observe(&mut self, presentation: &mut PresentationApp) {
        let Some(outcome) = self
            .active
            .as_mut()
            .and_then(|active| active.receipt.try_result())
        else {
            return;
        };
        if let Some(active) = self.active.take() {
            match outcome {
                Ok(_) => self.retry_blocked = false,
                Err(error) if matches!(*error, DesktopPuzzleServiceError::Busy) => {
                    if self.queued.is_none() {
                        self.queued = Some(active.interruption);
                    }
                    self.retry_blocked = true;
                }
                Err(error) => {
                    self.retry_blocked = false;
                    presentation.report_failure(PresentationAppError::PuzzleInterruption(error));
                }
            }
        }
    }

    fn admit<P: DesktopPuzzleServiceProvider>(
        &mut self,
        provider: &P,
        presentation: &mut PresentationApp,
    ) {
        if self.active.is_some() || self.retry_blocked {
            return;
        }
        let Some(interruption) = self.queued.take() else {
            return;
        };
        match provider.interrupt(interruption.session, interruption.reason) {
            Ok(receipt) => {
                self.active = Some(DesktopNativePuzzleWindowActive {
                    interruption,
                    receipt,
                });
            }
            Err(error) if matches!(*error, DesktopPuzzleServiceError::Busy) => {
                self.queued = Some(interruption);
                self.retry_blocked = true;
            }
            Err(error) => {
                presentation.report_failure(PresentationAppError::PuzzleInterruption(error));
            }
        }
    }

    fn is_settled(&self) -> bool {
        self.active.is_none() && self.queued.is_none()
    }
}

struct DesktopNativePuzzleWindowReadiness {
    value: PuzzleTimerEnumReadiness,
    context: Option<PuzzleSessionContext>,
    pending: bool,
}

impl DesktopNativePuzzleWindowReadiness {
    fn new(
        value: PuzzleTimerEnumReadiness,
        context: Option<&PuzzleSessionContext>,
        pending: bool,
    ) -> Self {
        Self {
            value,
            context: context.cloned(),
            pending,
        }
    }

    fn matches(
        &self,
        value: PuzzleTimerEnumReadiness,
        context: Option<&PuzzleSessionContext>,
        pending: bool,
    ) -> bool {
        self.value == value
            && if value == PuzzleTimerEnumReadiness::Hidden {
                self.context.as_ref().map(PuzzleSessionContext::session_id)
                    == context.map(PuzzleSessionContext::session_id)
            } else {
                self.context.as_ref() == context && self.pending == pending
            }
    }
}

pub struct DesktopNativePuzzleWindow<P: DesktopPuzzleServiceProvider> {
    window: Entity,
    window_id: DesktopNativeLifecyclePuzzleWindowId,
    lifecycle: DesktopNativeLifecycle,
    provider: P,
    state: watch::Receiver<DesktopPuzzleServiceState>,
    service_progress: DesktopNativePuzzleWindowServiceProgress,
    state_closed: bool,
    focus_events: MessageCursor<WindowFocused>,
    occlusion_events: MessageCursor<WindowOccluded>,
    lifecycle_events: MessageCursor<AppLifecycle>,
    focused: Option<bool>,
    visible: Option<bool>,
    occluded: bool,
    suspended: bool,
    graphics_failed: bool,
    interruptions: DesktopNativePuzzleWindowInterruptions,
    readiness: Option<DesktopNativePuzzleWindowReadiness>,
}

impl<P: DesktopPuzzleServiceProvider> DesktopNativePuzzleWindow<P> {
    pub fn new(window: Entity, provider: P) -> Result<Self, DesktopNativePuzzleWindowError> {
        let window_id = DesktopNativeLifecyclePuzzleWindowId::new(
            NonZeroU64::new(window.to_bits())
                .ok_or(DesktopNativePuzzleWindowError::WindowIdentity(window))?,
        );
        let state = provider.state();
        let service_progress = DesktopNativePuzzleWindowServiceProgress::new(&state.borrow());
        Ok(Self {
            window,
            window_id,
            lifecycle: DesktopNativeLifecycle::new(window_id),
            provider,
            state,
            service_progress,
            state_closed: false,
            focus_events: MessageCursor::default(),
            occlusion_events: MessageCursor::default(),
            lifecycle_events: MessageCursor::default(),
            focused: None,
            visible: None,
            occluded: false,
            suspended: false,
            graphics_failed: false,
            interruptions: DesktopNativePuzzleWindowInterruptions::default(),
            readiness: None,
        })
    }

    pub fn update<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        world: &World,
        presentation: &mut PresentationApp,
        application: &DesktopApplicationPort<C, W>,
        graphics_failed: bool,
    ) -> Result<(), DesktopNativePuzzleWindowError> {
        self.interruptions.observe(presentation);
        self.observe_service(presentation);
        self.observe_window(world)?;
        self.observe_lifecycle(world);
        if graphics_failed && !self.graphics_failed {
            self.interruptions
                .capture(&self.provider, PuzzleTimerEnumPauseReason::Recovery);
        }
        self.graphics_failed = graphics_failed;
        self.interruptions.admit(&self.provider, presentation);
        if let Err(error) = presentation.synchronize_puzzles(application) {
            presentation.report_failure(error);
        }
        self.interruptions.observe_concealment(&self.state.borrow());
        if self.is_hidden(world)? || self.interruptions.concealed.is_some() {
            self.report(PuzzleTimerEnumReadiness::Hidden, presentation, application);
        }
        Ok(())
    }

    pub fn report_readiness<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        frame: DesktopNativePuzzleWindowFrame<'_>,
        presentation: &mut PresentationApp,
        application: &DesktopApplicationPort<C, W>,
    ) -> Result<(), DesktopNativePuzzleWindowError> {
        let synchronized = match presentation.synchronize_puzzles(application) {
            Ok(()) => true,
            Err(error) => {
                presentation.report_failure(error);
                false
            }
        };
        self.interruptions.observe_concealment(&self.state.borrow());
        let readiness = if self.is_hidden(frame.world)?
            || self.interruptions.concealed.is_some()
            || presentation.workspace() == PresentationAppEnumWorkspace::Games
        {
            PuzzleTimerEnumReadiness::Hidden
        } else if synchronized {
            frame.board_readiness(
                presentation
                    .puzzle_panel()
                    .and_then(|panel| panel.controller()),
                presentation.is_puzzle_input_ready(),
            )
        } else {
            PuzzleTimerEnumReadiness::BoardNotReady
        };
        self.report(readiness, presentation, application);
        Ok(())
    }

    pub fn is_settled(&self) -> bool {
        self.interruptions.is_settled()
    }

    fn observe_service(&mut self, presentation: &mut PresentationApp) {
        match self.state.has_changed() {
            Ok(true) => {
                let state = self.state.borrow_and_update();
                if !self.service_progress.matches(&state) {
                    self.service_progress = DesktopNativePuzzleWindowServiceProgress::new(&state);
                    self.interruptions.retry_blocked = false;
                }
            }
            Ok(false) => {}
            Err(error) if !self.state_closed => {
                self.state_closed = true;
                self.interruptions.retry_blocked = false;
                presentation.report_failure(PresentationAppError::PuzzleInterruption(Arc::new(
                    DesktopPuzzleServiceError::Outcome(error),
                )));
            }
            Err(_) => {}
        }
    }

    fn observe_window(&mut self, world: &World) -> Result<(), DesktopNativePuzzleWindowError> {
        let window = self.bound_window(world)?;
        if self.focused != Some(window.focused) {
            self.focused = Some(window.focused);
            self.window_signal(Self::focus_event(window.focused));
        }
        if self.visible != Some(window.visible) {
            self.visible = Some(window.visible);
            self.window_signal(if window.visible {
                DesktopNativeLifecycleEnumWindowEvent::Visible
            } else {
                DesktopNativeLifecycleEnumWindowEvent::Hidden
            });
        }
        if let Some(events) = world.get_resource::<Messages<WindowFocused>>() {
            for event in self.focus_events.read(events) {
                if event.window == self.window && self.focused != Some(event.focused) {
                    self.focused = Some(event.focused);
                    self.interruptions.signal(
                        &self.provider,
                        &self.lifecycle,
                        DesktopNativeLifecycleEnumSignal::Window {
                            window: self.window_id,
                            event: Self::focus_event(event.focused),
                        },
                    );
                }
            }
        }
        if let Some(events) = world.get_resource::<Messages<WindowOccluded>>() {
            for event in self.occlusion_events.read(events) {
                if event.window == self.window && self.occluded != event.occluded {
                    self.occluded = event.occluded;
                    self.interruptions.signal(
                        &self.provider,
                        &self.lifecycle,
                        DesktopNativeLifecycleEnumSignal::Window {
                            window: self.window_id,
                            event: if event.occluded {
                                DesktopNativeLifecycleEnumWindowEvent::Occluded
                            } else {
                                DesktopNativeLifecycleEnumWindowEvent::Exposed
                            },
                        },
                    );
                }
            }
        }
        Ok(())
    }

    fn window_signal(&mut self, event: DesktopNativeLifecycleEnumWindowEvent) {
        self.interruptions.signal(
            &self.provider,
            &self.lifecycle,
            DesktopNativeLifecycleEnumSignal::Window {
                window: self.window_id,
                event,
            },
        );
    }

    fn focus_event(focused: bool) -> DesktopNativeLifecycleEnumWindowEvent {
        if focused {
            DesktopNativeLifecycleEnumWindowEvent::FocusGained
        } else {
            DesktopNativeLifecycleEnumWindowEvent::FocusLost
        }
    }

    fn observe_lifecycle(&mut self, world: &World) {
        if let Some(events) = world.get_resource::<Messages<AppLifecycle>>() {
            for event in self.lifecycle_events.read(events) {
                match event {
                    AppLifecycle::WillSuspend | AppLifecycle::Suspended if !self.suspended => {
                        self.suspended = true;
                        self.interruptions.signal(
                            &self.provider,
                            &self.lifecycle,
                            DesktopNativeLifecycleEnumSignal::Suspend,
                        );
                    }
                    AppLifecycle::WillResume | AppLifecycle::Running => self.suspended = false,
                    AppLifecycle::Idle | AppLifecycle::WillSuspend | AppLifecycle::Suspended => {}
                }
            }
        }
    }

    fn is_hidden(&self, world: &World) -> Result<bool, DesktopNativePuzzleWindowError> {
        let window = self.bound_window(world)?;
        Ok(!window.visible
            || !window.focused
            || self.focused == Some(false)
            || self.occluded
            || self.suspended
            || self.graphics_failed)
    }

    fn bound_window<'a>(
        &self,
        world: &'a World,
    ) -> Result<&'a Window, DesktopNativePuzzleWindowError> {
        world
            .get::<Window>(self.window)
            .ok_or(DesktopNativePuzzleWindowError::MissingWindow(self.window))
    }

    fn report<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        value: PuzzleTimerEnumReadiness,
        presentation: &mut PresentationApp,
        application: &DesktopApplicationPort<C, W>,
    ) {
        let panel = presentation.puzzle_panel();
        let context = panel
            .and_then(|panel| panel.controller())
            .map(|controller| controller.snapshot().context());
        let pending = panel.is_some_and(|panel| panel.has_pending_work());
        if self
            .readiness
            .as_ref()
            .is_some_and(|previous| previous.matches(value, context, pending))
        {
            return;
        }
        let mut readiness = DesktopNativePuzzleWindowReadiness::new(value, context, pending);
        let reported = match presentation.report_puzzle_readiness(value, application) {
            Ok(()) => true,
            Err(error) => {
                presentation.report_failure(error);
                false
            }
        };
        readiness.pending = presentation
            .puzzle_panel()
            .is_some_and(|panel| panel.has_pending_work());
        if reported
            && value == PuzzleTimerEnumReadiness::Ready
            && readiness.context.is_some()
            && !readiness.pending
            && let Err(error) = application.rearm_wake()
        {
            presentation.report_failure(PresentationAppError::Application(error));
        }
        self.readiness = Some(readiness);
    }
}
