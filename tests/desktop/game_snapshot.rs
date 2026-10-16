use serde::Deserialize;

use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::dto::DesktopBoardSnapshot;
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::game::pause::GamePauseEnumEngineBarrier;
use chess::game::premove::GamePremoveIntent;
use chess::game::save::GameSaveFailureRequest;
use chess::game::takeback::GameTakebackRequest;

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopGameSnapshotMoveFields {
    origin: Option<BoardSquare>,
    destination: BoardSquare,
    promotion: Option<BoardEnumPieceKind>,
    castling_rook: Option<BoardSquare>,
}

#[test]
fn board_snapshot_retains_all_native_legal_moves_without_changing_serialized_fields()
-> Result<(), Box<dyn std::error::Error>> {
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
        let position = BoardPosition::from_fen(fen, variant)?;
        let snapshot = DesktopBoardSnapshot::from_position(&position);
        assert_eq!(
            snapshot
                .legal_moves()
                .iter()
                .map(|movement| movement.movement())
                .collect::<Vec<_>>(),
            position.legal_moves()
        );
        for &uci in choices {
            let expected = position.parse_uci(uci)?;
            let retained = snapshot
                .legal_moves()
                .iter()
                .find(|movement| movement.movement() == expected)
                .ok_or("Snapshot omitted a legal native move")?;
            assert_eq!(retained.movement().uci(variant), uci);
            let serialized = serde_json::to_string(retained)?;
            let fields: DesktopGameSnapshotMoveFields = serde_json::from_str(&serialized)?;
            assert_eq!(fields.origin, expected.origin());
            assert_eq!(fields.destination, expected.destination());
            assert_eq!(fields.promotion, expected.promotion());
            assert_eq!(fields.castling_rook, expected.castling_rook());
        }
    }
    Ok(())
}

#[test]
fn board_snapshot_distinguishes_chess960_king_move_from_castling_to_the_same_square()
-> Result<(), Box<dyn std::error::Error>> {
    let position =
        BoardPosition::from_fen("4k3/8/8/8/8/8/8/5K1R w H - 0 1", BoardEnumVariant::Chess960)?;
    let snapshot = DesktopBoardSnapshot::from_position(&position);
    let king_move = snapshot
        .legal_moves()
        .iter()
        .find(|movement| movement.movement().uci(snapshot.variant()) == "f1g1")
        .ok_or("Missing ordinary king move")?;
    let castle = snapshot
        .legal_moves()
        .iter()
        .find(|movement| movement.movement().uci(snapshot.variant()) == "f1h1")
        .ok_or("Missing Chess960 castle")?;
    assert_eq!(king_move.origin(), castle.origin());
    assert_eq!(king_move.destination(), castle.destination());
    assert_eq!(king_move.castling_rook(), None);
    assert_eq!(castle.castling_rook(), Some("h1".parse()?));
    assert_ne!(king_move.movement(), castle.movement());
    Ok(())
}

#[test]
fn unsaved_interruption_still_publishes_a_snapshot_with_takeback_disabled()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 0)?;
    session.interrupt_for_save_failure(
        GameSaveFailureRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_nanos(0),
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    assert!(snapshot.is_save_required());
    assert_eq!(snapshot.takeback_target(), None);
    assert_eq!(snapshot.board().turn(), BoardEnumColor::Black);
    assert!(!snapshot.clock().is_running());
    Ok(())
}

#[test]
fn snapshot_keeps_live_clocks_and_moves_separate_from_historical_board_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    let root = session.live();
    let initial = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    assert_eq!(initial.root(), root);
    assert_eq!(initial.takeback_target(), None);
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    session.select_view(root)?;
    let revision = session.revision();
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_millis(2_000)?)?;
    assert_eq!(snapshot.revision(), revision);
    assert_eq!(snapshot.board().turn(), BoardEnumColor::White);
    assert_eq!(snapshot.live_turn(), BoardEnumColor::Black);
    assert!(!snapshot.is_viewing_live());
    assert_eq!(snapshot.root(), root);
    assert_eq!(snapshot.takeback_target(), Some(root));
    assert_eq!(snapshot.moves().len(), 1);
    assert_eq!(snapshot.moves()[0].san(), "e4");
    assert_eq!(snapshot.moves()[0].number(), 1);
    assert_eq!(snapshot.moves()[0].mover(), BoardEnumColor::White);
    assert_eq!(
        snapshot
            .clock()
            .view(ClockInstant::from_millis(2_000)?)?
            .side(BoardEnumColor::Black)
            .remaining(),
        Some(ClockDuration::from_millis(599_000)?)
    );
    assert!(session.clock().is_running());
    assert_eq!(session.revision(), revision);
    GameTransitionFixture::play(&mut session, 3, "e7e5", 3_000)?;
    assert_eq!(snapshot.moves().len(), 1);
    assert_eq!(snapshot.live(), GameTransitionFixture::id(2)?);
    assert_eq!(snapshot.view(), root);
    Ok(())
}

#[test]
fn snapshot_exposes_native_premove_and_consent_state_without_submitting_actions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut engine =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    engine.set_premoves_enabled(engine.revision(), true)?;
    GameTransitionFixture::play(&mut engine, 2, "e2e4", 1_000)?;
    let intent = GamePremoveIntent::new("g1".parse()?, "f3".parse()?, None)?;
    engine.queue_premove(engine.revision(), intent)?;
    let snapshot = DesktopGameSnapshot::capture(&engine, ClockInstant::from_millis(2_000)?)?;
    assert_eq!(snapshot.premove(), Some(intent));
    assert!(snapshot.are_premoves_enabled());
    assert!(snapshot.takeback_pending().is_none());

    let mut local = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play(&mut local, 2, "e2e4", 1_000)?;
    local.request_takeback(
        GameTakebackRequest {
            expected_revision: local.revision(),
            requester: BoardEnumColor::White,
            received_at: ClockInstant::from_millis(2_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let snapshot = DesktopGameSnapshot::capture(&local, ClockInstant::from_millis(3_000)?)?;
    assert_eq!(snapshot.takeback_pending(), local.takeback().pending());
    assert!(local.clock().is_running());
    Ok(())
}
