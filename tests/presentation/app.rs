use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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
use chess::desktop::preferences::{DesktopPreferencesEnumMotion, DesktopPreferencesEnumTheme};
use chess::presentation::action::registry::PresentationActionEnumId;
use chess::presentation::app::{PresentationApp, PresentationAppError};
use chess::presentation::board::scene::PresentationBoardScene;
use chess::scene::camera::SceneCamera;
use chess::scene::enums::SceneEnumLifecycle;
use chess::scene::picking::ScenePickingViewport;
use chess::scene::renderer::SceneRendererGeneration;

type PresentationAppTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct PresentationAppFixtureClock {
    nanos: AtomicU64,
}

impl ClockTimeProvider for PresentationAppFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.nanos.load(Ordering::SeqCst)))
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
    clock: Arc<PresentationAppFixtureClock>,
    board_actions: Vec<chess::presentation::board::view::PresentationBoardAction>,
}

impl PresentationAppFixture {
    fn assert_board_focus(&self, square: &str) -> PresentationAppTestResult {
        assert_eq!(
            self.context.memory(|memory| memory.focused()),
            Some(
                chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid::square_id(
                    square.parse()?
                )
            )
        );
        Ok(())
    }

    fn assert_premove_markers(output: &egui::FullOutput, expected: usize) {
        assert_eq!(output.shapes.iter().filter(|shape| matches!(&shape.shape,
            egui::epaint::Shape::Text(shape) if matches!(shape.galley.job.text.as_str(), "Premove from" | "Premove to")
        )).count(), expected);
    }

    fn queued_session() -> PresentationAppTestResult<chess::game::session::GameSession> {
        let mut session = crate::game_transition_fixture::GameTransitionFixture::session(
            chess::clock::time_control::ClockTimeControl::untimed(),
            Some(chess::board::enums::BoardEnumColor::Black),
        )?;
        crate::game_transition_fixture::GameTransitionFixture::play(&mut session, 2, "e2e4", 0)?;
        session.set_premoves_enabled(session.revision(), true)?;
        session.queue_premove(
            session.revision(),
            chess::game::premove::GamePremoveIntent::new("g1".parse()?, "f3".parse()?, None)?,
        )?;
        Ok(session)
    }

    fn promotion_session(
        engine: Option<chess::board::enums::BoardEnumColor>,
    ) -> PresentationAppTestResult<chess::game::session::GameSession> {
        let fen = match engine {
            Some(chess::board::enums::BoardEnumColor::Black) => "4k3/P7/8/8/8/8/8/4K3 b - - 0 1",
            Some(chess::board::enums::BoardEnumColor::White) | None => {
                "4k3/P7/8/8/8/8/8/4K3 w - - 0 1"
            }
        };
        crate::game_transition_fixture::GameTransitionFixture::session_with_start(
            chess::clock::time_control::ClockTimeControl::untimed(),
            engine,
            chess::board::start::BoardStart::custom(BoardPosition::from_fen(
                fen,
                chess::board::enums::BoardEnumVariant::Standard,
            )?),
        )
    }

    fn open_promotion(
        &mut self,
        scene: &PresentationBoardScene,
    ) -> PresentationAppTestResult<egui::FullOutput> {
        for square in ["a7", "a8"] {
            self.frame(
                Some(scene),
                vec![Self::square_event(square, accesskit::Action::Click)?],
            )?;
        }
        self.frame(Some(scene), Vec::new())?;
        let (output, _) = self.frame(Some(scene), Vec::new())?;
        Ok(output)
    }

    fn control_event(
        output: &egui::FullOutput,
        label: &str,
    ) -> PresentationAppTestResult<egui::Event> {
        let (target, _) = output
            .platform_output
            .accesskit_update
            .as_ref()
            .ok_or("missing native action semantics")?
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .ok_or("missing native action control")?;
        Ok(egui::Event::AccessKitActionRequest(
            accesskit::ActionRequest {
                action: accesskit::Action::Click,
                target_tree: accesskit::TreeId::ROOT,
                target_node: *target,
                data: None,
            },
        ))
    }

    fn new() -> Self {
        let context = Context::default();
        context.enable_accesskit();
        let clock = Arc::new(PresentationAppFixtureClock::default());
        Self {
            context,
            presentation: PresentationApp::new(),
            application: DesktopApplication::new(
                Arc::clone(&clock),
                Arc::new(PresentationAppFixtureWake),
            ),
            clock,
            board_actions: Vec::new(),
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
        self.frame_with_passes(scene, events, false)
    }

    fn frame_with_passes(
        &mut self,
        scene: Option<&PresentationBoardScene>,
        events: Vec<egui::Event>,
        repeat: bool,
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
                if repeat && ui.ctx().current_pass_index() == 0 {
                    ui.ctx()
                        .request_discard("Verify mounted control deduplication");
                }
            },
        );
        output.textures_delta.clear();
        let frame = result.ok_or("missing presentation frame")??;
        self.board_actions.extend_from_slice(frame.board_actions());
        Ok((output, frame.viewport()))
    }

    fn update_game(
        &mut self,
        session: &chess::game::session::GameSession,
    ) -> PresentationAppTestResult {
        self.clock.nanos.fetch_add(1, Ordering::SeqCst);
        assert!(self.presentation.accept_game_snapshot(
            chess::desktop::game_snapshot::DesktopGameSnapshot::capture(
                session,
                self.clock.now()?
            )?
        ));
        Ok(())
    }

    fn mount(
        &mut self,
        session: &chess::game::session::GameSession,
    ) -> PresentationAppTestResult<PresentationBoardScene> {
        self.presentation.accept_game_snapshot(
            chess::desktop::game_snapshot::DesktopGameSnapshot::capture(
                session,
                ClockInstant::from_nanos(0),
            )?,
        );
        self.frame(None, Vec::new())?;
        let (_, viewport) = self.frame(None, Vec::new())?;
        let viewport = viewport.ok_or("missing mounted board viewport")?;
        let scene = PresentationBoardScene::new(
            egui::TextureId::User(43),
            SceneRendererGeneration::initial(),
            SceneEnumLifecycle::Ready,
            viewport,
            SceneCamera::initial(chess::board::enums::BoardEnumColor::White)
                .fit(viewport.physical_size())?,
        );
        self.frame(Some(&scene), Vec::new())?;
        Ok(scene)
    }

    fn square_event(
        square: &str,
        action: accesskit::Action,
    ) -> PresentationAppTestResult<egui::Event> {
        Ok(egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node: chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid::square_id(square.parse()?).accesskit_id(),
            data: None,
        }))
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn click(&mut self, output: &egui::FullOutput, label: &str) -> PresentationAppTestResult {
        self.frame(None, vec![Self::control_event(output, label)?])?;
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
    assert_eq!(
        fixture.presentation.preferences().motion(),
        DesktopPreferencesEnumMotion::Full
    );
    fixture.click(&output, "Reduced motion")?;
    fixture.frame(None, Vec::new())?;
    assert_eq!(
        fixture.presentation.preferences().motion(),
        DesktopPreferencesEnumMotion::Reduced
    );
    let (output, _) = fixture.frame(None, Vec::new())?;
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
fn mounted_retry_adjudication_dispatches_once_per_click_and_preserves_unknown_state()
-> PresentationAppTestResult {
    use crate::game_transition_fixture::{
        GameTransitionFixture, GameTransitionFixtureUnresolvedProof,
    };
    use chess::clock::state::ClockStateEnumStatus;
    use chess::clock::time_control::ClockTimeControl;
    use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
    use chess::desktop::game_commands::{
        DesktopGameCommandsEnumAction, DesktopGameCommandsRequest,
    };
    use chess::desktop::game_snapshot::DesktopGameSnapshot;
    use chess::desktop::game_transition::{
        DesktopGameTransition, DesktopGameTransitionEnumEffect, DesktopGameTransitionOptions,
    };
    use chess::desktop::identity::DesktopRevision;
    use chess::game::adjudication::GameAdjudicationEnumMateQuestion;
    use chess::game::enums::{GameEnumEngineReadiness, GameEnumPhase};
    use chess::game::pause::GamePauseEnumEngineBarrier;
    use chess::game::session::GameSessionEnumInterruption;

    let mut fixture = PresentationAppFixture::new();
    let proof =
        GameTransitionFixtureUnresolvedProof::new(GameAdjudicationEnumMateQuestion::AnySide);
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play_with_proof(&mut session, 2, "e2e4", 0, &proof)?;
    let before = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    assert!(fixture.presentation.accept_game_snapshot(before.clone()));
    fixture.context.on_end_pass(
        "retry adjudication repeated passes",
        Arc::new(|context| {
            if context.current_pass_index() == 0 {
                context.request_discard("exercise retained adjudication retry");
            }
        }),
    );
    let mut last_request = None;
    for attempt in 1..=2 {
        let revision = session.revision();
        let (output, _) = fixture.frame(None, Vec::new())?;
        let passes = fixture.context.cumulative_pass_nr();
        fixture.click(&output, "Retry Adjudication")?;
        assert!(fixture.context.cumulative_pass_nr() - passes > 1);
        let work = fixture
            .application
            .try_next()?
            .ok_or("Retry Adjudication did not submit a native request")?;
        let DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
            game,
            revision: requested_revision,
            action,
        }) = work.request().command().operation()
        else {
            return Err("Retry Adjudication submitted a different operation".into());
        };
        assert_eq!(*game, session.id());
        assert_eq!(*requested_revision, revision);
        assert_eq!(*action, DesktopGameCommandsEnumAction::Resume);
        assert_eq!(
            work.request().command().expected_revision(),
            Some(DesktopRevision::for_game(revision))
        );
        assert_ne!(Some(work.request().request_id()), last_request);
        last_request = Some(work.request().request_id());
        for _ in 0..8 {
            fixture.frame(None, Vec::new())?;
            assert!(fixture.application.try_next()?.is_none());
        }
        fixture.application.begin_commit(&work)?;
        assert_eq!(
            DesktopGameTransition::apply(
                &mut session,
                DesktopGameTransitionOptions {
                    request: DesktopGameCommandsRequest {
                        request_id: work.request().request_id(),
                        expected_revision: *requested_revision,
                        received_at: work.receipt().received_at(),
                        action: action.clone(),
                    },
                    engine: GamePauseEnumEngineBarrier::NoEngine,
                    readiness: GameEnumEngineReadiness::NotRequired,
                },
                &proof,
            )?,
            DesktopGameTransitionEnumEffect::Autosave
        );
        assert_eq!(proof.calls(), attempt + 1);
        assert_eq!(session.revision(), revision.next()?);
        let accepted = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
        fixture.application.complete_value(
            &work,
            DesktopEnumValue::Game(Box::new(accepted)),
            None,
        )?;
        for _ in 0..8 {
            fixture.frame(None, Vec::new())?;
            assert!(fixture.application.try_next()?.is_none());
        }
        let retained = fixture
            .presentation
            .game_snapshot()
            .ok_or("missing native adjudication snapshot")?;
        assert_eq!(retained.revision(), session.revision());
        assert_eq!(retained.phase(), GameEnumPhase::Interrupted);
        assert!(matches!(
            retained.interruption(),
            Some(GameSessionEnumInterruption::Adjudication { .. })
        ));
        assert_eq!(retained.board().fen(), before.board().fen());
        assert_eq!(retained.live(), before.live());
        assert_eq!(retained.view(), before.view());
        assert_eq!(retained.clock(), before.clock());
        assert_eq!(retained.clock().status(), ClockStateEnumStatus::Paused);
        assert!(retained.outcome().is_none());
        assert!(fixture.presentation.failure().is_none());
        assert!(!fixture.presentation.has_pending_work());
    }
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

#[test]
fn mounted_premove_cancellation_is_one_revision_checked_request_from_button_or_escape()
-> PresentationAppTestResult {
    use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
    use chess::desktop::game_commands::DesktopGameCommandsEnumAction;

    for escape in [false, true] {
        let session = PresentationAppFixture::queued_session()?;
        let clock = session.clock().clone();
        let mut fixture = PresentationAppFixture::new();
        let scene = fixture.mount(&session)?;
        let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
        let event = if escape {
            PresentationAppFixture::key(egui::Key::Escape, egui::Modifiers::NONE)
        } else {
            PresentationAppFixture::control_event(&output, "Cancel Premove")?
        };
        let (output, _) = fixture.frame_with_passes(Some(&scene), vec![event], true)?;
        assert_eq!(output.platform_output.num_completed_passes, 2);
        let work = fixture
            .application
            .try_next()?
            .ok_or("missing premove cancellation")?;
        assert!(matches!(work.request().command().operation(),
            DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
                game, revision, action: DesktopGameCommandsEnumAction::CancelPremove,
            }) if *game == session.id() && *revision == session.revision()
        ));
        for _ in 0..4 {
            fixture.frame(Some(&scene), Vec::new())?;
        }
        assert!(fixture.application.try_next()?.is_none());
        assert!(session.premove().pending().is_some());
        assert_eq!(
            fixture
                .presentation
                .game_snapshot()
                .ok_or("missing queued snapshot")?
                .premove(),
            session.premove().pending()
        );
        assert_eq!(session.clock(), &clock);
        assert_eq!(session.tree().len(), 2);
    }
    Ok(())
}

#[test]
fn mounted_local_takeback_without_consent_submits_immediately_once_without_approval()
-> PresentationAppTestResult {
    use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};
    use chess::board::enums::BoardEnumColor;
    use chess::clock::time_control::ClockTimeControl;
    use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
    use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
    use chess::desktop::game_snapshot::DesktopGameSnapshot;
    use chess::game::pause::GamePauseEnumEngineBarrier;
    use chess::game::takeback::{GameTakebackEnumConsent, GameTakebackRequest};

    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 0)?;
    session.configure_takeback_consent(session.revision(), GameTakebackEnumConsent::NotRequired)?;
    let mut fixture = PresentationAppFixture::new();
    let scene = fixture.mount(&session)?;
    let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
    let event = PresentationAppFixture::control_event(&output, "White: Request Takeback")?;
    fixture.frame_with_passes(Some(&scene), vec![event], true)?;
    let work = fixture
        .application
        .try_next()?
        .ok_or("missing local takeback")?;
    assert!(matches!(work.request().command().operation(),
        DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
            game, revision, action: DesktopGameCommandsEnumAction::RequestTakeback { requester: BoardEnumColor::White },
        }) if *game == session.id() && *revision == session.revision()
    ));
    assert_eq!(session.live(), GameTransitionFixture::id(2)?);
    fixture.application.begin_commit(&work)?;
    session.request_takeback(
        GameTakebackRequest {
            expected_revision: session.revision(),
            requester: BoardEnumColor::White,
            received_at: ClockInstant::from_nanos(0),
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(session.live(), session.tree().root());
    assert!(session.takeback().pending().is_none());
    fixture.application.complete_value(
        &work,
        DesktopEnumValue::Game(Box::new(DesktopGameSnapshot::capture(
            &session,
            ClockInstant::from_nanos(0),
        )?)),
        None,
    )?;
    let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .ok_or("missing takeback feedback")?
            .nodes
            .iter()
            .all(|(_, node)| node
                .label()
                .is_none_or(|label| !label.contains("Accept Takeback")))
    );
    assert!(fixture.application.try_next()?.is_none());
    assert_eq!(
        fixture
            .presentation
            .game_snapshot()
            .ok_or("missing game")?
            .live(),
        session.tree().root()
    );
    Ok(())
}

#[test]
fn mounted_local_resignation_confirms_each_side_and_cancellation_never_dispatches()
-> PresentationAppTestResult {
    use crate::game_transition_fixture::GameTransitionFixture;
    use chess::board::enums::BoardEnumColor;
    use chess::clock::time_control::ClockTimeControl;
    use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
    use chess::desktop::game_commands::DesktopGameCommandsEnumAction;

    for (color, label) in [
        (BoardEnumColor::White, "White: Resign"),
        (BoardEnumColor::Black, "Black: Resign"),
    ] {
        let session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
        let clock = session.clock().clone();
        let mut fixture = PresentationAppFixture::new();
        let scene = fixture.mount(&session)?;
        let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
        let resign = PresentationAppFixture::control_event(&output, label)?;
        let (output, _) = fixture.frame(Some(&scene), vec![resign.clone()])?;
        assert!(fixture.application.try_next()?.is_none());
        let cancel = PresentationAppFixture::control_event(&output, "Keep Playing")?;
        fixture.frame_with_passes(Some(&scene), vec![cancel], true)?;
        assert!(fixture.application.try_next()?.is_none());
        assert_eq!(session.clock(), &clock);
        assert_eq!(session.tree().len(), 1);
        let (output, _) = fixture.frame(Some(&scene), vec![resign])?;
        let confirm = PresentationAppFixture::control_event(&output, "Confirm Resignation")?;
        fixture.frame_with_passes(Some(&scene), vec![confirm], true)?;
        let work = fixture
            .application
            .try_next()?
            .ok_or("missing confirmed local resignation")?;
        assert!(matches!(work.request().command().operation(),
            DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
                game, revision, action: DesktopGameCommandsEnumAction::Resign { side },
            }) if *game == session.id() && *revision == session.revision() && *side == color
        ));
        assert!(fixture.application.try_next()?.is_none());
        assert_eq!(session.clock(), &clock);
        assert!(session.outcome().is_none());
    }
    Ok(())
}

#[test]
fn mounted_queued_premove_marks_both_squares_with_visible_and_accessible_roles()
-> PresentationAppTestResult {
    let session = PresentationAppFixture::queued_session()?;
    let mut fixture = PresentationAppFixture::new();
    let scene = fixture.mount(&session)?;
    let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
    for (square, role, text) in [
        ("g1", "queued premove source", "Premove from"),
        ("f3", "queued premove destination", "Premove to"),
    ] {
        let id =
            chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid::square_id(
                square.parse()?,
            )
            .accesskit_id();
        let (_, node) = output
            .platform_output
            .accesskit_update
            .as_ref()
            .ok_or("missing board semantics")?
            .nodes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .ok_or("missing queued square")?;
        assert!(
            node.label().is_some_and(|label| label.contains(role)),
            "{square}: {:?}",
            node.label()
        );
        assert!(
            output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::epaint::Shape::Text(shape) if shape.galley.job.text == text
            )),
            "missing visible {text} marker"
        );
    }
    Ok(())
}

#[test]
fn mounted_promotion_modal_owns_keyboard_focus_and_escape_returns_to_origin()
-> PresentationAppTestResult {
    let session = PresentationAppFixture::promotion_session(None)?;
    let mut fixture = PresentationAppFixture::new();
    let scene = fixture.mount(&session)?;
    let output = fixture.open_promotion(&scene)?;
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .ok_or("missing promotion semantics")?
        .nodes;
    assert!(
        nodes
            .iter()
            .any(|(_, node)| node.role() == accesskit::Role::Dialog),
        "promotion must be a modal dialog"
    );
    let (first, _) = nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Promote to Queen"))
        .ok_or("missing initial promotion choice")?;
    assert_eq!(
        fixture
            .context
            .memory(|memory| memory.focused())
            .map(|id| id.accesskit_id()),
        Some(*first)
    );
    for step in 0..12 {
        fixture.frame(
            Some(&scene),
            vec![PresentationAppFixture::key(
                egui::Key::Tab,
                if step < 6 {
                    egui::Modifiers::NONE
                } else {
                    egui::Modifiers::SHIFT
                },
            )],
        )?;
        let focused = fixture
            .context
            .memory(|memory| memory.focused())
            .ok_or("modal lost focus")?
            .accesskit_id();
        assert!(
            nodes.iter().any(|(id, node)| *id == focused
                && node.role() == accesskit::Role::Button
                && node.label().is_some_and(
                    |label| label.starts_with("Promote to ") || label == "Cancel Promotion"
                )),
            "focus escaped promotion modal"
        );
        if step == 0 {
            assert!(
                nodes
                    .iter()
                    .any(|(id, node)| *id == focused && node.label() == Some("Promote to Rook"))
            );
        }
    }
    fixture.frame_with_passes(
        Some(&scene),
        vec![PresentationAppFixture::key(
            egui::Key::Escape,
            egui::Modifiers::NONE,
        )],
        true,
    )?;
    fixture.frame(Some(&scene), Vec::new())?;
    fixture.assert_board_focus("a7")?;
    assert!(fixture.application.try_next()?.is_none());
    assert_eq!(session.tree().len(), 1);
    Ok(())
}

#[test]
fn mounted_promotion_keyboard_and_accessibility_keep_all_four_choices_and_one_original_action()
-> PresentationAppTestResult {
    use chess::board::enums::BoardEnumPieceKind;
    use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
    use chess::desktop::game_commands::DesktopGameCommandsEnumAction;

    for (kind, key, label) in [
        (BoardEnumPieceKind::Queen, egui::Key::Q, "Promote to Queen"),
        (BoardEnumPieceKind::Rook, egui::Key::R, "Promote to Rook"),
        (
            BoardEnumPieceKind::Bishop,
            egui::Key::B,
            "Promote to Bishop",
        ),
        (
            BoardEnumPieceKind::Knight,
            egui::Key::N,
            "Promote to Knight",
        ),
    ] {
        for keyboard in [false, true] {
            let session = PresentationAppFixture::promotion_session(None)?;
            let mut fixture = PresentationAppFixture::new();
            let scene = fixture.mount(&session)?;
            let output = fixture.open_promotion(&scene)?;
            assert!(fixture.application.try_next()?.is_none());
            let action = *fixture
                .board_actions
                .last()
                .ok_or("missing originating promotion action")?;
            let selection = if keyboard {
                PresentationAppFixture::key(key, egui::Modifiers::NONE)
            } else {
                PresentationAppFixture::control_event(&output, label)?
            };
            fixture.frame_with_passes(
                Some(&scene),
                vec![
                    PresentationAppFixture::square_event("e1", accesskit::Action::Click)?,
                    selection,
                ],
                true,
            )?;
            let work = fixture
                .application
                .try_next()?
                .ok_or("promotion did not dispatch")?;
            assert!(matches!(work.request().command().operation(),
                DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
                    game, revision, action: DesktopGameCommandsEnumAction::Play { node, origin, destination, promotion: Some(selected), .. },
                }) if *game == session.id() && *revision == session.revision()
                    && *node == String::from(action.identity()).parse()?
                    && *origin == "a7".parse()? && *destination == "a8".parse()? && *selected == kind
            ));
            assert_eq!(fixture.board_actions.len(), 2);
            assert!(fixture.application.try_next()?.is_none());
            fixture.frame(Some(&scene), Vec::new())?;
            fixture.assert_board_focus("a7")?;
            assert_eq!(session.tree().len(), 1);
            assert_eq!(
                fixture
                    .presentation
                    .game_snapshot()
                    .ok_or("missing unchanged snapshot")?
                    .board()
                    .fen(),
                session.start_position().initial_fen()
            );
        }
    }
    Ok(())
}

#[test]
fn mounted_premove_promotion_cancel_dismisses_only_the_modal_and_keyboard_choice_queues_later()
-> PresentationAppTestResult {
    use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind};
    use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
    use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
    use chess::game::premove::GamePremoveIntent;

    for escape in [false, true] {
        let mut session = PresentationAppFixture::promotion_session(Some(BoardEnumColor::Black))?;
        session.set_premoves_enabled(session.revision(), true)?;
        let queued = GamePremoveIntent::new("e1".parse()?, "e2".parse()?, None)?;
        session.queue_premove(session.revision(), queued)?;
        let mut fixture = PresentationAppFixture::new();
        let scene = fixture.mount(&session)?;
        let output = fixture.open_promotion(&scene)?;
        let cancel = if escape {
            PresentationAppFixture::key(egui::Key::Escape, egui::Modifiers::NONE)
        } else {
            PresentationAppFixture::control_event(&output, "Cancel Promotion")?
        };
        fixture.frame_with_passes(Some(&scene), vec![cancel], true)?;
        fixture.frame(Some(&scene), Vec::new())?;
        assert!(fixture.application.try_next()?.is_none());
        assert_eq!(session.premove().pending(), Some(queued));
        assert_eq!(
            fixture
                .presentation
                .game_snapshot()
                .ok_or("missing queued game")?
                .premove(),
            Some(queued)
        );
        fixture.open_promotion(&scene)?;
        fixture.frame_with_passes(
            Some(&scene),
            vec![PresentationAppFixture::key(
                egui::Key::N,
                egui::Modifiers::NONE,
            )],
            true,
        )?;
        let work = fixture
            .application
            .try_next()?
            .ok_or("missing promotion premove")?;
        assert!(matches!(work.request().command().operation(),
            DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
                game, revision, action: DesktopGameCommandsEnumAction::QueuePremove { intent },
            }) if *game == session.id() && *revision == session.revision()
                && intent.origin() == "a7".parse()? && intent.destination() == "a8".parse()?
                && intent.promotion() == Some(BoardEnumPieceKind::Knight)
        ));
        assert!(fixture.application.try_next()?.is_none());
    }
    Ok(())
}

#[test]
fn mounted_premove_feedback_tracks_history_replacement_cancellation_pause_and_stale_snapshots()
-> PresentationAppTestResult {
    use crate::game_transition_fixture::GameTransitionFixtureProof;
    use chess::desktop::game_snapshot::DesktopGameSnapshot;
    use chess::game::pause::{
        GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseEnumFault, GamePauseRequest,
    };
    use chess::game::premove::{GamePremoveEnumClearReason, GamePremoveIntent};

    let mut session = PresentationAppFixture::queued_session()?;
    let stale = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let mut fixture = PresentationAppFixture::new();
    let scene = fixture.mount(&session)?;
    session.select_view(session.tree().root())?;
    fixture.update_game(&session)?;
    let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
    PresentationAppFixture::assert_premove_markers(&output, 0);
    assert!(session.premove().pending().is_some());
    session.return_to_live();
    fixture.update_game(&session)?;
    let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
    PresentationAppFixture::assert_premove_markers(&output, 2);
    session.queue_premove(
        session.revision(),
        GamePremoveIntent::new("g1".parse()?, "h3".parse()?, None)?,
    )?;
    fixture.update_game(&session)?;
    let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .ok_or("missing replacement semantics")?
        .nodes;
    assert!(nodes.iter().any(|(_, node)| node.label().is_some_and(
        |label| label.starts_with("h3:") && label.contains("queued premove destination")
    )));
    assert!(nodes.iter().all(|(_, node)| {
        node.label()
            .is_none_or(|label| !label.starts_with("f3:") || !label.contains("queued premove"))
    }));
    session.clear_premove(session.revision(), GamePremoveEnumClearReason::Explicit)?;
    fixture.update_game(&session)?;
    assert!(!fixture.presentation.accept_game_snapshot(stale));
    let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
    PresentationAppFixture::assert_premove_markers(&output, 0);
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .ok_or("missing cleared semantics")?
            .nodes
            .iter()
            .all(|(_, node)| node
                .label()
                .is_none_or(|label| !label.contains("queued premove")))
    );
    session.queue_premove(
        session.revision(),
        GamePremoveIntent::new("g1".parse()?, "f3".parse()?, None)?,
    )?;
    fixture.update_game(&session)?;
    let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
    PresentationAppFixture::assert_premove_markers(&output, 2);
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Engine),
            received_at: fixture.clock.now()?,
            engine: GamePauseEnumEngineBarrier::FailedEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    fixture.update_game(&session)?;
    let (output, _) = fixture.frame(Some(&scene), Vec::new())?;
    PresentationAppFixture::assert_premove_markers(&output, 0);
    assert!(session.premove().pending().is_none());
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}

#[test]
fn mounted_revision_change_dismisses_promotion_and_ignores_the_stale_modal_choice()
-> PresentationAppTestResult {
    use crate::game_transition_fixture::GameTransitionFixture;

    let mut session = PresentationAppFixture::promotion_session(None)?;
    let mut fixture = PresentationAppFixture::new();
    let scene = fixture.mount(&session)?;
    let output = fixture.open_promotion(&scene)?;
    let stale_choice = PresentationAppFixture::control_event(&output, "Promote to Queen")?;
    GameTransitionFixture::play(&mut session, 2, "e1e2", 0)?;
    fixture.update_game(&session)?;
    let (output, _) = fixture.frame_with_passes(Some(&scene), vec![stale_choice], true)?;
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .ok_or("missing updated semantics")?
            .nodes
            .iter()
            .all(|(_, node)| node.role() != accesskit::Role::Dialog)
    );
    assert!(fixture.application.try_next()?.is_none());
    assert_eq!(session.live(), GameTransitionFixture::id(2)?);
    assert_eq!(
        fixture
            .presentation
            .game_snapshot()
            .ok_or("missing updated game")?
            .revision(),
        session.revision()
    );
    Ok(())
}

#[test]
fn mounted_stale_promotion_returns_focus_only_after_the_current_board_is_mounted()
-> PresentationAppTestResult {
    use crate::game_transition_fixture::GameTransitionFixture;

    let mut session = PresentationAppFixture::promotion_session(None)?;
    let mut fixture = PresentationAppFixture::new();
    let scene = fixture.mount(&session)?;
    for square in ["a7", "a8"] {
        fixture.frame(
            Some(&scene),
            vec![PresentationAppFixture::square_event(
                square,
                accesskit::Action::Click,
            )?],
        )?;
    }
    GameTransitionFixture::play(&mut session, 2, "e1e2", 0)?;
    fixture.update_game(&session)?;
    let viewport = ScenePickingViewport::new(
        scene.viewport().image().translate(egui::vec2(10.0, 0.0)),
        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0)),
        1.0,
        1.0,
    )?;
    let stale_scene = PresentationBoardScene::new(
        scene.texture(),
        scene.generation(),
        scene.lifecycle(),
        viewport,
        scene.camera().clone(),
    );
    fixture.frame(Some(&stale_scene), Vec::new())?;
    assert!(
        fixture.context.memory(|memory| memory.focused()).is_none(),
        "focus must not target an unmounted square"
    );
    fixture.frame(Some(&scene), Vec::new())?;
    fixture.assert_board_focus("a7")?;
    assert!(fixture.application.try_next()?.is_none());
    Ok(())
}
