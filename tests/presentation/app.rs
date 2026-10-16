use std::num::NonZeroU32;
use std::sync::Arc;

use bevy_egui::egui::{self, Context, RawInput, Rect, accesskit};

use chess::board::position::BoardPosition;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::application::{
    DesktopApplication, DesktopApplicationWakeError, DesktopApplicationWakeProvider,
};
use chess::desktop::dto::{
    DesktopBootstrapSnapshot, DesktopEngineIdentity, DesktopEnumEngineSnapshot, DesktopEnumValue,
};
use chess::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use chess::desktop::messages::DesktopEnumCommand;
use chess::presentation::app::{PresentationApp, PresentationAppEnumAction, PresentationAppError};
use chess::presentation::board::scene::PresentationBoardScene;
use chess::scene::camera::SceneCamera;
use chess::scene::enums::SceneEnumLifecycle;
use chess::scene::picking::ScenePickingViewport;
use chess::scene::renderer::SceneRendererGeneration;

type PresentationAppTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationAppFixtureClock;

impl ClockTimeProvider for PresentationAppFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(0))
    }
}

struct PresentationAppFixtureWake;

impl DesktopApplicationWakeProvider for PresentationAppFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

struct PresentationAppFixture {
    context: Context,
    presentation: PresentationApp,
    application: DesktopApplication<PresentationAppFixtureClock, PresentationAppFixtureWake>,
}

impl PresentationAppFixture {
    fn new() -> Self {
        let context = Context::default();
        context.enable_accesskit();
        Self {
            context,
            presentation: PresentationApp::new(),
            application: DesktopApplication::new(
                Arc::new(PresentationAppFixtureClock),
                Arc::new(PresentationAppFixtureWake),
            ),
        }
    }

    fn snapshot() -> PresentationAppTestResult<DesktopBootstrapSnapshot> {
        Ok(DesktopBootstrapSnapshot::new(
            &BoardPosition::standard(),
            NonZeroU32::new(8).ok_or("invalid fixture capacity")?,
            DesktopEnumEngineSnapshot::Ready {
                identity: Box::new(DesktopEngineIdentity::new(
                    "Reckless fixture".to_owned(),
                    "Engine author".to_owned(),
                    vec!["Threads".to_owned()],
                )),
            },
        ))
    }

    fn frame(
        &mut self,
        scene: Option<&PresentationBoardScene>,
        events: Vec<egui::Event>,
    ) -> PresentationAppTestResult<(egui::FullOutput, Option<ScenePickingViewport>)> {
        let mut result = None;
        let mut output = self.context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
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
        let frame = result.ok_or("missing presentation frame")??;
        Ok((output, frame.viewport()))
    }

    fn click(&mut self, output: &egui::FullOutput, label: &str) -> PresentationAppTestResult {
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .ok_or("missing native action semantics")?
            .nodes;
        let (target, _) = nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .ok_or("missing native action control")?;
        self.frame(
            None,
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: *target,
                    data: None,
                },
            )],
        )?;
        Ok(())
    }
}

#[test]
fn repeated_presentation_actions_keep_one_identity_dispatch_and_authoritative_result()
-> PresentationAppTestResult {
    let mut fixture = PresentationAppFixture::new();
    let port = fixture.application.port();
    let identity = fixture
        .presentation
        .activate(PresentationAppEnumAction::Refresh, 7, &port)?;
    for _ in 0..1000 {
        assert_eq!(
            fixture
                .presentation
                .activate(PresentationAppEnumAction::Refresh, 7, &port)?,
            identity
        );
    }
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing admitted bootstrap")?;
    assert_eq!(work.request().request_id(), identity);
    assert!(fixture.application.try_next()?.is_none());
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Bootstrap(PresentationAppFixture::snapshot()?),
        None,
    )?;
    fixture.presentation.poll();
    assert!(!fixture.presentation.has_pending_work());
    assert_eq!(
        fixture
            .presentation
            .snapshot()
            .ok_or("missing snapshot")?
            .board()
            .fen(),
        BoardPosition::standard().fen()
    );
    assert_eq!(
        fixture
            .presentation
            .activate(PresentationAppEnumAction::Refresh, 7, &port)?,
        identity
    );
    assert!(fixture.application.try_next()?.is_none());
    assert_ne!(
        fixture
            .presentation
            .activate(PresentationAppEnumAction::Refresh, 8, &port)?,
        identity
    );
    assert!(fixture.application.try_next()?.is_some());
    Ok(())
}

#[test]
fn an_admission_refusal_preserves_the_original_action_identity_for_the_same_pass()
-> PresentationAppTestResult {
    let mut fixture = PresentationAppFixture::new();
    let port = fixture.application.port();
    for _ in 0..DesktopApplication::<PresentationAppFixtureClock, PresentationAppFixtureWake>::ORDINARY_CAPACITY {
        let request = port.request(chess::desktop::messages::DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        port.try_submit(&request)?;
    }
    let identity = match fixture
        .presentation
        .activate(PresentationAppEnumAction::Refresh, 1, &port)
    {
        Err(PresentationAppError::Admission {
            identity,
            source: chess::desktop::application::DesktopApplicationError::Busy,
        }) => identity,
        outcome => {
            return Err(
                format!("expected a bounded admission refusal, received {outcome:?}").into(),
            );
        }
    };
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing capacity fixture work")?;
    fixture
        .application
        .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    assert_eq!(
        fixture
            .presentation
            .activate(PresentationAppEnumAction::Refresh, 1, &port)?,
        identity
    );
    Ok(())
}

#[test]
fn native_shell_uses_real_snapshot_engine_status_and_offscreen_semantic_board()
-> PresentationAppTestResult {
    let mut fixture = PresentationAppFixture::new();
    fixture
        .presentation
        .accept_snapshot(PresentationAppFixture::snapshot()?);
    fixture.frame(None, Vec::new())?;
    let (_, viewport) = fixture.frame(None, Vec::new())?;
    let viewport = viewport.ok_or("missing requested offscreen viewport")?;
    let scene = PresentationBoardScene::new(
        egui::TextureId::User(42),
        SceneRendererGeneration::initial(),
        SceneEnumLifecycle::Ready,
        viewport,
        SceneCamera::initial(chess::board::enums::BoardEnumColor::White)
            .fit(viewport.physical_size())?,
    );
    let (output, requested_viewport) = fixture.frame(Some(&scene), Vec::new())?;
    let nodes = output
        .platform_output
        .accesskit_update
        .ok_or("missing native semantic update")?
        .nodes;
    assert_eq!(
        nodes
            .iter()
            .filter(|(_, node)| node.role() == accesskit::Role::GridCell)
            .count(),
        64,
        "scene {:?}, requested {:?}",
        viewport,
        requested_viewport
    );
    assert!(
        nodes
            .iter()
            .any(|(_, node)| node.value() == Some("Reckless fixture"))
    );
    assert!(
        nodes
            .iter()
            .any(|(_, node)| node.role() == accesskit::Role::Status
                && node.live() == Some(accesskit::Live::Polite))
    );
    assert!(fixture.application.try_next()?.is_none());
    assert_eq!(
        fixture
            .presentation
            .snapshot()
            .ok_or("missing retained snapshot")?
            .board()
            .fen(),
        BoardPosition::standard().fen()
    );
    Ok(())
}

#[test]
fn file_selection_is_one_revision_checked_intent_and_failures_remain_visible()
-> PresentationAppTestResult {
    let mut fixture = PresentationAppFixture::new();
    let port = fixture.application.port();
    let identity =
        fixture
            .presentation
            .activate(PresentationAppEnumAction::SelectFile, 1, &port)?;
    assert_eq!(
        fixture
            .presentation
            .activate(PresentationAppEnumAction::SelectFile, 2, &port)?,
        identity
    );
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing file selection")?;
    assert!(matches!(
        work.request().command().operation(),
        DesktopEnumCommand::FileSelect { .. }
    ));
    assert!(work.request().command().expected_revision().is_some());
    fixture.application.complete_failure(
        &work,
        DesktopFailure::new(
            DesktopEnumFailureCode::FileUnavailable,
            "The selected file is unavailable".to_owned(),
            true,
        ),
    )?;
    fixture.presentation.poll();
    assert_eq!(
        fixture.presentation.failure(),
        Some("The selected file is unavailable")
    );
    assert!(!fixture.presentation.has_pending_work());
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn quit_waits_for_explicit_confirmation_and_submits_one_owned_control() -> PresentationAppTestResult
{
    let mut fixture = PresentationAppFixture::new();
    assert!(!fixture.presentation.is_quit_ready());
    fixture.presentation.request_quit();
    assert!(fixture.presentation.is_quit_confirmation_open());
    assert!(fixture.application.try_next()?.is_none());
    let (output, _) = fixture.frame(None, Vec::new())?;
    fixture.click(&output, "Confirm quit")?;
    assert!(!fixture.presentation.is_quit_confirmation_open());
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing confirmed quit")?;
    assert!(matches!(
        work.request().command().operation(),
        DesktopEnumCommand::Quit
    ));
    assert!(fixture.application.try_next()?.is_none());
    fixture.presentation.poll();
    assert!(!fixture.presentation.is_quit_ready());
    fixture.application.complete_control(&work)?;
    fixture.presentation.poll();
    assert!(fixture.presentation.is_quit_ready());
    assert_eq!(
        fixture.presentation.status(),
        "Application is ready to close"
    );
    Ok(())
}

#[test]
fn pending_quit_shows_updated_failures_and_an_accessible_explicit_retry()
-> PresentationAppTestResult {
    let mut fixture = PresentationAppFixture::new();
    let port = fixture.application.port();
    assert!(port.retry_quit().is_err());
    fixture
        .presentation
        .activate(PresentationAppEnumAction::ConfirmQuit, 1, &port)?;
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing pending quit")?;
    fixture.application.report_pending_control(
        &work,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "Close the file dialog".to_owned(),
            true,
        ),
    )?;
    fixture.application.report_pending_control(
        &work,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "Cleanup needs an explicit retry".to_owned(),
            true,
        ),
    )?;
    let (output, _) = fixture.frame(None, Vec::new())?;
    assert_eq!(
        fixture.presentation.failure(),
        Some("Cleanup needs an explicit retry")
    );
    assert!(!fixture.application.take_quit_retry());
    fixture.click(&output, "Retry quit")?;
    assert!(fixture.application.take_quit_retry());
    assert!(!fixture.application.take_quit_retry());
    assert!(!fixture.presentation.is_quit_ready());
    assert!(fixture.application.try_next()?.is_none());
    fixture.application.complete_control(&work)?;
    fixture.presentation.poll();
    assert!(fixture.presentation.is_quit_ready());
    Ok(())
}
