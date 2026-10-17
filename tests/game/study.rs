use chess::board::enums::BoardEnumColor;
use chess::clock::duration::ClockInstant;
use chess::clock::time_control::ClockTimeControl;
use chess::game::annotations::GameAnnotationsEnumChange;
use chess::game::pause::GamePauseEnumEngineBarrier;
use chess::game::result::GameResultResignRequest;
use chess::game::session::{GameSession, GameSessionError};

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

struct GameStudyFixture;

impl GameStudyFixture {
    fn analysis() -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
        session.resign(
            GameResultResignRequest {
                expected_revision: session.revision(),
                resigning: BoardEnumColor::White,
                received_at: ClockInstant::from_nanos(0),
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &GameTransitionFixtureProof,
        )?;
        session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
        Ok(session)
    }
}

#[test]
fn study_continuations_preserve_played_line_result_and_existing_clocks()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let root = session.live();
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    let played = session.live();
    let before_clock = session.tree().node(played)?.before_clock().cloned();
    let after_clock = session.tree().node(played)?.clock().cloned();
    session.resign(
        GameResultResignRequest {
            expected_revision: session.revision(),
            resigning: BoardEnumColor::Black,
            received_at: ClockInstant::from_millis(2_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let outcome = session.outcome();
    session.select_view(root)?;
    let movement = session.tree().node(root)?.position().parse_uci("d2d4")?;
    let applied = session.append_study_variation(
        session.revision(),
        GameTransitionFixture::id(3)?,
        movement,
    )?;
    assert_eq!(applied.change(), GameAnnotationsEnumChange::Changed);
    assert_eq!(applied.mainline_tip(), session.study_mainline_tip());
    let branch = applied.view();
    assert_eq!(session.view(), branch);
    assert_eq!(session.live(), played);
    assert_eq!(session.outcome(), outcome);
    assert_eq!(session.tree().node(root)?.children(), &[played, branch]);
    session.select_view(root)?;
    let movement = session.tree().node(root)?.position().parse_uci("e2e4")?;
    assert_eq!(
        session
            .append_study_variation(session.revision(), GameTransitionFixture::id(4)?, movement)?
            .view(),
        played
    );
    assert_eq!(session.tree().len(), 3);
    assert_eq!(
        session.tree().node(played)?.before_clock(),
        before_clock.as_ref()
    );
    assert_eq!(session.tree().node(played)?.clock(), after_clock.as_ref());
    Ok(())
}

#[test]
fn reusing_a_study_continuation_reports_no_content_change_while_selecting_its_real_view()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameStudyFixture::analysis()?;
    let root = session.tree().root();
    let movement = session.tree().node(root)?.position().parse_uci("e2e4")?;
    let inserted = session.append_study_variation(
        session.revision(),
        GameTransitionFixture::id(2)?,
        movement,
    )?;
    assert_eq!(inserted.change(), GameAnnotationsEnumChange::Changed);
    let existing = inserted.view();
    session.select_view(root)?;
    let revision = session.revision();
    let reused =
        session.append_study_variation(revision, GameTransitionFixture::id(3)?, movement)?;
    assert_eq!(reused.change(), GameAnnotationsEnumChange::Unchanged);
    assert_eq!(reused.view(), existing);
    assert_eq!(reused.mainline_tip(), existing);
    assert_eq!(session.tree().len(), 2);
    assert_eq!(session.revision(), revision.next()?);
    assert_eq!(session.view(), existing);
    Ok(())
}

#[test]
fn stale_study_reuse_preserves_selected_view_tip_and_existing_annotations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameStudyFixture::analysis()?;
    let root = session.tree().root();
    let movement = session.tree().node(root)?.position().parse_uci("e2e4")?;
    let inserted = session
        .append_study_variation(session.revision(), GameTransitionFixture::id(2)?, movement)?
        .view();
    let stale = session.revision();
    session.save_notes_and_nags(stale, inserted, "kept".to_owned(), Vec::new())?;
    session.select_view(root)?;
    let revision = session.revision();
    assert_ne!(revision, stale);
    assert!(matches!(
        session.append_study_variation(stale, GameTransitionFixture::id(3)?, movement),
        Err(GameSessionError::Revision(_))
    ));
    assert_eq!(session.view(), root);
    assert_eq!(session.study_mainline_tip(), inserted);
    assert_eq!(session.tree().len(), 2);
    assert_eq!(session.revision(), revision);
    assert_eq!(session.tree().node(inserted)?.annotations().notes(), "kept");
    assert_eq!(session.live(), root);
    assert!(session.live_moves()?.is_empty());
    Ok(())
}

#[test]
fn a_study_action_cannot_submit_a_running_games_move_or_reuse_a_stale_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let revision = session.revision();
    let movement = session
        .tree()
        .node(session.view())?
        .position()
        .parse_uci("e2e4")?;
    assert!(
        session
            .append_study_variation(revision, GameTransitionFixture::id(2)?, movement)
            .is_err()
    );
    assert_eq!(session.revision(), revision);
    assert_eq!(session.tree().len(), 1);
    Ok(())
}

#[test]
fn premove_configuration_advances_revision_and_rejects_stale_settings()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    let previous = session.revision();
    assert_eq!(
        session.set_premoves_enabled(previous, true)?,
        previous.next()?
    );
    assert!(session.premove().is_enabled());
    assert!(session.set_premoves_enabled(previous, false).is_err());
    assert!(session.premove().is_enabled());
    Ok(())
}
