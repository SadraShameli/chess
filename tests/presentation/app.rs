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
use chess::desktop::preferences::DesktopPreferencesEnumTheme;
use chess::presentation::action::registry::PresentationActionEnumId;
use chess::presentation::app::{PresentationApp, PresentationAppError};
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

    fn subscribe(
        &mut self,
        subscription: chess::desktop::identity::DesktopRequestId,
    ) -> PresentationAppTestResult {
        self.presentation.activate(
            PresentationActionEnumId::Subscribe,
            1,
            &self.application.port(),
        )?;
        let work = self
            .application
            .try_next()?
            .ok_or("missing native subscription")?;
        self.application.complete_value(
            &work,
            DesktopEnumValue::StreamSubscribe {
                subscription_id: subscription,
                sequence: chess::desktop::identity::DesktopRevision::ZERO,
                snapshot: Self::snapshot()?,
            },
            None,
        )?;
        self.presentation.poll();
        Ok(())
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
        .activate(PresentationActionEnumId::Refresh, 7, &port)?;
    for _ in 0..1000 {
        assert_eq!(
            fixture
                .presentation
                .activate(PresentationActionEnumId::Refresh, 7, &port)?,
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
            .activate(PresentationActionEnumId::Refresh, 7, &port)?,
        identity
    );
    assert!(fixture.application.try_next()?.is_none());
    assert_ne!(
        fixture
            .presentation
            .activate(PresentationActionEnumId::Refresh, 8, &port)?,
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
        .activate(PresentationActionEnumId::Refresh, 1, &port)
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
            .activate(PresentationActionEnumId::Refresh, 1, &port)?,
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
    let identity = fixture
        .presentation
        .activate(PresentationActionEnumId::SelectFile, 1, &port)?;
    assert_eq!(
        fixture
            .presentation
            .activate(PresentationActionEnumId::SelectFile, 2, &port)?,
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
fn quit_confirmation_defers_control_admission_until_the_native_host_is_ready()
-> PresentationAppTestResult {
    let mut fixture = PresentationAppFixture::new();
    assert!(!fixture.presentation.is_quit_ready());
    fixture.presentation.request_quit();
    assert!(fixture.presentation.is_quit_confirmation_open());
    assert!(fixture.application.try_next()?.is_none());
    let (output, _) = fixture.frame(None, Vec::new())?;
    fixture.click(&output, "Confirm quit")?;
    assert!(!fixture.presentation.is_quit_confirmation_open());
    assert!(fixture.application.try_next()?.is_none());
    assert!(fixture.presentation.take_confirmed_quit_request());
    assert!(!fixture.presentation.take_confirmed_quit_request());
    fixture.presentation.activate(
        PresentationActionEnumId::ConfirmQuit,
        fixture.context.cumulative_frame_nr(),
        &fixture.application.port(),
    )?;
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
        .activate(PresentationActionEnumId::ConfirmQuit, 1, &port)?;
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

#[test]
fn settings_appearance_changes_are_immediate_and_truthfully_session_scoped()
-> PresentationAppTestResult {
    let mut fixture = PresentationAppFixture::new();
    fixture
        .presentation
        .accept_snapshot(PresentationAppFixture::snapshot()?);
    let (output, _) = fixture.frame(None, Vec::new())?;
    fixture.click(&output, "Settings")?;
    assert!(fixture.presentation.is_settings_open());
    let (output, _) = fixture.frame(None, Vec::new())?;
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .ok_or("missing Settings semantics")?
            .nodes
            .iter()
            .any(|(_, node)| node
                .value()
                .is_some_and(|value| value.contains("for this session")
                    && value.contains("not available yet")))
    );
    fixture.click(&output, "Light")?;
    fixture.frame(None, Vec::new())?;
    assert_eq!(
        fixture.presentation.preferences().theme(),
        DesktopPreferencesEnumTheme::Light
    );
    assert_eq!(fixture.context.theme(), egui::Theme::Light);
    assert!(!fixture.context.global_style().visuals.dark_mode);
    let light_fill = fixture.context.global_style().visuals.panel_fill;
    let (output, _) = fixture.frame(None, Vec::new())?;
    fixture.click(&output, "Dark")?;
    fixture.frame(None, Vec::new())?;
    assert_eq!(
        fixture.presentation.preferences().theme(),
        DesktopPreferencesEnumTheme::Dark
    );
    assert_eq!(fixture.context.theme(), egui::Theme::Dark);
    assert!(fixture.context.global_style().visuals.dark_mode);
    assert_ne!(
        fixture.context.global_style().visuals.panel_fill,
        light_fill
    );
    assert_eq!(
        fixture
            .presentation
            .snapshot()
            .ok_or("missing preserved board")?
            .board()
            .fen(),
        BoardPosition::standard().fen()
    );
    assert!(fixture.application.try_next()?.is_none());
    assert_eq!(
        PresentationApp::new().preferences().theme(),
        DesktopPreferencesEnumTheme::Dark
    );
    Ok(())
}

#[test]
fn native_new_game_view_emits_one_configuration_only_after_start() -> PresentationAppTestResult {
    let mut fixture = PresentationAppFixture::new();
    let (output, _) = fixture.frame(None, Vec::new())?;
    fixture.click(&output, "New Game")?;
    assert!(fixture.application.try_next()?.is_none());
    let (output, _) = fixture.frame(None, Vec::new())?;
    fixture.click(&output, "Local two-player")?;
    let (output, _) = fixture.frame(None, Vec::new())?;
    fixture.click(&output, "Start Game")?;
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing native New Game request")?;
    assert!(
        matches!(work.request().command().operation(), DesktopEnumCommand::Game(
        chess::desktop::game_bridge::DesktopGameBridgeEnumOperation::Create { configuration, .. }
    ) if configuration.opponent() == chess::game::configuration::GameConfigurationEnumOpponent::LocalTwoPlayer)
    );
    assert!(fixture.application.try_next()?.is_none());
    assert!(fixture.presentation.game_snapshot().is_none());
    Ok(())
}

#[test]
fn game_requests_retain_identity_and_accept_only_native_snapshots() -> PresentationAppTestResult {
    use crate::game_transition_fixture::GameTransitionFixture;
    use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
    use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
    use chess::desktop::game_snapshot::DesktopGameSnapshot;

    let mut fixture = PresentationAppFixture::new();
    let mut session = GameTransitionFixture::session(
        chess::clock::time_control::ClockTimeControl::untimed(),
        None,
    )?;
    let before = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    assert!(fixture.presentation.accept_game_snapshot(before.clone()));
    let input = "123e4567-e89b-42d3-a456-426614174080".parse()?;
    let operation = DesktopGameBridgeEnumOperation::Session {
        game: session.id(),
        revision: session.revision(),
        action: DesktopGameCommandsEnumAction::Play {
            node: GameTransitionFixture::id(2)?,
            origin: "e2".parse()?,
            destination: "e4".parse()?,
            promotion: None,
            color: chess::board::enums::BoardEnumColor::White,
        },
    };
    let port = fixture.application.port();
    let identity = fixture
        .presentation
        .activate_game(operation.clone(), input, &port)?;
    for _ in 0..8 {
        assert_eq!(
            fixture
                .presentation
                .activate_game(operation.clone(), input, &port)?,
            identity
        );
    }
    assert_eq!(
        fixture
            .presentation
            .game_snapshot()
            .ok_or("missing pre-move snapshot")?
            .board()
            .fen(),
        before.board().fen()
    );
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing native game work")?;
    assert_eq!(work.request().request_id(), identity);
    assert!(fixture.application.try_next()?.is_none());
    fixture.application.begin_commit(&work)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 0)?;
    let accepted = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Game(Box::new(accepted.clone())),
        None,
    )?;
    fixture.presentation.poll();
    assert_eq!(
        fixture
            .presentation
            .game_snapshot()
            .ok_or("missing accepted game")?
            .revision(),
        session.revision()
    );
    assert_eq!(
        fixture
            .presentation
            .game_snapshot()
            .ok_or("missing accepted board")?
            .board()
            .fen(),
        accepted.board().fen()
    );
    assert!(!fixture.presentation.accept_game_snapshot(before));
    assert_eq!(
        fixture
            .presentation
            .game_snapshot()
            .ok_or("missing retained native board")?
            .board()
            .fen(),
        accepted.board().fen()
    );
    assert!(!fixture.presentation.has_pending_work());
    Ok(())
}

#[test]
fn unsolicited_game_events_are_acknowledged_once_and_update_the_mounted_board()
-> PresentationAppTestResult {
    use crate::game_transition_fixture::GameTransitionFixture;
    use chess::desktop::dto::DesktopEnumEvent;
    use chess::desktop::game_snapshot::DesktopGameSnapshot;
    use chess::desktop::stream::{DesktopEnumStreamClass, DesktopStream};

    let mut fixture = PresentationAppFixture::new();
    let subscription = "123e4567-e89b-42d3-a456-426614174081".parse()?;
    fixture.subscribe(subscription)?;
    let session = GameTransitionFixture::session(
        chess::clock::time_control::ClockTimeControl::untimed(),
        None,
    )?;
    let mut stream = DesktopStream::new(subscription, ClockInstant::from_nanos(0));
    let event = stream
        .publish(
            DesktopEnumEvent::Games(vec![DesktopGameSnapshot::capture(
                &session,
                ClockInstant::from_nanos(0),
            )?]),
            DesktopEnumStreamClass::Snapshot,
            ClockInstant::from_nanos(0),
        )?
        .ok_or("missing fixture event")?;
    fixture.application.publish_event(event)?;
    fixture.frame(None, Vec::new())?;
    fixture.frame(None, Vec::new())?;
    assert_eq!(
        fixture
            .presentation
            .game_snapshot()
            .ok_or("missing streamed native game")?
            .id(),
        session.id()
    );
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing stream acknowledgement")?;
    assert!(
        matches!(work.request().command().operation(), DesktopEnumCommand::StreamAcknowledge { subscription_id, .. } if *subscription_id == subscription)
    );
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn refused_game_admission_retries_the_original_identity_without_closing_the_host()
-> PresentationAppTestResult {
    use crate::game_transition_fixture::GameTransitionFixture;
    use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
    use chess::desktop::game_commands::DesktopGameCommandsEnumAction;

    let mut fixture = PresentationAppFixture::new();
    let port = fixture.application.port();
    for _ in 0..DesktopApplication::<PresentationAppFixtureClock, PresentationAppFixtureWake>::ORDINARY_CAPACITY {
        let request = port.request(chess::desktop::messages::DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        port.try_submit(&request)?;
    }
    let session = GameTransitionFixture::session(
        chess::clock::time_control::ClockTimeControl::untimed(),
        None,
    )?;
    let operation = DesktopGameBridgeEnumOperation::Session {
        game: session.id(),
        revision: session.revision(),
        action: DesktopGameCommandsEnumAction::ReturnToLive,
    };
    let input = "123e4567-e89b-42d3-a456-426614174082".parse()?;
    let identity = match fixture
        .presentation
        .activate_game(operation.clone(), input, &port)
    {
        Err(PresentationAppError::Admission {
            identity,
            source: chess::desktop::application::DesktopApplicationError::Busy,
        }) => identity,
        outcome => {
            return Err(format!("expected bounded game refusal, received {outcome:?}").into());
        }
    };
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing queue capacity fixture")?;
    fixture
        .application
        .complete_value(&work, DesktopEnumValue::StreamAcknowledge, None)?;
    fixture.frame(None, Vec::new())?;
    assert_eq!(
        fixture
            .presentation
            .activate_game(operation, input, &port)?,
        identity
    );
    assert!(fixture.presentation.failure().is_none());
    Ok(())
}

#[test]
fn mounted_semantic_board_submits_native_moves_and_keeps_rejections_visible()
-> PresentationAppTestResult {
    use crate::game_transition_fixture::GameTransitionFixture;
    use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
    use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
    use chess::desktop::game_snapshot::DesktopGameSnapshot;
    use chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid;

    let mut fixture = PresentationAppFixture::new();
    let session = GameTransitionFixture::session(
        chess::clock::time_control::ClockTimeControl::untimed(),
        None,
    )?;
    fixture
        .presentation
        .accept_game_snapshot(DesktopGameSnapshot::capture(
            &session,
            ClockInstant::from_nanos(0),
        )?);
    fixture.frame(None, Vec::new())?;
    let (_, viewport) = fixture.frame(None, Vec::new())?;
    let viewport = viewport.ok_or("missing game board viewport")?;
    let scene = PresentationBoardScene::new(
        egui::TextureId::User(43),
        SceneRendererGeneration::initial(),
        SceneEnumLifecycle::Ready,
        viewport,
        SceneCamera::initial(chess::board::enums::BoardEnumColor::White)
            .fit(viewport.physical_size())?,
    );
    fixture.frame(Some(&scene), Vec::new())?;
    for square in ["e2", "e4"] {
        fixture.frame(
            Some(&scene),
            vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: PresentationBoardSemanticGrid::square_id(square.parse()?)
                        .accesskit_id(),
                    data: None,
                },
            )],
        )?;
    }
    let work = fixture
        .application
        .try_next()?
        .ok_or("mounted board did not submit a native move")?;
    assert!(matches!(
        work.request().command().operation(),
        DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
            game,
            revision,
            action: DesktopGameCommandsEnumAction::Play { origin, destination, .. },
        }) if *game == session.id()
            && *revision == session.revision()
            && *origin == "e2".parse()?
            && *destination == "e4".parse()?
    ));
    assert!(fixture.application.try_next()?.is_none());
    assert_eq!(
        fixture
            .presentation
            .game_snapshot()
            .ok_or("missing mounted game")?
            .board()
            .fen(),
        session.start_position().initial_fen()
    );
    fixture.application.complete_failure(
        &work,
        DesktopFailure::new(
            DesktopEnumFailureCode::StaleRevision,
            "The native move was rejected after the game changed".to_owned(),
            false,
        ),
    )?;
    fixture.frame(Some(&scene), Vec::new())?;
    assert_eq!(
        fixture.presentation.failure(),
        Some("The native move was rejected after the game changed")
    );
    assert!(!fixture.presentation.is_quit_ready());
    assert!(!fixture.presentation.has_pending_work());
    Ok(())
}

#[test]
fn stream_gaps_request_resynchronization_before_applying_game_events() -> PresentationAppTestResult
{
    use crate::game_transition_fixture::GameTransitionFixture;
    use chess::desktop::dto::DesktopEnumEvent;
    use chess::desktop::game_snapshot::DesktopGameSnapshot;
    use chess::desktop::stream::{DesktopEnumStreamClass, DesktopStream};

    let mut fixture = PresentationAppFixture::new();
    let subscription = "123e4567-e89b-42d3-a456-426614174083".parse()?;
    fixture.subscribe(subscription)?;
    let session = GameTransitionFixture::session(
        chess::clock::time_control::ClockTimeControl::untimed(),
        None,
    )?;
    let mut stream = DesktopStream::new(subscription, ClockInstant::from_nanos(0));
    let missed = stream
        .publish(
            DesktopEnumEvent::Snapshot(PresentationAppFixture::snapshot()?),
            DesktopEnumStreamClass::Snapshot,
            ClockInstant::from_nanos(0),
        )?
        .ok_or("missing first event")?;
    let later = stream
        .publish(
            DesktopEnumEvent::Games(vec![DesktopGameSnapshot::capture(
                &session,
                ClockInstant::from_nanos(0),
            )?]),
            DesktopEnumStreamClass::Snapshot,
            ClockInstant::from_nanos(0),
        )?
        .ok_or("missing second event")?;
    assert!(later.sequence() > missed.sequence());
    fixture.application.publish_event(later)?;
    fixture.frame(None, Vec::new())?;
    assert!(fixture.presentation.game_snapshot().is_none());
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing resynchronization request")?;
    assert!(
        matches!(work.request().command().operation(), DesktopEnumCommand::StreamResync { subscription_id } if *subscription_id == subscription)
    );
    fixture.frame(None, Vec::new())?;
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn failed_stream_ack_resynchronizes_once_then_waits_for_explicit_retry() -> PresentationAppTestResult
{
    use chess::desktop::dto::DesktopEnumEvent;
    use chess::desktop::stream::{DesktopEnumStreamClass, DesktopStream};

    let mut fixture = PresentationAppFixture::new();
    let subscription = "123e4567-e89b-42d3-a456-426614174084".parse()?;
    fixture.subscribe(subscription)?;
    let mut stream = DesktopStream::new(subscription, ClockInstant::from_nanos(0));
    let event = stream
        .publish(
            DesktopEnumEvent::Snapshot(PresentationAppFixture::snapshot()?),
            DesktopEnumStreamClass::Snapshot,
            ClockInstant::from_nanos(0),
        )?
        .ok_or("missing native stream event")?;
    fixture.application.publish_event(event)?;
    fixture.frame(None, Vec::new())?;
    let acknowledgment = fixture
        .application
        .try_next()?
        .ok_or("missing native stream acknowledgement")?;
    assert!(matches!(
        acknowledgment.request().command().operation(),
        DesktopEnumCommand::StreamAcknowledge { .. }
    ));
    fixture.application.complete_failure(
        &acknowledgment,
        DesktopFailure::new(
            DesktopEnumFailureCode::InvalidAcknowledgement,
            "The native stream acknowledgement is no longer available".to_owned(),
            true,
        ),
    )?;
    fixture.frame(None, Vec::new())?;
    let resync = fixture
        .application
        .try_next()?
        .ok_or("failed acknowledgement did not request resynchronization")?;
    assert!(
        matches!(resync.request().command().operation(), DesktopEnumCommand::StreamResync { subscription_id } if *subscription_id == subscription)
    );
    fixture.application.complete_failure(
        &resync,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "Native stream resynchronization is unavailable".to_owned(),
            true,
        ),
    )?;
    for _ in 0..8 {
        fixture.frame(None, Vec::new())?;
        assert!(fixture.application.try_next()?.is_none());
    }
    assert_eq!(
        fixture.presentation.failure(),
        Some("Native stream resynchronization is unavailable")
    );
    assert!(!fixture.presentation.has_pending_work());
    let (output, _) = fixture.frame(None, Vec::new())?;
    fixture.click(&output, "Retry sync")?;
    fixture.frame(None, Vec::new())?;
    let retry = fixture
        .application
        .try_next()?
        .ok_or("explicit stream retry was not admitted")?;
    assert!(
        matches!(retry.request().command().operation(), DesktopEnumCommand::StreamResync { subscription_id } if *subscription_id == subscription)
    );
    assert_ne!(retry.request().request_id(), resync.request().request_id());
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}
