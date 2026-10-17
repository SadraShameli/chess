use chess::clock::duration::ClockInstant;
use chess::clock::time_control::ClockTimeControl;
use chess::game::annotations::GameAnnotationsEnumVariationRequest;
use chess::game::enums::{GameEnumMarkingColor, GameEnumPhase, GameRevisionError};
use chess::game::move_node::{GameMoveNodeMarking, GameMoveNodeNag};
use chess::game::pause::{GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseRequest};
use chess::game::save::{GameSaveError, GameSaveFailureRequest};
use chess::game::session::{GameSession, GameSessionError};
use chess::game::tree::GameTreeError;

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

struct GameAnnotationsFixture;

impl GameAnnotationsFixture {
    fn completed() -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session =
            GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
        for (id, movement, millis) in [
            (2, "f2f3", 1_000),
            (3, "e7e5", 2_000),
            (4, "g2g4", 3_000),
            (5, "d8h4", 4_000),
        ] {
            GameTransitionFixture::play(&mut session, id, movement, millis)?;
        }
        assert_eq!(session.phase(), GameEnumPhase::Completed);
        Ok(session)
    }

    fn append(
        session: &mut GameSession,
        parent: u128,
        id: u128,
        uci: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        session.select_view(GameTransitionFixture::id(parent)?)?;
        let movement = session
            .tree()
            .node(session.view())?
            .position()
            .parse_uci(uci)?;
        session.append_study_variation(
            session.revision(),
            GameTransitionFixture::id(id)?,
            movement,
        )?;
        Ok(())
    }

    fn branched() -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session = Self::completed()?;
        Self::append(&mut session, 1, 10, "d2d4")?;
        Self::append(&mut session, 10, 11, "d7d5")?;
        Self::append(&mut session, 10, 12, "c7c5")?;
        Self::append(&mut session, 12, 13, "g1f3")?;
        Self::append(&mut session, 1, 14, "e2e4")?;
        session.return_to_live();
        Ok(session)
    }
}

#[test]
fn explicit_saves_preserve_counterpart_and_live_state_and_skip_noop_revisions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameAnnotationsFixture::completed()?;
    let root = session.tree().root();
    let state = session.state();
    let clock = session.clock().clone();
    let history = session.live_moves()?;
    let live = session.live();
    let study = session.study_mainline_tip();
    let revision = session.revision();
    let markings = vec![GameMoveNodeMarking::circle(
        GameEnumMarkingColor::Green,
        "e4".parse()?,
    )];
    assert_eq!(
        session.save_markings(revision, root, markings.clone())?,
        revision.next()?
    );
    let notes = "Plain <b>text</b>\nSecond line".to_owned();
    let nags = vec![GameMoveNodeNag::new(1), GameMoveNodeNag::new(146)];
    let revision = session.revision();
    assert_eq!(
        session.save_notes_and_nags(revision, root, notes.clone(), nags.clone())?,
        revision.next()?
    );
    let revision = session.revision();
    assert_eq!(
        session.save_notes_and_nags(revision, root, notes.clone(), nags.clone())?,
        revision
    );
    assert_eq!(
        session.save_markings(revision, root, markings.clone())?,
        revision
    );
    assert_eq!(
        session.tree().node(root)?.annotations().markings(),
        markings
    );
    session.save_markings(revision, root, Vec::new())?;
    assert_eq!(session.tree().node(root)?.annotations().notes(), notes);
    assert_eq!(session.tree().node(root)?.annotations().nags(), nags);
    assert_eq!(session.state(), state);
    assert_eq!(session.clock(), &clock);
    assert_eq!(session.live_moves()?, history);
    assert_eq!(
        (session.live(), session.view(), session.study_mainline_tip()),
        (live, live, study)
    );
    Ok(())
}

#[test]
fn invalid_annotations_nodes_and_stale_revisions_are_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameAnnotationsFixture::completed()?;
    let revision = session.revision();
    let root = session.tree().root();
    assert!(
        session
            .save_notes_and_nags(revision, root, "nul\0text".to_owned(), Vec::new())
            .is_err()
    );
    assert!(matches!(
        session.save_markings(revision, GameTransitionFixture::id(99)?, Vec::new()),
        Err(GameSessionError::Tree(GameTreeError::UnknownNode))
    ));
    assert!(matches!(
        session.save_notes_and_nags(revision.next()?, root, String::new(), Vec::new()),
        Err(GameSessionError::Revision(GameRevisionError::Stale { .. }))
    ));
    assert_eq!(session.revision(), revision);
    assert!(session.tree().node(root)?.annotations().notes().is_empty());
    Ok(())
}

#[test]
fn setup_active_and_paused_sessions_refuse_all_annotation_edit_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let active = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let setup = GameSession::new(active.restoration(ClockInstant::from_nanos(0))?.session)?;
    let mut paused = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    paused.pause(
        GamePauseRequest {
            expected_revision: paused.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_nanos(0),
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    for mut session in [setup, active, paused] {
        let phase = session.phase();
        let revision = session.revision();
        let root = session.tree().root();
        assert!(
            matches!(session.save_notes_and_nags(revision, root, String::new(), Vec::new()), Err(GameSessionError::WrongPhase(actual)) if actual == phase)
        );
        assert!(
            matches!(session.save_markings(revision, root, Vec::new()), Err(GameSessionError::WrongPhase(actual)) if actual == phase)
        );
        for request in [
            GameAnnotationsEnumVariationRequest::Promote { tip: root },
            GameAnnotationsEnumVariationRequest::Delete { root },
        ] {
            assert!(
                matches!(session.preview_variation(revision, request), Err(GameSessionError::WrongPhase(actual)) if actual == phase)
            );
        }
        assert_eq!(session.revision(), revision);
    }
    Ok(())
}

#[test]
fn promotion_preview_is_readonly_and_confirmation_preserves_played_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameAnnotationsFixture::branched()?;
    let revision = session.revision();
    let live = session.live();
    let clock = session.clock().clone();
    let state = session.state();
    let history = session.live_moves()?;
    let tip = GameTransitionFixture::id(13)?;
    let preview = session.preview_variation(
        revision,
        GameAnnotationsEnumVariationRequest::Promote { tip },
    )?;
    assert_eq!(preview.game(), session.id());
    assert_eq!(preview.revision(), revision);
    assert_eq!(preview.variation().previous_tip(), live);
    assert_eq!(
        preview.variation().selected_path(),
        Some(
            [
                GameTransitionFixture::id(1)?,
                GameTransitionFixture::id(10)?,
                GameTransitionFixture::id(12)?,
                tip
            ]
            .as_slice()
        )
    );
    assert_eq!(session.study_mainline_tip(), live);
    assert_eq!(session.revision(), revision);
    assert_eq!(
        session.apply_variation(preview.confirm())?,
        revision.next()?
    );
    assert_eq!(session.study_mainline_tip(), tip);
    assert_eq!(
        session.tree().node(session.tree().root())?.children(),
        &[
            GameTransitionFixture::id(10)?,
            GameTransitionFixture::id(2)?,
            GameTransitionFixture::id(14)?
        ]
    );
    assert_eq!(
        session
            .tree()
            .node(GameTransitionFixture::id(10)?)?
            .children(),
        &[
            GameTransitionFixture::id(12)?,
            GameTransitionFixture::id(11)?
        ]
    );
    assert_eq!(session.live(), live);
    assert_eq!(session.view(), live);
    assert_eq!(session.clock(), &clock);
    assert_eq!(session.state(), state);
    assert_eq!(session.live_moves()?, history);
    let revision = session.revision();
    let noop = session.preview_variation(
        revision,
        GameAnnotationsEnumVariationRequest::Promote { tip },
    )?;
    assert_eq!(session.apply_variation(noop.confirm())?, revision);
    let restored = GameSession::restore(session.restoration(ClockInstant::from_millis(8_000)?)?)?;
    assert_eq!(restored.study_mainline_tip(), tip);
    assert_eq!(restored.outcome(), session.outcome());
    Ok(())
}

#[test]
fn deletion_previews_preorder_scope_replaces_tip_and_repairs_only_removed_view()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameAnnotationsFixture::branched()?;
    let root = GameTransitionFixture::id(10)?;
    let tip = GameTransitionFixture::id(13)?;
    let promote = session.preview_variation(
        session.revision(),
        GameAnnotationsEnumVariationRequest::Promote { tip },
    )?;
    session.apply_variation(promote.confirm())?;
    session.select_view(tip)?;
    let revision = session.revision();
    let preview = session.preview_variation(
        revision,
        GameAnnotationsEnumVariationRequest::Delete { root },
    )?;
    assert_eq!(
        preview.variation().deleted_nodes(),
        &[
            root,
            GameTransitionFixture::id(12)?,
            tip,
            GameTransitionFixture::id(11)?
        ]
    );
    assert_eq!(preview.variation().parent(), Some(session.tree().root()));
    assert_eq!(preview.variation().resulting_tip(), session.live());
    assert_eq!(session.tree().len(), 10);
    assert_eq!(
        session.apply_variation(preview.confirm())?,
        revision.next()?
    );
    assert_eq!(session.view(), session.tree().root());
    assert_eq!(session.study_mainline_tip(), session.live());
    assert_eq!(session.tree().len(), 6);
    let preview = session.preview_variation(
        session.revision(),
        GameAnnotationsEnumVariationRequest::Delete {
            root: GameTransitionFixture::id(14)?,
        },
    )?;
    session.select_view(session.live())?;
    session.apply_variation(preview.confirm())?;
    assert_eq!(session.view(), session.live());
    GameSession::restore(session.restoration(ClockInstant::from_millis(9_000)?)?)?;
    Ok(())
}

#[test]
fn session_rejects_root_live_path_and_interior_tip_and_stale_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameAnnotationsFixture::branched()?;
    for root in [
        session.tree().root(),
        GameTransitionFixture::id(2)?,
        session.live(),
    ] {
        assert!(
            session
                .preview_variation(
                    session.revision(),
                    GameAnnotationsEnumVariationRequest::Delete { root }
                )
                .is_err()
        );
    }
    assert!(matches!(
        session.preview_variation(
            session.revision(),
            GameAnnotationsEnumVariationRequest::Promote {
                tip: GameTransitionFixture::id(10)?
            }
        ),
        Err(GameSessionError::Tree(GameTreeError::InteriorStudyTip))
    ));
    let preview = session.preview_variation(
        session.revision(),
        GameAnnotationsEnumVariationRequest::Delete {
            root: GameTransitionFixture::id(10)?,
        },
    )?;
    session.save_notes_and_nags(
        session.revision(),
        session.live(),
        "Saved later".to_owned(),
        Vec::new(),
    )?;
    let revision = session.revision();
    assert!(matches!(
        session.apply_variation(preview.confirm()),
        Err(GameSessionError::Revision(GameRevisionError::Stale { .. }))
    ));
    assert_eq!(session.revision(), revision);
    assert!(session.tree().node(GameTransitionFixture::id(10)?).is_ok());
    Ok(())
}

#[test]
fn confirmation_is_bound_to_actual_game_identity() -> Result<(), Box<dyn std::error::Error>> {
    let source = GameAnnotationsFixture::branched()?;
    let preview = source.preview_variation(
        source.revision(),
        GameAnnotationsEnumVariationRequest::Delete {
            root: GameTransitionFixture::id(10)?,
        },
    )?;
    let mut options = source.restoration(ClockInstant::from_millis(8_000)?)?;
    options.session.id = "223e4567-e89b-42d3-a456-426614174000".parse()?;
    let mut foreign = GameSession::restore(options)?;
    let revision = foreign.revision();
    assert!(foreign.apply_variation(preview.confirm()).is_err());
    assert_eq!(foreign.revision(), revision);
    assert_eq!(foreign.tree().len(), source.tree().len());
    Ok(())
}

#[test]
fn analysis_deleting_last_study_continuation_restores_leaf_tip_without_changing_outcome()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameAnnotationsFixture::completed()?;
    let outcome = session.outcome();
    let live = session.live();
    session.begin_analysis(session.revision(), ClockInstant::from_millis(5_000)?)?;
    let root = session.tree().root();
    GameAnnotationsFixture::append(&mut session, 1, 10, "d2d4")?;
    let tip = GameTransitionFixture::id(10)?;
    let preview = session.preview_variation(
        session.revision(),
        GameAnnotationsEnumVariationRequest::Promote { tip },
    )?;
    session.apply_variation(preview.confirm())?;
    GameAnnotationsFixture::append(&mut session, 10, 11, "d7d5")?;
    let child = GameTransitionFixture::id(11)?;
    assert_eq!(session.study_mainline_tip(), child);
    let revision = session.revision();
    session.save_notes_and_nags(revision, root, "Analysis notes".to_owned(), Vec::new())?;
    session.save_markings(
        session.revision(),
        root,
        vec![GameMoveNodeMarking::circle(
            GameEnumMarkingColor::Red,
            "d4".parse()?,
        )],
    )?;
    let preview = session.preview_variation(
        session.revision(),
        GameAnnotationsEnumVariationRequest::Delete { root: child },
    )?;
    assert_eq!(preview.variation().resulting_tip(), tip);
    session.apply_variation(preview.confirm())?;
    assert_eq!(session.study_mainline_tip(), tip);
    assert_eq!(session.view(), tip);
    assert_eq!(session.live(), live);
    assert_eq!(session.outcome(), outcome);
    assert_eq!(session.phase(), GameEnumPhase::Analysis);
    GameSession::restore(session.restoration(ClockInstant::from_millis(9_000)?)?)?;
    Ok(())
}

#[test]
fn save_gate_and_revision_exhaustion_precede_any_content_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameAnnotationsFixture::branched()?;
    let preview = session.preview_variation(
        session.revision(),
        GameAnnotationsEnumVariationRequest::Delete {
            root: GameTransitionFixture::id(10)?,
        },
    )?;
    session.interrupt_for_save_failure(
        GameSaveFailureRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(8_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let revision = session.revision();
    assert!(matches!(
        session.save_markings(revision, session.live(), Vec::new()),
        Err(GameSessionError::Save(GameSaveError::SaveRequired))
    ));
    assert!(matches!(
        session.save_notes_and_nags(revision, session.live(), String::new(), Vec::new()),
        Err(GameSessionError::Save(GameSaveError::SaveRequired))
    ));
    assert!(matches!(
        session.preview_variation(
            revision,
            GameAnnotationsEnumVariationRequest::Promote {
                tip: session.live()
            }
        ),
        Err(GameSessionError::Save(GameSaveError::SaveRequired))
    ));
    assert!(session.apply_variation(preview.confirm()).is_err());
    let source = GameAnnotationsFixture::branched()?;
    let mut options = source.restoration(ClockInstant::from_millis(9_000)?)?;
    options.revision = u64::MAX.to_string().parse()?;
    let mut exhausted = GameSession::restore(options)?;
    let revision = exhausted.revision();
    assert_eq!(
        exhausted.save_markings(revision, exhausted.live(), Vec::new())?,
        revision
    );
    assert!(matches!(
        exhausted.save_notes_and_nags(revision, exhausted.live(), "New".to_owned(), Vec::new()),
        Err(GameSessionError::Revision(GameRevisionError::Exhausted))
    ));
    let preview = exhausted.preview_variation(
        revision,
        GameAnnotationsEnumVariationRequest::Delete {
            root: GameTransitionFixture::id(10)?,
        },
    )?;
    assert!(matches!(
        exhausted.apply_variation(preview.confirm()),
        Err(GameSessionError::Revision(GameRevisionError::Exhausted))
    ));
    assert_eq!(exhausted.tree().len(), source.tree().len());
    Ok(())
}
