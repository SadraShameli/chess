#[path = "analysis/fixture.rs"]
mod fixture;

use bevy_egui::egui;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use chess::board::position::BoardPosition;
use chess::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::preferences::DesktopPreferencesEnumTheme;
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::session::EngineSearchSessionEnumEvent;
use chess::presentation::analysis::configuration::{
    PresentationAnalysisConfiguration, PresentationAnalysisConfigurationClock,
    PresentationAnalysisConfigurationEnumMode,
};
use chess::presentation::analysis::controller::{
    PresentationAnalysisController, PresentationAnalysisControllerEnumAction,
    PresentationAnalysisControllerEnumState, PresentationAnalysisControllerEnumWorkspace,
    PresentationAnalysisControllerIntent, PresentationAnalysisControllerSelection,
};
use chess::presentation::analysis::view::PresentationAnalysisView;
use chess::presentation::theme::tokens::PresentationThemeTokens;

use fixture::{PresentationAnalysisFixture, PresentationAnalysisTestResult};

struct PresentationAnalysisMounted {
    needs_theme: bool,
    view: PresentationAnalysisView,
    controller: PresentationAnalysisController,
    intents: Vec<PresentationAnalysisControllerIntent>,
    errors: Vec<String>,
}

impl PresentationAnalysisMounted {
    fn new(controller: PresentationAnalysisController) -> Self {
        Self {
            needs_theme: true,
            view: PresentationAnalysisView::new(),
            controller,
            intents: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_theme {
            PresentationThemeTokens::apply(ui.ctx(), DesktopPreferencesEnumTheme::Dark);
            state.needs_theme = false;
            ui.ctx().request_repaint();
            return;
        }
        match state.view.show(ui, &mut state.controller) {
            Ok(Some(intent)) => {
                state.intents.push(intent);
                ui.ctx()
                    .request_discard("Exercise repeated analysis passes");
            }
            Ok(None) => {}
            Err(error) => state.errors.push(error.to_string()),
        }
    }

    fn mount(controller: PresentationAnalysisController) -> Harness<'static, Self> {
        Harness::builder()
            .with_size(egui::vec2(1280.0, 1600.0))
            .build_ui_state(Self::show, Self::new(controller))
    }
}

#[test]
fn exclusive_modes_use_validated_backend_limits_and_reject_invalid_inputs()
-> PresentationAnalysisTestResult {
    for mode in PresentationAnalysisConfigurationEnumMode::ALL {
        let configuration = PresentationAnalysisConfiguration::for_mode(mode);
        assert_eq!(configuration.mode(), mode);
        configuration.validate()?;
    }
    let cases = [
        PresentationAnalysisConfiguration::Depth("0".into()),
        PresentationAnalysisConfiguration::Depth("128".into()),
        PresentationAnalysisConfiguration::Nodes("2147483648".into()),
        PresentationAnalysisConfiguration::Nodes("-1".into()),
        PresentationAnalysisConfiguration::MoveTime("0".into()),
        PresentationAnalysisConfiguration::MoveTime("18446744073709551615".into()),
        PresentationAnalysisConfiguration::Depth("go infinite".into()),
    ];
    for configuration in cases {
        assert!(configuration.validate().is_err());
    }
    let mut clock = PresentationAnalysisConfigurationClock {
        moves_to_go: "0".into(),
        ..PresentationAnalysisConfigurationClock::default()
    };
    assert!(
        PresentationAnalysisConfiguration::Clock(clock.clone())
            .validate()
            .is_err()
    );
    clock.moves_to_go = "23".into();
    let expected = chess::engine::search::limit::EngineSearchClock::new(
        chess::clock::duration::ClockDuration::from_millis(600_000)?,
        chess::clock::duration::ClockDuration::from_millis(600_000)?,
        chess::clock::duration::ClockDuration::from_millis(5_000)?,
        chess::clock::duration::ClockDuration::from_millis(5_000)?,
        std::num::NonZeroU32::new(23),
    )?;
    assert_eq!(
        PresentationAnalysisConfiguration::Clock(clock.clone()).validate()?,
        EngineSearchLimit::clock(expected)
    );
    clock.white_time = "2147483648".into();
    assert!(
        PresentationAnalysisConfiguration::Clock(clock)
            .validate()
            .is_err()
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn mounted_candidates_keep_rank_san_white_and_reported_scores_and_coherent_statistics()
-> PresentationAnalysisTestResult {
    let mut fixture = PresentationAnalysisFixture::new().await?;
    let position = EngineSearchPosition::new(BoardPosition::standard(), ["e2e4"])?;
    let mut controller = fixture.controller(position, 1)?;
    let tag = fixture.start(&mut controller).await?;
    let event = fixture.snapshot(tag, &[
        "info depth 8 multipv 2 score mate -3 lowerbound nodes 0 time 0 nps 0 hashfull 0 tbhits 0 pv e7e5 g1f3",
        "info depth 9 seldepth 13 multipv 1 score cp 34 lowerbound nodes 18446744073709551615 time 18446744073709551615 nps 9007199254740993 tbhits 18446744073709551614 pv c7c5 g1f3",
        "info nodes 77 nps 88",
    ]).await?;
    assert!(controller.observe_event(&event)?);
    let mut harness = PresentationAnalysisMounted::mount(controller);
    harness.run_ok().ok_or("analysis panel did not settle")?;
    let first = harness.get_by_label("Candidate 1").rect();
    let second = harness.get_by_label("Candidate 2").rect();
    assert!(first.top() < second.top());
    harness.get_by_label("1... c5 2. Nf3");
    harness.get_by_label("1... e5 2. Nf3");
    harness.get_by_label("White: ≤ -0.34 pawns (upper bound)");
    harness.get_by_label("White: ≤ mate +3 (upper bound)");
    harness.get_by_label("Reported Black: ≥ +34 cp (lower bound)");
    harness.get_by_label("Reported Black: ≥ mate -3 (lower bound)");
    harness
        .get_by_label("Candidate 1 statistics")
        .click_accesskit();
    harness
        .get_by_label("Candidate 2 statistics")
        .click_accesskit();
    harness.run_ok().ok_or("statistics did not expand")?;
    harness.get_by_label("Nodes: 18446744073709551615");
    harness.get_by_label("Elapsed: 18446744073709551615 ms");
    harness.get_by_label("NPS: 9007199254740993");
    harness.get_by_label("Hash occupancy: unavailable");
    harness.get_by_label("Tablebase hits: 18446744073709551614");
    harness.get_by_label("Nodes: 0");
    harness.get_by_label("Elapsed: 0 ms");
    harness.get_by_label("NPS: 0");
    harness.get_by_label("Hash occupancy: 0/1000");
    harness.get_by_label("Tablebase hits: 0");
    assert!(harness.state().errors.is_empty());
    fixture.close().await
}

#[test]
fn mounted_invalid_mode_is_actionable_and_start_is_one_shot_even_on_repeated_passes()
-> PresentationAnalysisTestResult {
    let selection = PresentationAnalysisFixture::selection(
        EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
        1,
    )?;
    let mut harness =
        PresentationAnalysisMounted::mount(PresentationAnalysisController::new(selection));
    harness
        .state_mut()
        .view
        .set_configuration(PresentationAnalysisConfiguration::Depth("0".into()));
    harness.run_ok().ok_or("invalid form did not settle")?;
    assert!(
        harness
            .get_by_label("Start Analysis")
            .accesskit_node()
            .is_disabled()
    );
    harness
        .get_by_label("Depth: The search depth exceeds Reckless's supported positive depth range");
    assert!(harness.state().intents.is_empty());
    harness
        .state_mut()
        .view
        .set_configuration(PresentationAnalysisConfiguration::Nodes("123456".into()));
    harness.run_ok().ok_or("valid form did not settle")?;
    harness.get_by_label("Start Analysis").focus();
    harness.get_by_label("Start Analysis").click_accesskit();
    harness.run_ok().ok_or("Start did not settle")?;
    harness.run_steps(6);
    assert_eq!(harness.state().intents.len(), 1);
    assert!(
        matches!(harness.state().intents[0].action(), PresentationAnalysisControllerEnumAction::Start(limit) if *limit == EngineSearchLimit::nodes(123456)?)
    );
    harness.get_by_label("Starting analysis");
    assert!(harness.state().errors.is_empty());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn mounted_stop_emits_one_exact_tag_and_terminal_wins_over_pending_telemetry()
-> PresentationAnalysisTestResult {
    let mut fixture = PresentationAnalysisFixture::new().await?;
    let mut controller = fixture.controller(PresentationAnalysisFixture::position()?, 1)?;
    let tag = fixture.start(&mut controller).await?;
    let stale = fixture
        .snapshot(tag, &["info depth 5 score cp 5 pv e2e4"])
        .await?;
    controller.observe_event(&stale)?;
    let mut harness = PresentationAnalysisMounted::mount(controller);
    harness.run_ok().ok_or("search did not mount")?;
    harness.get_by_label("Stop Analysis").click_accesskit();
    harness.run_ok().ok_or("Stop did not settle")?;
    harness.run_steps(6);
    assert_eq!(harness.state().intents.len(), 1);
    assert!(
        matches!(harness.state().intents[0].action(), PresentationAnalysisControllerEnumAction::Stop(received) if *received == tag)
    );
    fixture.emit("info depth 12 score cp 60 pv d2d4").await?;
    fixture.emit("bestmove d2d4").await?;
    let finished = fixture.next(tag).await?;
    assert!(matches!(
        finished,
        EngineSearchSessionEnumEvent::Finished(_)
    ));
    harness.state_mut().controller.observe_event(&finished)?;
    assert!(!harness.state_mut().controller.observe_event(&stale)?);
    assert_eq!(
        harness.state().controller.state(),
        PresentationAnalysisControllerEnumState::Completed
    );
    harness.run_ok().ok_or("completion did not settle")?;
    harness.get_by_label("Analysis complete: d4");
    harness.get_by_label("White: +0.60 pawns (exact)");
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn stale_context_history_request_search_and_generation_are_rejected_and_replacement_is_empty()
-> PresentationAnalysisTestResult {
    let mut fixture = PresentationAnalysisFixture::new().await?;
    let mut controller = fixture.controller(PresentationAnalysisFixture::position()?, 1)?;
    let old_tag = fixture.start(&mut controller).await?;
    let old = fixture
        .snapshot(old_tag, &["info score cp 22 pv e2e4"])
        .await?;
    controller.observe_event(&old)?;
    let mut foreign = PresentationAnalysisFixture::new().await?;
    let mut foreign_controller = foreign.controller(PresentationAnalysisFixture::position()?, 1)?;
    let foreign_tag = foreign.start(&mut foreign_controller).await?;
    assert_ne!(old_tag.generation(), foreign_tag.generation());
    let foreign_event = foreign
        .snapshot(foreign_tag, &["info score cp 9 pv d2d4"])
        .await?;
    assert!(!controller.observe_event(&foreign_event)?);
    fixture.emit("bestmove e2e4").await?;
    controller.observe_event(&fixture.next(old_tag).await?)?;
    let replacement_tag = fixture.start(&mut controller).await?;
    assert_ne!(old_tag, replacement_tag);
    assert!(controller.snapshot().is_none());
    assert!(!controller.observe_event(&old)?);
    let changed_history =
        EngineSearchPosition::new(BoardPosition::standard(), ["g1f3", "g8f6", "f3g1", "f6g8"])?;
    assert_eq!(
        changed_history.current().identity(),
        PresentationAnalysisFixture::position()?
            .current()
            .identity()
    );
    assert_eq!(
        controller.synchronize(PresentationAnalysisFixture::selection(changed_history, 1)?),
        Some(replacement_tag)
    );
    assert!(!controller.observe_event(&old)?);
    let request = DesktopRequestId::new();
    let intent = controller
        .start(EngineSearchLimit::infinite(), request)?
        .ok_or("missing pending Start")?;
    assert!(controller.is_intent_current(&intent));
    controller.synchronize(PresentationAnalysisFixture::selection(
        PresentationAnalysisFixture::position()?,
        2,
    )?);
    assert!(!controller.is_intent_current(&intent));
    assert!(
        controller
            .observe_started(request, fixture.metadata()?)
            .is_err()
    );
    assert_eq!(
        controller.state(),
        PresentationAnalysisControllerEnumState::Idle
    );
    foreign.close().await?;
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn immediate_failure_cancellation_and_explicit_retry_are_visible()
-> PresentationAnalysisTestResult {
    let mut fixture = PresentationAnalysisFixture::new().await?;
    let mut controller = fixture.controller(PresentationAnalysisFixture::position()?, 1)?;
    let tag = fixture.start(&mut controller).await?;
    fixture.emit("info score cp 40 pv e2e5").await?;
    controller.observe_event(&fixture.next(tag).await?)?;
    assert_eq!(
        controller.state(),
        PresentationAnalysisControllerEnumState::Failed
    );
    let mut harness = PresentationAnalysisMounted::mount(controller);
    harness.run_ok().ok_or("failure did not mount")?;
    harness.get_by_label("Retry Analysis").click_accesskit();
    harness.run_ok().ok_or("Retry did not settle")?;
    assert_eq!(harness.state().intents.len(), 1);
    let request = harness.state().intents[0].request_id();
    harness.state_mut().controller.observe_failure(
        request,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "Free CPU capacity or stop another search, then retry".into(),
            true,
        ),
    )?;
    harness.run_ok().ok_or("resource refusal did not settle")?;
    harness.get_by_label("Free CPU capacity or stop another search, then retry");
    fixture.close().await?;
    let mut cancelled_fixture = PresentationAnalysisFixture::new().await?;
    let mut cancelled =
        cancelled_fixture.controller(PresentationAnalysisFixture::position()?, 1)?;
    cancelled_fixture.start(&mut cancelled).await?;
    let terminal = cancelled_fixture.cancel().await?;
    cancelled.observe_terminal(&terminal)?;
    assert_eq!(
        cancelled.state(),
        PresentationAnalysisControllerEnumState::Cancelled
    );
    let mut harness = PresentationAnalysisMounted::mount(cancelled);
    harness.run_ok().ok_or("cancellation did not mount")?;
    harness.get_by_label("Analysis cancelled. Start again when ready.");
    cancelled_fixture.close().await
}

#[test]
fn evaluation_bar_is_accessible_in_analysis_review_and_excluded_from_live_play()
-> PresentationAnalysisTestResult {
    for workspace in [
        PresentationAnalysisControllerEnumWorkspace::Analysis,
        PresentationAnalysisControllerEnumWorkspace::Review,
        PresentationAnalysisControllerEnumWorkspace::LivePlay,
    ] {
        let selection =
            PresentationAnalysisFixture::selection(PresentationAnalysisFixture::position()?, 1)?;
        let selection = PresentationAnalysisControllerSelection::new(
            selection.context(),
            selection.position().clone(),
            workspace,
        );
        let mut harness =
            PresentationAnalysisMounted::mount(PresentationAnalysisController::new(selection));
        harness.run_ok().ok_or("workspace did not mount")?;
        assert_eq!(
            harness
                .query_by_label("White evaluation: unavailable")
                .is_some(),
            workspace != PresentationAnalysisControllerEnumWorkspace::LivePlay
        );
        if workspace != PresentationAnalysisControllerEnumWorkspace::LivePlay {
            assert!(
                harness
                    .get_by_label("White evaluation: unavailable")
                    .accesskit_node()
                    .numeric_value()
                    .is_none()
            );
        }
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn white_and_black_scores_preserve_sign_bounds_extremes_and_signed_mates()
-> PresentationAnalysisTestResult {
    let cases = [
        (
            false,
            "info score cp -1 upperbound pv e2e4",
            "White: ≤ -0.01 pawns (upper bound)",
            "Reported White: ≤ -1 cp (upper bound)",
        ),
        (
            false,
            "info score cp 0 pv e2e4",
            "White: +0.00 pawns (exact)",
            "Reported White: +0 cp (exact)",
        ),
        (
            false,
            "info score mate 2 lowerbound pv e2e4",
            "White: ≥ mate +2 (lower bound)",
            "Reported White: ≥ mate +2 (lower bound)",
        ),
        (
            true,
            "info score cp -2147483648 upperbound pv e7e5",
            "White: ≥ +21474836.48 pawns (lower bound)",
            "Reported Black: ≤ -2147483648 cp (upper bound)",
        ),
        (
            true,
            "info score mate 0 pv e7e5",
            "White: mate +0 (exact)",
            "Reported Black: mate +0 (exact)",
        ),
        (
            true,
            "info score mate 5 upperbound pv e7e5",
            "White: ≥ mate -5 (lower bound)",
            "Reported Black: ≤ mate +5 (upper bound)",
        ),
    ];
    for (is_black, text, white, raw) in cases {
        let mut fixture = PresentationAnalysisFixture::new().await?;
        let position = if is_black {
            EngineSearchPosition::new(BoardPosition::standard(), ["e2e4"])?
        } else {
            PresentationAnalysisFixture::position()?
        };
        let mut controller = fixture.controller(position, 1)?;
        let tag = fixture.start(&mut controller).await?;
        controller.observe_event(&fixture.snapshot(tag, &[text]).await?)?;
        let mut harness = PresentationAnalysisMounted::mount(controller);
        harness.run_ok().ok_or("score did not mount")?;
        harness.get_by_label(white);
        harness.get_by_label(raw);
        assert!(harness.state().errors.is_empty());
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn explicit_backend_stop_and_immediate_natural_completion_need_no_visual_snapshot()
-> PresentationAnalysisTestResult {
    for should_stop in [false, true] {
        let mut fixture = PresentationAnalysisFixture::new().await?;
        let mut controller = fixture.controller(PresentationAnalysisFixture::position()?, 1)?;
        let tag = fixture.start(&mut controller).await?;
        if should_stop {
            let request = DesktopRequestId::new();
            assert!(controller.stop(request)?.is_some());
            assert!(controller.stop(request)?.is_none());
            controller.observe_terminal(&fixture.stop(tag).await?)?;
            assert_eq!(
                controller.state(),
                PresentationAnalysisControllerEnumState::Stopped
            );
            assert!(
                controller
                    .observe_failure(
                        request,
                        DesktopFailure::new(
                            DesktopEnumFailureCode::InternalFailure,
                            "late Stop failure".into(),
                            true
                        )
                    )
                    .is_err()
            );
        } else {
            fixture.emit("bestmove e2e4").await?;
            controller.observe_event(&fixture.next(tag).await?)?;
            assert_eq!(
                controller.state(),
                PresentationAnalysisControllerEnumState::Completed
            );
        }
        assert!(
            controller
                .snapshot()
                .is_some_and(|snapshot| snapshot.lines().is_empty())
        );
        let mut harness = PresentationAnalysisMounted::mount(controller);
        harness.run_ok().ok_or("immediate terminal did not mount")?;
        harness.get_by_label(if should_stop {
            "Analysis stopped. Start again when ready."
        } else {
            "Analysis complete: e4"
        });
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn foreign_game_and_revision_updates_cannot_repopulate_a_changed_selection()
-> PresentationAnalysisTestResult {
    use chess::engine::search::identity::{
        EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
    };
    for context in [
        EngineSearchContext::new(
            EngineSearchGameId::try_from(uuid::Uuid::from_u128(2))?,
            EngineSearchRevision::new(1),
        ),
        EngineSearchContext::new(
            EngineSearchGameId::try_from(uuid::Uuid::from_u128(1))?,
            EngineSearchRevision::new(2),
        ),
    ] {
        let mut fixture = PresentationAnalysisFixture::new().await?;
        let mut controller = fixture.controller(PresentationAnalysisFixture::position()?, 1)?;
        let tag = fixture.start(&mut controller).await?;
        let event = fixture.snapshot(tag, &["info score cp 25 pv e2e4"]).await?;
        controller.observe_event(&event)?;
        assert_eq!(
            controller.synchronize(PresentationAnalysisControllerSelection::new(
                context,
                PresentationAnalysisFixture::position()?,
                PresentationAnalysisControllerEnumWorkspace::Analysis
            )),
            Some(tag)
        );
        assert!(!controller.observe_event(&event)?);
        assert!(controller.snapshot().is_none());
        assert_eq!(
            controller.state(),
            PresentationAnalysisControllerEnumState::Idle
        );
        fixture.close().await?;
    }
    Ok(())
}
