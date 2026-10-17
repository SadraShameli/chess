use chess::clock::duration::ClockInstant;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::game_snapshot::{DesktopGameSnapshot, DesktopGameSnapshotEnumAdmission};
use chess::game::pause::{GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseRequest};
use chess::presentation::game::overview::{
    PresentationGameOverview, PresentationGameOverviewEnumStatus,
};

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

type PresentationConcurrentGamesResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn overview_rows_keep_stable_ordinals_turns_clocks_and_running_status()
-> PresentationConcurrentGamesResult {
    let mut first = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play(&mut first, 2, "e2e4", 1_000)?;
    let second = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    let now = ClockInstant::from_millis(5_000)?;
    let snapshots = [
        DesktopGameSnapshot::capture(&first, now)?,
        DesktopGameSnapshot::capture(&second, now)?,
    ];
    let rows = PresentationGameOverview::rows(snapshots.iter(), now)?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].ordinal(), 1);
    assert_eq!(rows[1].ordinal(), 2);
    assert_eq!(rows[0].id(), snapshots[0].id());
    assert_eq!(
        rows[0].status(),
        PresentationGameOverviewEnumStatus::Running
    );
    assert!(rows[0].is_unsaved());
    assert_eq!(rows[0].tab_label(), "Game 1 *");
    assert_ne!(rows[0].turn(), rows[1].turn());
    assert!(rows[0].resources().is_none());
    assert_eq!(
        snapshots[0].admission(),
        DesktopGameSnapshotEnumAdmission::Admitted
    );
    Ok(())
}

#[test]
fn a_paused_game_with_unsaved_changes_is_marked_in_its_tab_and_summary()
-> PresentationConcurrentGamesResult {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(1_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let now = ClockInstant::from_millis(9_000)?;
    let snapshot = DesktopGameSnapshot::capture(&session, now)?;
    let row = PresentationGameOverview::row(3, &snapshot, now)?;
    assert_eq!(row.status(), PresentationGameOverviewEnumStatus::Paused);
    assert!(row.is_unsaved());
    assert_eq!(row.tab_label(), "Game 3 *");
    let summary = row.summary();
    assert!(summary.contains("Paused"));
    assert!(summary.contains("unsaved changes"));
    Ok(())
}

struct PresentationConcurrentGamesClock;

impl chess::clock::time_provider::ClockTimeProvider for PresentationConcurrentGamesClock {
    fn now(&self) -> Result<ClockInstant, chess::clock::duration::ClockDurationError> {
        ClockInstant::from_millis(5_000)
    }
}

fn second_session() -> Result<chess::game::session::GameSession, Box<dyn std::error::Error>> {
    use chess::board::start::BoardStart;
    use chess::clock::state::ClockState;
    use chess::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameEnumSeat};
    use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
    let start = BoardStart::standard();
    let mut session = GameSession::new(GameSessionOptions {
        id: "123e4567-e89b-42d3-a456-426614174099".parse()?,
        root: GameTransitionFixture::id(901)?,
        clock: ClockState::new(ClockTimeControl::initial_default(), start.position().turn()),
        start,
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
        GameEnumEngineReadiness::NotRequired,
        &GameTransitionFixtureProof,
    )?;
    Ok(session)
}

#[test]
fn each_tab_keeps_its_own_history_cursor_and_revision_when_snapshots_interleave()
-> PresentationConcurrentGamesResult {
    use chess::presentation::app::PresentationApp;
    let mut first = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play(&mut first, 2, "e2e4", 1_000)?;
    GameTransitionFixture::play(&mut first, 3, "e7e5", 2_000)?;
    let root = first.tree().root();
    first.select_view(root)?;
    let second = second_session()?;
    let now = ClockInstant::from_millis(5_000)?;
    let mut app = PresentationApp::new(std::sync::Arc::new(PresentationConcurrentGamesClock));
    assert!(app.accept_game_snapshot(DesktopGameSnapshot::capture(&first, now)?));
    assert!(app.accept_game_snapshot(DesktopGameSnapshot::capture(&second, now)?));
    let selected = app.game_snapshot().ok_or("missing selected game")?;
    assert_eq!(selected.id(), first.id());
    assert!(!selected.is_viewing_live());
    assert_eq!(selected.revision(), first.revision());
    let stale = DesktopGameSnapshot::capture(&first, ClockInstant::from_millis(4_000)?)?;
    assert!(!app.accept_game_snapshot(stale));
    assert!(
        app.game_snapshot()
            .is_some_and(|game| !game.is_viewing_live())
    );
    Ok(())
}

#[test]
fn a_capacity_waiting_change_reaches_the_tab_even_at_the_same_revision()
-> PresentationConcurrentGamesResult {
    use chess::desktop::game_snapshot::DesktopGameSnapshotAllocation;
    use chess::presentation::app::PresentationApp;
    let session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    let now = ClockInstant::from_millis(5_000)?;
    let mut app = PresentationApp::new(std::sync::Arc::new(PresentationConcurrentGamesClock));
    let base = DesktopGameSnapshot::capture(&session, now)?;
    assert!(app.accept_game_snapshot(base.clone()));
    assert!(!app.accept_game_snapshot(base.clone()));
    let waiting = base
        .clone()
        .with_resources(DesktopGameSnapshotEnumAdmission::WaitingForCapacity, None);
    assert!(app.accept_game_snapshot(waiting.clone()));
    let row = PresentationGameOverview::row(1, &waiting, now)?;
    assert_eq!(
        row.status(),
        PresentationGameOverviewEnumStatus::WaitingForCapacity
    );
    assert!(row.summary().contains("Waiting for engine capacity"));
    let admitted = base.with_resources(
        DesktopGameSnapshotEnumAdmission::Admitted,
        Some(DesktopGameSnapshotAllocation::new(3, 64)),
    );
    assert!(app.accept_game_snapshot(admitted.clone()));
    let row = PresentationGameOverview::row(1, &admitted, now)?;
    assert_eq!(row.resources(), Some("3 threads, 64 MiB hash"));
    Ok(())
}

#[test]
fn closing_the_application_with_open_games_needs_an_explicit_confirmation()
-> PresentationConcurrentGamesResult {
    use chess::presentation::app::PresentationApp;
    let session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    let now = ClockInstant::from_millis(5_000)?;
    let mut app = PresentationApp::new(std::sync::Arc::new(PresentationConcurrentGamesClock));
    assert!(app.accept_game_snapshot(DesktopGameSnapshot::capture(&session, now)?));
    app.request_quit();
    assert!(!app.take_confirmed_quit_request());
    Ok(())
}
