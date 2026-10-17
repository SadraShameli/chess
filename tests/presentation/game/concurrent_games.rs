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
    assert_eq!(rows[0].tab_label(), "Game 1");
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
