use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};

use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::clock::duration::ClockInstant;
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::identity::DesktopRequestId;
use chess::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameEnumSeat};
use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use chess::presentation::game::controller::{
    PresentationGameController, PresentationGameControllerError, PresentationGameControllerIntent,
};
use chess::presentation::theme::font::PresentationThemeFont;

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

type PresentationGameControllerTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationGameControllerFixture {
    needs_fonts: bool,
    controller: PresentationGameController,
    snapshot: DesktopGameSnapshot,
    submissions: Vec<PresentationGameControllerIntent>,
    failures: Vec<String>,
}

impl PresentationGameControllerFixture {
    fn request(value: u128) -> PresentationGameControllerTestResult<DesktopRequestId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn snapshot(
        session: &GameSession,
    ) -> PresentationGameControllerTestResult<DesktopGameSnapshot> {
        Ok(DesktopGameSnapshot::capture(
            session,
            ClockInstant::from_millis(10_000)?,
        )?)
    }

    fn custom(
        fen: &str,
        variant: BoardEnumVariant,
        engine: Option<BoardEnumColor>,
    ) -> PresentationGameControllerTestResult<GameSession> {
        let position = BoardPosition::from_fen(fen, variant)?;
        let mode = if engine.is_some() {
            GameEnumMode::HumanVersusEngine
        } else {
            GameEnumMode::LocalTwoPlayer
        };
        let seat = |color| {
            if engine == Some(color) {
                GameEnumSeat::Reckless
            } else {
                GameEnumSeat::Human
            }
        };
        let mut session = GameSession::new(GameSessionOptions {
            id: "123e4567-e89b-42d3-a456-426614174000".parse()?,
            root: GameTransitionFixture::id(1)?,
            clock: ClockState::new(ClockTimeControl::untimed(), position.turn()),
            start: BoardStart::custom(position),
            seats: GameSeats::new(
                mode,
                seat(BoardEnumColor::White),
                seat(BoardEnumColor::Black),
            )?,
            metadata: GameMetadata::default(),
        })?;
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::Ready,
            &GameTransitionFixtureProof,
        )?;
        Ok(session)
    }

    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_fonts {
            ui.ctx().set_fonts(PresentationThemeFont::definitions());
            ui.ctx().request_repaint();
            state.needs_fonts = false;
            return;
        }
        match state.controller.show(ui, &state.snapshot, false) {
            Ok(Some(intent)) => {
                state.submissions.push(intent);
                ui.ctx().request_discard("Verify repeated promotion passes");
            }
            Ok(None) => {}
            Err(error) => state.failures.push(error.to_string()),
        }
    }
}

#[test]
fn click_and_drag_emit_one_stable_typed_move_without_changing_native_state()
-> PresentationGameControllerTestResult {
    let session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let snapshot = PresentationGameControllerFixture::snapshot(&session)?;
    let origin = "e2".parse()?;
    let destination = "e4".parse()?;
    let first = PresentationGameControllerFixture::request(100)?;
    let second = PresentationGameControllerFixture::request(101)?;
    let mut controller = PresentationGameController::new();
    assert!(controller.select(&snapshot, origin, first)?.is_none());
    assert_eq!(controller.selected(), Some(origin));
    let intent = controller
        .select(&snapshot, destination, second)?
        .ok_or("missing move")?;
    assert_eq!(intent.game(), session.id());
    assert_eq!(intent.revision(), session.revision());
    assert_eq!(intent.request_id(), second);
    assert!(
        matches!(intent.action(), DesktopGameCommandsEnumAction::Play {
        node, origin: from, destination: to, promotion: None, color: BoardEnumColor::White,
    } if *node == String::from(second).parse()? && *from == origin && *to == destination)
    );
    for _ in 0..8 {
        assert!(controller.select(&snapshot, origin, first)?.is_none());
        assert!(controller.select(&snapshot, destination, second)?.is_none());
    }
    let drag_id = PresentationGameControllerFixture::request(102)?;
    assert!(
        controller
            .drag(&snapshot, origin, destination, drag_id)?
            .is_some()
    );
    assert!(
        controller
            .drag(&snapshot, origin, destination, drag_id)?
            .is_none()
    );
    assert_eq!(session.live(), session.tree().root());
    assert_eq!(session.clock().active(), BoardEnumColor::White);
    Ok(())
}

#[test]
fn chess960_king_destination_ambiguity_requires_the_explicit_castling_rook()
-> PresentationGameControllerTestResult {
    let session = PresentationGameControllerFixture::custom(
        "4k3/8/8/8/8/8/8/5K1R w H - 0 1",
        BoardEnumVariant::Chess960,
        None,
    )?;
    let snapshot = PresentationGameControllerFixture::snapshot(&session)?;
    let mut controller = PresentationGameController::new();
    assert!(matches!(
        controller.drag(
            &snapshot,
            "f1".parse()?,
            "g1".parse()?,
            PresentationGameControllerFixture::request(110)?
        ),
        Err(PresentationGameControllerError::AmbiguousCastling)
    ));
    let intent = controller
        .drag(
            &snapshot,
            "f1".parse()?,
            "h1".parse()?,
            PresentationGameControllerFixture::request(111)?,
        )?
        .ok_or("missing castling intent")?;
    assert!(
        matches!(intent.action(), DesktopGameCommandsEnumAction::Play { origin, destination, promotion: None, .. }
        if *origin == "f1".parse()? && *destination == "h1".parse()?)
    );
    Ok(())
}

#[test]
fn all_four_promotions_preserve_the_original_event_and_wait_for_explicit_choice()
-> PresentationGameControllerTestResult {
    let session = PresentationGameControllerFixture::custom(
        "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
        BoardEnumVariant::Standard,
        None,
    )?;
    let snapshot = PresentationGameControllerFixture::snapshot(&session)?;
    for kind in [
        BoardEnumPieceKind::Queen,
        BoardEnumPieceKind::Rook,
        BoardEnumPieceKind::Bishop,
        BoardEnumPieceKind::Knight,
    ] {
        let mut controller = PresentationGameController::new();
        let request = PresentationGameControllerFixture::request(120)?;
        assert!(
            controller
                .drag(&snapshot, "a7".parse()?, "a8".parse()?, request)?
                .is_none()
        );
        assert!(controller.is_promotion_pending());
        assert_eq!(controller.promotion_choices().len(), 4);
        let intent = controller
            .choose_promotion(&snapshot, kind)?
            .ok_or("missing promotion")?;
        assert_eq!(intent.request_id(), request);
        assert!(
            matches!(intent.action(), DesktopGameCommandsEnumAction::Play { promotion: Some(selected), .. } if *selected == kind)
        );
        assert!(!controller.is_promotion_pending());
    }
    Ok(())
}

#[test]
fn promotion_ui_cancels_cleanly_and_repeated_passes_emit_only_one_choice()
-> PresentationGameControllerTestResult {
    let session = PresentationGameControllerFixture::custom(
        "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
        BoardEnumVariant::Standard,
        None,
    )?;
    let snapshot = PresentationGameControllerFixture::snapshot(&session)?;
    let mut controller = PresentationGameController::new();
    controller.drag(
        &snapshot,
        "a7".parse()?,
        "a8".parse()?,
        PresentationGameControllerFixture::request(130)?,
    )?;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(
            PresentationGameControllerFixture::show,
            PresentationGameControllerFixture {
                needs_fonts: true,
                controller,
                snapshot,
                submissions: Vec::new(),
                failures: Vec::new(),
            },
        );
    harness.run_ok().ok_or("Promotion prompt did not settle")?;
    harness.get_by_label("Cancel Promotion").click_accesskit();
    harness
        .run_ok()
        .ok_or("Promotion cancellation did not settle")?;
    assert!(harness.state().submissions.is_empty());
    assert!(!harness.state().controller.is_promotion_pending());
    let snapshot = harness.state().snapshot.clone();
    harness.state_mut().controller.drag(
        &snapshot,
        "a7".parse()?,
        "a8".parse()?,
        PresentationGameControllerFixture::request(131)?,
    )?;
    harness.run_ok().ok_or("Promotion prompt did not reopen")?;
    harness.get_by_label("Promote to Knight").click_accesskit();
    harness
        .run_ok()
        .ok_or("Promotion selection did not settle")?;
    harness.run_steps(8);
    assert_eq!(harness.state().submissions.len(), 1);
    assert!(harness.state().failures.is_empty());
    assert!(matches!(
        harness.state().submissions[0].action(),
        DesktopGameCommandsEnumAction::Play {
            promotion: Some(BoardEnumPieceKind::Knight),
            ..
        }
    ));
    Ok(())
}

#[test]
fn native_revision_change_rejects_a_stale_promotion_and_clears_its_selection()
-> PresentationGameControllerTestResult {
    let mut session = PresentationGameControllerFixture::custom(
        "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
        BoardEnumVariant::Standard,
        None,
    )?;
    let mut controller = PresentationGameController::new();
    controller.drag(
        &PresentationGameControllerFixture::snapshot(&session)?,
        "a7".parse()?,
        "a8".parse()?,
        PresentationGameControllerFixture::request(140)?,
    )?;
    GameTransitionFixture::play(&mut session, 2, "e1e2", 1000)?;
    assert!(matches!(
        controller.choose_promotion(
            &PresentationGameControllerFixture::snapshot(&session)?,
            BoardEnumPieceKind::Queen
        ),
        Err(PresentationGameControllerError::StalePromotion)
    ));
    assert!(!controller.is_promotion_pending());
    assert!(controller.selected().is_none());
    Ok(())
}

#[test]
fn premoves_are_disabled_initially_then_queue_replace_and_cancel_with_native_validation()
-> PresentationGameControllerTestResult {
    let mut session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1000)?;
    let mut controller = PresentationGameController::new();
    assert!(matches!(
        controller.drag(
            &PresentationGameControllerFixture::snapshot(&session)?,
            "g1".parse()?,
            "f3".parse()?,
            PresentationGameControllerFixture::request(150)?
        ),
        Err(PresentationGameControllerError::PremovesDisabled)
    ));
    session.set_premoves_enabled(session.revision(), true)?;
    let snapshot = PresentationGameControllerFixture::snapshot(&session)?;
    let first = controller
        .drag(
            &snapshot,
            "g1".parse()?,
            "f3".parse()?,
            PresentationGameControllerFixture::request(151)?,
        )?
        .ok_or("missing premove")?;
    if let DesktopGameCommandsEnumAction::QueuePremove { intent } = first.action() {
        session.queue_premove(session.revision(), *intent)?;
    } else {
        return Err("expected a queued premove".into());
    }
    let snapshot = PresentationGameControllerFixture::snapshot(&session)?;
    let replacement = controller
        .drag(
            &snapshot,
            "b1".parse()?,
            "c3".parse()?,
            PresentationGameControllerFixture::request(152)?,
        )?
        .ok_or("missing replacement")?;
    assert!(
        matches!(replacement.action(), DesktopGameCommandsEnumAction::QueuePremove { intent } if intent.origin() == "b1".parse()?)
    );
    assert!(matches!(
        controller
            .cancel(&snapshot, PresentationGameControllerFixture::request(153)?)?
            .ok_or("missing cancel")?
            .action(),
        DesktopGameCommandsEnumAction::CancelPremove
    ));
    assert!(session.premove().pending().is_some());
    Ok(())
}

#[test]
fn premove_promotions_use_the_shared_requirement_and_preselect_all_four_pieces()
-> PresentationGameControllerTestResult {
    let mut session = PresentationGameControllerFixture::custom(
        "4k3/P7/8/8/8/8/8/4K3 b - - 0 1",
        BoardEnumVariant::Standard,
        Some(BoardEnumColor::Black),
    )?;
    session.set_premoves_enabled(session.revision(), true)?;
    let snapshot = PresentationGameControllerFixture::snapshot(&session)?;
    let mut controller = PresentationGameController::new();
    assert!(
        controller
            .drag(
                &snapshot,
                "a7".parse()?,
                "a8".parse()?,
                PresentationGameControllerFixture::request(160)?
            )?
            .is_none()
    );
    assert_eq!(controller.promotion_choices().len(), 4);
    let promotion = controller
        .choose_promotion(&snapshot, BoardEnumPieceKind::Bishop)?
        .ok_or("missing promotion premove")?;
    assert!(
        matches!(promotion.action(), DesktopGameCommandsEnumAction::QueuePremove { intent } if intent.promotion() == Some(BoardEnumPieceKind::Bishop))
    );
    Ok(())
}

#[test]
fn authoritative_acceptance_and_rejection_are_visible_without_optimistic_board_changes()
-> PresentationGameControllerTestResult {
    let session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let snapshot = PresentationGameControllerFixture::snapshot(&session)?;
    let mut controller = PresentationGameController::new();
    let request = PresentationGameControllerFixture::request(170)?;
    controller.drag(&snapshot, "e2".parse()?, "e4".parse()?, request)?;
    controller.observe_result(
        request,
        Err(DesktopFailure::new(
            DesktopEnumFailureCode::StaleRevision,
            "The native game changed before accepting this move".to_owned(),
            false,
        )),
    )?;
    assert!(
        controller
            .feedback()
            .is_some_and(|text| text.contains("native game changed"))
    );
    assert_eq!(
        snapshot.board().fen(),
        session.start_position().initial_fen()
    );
    let request = PresentationGameControllerFixture::request(171)?;
    controller.drag(&snapshot, "e2".parse()?, "e4".parse()?, request)?;
    controller.observe_result(request, Ok(()))?;
    assert_eq!(
        controller.feedback(),
        Some("Move accepted by the native game.")
    );
    assert_eq!(session.live(), session.tree().root());
    Ok(())
}

#[test]
fn conflicting_event_payloads_and_historical_input_are_rejected()
-> PresentationGameControllerTestResult {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let mut controller = PresentationGameController::new();
    let snapshot = PresentationGameControllerFixture::snapshot(&session)?;
    let request = PresentationGameControllerFixture::request(180)?;
    controller.select(&snapshot, "e2".parse()?, request)?;
    assert!(matches!(
        controller.select(&snapshot, "d2".parse()?, request),
        Err(PresentationGameControllerError::RequestConflict)
    ));
    let root = session.live();
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1000)?;
    session.select_view(root)?;
    assert!(matches!(
        controller.select(
            &PresentationGameControllerFixture::snapshot(&session)?,
            "e2".parse()?,
            PresentationGameControllerFixture::request(181)?
        ),
        Err(PresentationGameControllerError::ViewingHistory)
    ));
    Ok(())
}
