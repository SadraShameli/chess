use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::clock::duration::ClockInstant;
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::engine_commands::DesktopEngineCommandsSelectionProvider;
use chess::desktop::engine_selection::DesktopEngineSelection;
use chess::desktop::game_snapshot::{DesktopGameSnapshot, DesktopGameSnapshotEnumAnalysis};
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::position::EngineSearchPosition;
use chess::game::enums::{GameEnumMode, GameEnumSeat, GameMoveId};
use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use tokio::sync::watch;

use crate::game_transition_fixture::GameTransitionFixture;

type DesktopEngineSelectionTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct DesktopEngineSelectionFixture;

impl DesktopEngineSelectionFixture {
    fn session(start: BoardStart) -> DesktopEngineSelectionTestResult<GameSession> {
        let mut session = GameSession::new(GameSessionOptions {
            id: "123e4567-e89b-42d3-a456-426614174000".parse()?,
            root: GameTransitionFixture::id(1)?,
            clock: ClockState::new(ClockTimeControl::untimed(), start.position().turn()),
            start,
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            metadata: GameMetadata::default(),
        })?;
        session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
        Ok(session)
    }

    fn append(
        session: &mut GameSession,
        id: u128,
        uci: &str,
    ) -> DesktopEngineSelectionTestResult<GameMoveId> {
        let movement = session
            .tree()
            .node(session.view())?
            .position()
            .parse_uci(uci)?;
        Ok(session
            .append_study_variation(session.revision(), GameTransitionFixture::id(id)?, movement)?
            .view())
    }

    fn snapshot(session: &GameSession) -> DesktopEngineSelectionTestResult<DesktopGameSnapshot> {
        Ok(DesktopGameSnapshot::capture(
            session,
            ClockInstant::from_nanos(0),
        )?)
    }

    fn context(
        snapshot: &DesktopGameSnapshot,
    ) -> DesktopEngineSelectionTestResult<EngineSearchContext> {
        Ok(EngineSearchContext::new(
            EngineSearchGameId::try_from(snapshot.id().as_uuid())?,
            EngineSearchRevision::new(snapshot.revision().value()),
        ))
    }
}

#[test]
fn selected_variation_uses_original_position_and_complete_uci_path()
-> DesktopEngineSelectionTestResult {
    let mut session = DesktopEngineSelectionFixture::session(BoardStart::standard())?;
    let root = session.view();
    DesktopEngineSelectionFixture::append(&mut session, 2, "e2e4")?;
    DesktopEngineSelectionFixture::append(&mut session, 3, "e7e5")?;
    session.select_view(root)?;
    DesktopEngineSelectionFixture::append(&mut session, 4, "d2d4")?;
    DesktopEngineSelectionFixture::append(&mut session, 5, "d7d5")?;
    let snapshot = DesktopEngineSelectionFixture::snapshot(&session)?;
    assert!(snapshot.moves().is_empty());
    let DesktopGameSnapshotEnumAnalysis::Available(projection) = snapshot.analysis() else {
        return Err("Missing analysis projection".into());
    };
    assert_eq!(projection.node(), session.view());
    assert_eq!(
        projection.position().command(),
        "position startpos moves d2d4 d7d5"
    );
    let (_snapshot_sender, snapshots) = watch::channel(vec![snapshot.clone()]);
    let (_selected_sender, selected) = watch::channel(Some(snapshot.id()));
    let selection = DesktopEngineSelection::new(snapshots, selected);
    selection.authorize(
        DesktopEngineSelectionFixture::context(&snapshot)?,
        projection.position(),
    )?;
    let fen_only = EngineSearchPosition::new(
        projection.position().current().clone(),
        std::iter::empty::<&str>(),
    )?;
    assert!(
        selection
            .authorize(
                DesktopEngineSelectionFixture::context(&snapshot)?,
                &fen_only
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn fresh_snapshot_rejects_old_revision_selection_and_same_revision_view_change()
-> DesktopEngineSelectionTestResult {
    let mut session = DesktopEngineSelectionFixture::session(BoardStart::standard())?;
    let root = session.view();
    DesktopEngineSelectionFixture::append(&mut session, 2, "e2e4")?;
    let old = DesktopEngineSelectionFixture::snapshot(&session)?;
    let position = match old.analysis() {
        DesktopGameSnapshotEnumAnalysis::Available(value) => value.position().clone(),
        DesktopGameSnapshotEnumAnalysis::Unavailable(_) => return Err("Missing projection".into()),
    };
    let (sender, snapshots) = watch::channel(vec![old.clone()]);
    let (selected_sender, selected) = watch::channel(Some(old.id()));
    let selection = DesktopEngineSelection::new(snapshots, selected);
    session.select_view(root)?;
    sender.send_replace(vec![DesktopEngineSelectionFixture::snapshot(&session)?]);
    assert!(
        selection
            .authorize(DesktopEngineSelectionFixture::context(&old)?, &position)
            .is_err()
    );
    DesktopEngineSelectionFixture::append(&mut session, 3, "d2d4")?;
    sender.send_replace(vec![DesktopEngineSelectionFixture::snapshot(&session)?]);
    assert!(
        selection
            .authorize_context(DesktopEngineSelectionFixture::context(&old)?)
            .is_err()
    );
    assert!(selected_sender.send_replace(None).is_some());
    assert!(
        selection
            .authorize_context(DesktopEngineSelectionFixture::context(&old)?)
            .is_err()
    );
    Ok(())
}

#[test]
fn live_game_history_never_authorizes_assistance() -> DesktopEngineSelectionTestResult {
    let session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let snapshot = DesktopEngineSelectionFixture::snapshot(&session)?;
    let (_snapshot_sender, snapshots) = watch::channel(vec![snapshot.clone()]);
    let (_selected_sender, selected) = watch::channel(Some(snapshot.id()));
    let selection = DesktopEngineSelection::new(snapshots, selected);
    assert!(
        selection
            .authorize_context(DesktopEngineSelectionFixture::context(&snapshot)?)
            .is_err()
    );
    assert!(matches!(
        snapshot.analysis(),
        DesktopGameSnapshotEnumAnalysis::Unavailable(_)
    ));
    Ok(())
}

#[test]
fn closed_authoritative_sources_refuse_their_last_snapshot() -> DesktopEngineSelectionTestResult {
    let session = DesktopEngineSelectionFixture::session(BoardStart::standard())?;
    let snapshot = DesktopEngineSelectionFixture::snapshot(&session)?;
    let (sender, snapshots) = watch::channel(vec![snapshot.clone()]);
    let (_selected_sender, selected) = watch::channel(Some(snapshot.id()));
    let selection = DesktopEngineSelection::new(snapshots, selected);
    selection.authorize_context(DesktopEngineSelectionFixture::context(&snapshot)?)?;
    drop(sender);
    assert!(
        selection
            .authorize_context(DesktopEngineSelectionFixture::context(&snapshot)?)
            .is_err()
    );
    Ok(())
}

#[test]
fn custom_castling_and_randomized_starts_preserve_variant_and_root()
-> DesktopEngineSelectionTestResult {
    for start in [
        BoardStart::custom(BoardPosition::from_fen(
            "4k3/8/8/8/8/8/8/5K1R w H - 0 1",
            BoardEnumVariant::Chess960,
        )?),
        BoardStart::chess960(42_u16.try_into()?)?,
        BoardStart::dfrc(42_u16.try_into()?, 901_u16.try_into()?)?,
    ] {
        let initial = start.position().clone();
        let mut session = DesktopEngineSelectionFixture::session(start)?;
        let movement = if initial.parse_uci("f1h1").is_ok() {
            "f1h1".to_owned()
        } else {
            initial
                .legal_moves()
                .first()
                .ok_or("No move")?
                .uci(initial.variant())
        };
        DesktopEngineSelectionFixture::append(&mut session, 2, &movement)?;
        let snapshot = DesktopEngineSelectionFixture::snapshot(&session)?;
        let DesktopGameSnapshotEnumAnalysis::Available(projection) = snapshot.analysis() else {
            return Err("Missing projection".into());
        };
        assert_eq!(projection.position().initial(), &initial);
        assert_eq!(projection.position().history().len(), 1);
        assert!(projection.position().command().ends_with(&movement));
    }
    Ok(())
}

#[test]
fn oversized_history_keeps_the_ordinary_game_snapshot_available() -> DesktopEngineSelectionTestResult
{
    let mut session = DesktopEngineSelectionFixture::session(BoardStart::standard())?;
    for (index, uci) in ["g1f3", "g8f6", "f3g1", "f6g8"]
        .into_iter()
        .cycle()
        .take(13_200)
        .enumerate()
    {
        DesktopEngineSelectionFixture::append(&mut session, u128::try_from(index)? + 2, uci)?;
    }
    let snapshot = DesktopEngineSelectionFixture::snapshot(&session)?;
    assert_eq!(snapshot.view(), session.view());
    assert!(matches!(
        snapshot.analysis(),
        DesktopGameSnapshotEnumAnalysis::Unavailable(_)
    ));
    Ok(())
}
