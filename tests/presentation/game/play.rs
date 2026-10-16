use bevy_egui::egui;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::game::draw_claim::{
    GameDrawClaimEnumAvailability, GameDrawClaimEnumIntent, GameDrawClaimEnumKind,
};
use chess::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameEnumPhase, GameEnumSeat};
use chess::game::pause::{
    GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseRequest, GameResumeRequest,
};
use chess::game::save::GameSaveFailureRequest;
use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use chess::game::takeback::{GameTakebackEnumResponse, GameTakebackRequest};
use chess::presentation::game::play::{
    PresentationGamePlay, PresentationGamePlayClock, PresentationGamePlayEnumOperation,
    PresentationGamePlayIntent,
};
use chess::presentation::theme::font::PresentationThemeFont;

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

type PresentationGamePlayTestResult = Result<(), Box<dyn std::error::Error>>;

struct PresentationGamePlayFixture {
    needs_fonts: bool,
    panel: PresentationGamePlay,
    snapshot: DesktopGameSnapshot,
    now: ClockInstant,
    busy: bool,
    submissions: Vec<PresentationGamePlayIntent>,
    failures: Vec<String>,
}

impl PresentationGamePlayFixture {
    const INTENDED_CLAIMS: [&str; 2] = [
        "Claim Threefold with Intended Move",
        "Claim Fifty-Move with Intended Move",
    ];

    fn custom_session(
        fen: &str,
        variant: BoardEnumVariant,
    ) -> Result<GameSession, Box<dyn std::error::Error>> {
        let position = BoardPosition::from_fen(fen, variant)?;
        let mut session = GameSession::new(GameSessionOptions {
            id: "123e4567-e89b-42d3-a456-426614174001".parse()?,
            root: GameTransitionFixture::id(1)?,
            clock: ClockState::new(ClockTimeControl::untimed(), position.turn()),
            start: BoardStart::custom(position),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
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

    fn prepare_repetition(session: &mut GameSession) -> PresentationGamePlayTestResult {
        for (offset, uci) in ["g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1"]
            .into_iter()
            .enumerate()
        {
            let id = u128::try_from(offset)?
                .checked_add(2)
                .ok_or("fixture identity overflow")?;
            let millis = u64::try_from(offset)?
                .checked_add(1)
                .ok_or("fixture time overflow")?;
            GameTransitionFixture::play(session, id, uci, millis)?;
        }
        assert_eq!(session.claimable(), GameDrawClaimEnumAvailability::None);
        Ok(())
    }

    fn select_intended(
        harness: &mut Harness<'_, Self>,
        uci: &str,
    ) -> PresentationGamePlayTestResult {
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Intended Move")
            .click_accesskit();
        harness.run_ok().ok_or("Intended move menu did not open")?;
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, uci)
            .click_accesskit();
        harness.run_ok().ok_or("Intended move did not settle")?;
        Ok(())
    }

    fn update_snapshot(
        harness: &mut Harness<'_, Self>,
        session: &GameSession,
        millis: u64,
    ) -> PresentationGamePlayTestResult {
        let now = ClockInstant::from_millis(millis)?;
        harness.state_mut().snapshot = DesktopGameSnapshot::capture(session, now)?;
        harness.state_mut().now = now;
        harness.run_ok().ok_or("Updated game did not settle")?;
        Ok(())
    }

    fn assert_intended_disabled(harness: &Harness<'_, Self>) {
        for label in Self::INTENDED_CLAIMS {
            assert!(harness.get_by_label(label).accesskit_node().is_disabled());
        }
        assert!(harness.state().submissions.is_empty());
        assert!(harness.state().failures.is_empty());
    }

    fn assert_selector_disabled(harness: &Harness<'_, Self>) {
        assert!(
            harness
                .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Intended Move")
                .accesskit_node()
                .is_disabled()
        );
        Self::assert_intended_disabled(harness);
    }

    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_fonts {
            ui.ctx().set_fonts(PresentationThemeFont::definitions());
            ui.ctx().request_repaint();
            state.needs_fonts = false;
            return;
        }
        match state.panel.show(ui, &state.snapshot, state.now, state.busy) {
            Ok(Some(intent)) => {
                state.submissions.push(intent);
                ui.ctx()
                    .request_discard("Verify repeated game-control passes");
            }
            Ok(None) => {}
            Err(error) => state.failures.push(error.to_string()),
        }
    }

    fn harness(
        session: &GameSession,
        millis: u64,
    ) -> Result<Harness<'static, Self>, Box<dyn std::error::Error>> {
        let now = ClockInstant::from_millis(millis)?;
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(
                Self::show,
                Self {
                    needs_fonts: true,
                    panel: PresentationGamePlay::new(),
                    snapshot: DesktopGameSnapshot::capture(session, now)?,
                    now,
                    busy: false,
                    submissions: Vec::new(),
                    failures: Vec::new(),
                },
            );
        harness.run_ok().ok_or("Game panel did not settle")?;
        Ok(harness)
    }
}

#[test]
fn clock_text_uses_integer_precision_and_rounds_only_the_display_quantum()
-> PresentationGamePlayTestResult {
    for (nanos, expected) in [
        (0, "0:00.0"),
        (1, "0:00.1"),
        (100_000_000, "0:00.1"),
        (100_000_001, "0:00.2"),
        (9_900_000_000, "0:09.9"),
        (10_000_000_000, "0:10"),
        (10_000_000_001, "0:11"),
        (59_100_000_000, "1:00"),
        (3_600_000_000_000, "1:00:00"),
    ] {
        let duration = ClockDuration::from_nanos(nanos);
        assert_eq!(PresentationGamePlayClock::text(Some(duration)), expected);
        assert_eq!(duration.as_nanos(), nanos);
    }
    assert_eq!(PresentationGamePlayClock::text(None), "Untimed");
    assert!(PresentationGamePlayClock::is_low_time(Some(
        ClockDuration::from_millis(9999)?
    )));
    assert!(!PresentationGamePlayClock::is_low_time(Some(
        ClockDuration::from_millis(10_000)?
    )));
    assert!(!PresentationGamePlayClock::is_low_time(None));
    Ok(())
}

#[test]
fn engine_takeback_is_one_immediate_revision_checked_intent_without_a_dialog()
-> PresentationGamePlayTestResult {
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::initial_default(),
        Some(BoardEnumColor::Black),
    )?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1000)?;
    let mut harness = PresentationGamePlayFixture::harness(&session, 2000)?;
    harness.get_by_label("Takeback").click_accesskit();
    harness.run_ok().ok_or("Takeback did not settle")?;
    harness.run_steps(8);
    assert_eq!(harness.state().submissions.len(), 1);
    let intent = harness
        .state()
        .submissions
        .first()
        .ok_or("missing takeback intent")?;
    assert_eq!(intent.game(), session.id());
    assert_eq!(intent.revision(), session.revision());
    assert_eq!(
        intent.operation(),
        PresentationGamePlayEnumOperation::Takeback {
            requester: BoardEnumColor::White
        }
    );
    assert!(!harness.state().panel.is_resignation_confirmation_open());
    assert_eq!(session.live(), GameTransitionFixture::id(2)?);
    assert_eq!(session.phase(), GameEnumPhase::Active);
    assert!(session.clock().is_running());
    assert!(harness.state().failures.is_empty());
    Ok(())
}

#[test]
fn resignation_requires_confirmation_and_cancel_never_submits_a_mutation()
-> PresentationGamePlayTestResult {
    let session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    let mut harness = PresentationGamePlayFixture::harness(&session, 1000)?;
    harness.get_by_label("Resign").click_accesskit();
    harness
        .run_ok()
        .ok_or("Resignation prompt did not settle")?;
    assert!(harness.state().panel.is_resignation_confirmation_open());
    assert!(harness.state().submissions.is_empty());
    harness.get_by_label("Keep Playing").click_accesskit();
    harness
        .run_ok()
        .ok_or("Resignation cancellation did not settle")?;
    assert!(!harness.state().panel.is_resignation_confirmation_open());
    assert!(harness.state().submissions.is_empty());
    harness.get_by_label("Resign").click_accesskit();
    harness
        .run_ok()
        .ok_or("Resignation prompt did not reopen")?;
    harness
        .get_by_label("Confirm Resignation")
        .click_accesskit();
    harness
        .run_ok()
        .ok_or("Confirmed resignation did not settle")?;
    harness.run_steps(8);
    assert_eq!(harness.state().submissions.len(), 1);
    assert_eq!(
        harness.state().submissions[0].operation(),
        PresentationGamePlayEnumOperation::Resign {
            resigning: BoardEnumColor::White
        }
    );
    assert_eq!(session.phase(), GameEnumPhase::Active);
    Ok(())
}

#[test]
fn stale_resignation_confirmation_is_closed_when_the_native_revision_changes()
-> PresentationGamePlayTestResult {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let mut harness = PresentationGamePlayFixture::harness(&session, 1000)?;
    harness.get_by_label("White: Resign").click_accesskit();
    harness
        .run_ok()
        .ok_or("Local resignation prompt did not settle")?;
    assert!(harness.state().panel.is_resignation_confirmation_open());
    GameTransitionFixture::play(&mut session, 2, "e2e4", 2000)?;
    harness.state_mut().snapshot =
        DesktopGameSnapshot::capture(&session, ClockInstant::from_millis(3000)?)?;
    harness.state_mut().now = ClockInstant::from_millis(3000)?;
    harness.run_ok().ok_or("Changed game did not settle")?;
    assert!(!harness.state().panel.is_resignation_confirmation_open());
    assert!(harness.state().submissions.is_empty());
    Ok(())
}

#[test]
fn local_takeback_consent_buttons_identify_the_opponent_and_do_not_pause_clocks()
-> PresentationGamePlayTestResult {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1000)?;
    session.request_takeback(
        GameTakebackRequest {
            expected_revision: session.revision(),
            requester: BoardEnumColor::White,
            received_at: ClockInstant::from_millis(2000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let mut harness = PresentationGamePlayFixture::harness(&session, 3000)?;
    harness
        .get_by_label("Black: Accept Takeback")
        .click_accesskit();
    harness.run_ok().ok_or("Takeback consent did not settle")?;
    assert_eq!(harness.state().submissions.len(), 1);
    assert_eq!(
        harness.state().submissions[0].operation(),
        PresentationGamePlayEnumOperation::RespondTakeback {
            responder: BoardEnumColor::Black,
            response: GameTakebackEnumResponse::Accept,
        }
    );
    assert!(session.clock().is_running());
    assert!(session.takeback().pending().is_some());
    Ok(())
}

#[test]
fn navigation_and_return_to_live_keep_native_clocks_and_history_unchanged()
-> PresentationGamePlayTestResult {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    let root = session.live();
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1000)?;
    GameTransitionFixture::play(&mut session, 3, "e7e5", 2000)?;
    session.select_view(root)?;
    let mut harness = PresentationGamePlayFixture::harness(&session, 3000)?;
    harness.get_by_label("Return to Live").click_accesskit();
    harness.run_ok().ok_or("Return to Live did not settle")?;
    assert_eq!(
        harness.state().submissions[0].operation(),
        PresentationGamePlayEnumOperation::ReturnToLive
    );
    harness.get_by_label("1. e4").click_accesskit();
    harness.run_ok().ok_or("Move navigation did not settle")?;
    assert_eq!(
        harness.state().submissions[1].operation(),
        PresentationGamePlayEnumOperation::Navigate(GameTransitionFixture::id(2)?)
    );
    assert_eq!(session.view(), root);
    assert_eq!(session.live(), GameTransitionFixture::id(3)?);
    assert!(session.clock().is_running());
    Ok(())
}

#[test]
fn explicit_claims_use_native_availability_and_the_live_human_turn()
-> PresentationGamePlayTestResult {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    PresentationGamePlayFixture::prepare_repetition(&mut session)?;
    GameTransitionFixture::play(&mut session, 9, "f6g8", 8)?;
    let mut harness = PresentationGamePlayFixture::harness(&session, 1000)?;
    assert!(harness.state().submissions.is_empty());
    harness
        .get_by_label("Claim Threefold Draw")
        .click_accesskit();
    harness.run_ok().ok_or("Draw claim did not settle")?;
    assert_eq!(harness.state().submissions.len(), 1);
    assert_eq!(
        harness.state().submissions[0].operation(),
        PresentationGamePlayEnumOperation::ClaimDraw {
            claimant: BoardEnumColor::White,
            kind: GameDrawClaimEnumKind::ThreefoldRepetition,
            intent: GameDrawClaimEnumIntent::Current,
        }
    );
    assert_eq!(session.phase(), GameEnumPhase::Active);
    Ok(())
}

#[test]
fn intended_threefold_claim_is_explicit_revision_checked_and_deduplicated()
-> PresentationGamePlayTestResult {
    let mut session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::White))?;
    PresentationGamePlayFixture::prepare_repetition(&mut session)?;
    let position = session.tree().node(session.live())?.position();
    let movement = position.parse_uci("f6g8")?;
    let fen = position.fen();
    let revision = session.revision();
    let live = session.live();
    let clock = session.clock().clone();
    let mut harness = PresentationGamePlayFixture::harness(&session, 1000)?;
    PresentationGamePlayFixture::assert_intended_disabled(&harness);
    assert!(
        harness
            .get_by_label("Claim Threefold Draw")
            .accesskit_node()
            .is_disabled()
    );
    PresentationGamePlayFixture::select_intended(&mut harness, "f6g8")?;
    assert!(harness.state().submissions.is_empty());
    harness
        .get_by_label("Claim Threefold with Intended Move")
        .click_accesskit();
    harness
        .run_ok()
        .ok_or("Intended draw claim did not settle")?;
    harness.run_steps(8);
    assert_eq!(
        harness.state().submissions,
        [PresentationGamePlayIntent::new(
            session.id(),
            revision,
            PresentationGamePlayEnumOperation::ClaimDraw {
                claimant: BoardEnumColor::Black,
                kind: GameDrawClaimEnumKind::ThreefoldRepetition,
                intent: GameDrawClaimEnumIntent::Intended(movement),
            },
        )]
    );
    assert_eq!(session.revision(), revision);
    assert_eq!(session.live(), live);
    assert_eq!(session.tree().len(), 8);
    assert_eq!(session.tree().node(live)?.position().fen(), fen);
    assert_eq!(session.phase(), GameEnumPhase::Active);
    assert_eq!(session.clock(), &clock);
    assert!(harness.state().failures.is_empty());
    Ok(())
}

#[test]
fn intended_fifty_move_claim_does_not_require_a_current_claim_or_play_the_move()
-> PresentationGamePlayTestResult {
    let session = PresentationGamePlayFixture::custom_session(
        "7k/8/5K2/8/8/8/8/R7 w - - 99 51",
        BoardEnumVariant::Standard,
    )?;
    let position = session.tree().node(session.live())?.position();
    let movement = position.parse_uci("a1a2")?;
    assert_eq!(session.claimable(), GameDrawClaimEnumAvailability::None);
    let mut harness = PresentationGamePlayFixture::harness(&session, 1000)?;
    PresentationGamePlayFixture::select_intended(&mut harness, "a1a2")?;
    harness
        .get_by_label("Claim Fifty-Move with Intended Move")
        .click_accesskit();
    harness
        .run_ok()
        .ok_or("Intended fifty-move claim did not settle")?;
    assert_eq!(
        harness.state().submissions,
        [PresentationGamePlayIntent::new(
            session.id(),
            session.revision(),
            PresentationGamePlayEnumOperation::ClaimDraw {
                claimant: BoardEnumColor::White,
                kind: GameDrawClaimEnumKind::FiftyMove,
                intent: GameDrawClaimEnumIntent::Intended(movement),
            },
        )]
    );
    assert_eq!(session.tree().len(), 1);
    assert_eq!(position.halfmoves(), 99);
    assert_eq!(session.phase(), GameEnumPhase::Active);
    assert!(harness.state().failures.is_empty());
    Ok(())
}

#[test]
fn intended_move_selector_preserves_promotions_and_chess960_castling_choices()
-> PresentationGamePlayTestResult {
    for (fen, variant, choices) in [
        (
            "7k/P7/8/8/8/8/8/7K w - - 0 1",
            BoardEnumVariant::Standard,
            ["a7a8q", "a7a8r", "a7a8b", "a7a8n"].as_slice(),
        ),
        (
            "4k3/8/8/8/8/8/8/5K1R w H - 0 1",
            BoardEnumVariant::Chess960,
            ["f1g1", "f1h1"].as_slice(),
        ),
    ] {
        let session = PresentationGamePlayFixture::custom_session(fen, variant)?;
        let position = session.tree().node(session.live())?.position();
        for &uci in choices {
            let mut harness = PresentationGamePlayFixture::harness(&session, 1000)?;
            PresentationGamePlayFixture::select_intended(&mut harness, uci)?;
            harness
                .get_by_label("Claim Fifty-Move with Intended Move")
                .click_accesskit();
            harness.run_ok().ok_or("Selected claim did not settle")?;
            assert_eq!(harness.state().submissions.len(), 1);
            assert_eq!(
                harness.state().submissions[0].operation(),
                PresentationGamePlayEnumOperation::ClaimDraw {
                    claimant: BoardEnumColor::White,
                    kind: GameDrawClaimEnumKind::FiftyMove,
                    intent: GameDrawClaimEnumIntent::Intended(position.parse_uci(uci)?),
                }
            );
            assert!(harness.state().failures.is_empty());
        }
    }
    Ok(())
}

#[test]
fn intended_claims_are_disabled_in_history_on_engine_turns_and_behind_operation_gates()
-> PresentationGamePlayTestResult {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    PresentationGamePlayFixture::prepare_repetition(&mut session)?;
    let mut harness = PresentationGamePlayFixture::harness(&session, 1000)?;
    PresentationGamePlayFixture::select_intended(&mut harness, "f6g8")?;
    harness.state_mut().busy = true;
    harness
        .run_ok()
        .ok_or("Busy draw controls did not settle")?;
    PresentationGamePlayFixture::assert_selector_disabled(&harness);
    harness.state_mut().busy = false;
    harness
        .run_ok()
        .ok_or("Available draw controls did not settle")?;
    PresentationGamePlayFixture::assert_intended_disabled(&harness);

    PresentationGamePlayFixture::select_intended(&mut harness, "f6g8")?;
    session.select_view(session.tree().root())?;
    PresentationGamePlayFixture::update_snapshot(&mut harness, &session, 2000)?;
    PresentationGamePlayFixture::assert_selector_disabled(&harness);
    session.select_view(session.live())?;
    PresentationGamePlayFixture::update_snapshot(&mut harness, &session, 2000)?;
    PresentationGamePlayFixture::assert_intended_disabled(&harness);

    PresentationGamePlayFixture::select_intended(&mut harness, "f6g8")?;
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(3000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    PresentationGamePlayFixture::update_snapshot(&mut harness, &session, 3000)?;
    PresentationGamePlayFixture::assert_selector_disabled(&harness);
    session.resume(
        GameResumeRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(4000)?,
            readiness: GameEnumEngineReadiness::Ready,
        },
        &GameTransitionFixtureProof,
    )?;
    PresentationGamePlayFixture::update_snapshot(&mut harness, &session, 4000)?;
    PresentationGamePlayFixture::assert_intended_disabled(&harness);

    PresentationGamePlayFixture::select_intended(&mut harness, "f6g8")?;
    session.interrupt_for_save_failure(
        GameSaveFailureRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(5000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    PresentationGamePlayFixture::update_snapshot(&mut harness, &session, 5000)?;
    PresentationGamePlayFixture::assert_selector_disabled(&harness);

    let mut engine =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    PresentationGamePlayFixture::prepare_repetition(&mut engine)?;
    let harness = PresentationGamePlayFixture::harness(&engine, 1000)?;
    PresentationGamePlayFixture::assert_selector_disabled(&harness);
    Ok(())
}

#[test]
fn intended_selection_survives_refresh_but_clears_for_a_new_revision_or_game()
-> PresentationGamePlayTestResult {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    PresentationGamePlayFixture::prepare_repetition(&mut session)?;
    let mut harness = PresentationGamePlayFixture::harness(&session, 1000)?;
    PresentationGamePlayFixture::select_intended(&mut harness, "f6g8")?;
    PresentationGamePlayFixture::update_snapshot(&mut harness, &session, 2000)?;
    for label in PresentationGamePlayFixture::INTENDED_CLAIMS {
        assert!(!harness.get_by_label(label).accesskit_node().is_disabled());
    }
    session.set_premoves_enabled(session.revision(), true)?;
    PresentationGamePlayFixture::update_snapshot(&mut harness, &session, 2000)?;
    PresentationGamePlayFixture::assert_intended_disabled(&harness);
    PresentationGamePlayFixture::select_intended(&mut harness, "f6g8")?;

    let mut other = PresentationGamePlayFixture::custom_session(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        BoardEnumVariant::Standard,
    )?;
    PresentationGamePlayFixture::prepare_repetition(&mut other)?;
    other.set_premoves_enabled(other.revision(), true)?;
    assert_eq!(session.revision(), other.revision());
    assert_ne!(session.id(), other.id());
    PresentationGamePlayFixture::update_snapshot(&mut harness, &other, 2000)?;
    PresentationGamePlayFixture::assert_intended_disabled(&harness);
    Ok(())
}

#[test]
fn pause_resume_and_busy_controls_submit_only_explicit_intentions() -> PresentationGamePlayTestResult
{
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    let mut harness = PresentationGamePlayFixture::harness(&session, 1000)?;
    harness.state_mut().busy = true;
    harness.run_ok().ok_or("Busy panel did not settle")?;
    assert!(
        harness
            .get_by_label("Pause Game")
            .accesskit_node()
            .is_disabled()
    );
    assert!(harness.state().submissions.is_empty());
    harness.state_mut().busy = false;
    harness.run_ok().ok_or("Available panel did not settle")?;
    harness.get_by_label("Pause Game").click_accesskit();
    harness.run_ok().ok_or("Pause did not settle")?;
    assert_eq!(
        harness.state().submissions[0].operation(),
        PresentationGamePlayEnumOperation::Pause
    );
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(2000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    harness.state_mut().snapshot =
        DesktopGameSnapshot::capture(&session, ClockInstant::from_millis(3000)?)?;
    harness.state_mut().now = ClockInstant::from_millis(3000)?;
    harness.run_ok().ok_or("Paused snapshot did not settle")?;
    harness.run_steps(8);
    assert_eq!(harness.state().submissions.len(), 1);
    harness.get_by_label("Resume Game").click_accesskit();
    harness.run_ok().ok_or("Resume did not settle")?;
    assert_eq!(harness.state().submissions.len(), 2);
    assert_eq!(
        harness.state().submissions[1].operation(),
        PresentationGamePlayEnumOperation::Resume
    );
    Ok(())
}
