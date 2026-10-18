use std::num::NonZeroU64;
use std::time::Duration;

use chess::board::enums::BoardEnumVariant;
use chess::board::r#move::BoardMove;
use chess::board::position::BoardPosition;
use chess::puzzle::attempt::PuzzleAttemptId;
use chess::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumEvent, PuzzleEnumHint,
    PuzzleEnumHintStage, PuzzleEnumMoveFeedback, PuzzleEnumPhase, PuzzleEnumPractice,
    PuzzleEnumPrimaryOutcome,
};
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::random_provider::{PuzzleRandomProvider, PuzzleRandomProviderSeeded};
use chess::puzzle::session::{
    PuzzleSession, PuzzleSessionConfig, PuzzleSessionContext, PuzzleSessionError, PuzzleSessionId,
    PuzzleSessionRevision,
};

use crate::puzzle_fixture::PuzzleFixture;

struct PuzzleSolutionFixture;

impl PuzzleSolutionFixture {
    fn session(lines: &[&[&str]]) -> Result<PuzzleSession, Box<dyn std::error::Error>> {
        Self::from_position(BoardPosition::standard(), lines)
    }

    fn from_position(
        initial: BoardPosition,
        lines: &[&[&str]],
    ) -> Result<PuzzleSession, Box<dyn std::error::Error>> {
        Ok(PuzzleSession::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleAttemptId::new(NonZeroU64::MIN),
            PuzzleFixture::definition(initial, lines)?,
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
        ))
    }

    fn movement(
        session: &mut PuzzleSession,
        uci: &str,
    ) -> Result<PuzzleEnumMoveFeedback, Box<dyn std::error::Error>> {
        let movement = session.attempt().position().parse_uci(uci)?;
        Ok(session.submit_move(&session.context(), movement)?)
    }

    fn reply(session: &mut PuzzleSession) -> Result<BoardMove, Box<dyn std::error::Error>> {
        Ok(session
            .play_opponent_reply(&session.context(), &mut PuzzleRandomProviderSeeded::new(19))?)
    }
}

#[test]
fn follows_listed_solution_and_enters_review_only_at_the_terminal_node()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["e2e4", "e7e5", "g1f3"]])?;
    assert_eq!(session.attempt().phase(), PuzzleEnumPhase::SolverMove);
    assert_eq!(session.attempt().primary_outcome(), None);
    assert_eq!(
        PuzzleSolutionFixture::movement(&mut session, "e2e4")?,
        PuzzleEnumMoveFeedback::Accepted
    );
    assert_eq!(session.attempt().phase(), PuzzleEnumPhase::OpponentReply);
    assert_eq!(session.attempt().primary_outcome(), None);
    assert_eq!(
        PuzzleSolutionFixture::reply(&mut session)?.uci(BoardEnumVariant::Standard),
        "e7e5"
    );
    assert_eq!(
        PuzzleSolutionFixture::movement(&mut session, "g1f3")?,
        PuzzleEnumMoveFeedback::Completed
    );
    assert_eq!(session.attempt().phase(), PuzzleEnumPhase::Review);
    assert_eq!(
        session.attempt().primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Unassisted
        ))
    );
    assert_eq!(session.attempt().events().len(), 3);
    assert_eq!(session.revision().value(), 3);
    Ok(())
}

#[test]
fn accepts_every_listed_solver_alternative_without_reordering_source_lines()
-> Result<(), Box<dyn std::error::Error>> {
    let lines: &[&[&str]] = &[
        &["e2e4", "e7e5", "g1f3"],
        &["e2e4", "e7e5", "f1c4"],
        &["d2d4", "d7d5", "c2c4"],
    ];
    for line in lines {
        let mut session = PuzzleSolutionFixture::session(lines)?;
        let original = session.attempt().definition().clone();
        PuzzleSolutionFixture::movement(&mut session, line[0])?;
        PuzzleSolutionFixture::reply(&mut session)?;
        assert_eq!(
            PuzzleSolutionFixture::movement(&mut session, line[2])?,
            PuzzleEnumMoveFeedback::Completed
        );
        assert_eq!(session.attempt().definition(), &original);
    }
    Ok(())
}

#[test]
fn transposed_boards_do_not_import_a_move_from_another_listed_path()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[
        &["g1f3", "g8f6", "b1c3", "b8c6", "e2e4"],
        &["b1c3", "b8c6", "g1f3", "g8f6", "d2d4"],
    ])?;
    for movement in ["g1f3", "b1c3"] {
        PuzzleSolutionFixture::movement(&mut session, movement)?;
        PuzzleSolutionFixture::reply(&mut session)?;
    }
    let position = session.attempt().position().clone();
    assert_eq!(
        PuzzleSolutionFixture::movement(&mut session, "d2d4")?,
        PuzzleEnumMoveFeedback::LegalUnlisted
    );
    assert_eq!(session.attempt().position(), &position);
    assert_eq!(
        session.attempt().primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Failed)
    );
    Ok(())
}

#[test]
fn legal_unlisted_move_fails_without_changing_the_position_or_claiming_a_loss()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["e2e4"]])?;
    let position = session.attempt().position().clone();
    let movement = position.parse_uci("d2d4")?;
    assert_eq!(
        session.submit_move(&session.context(), movement)?,
        PuzzleEnumMoveFeedback::LegalUnlisted
    );
    assert_eq!(session.attempt().position(), &position);
    assert_eq!(
        session.attempt().primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Failed)
    );
    assert_eq!(session.attempt().phase(), PuzzleEnumPhase::PracticeReady);
    assert_eq!(
        session.attempt().events(),
        &[PuzzleEnumEvent::LegalUnlisted(movement)]
    );
    Ok(())
}

#[test]
fn illegal_objects_from_other_positions_do_not_mutate_any_attempt_state()
-> Result<(), Box<dyn std::error::Error>> {
    let black = BoardPosition::standard().play_uci("e2e4")?;
    let absent =
        BoardPosition::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1", BoardEnumVariant::Standard)?;
    let impossible = BoardPosition::from_fen(
        "4k3/8/8/8/8/4P3/8/4K3 w - - 0 1",
        BoardEnumVariant::Standard,
    )?;
    let foreign = [
        black.parse_uci("e7e5")?,
        absent.parse_uci("a1a8")?,
        impossible.parse_uci("e3e4")?,
    ];
    for movement in foreign {
        let mut session = PuzzleSolutionFixture::session(&[&["e2e4"]])?;
        let before = session.clone();
        assert!(session.submit_move(&session.context(), movement).is_err());
        assert_eq!(session, before);
    }
    Ok(())
}

#[test]
fn rejects_a_previously_legal_move_that_now_exposes_the_king()
-> Result<(), Box<dyn std::error::Error>> {
    let free = BoardPosition::from_fen(
        "6k1/8/8/8/8/8/4R3/4K3 w - - 0 1",
        BoardEnumVariant::Standard,
    )?;
    let pinned = BoardPosition::from_fen(
        "4r1k1/8/8/8/8/8/4R3/4K3 w - - 0 1",
        BoardEnumVariant::Standard,
    )?;
    let mut session = PuzzleSolutionFixture::from_position(pinned, &[&["e2e8"]])?;
    let before = session.clone();
    assert!(
        session
            .submit_move(&session.context(), free.parse_uci("e2a2")?)
            .is_err()
    );
    assert_eq!(session, before);
    Ok(())
}

#[test]
fn hints_reveal_only_the_first_listed_source_destination_and_exact_move()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["d2d4"], &["e2e4"]])?;
    let movement = session.attempt().position().parse_uci("d2d4")?;
    let expected = [
        (
            PuzzleEnumHint::Source(movement.origin().ok_or("Missing source")?),
            PuzzleEnumHintStage::Source,
        ),
        (
            PuzzleEnumHint::Destination(movement.destination()),
            PuzzleEnumHintStage::Destination,
        ),
        (
            PuzzleEnumHint::NextMove(movement),
            PuzzleEnumHintStage::NextMove,
        ),
    ];
    let position = session.attempt().position().clone();
    for (hint, stage) in expected {
        assert_eq!(session.request_hint(&session.context())?, hint);
        assert_eq!(session.attempt().hint_stage(), stage);
        assert_eq!(
            session.attempt().assistance(),
            PuzzleEnumAssistance::Assisted
        );
        assert_eq!(session.attempt().position(), &position);
        assert_eq!(session.attempt().primary_outcome(), None);
    }
    let before = session.clone();
    assert!(session.request_hint(&session.context()).is_err());
    assert_eq!(session, before);
    assert_eq!(
        PuzzleSolutionFixture::movement(&mut session, "e2e4")?,
        PuzzleEnumMoveFeedback::Completed
    );
    assert_eq!(
        session.attempt().primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Assisted
        ))
    );
    Ok(())
}

#[test]
fn resets_hint_stages_for_each_solver_turn_and_keeps_attempt_wide_assistance()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["e2e4", "e7e5", "g1f3"]])?;
    session.request_hint(&session.context())?;
    session.request_hint(&session.context())?;
    session.request_hint(&session.context())?;
    PuzzleSolutionFixture::movement(&mut session, "e2e4")?;
    PuzzleSolutionFixture::reply(&mut session)?;
    assert_eq!(
        session.attempt().hint_stage(),
        PuzzleEnumHintStage::Unrequested
    );
    assert_eq!(
        session.attempt().assistance(),
        PuzzleEnumAssistance::Assisted
    );
    let next = session.attempt().position().parse_uci("g1f3")?;
    assert_eq!(
        session.request_hint(&session.context())?,
        PuzzleEnumHint::Source(next.origin().ok_or("Missing source")?)
    );
    PuzzleSolutionFixture::movement(&mut session, "g1f3")?;
    assert_eq!(
        session.attempt().primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Assisted
        ))
    );
    Ok(())
}

#[test]
fn failed_then_practice_solved_preserves_failure_and_separate_assistance_events()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["e2e4", "e7e5", "g1f3"]])?;
    PuzzleSolutionFixture::movement(&mut session, "e2e4")?;
    PuzzleSolutionFixture::reply(&mut session)?;
    let failed_position = session.attempt().position().clone();
    PuzzleSolutionFixture::movement(&mut session, "b1c3")?;
    assert_eq!(session.attempt().practice(), PuzzleEnumPractice::NotStarted);
    session.retry_practice(&session.context())?;
    assert_eq!(session.attempt().position(), &failed_position);
    assert_eq!(session.attempt().practice(), PuzzleEnumPractice::InProgress);
    session.request_hint(&session.context())?;
    PuzzleSolutionFixture::movement(&mut session, "g1f3")?;
    assert_eq!(session.attempt().phase(), PuzzleEnumPhase::Review);
    assert_eq!(
        session.attempt().primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Failed)
    );
    assert_eq!(session.attempt().practice(), PuzzleEnumPractice::Completed);
    assert_eq!(
        session.attempt().assistance(),
        PuzzleEnumAssistance::Assisted
    );
    assert!(
        session
            .attempt()
            .events()
            .contains(&PuzzleEnumEvent::PracticeCompleted)
    );
    Ok(())
}

#[test]
fn repeated_unlisted_practice_retries_keep_primary_failure_and_hint_stage()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["e2e4"]])?;
    session.request_hint(&session.context())?;
    for _ in 0..3 {
        PuzzleSolutionFixture::movement(&mut session, "d2d4")?;
        session.retry_practice(&session.context())?;
        assert_eq!(
            session.attempt().primary_outcome(),
            Some(PuzzleEnumPrimaryOutcome::Failed)
        );
        assert_eq!(session.attempt().hint_stage(), PuzzleEnumHintStage::Source);
    }
    PuzzleSolutionFixture::movement(&mut session, "e2e4")?;
    assert_eq!(session.attempt().practice(), PuzzleEnumPractice::Completed);
    Ok(())
}

#[test]
fn every_hint_depth_marks_the_solve_assisted() -> Result<(), Box<dyn std::error::Error>> {
    for count in 1..=3 {
        let mut session = PuzzleSolutionFixture::session(&[&["e2e4"]])?;
        for _ in 0..count {
            session.request_hint(&session.context())?;
        }
        PuzzleSolutionFixture::movement(&mut session, "e2e4")?;
        assert_eq!(
            session.attempt().primary_outcome(),
            Some(PuzzleEnumPrimaryOutcome::Solved(
                PuzzleEnumAssistance::Assisted
            ))
        );
    }
    Ok(())
}

#[test]
fn records_skip_and_timeout_without_overwriting_a_failed_primary_outcome()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        for failed in [false, true] {
            for timeout in [false, true] {
                let mut session = PuzzleSolutionFixture::session(&[&["e2e4"]])?;
                session = PuzzleSession::new(
                    session.id(),
                    session.attempt().id(),
                    session.attempt().definition().clone(),
                    PuzzleSessionConfig::defaults(mode),
                );
                if failed {
                    PuzzleSolutionFixture::movement(&mut session, "d2d4")?;
                }
                let position = session.attempt().position().clone();
                let expected = if failed {
                    PuzzleEnumPrimaryOutcome::Failed
                } else if timeout {
                    PuzzleEnumPrimaryOutcome::TimedOut
                } else {
                    PuzzleEnumPrimaryOutcome::Skipped
                };
                if timeout {
                    session.record_timeout(&session.context())?;
                    assert_eq!(
                        session.attempt().events().last(),
                        Some(&PuzzleEnumEvent::TimedOut(mode))
                    );
                } else {
                    session.skip_attempt(&session.context())?;
                    assert_eq!(
                        session.attempt().events().last(),
                        Some(&PuzzleEnumEvent::Skipped)
                    );
                }
                assert_eq!(session.attempt().primary_outcome(), Some(expected));
                assert_eq!(session.attempt().phase(), PuzzleEnumPhase::Finalized);
                assert_eq!(session.attempt().position(), &position);
                let before = session.clone();
                assert!(session.record_timeout(&session.context()).is_err());
                assert!(session.skip_attempt(&session.context()).is_err());
                assert_eq!(session, before);
            }
        }
    }
    Ok(())
}

#[test]
fn seeded_replies_are_repeatable_and_never_leave_listed_branches()
-> Result<(), Box<dyn std::error::Error>> {
    let lines: &[&[&str]] = &[
        &["e2e4", "e7e5", "g1f3"],
        &["e2e4", "c7c5", "g1f3"],
        &["e2e4", "e7e6", "d2d4"],
    ];
    let mut first_random = PuzzleRandomProviderSeeded::new(927);
    let mut second_random = PuzzleRandomProviderSeeded::new(927);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..48 {
        let mut first = PuzzleSolutionFixture::session(lines)?;
        let mut second = first.clone();
        PuzzleSolutionFixture::movement(&mut first, "e2e4")?;
        PuzzleSolutionFixture::movement(&mut second, "e2e4")?;
        let movement = first.play_opponent_reply(&first.context(), &mut first_random)?;
        assert_eq!(
            movement,
            second.play_opponent_reply(&second.context(), &mut second_random)?
        );
        assert_eq!(first, second);
        let uci = movement.uci(BoardEnumVariant::Standard);
        assert!(["e7e5", "c7c5", "e7e6"].contains(&uci.as_str()));
        seen.insert(uci);
        assert!(
            matches!(first.attempt().events().last(), Some(PuzzleEnumEvent::OpponentReply { movement: recorded, .. }) if *recorded == movement)
        );
    }
    assert_eq!(seen.len(), 3);
    Ok(())
}

struct PuzzleSolutionInvalidRandom;

impl PuzzleRandomProvider for PuzzleSolutionInvalidRandom {
    fn index(&mut self, alternatives: std::num::NonZeroUsize) -> usize {
        alternatives.get()
    }
}

#[test]
fn invalid_random_choice_is_rejected_without_mutating_the_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["e2e4", "e7e5"]])?;
    PuzzleSolutionFixture::movement(&mut session, "e2e4")?;
    let before = session.clone();
    assert!(
        session
            .play_opponent_reply(&session.context(), &mut PuzzleSolutionInvalidRandom)
            .is_err()
    );
    assert_eq!(session, before);
    Ok(())
}

#[test]
fn a_listed_terminal_opponent_reply_completes_without_inventing_a_solver_move()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["e2e4", "e7e5"]])?;
    PuzzleSolutionFixture::movement(&mut session, "e2e4")?;
    PuzzleSolutionFixture::reply(&mut session)?;
    assert_eq!(session.attempt().phase(), PuzzleEnumPhase::Review);
    assert_eq!(
        session.attempt().primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Unassisted
        ))
    );
    Ok(())
}

#[test]
fn a_black_solver_uses_the_definition_turn_and_actual_board_moves()
-> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::standard().play_uci("e2e4")?;
    let mut session = PuzzleSolutionFixture::from_position(initial, &[&["c7c5", "g1f3", "d7d6"]])?;
    PuzzleSolutionFixture::movement(&mut session, "c7c5")?;
    PuzzleSolutionFixture::reply(&mut session)?;
    PuzzleSolutionFixture::movement(&mut session, "d7d6")?;
    assert_eq!(session.attempt().phase(), PuzzleEnumPhase::Review);
    Ok(())
}

#[test]
fn hint_reveal_preserves_the_exact_listed_underpromotion() -> Result<(), Box<dyn std::error::Error>>
{
    let initial =
        BoardPosition::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", BoardEnumVariant::Standard)?;
    let mut session = PuzzleSolutionFixture::from_position(initial, &[&["a7a8n"]])?;
    session.request_hint(&session.context())?;
    session.request_hint(&session.context())?;
    let movement = session.attempt().position().parse_uci("a7a8n")?;
    assert_eq!(
        session.request_hint(&session.context())?,
        PuzzleEnumHint::NextMove(movement)
    );
    assert_eq!(
        session.submit_move(&session.context(), movement)?,
        PuzzleEnumMoveFeedback::Completed
    );
    Ok(())
}

#[test]
fn countdown_configuration_is_exclusive_positive_and_bounded()
-> Result<(), Box<dyn std::error::Error>> {
    for (mode, seconds) in [
        (PuzzleEnumCountdownMode::Session, 300),
        (PuzzleEnumCountdownMode::PerPuzzle, 60),
    ] {
        let defaults = PuzzleSessionConfig::defaults(mode);
        assert_eq!(defaults.mode(), mode);
        assert_eq!(
            defaults.duration().as_duration(),
            Duration::from_secs(seconds)
        );
        let custom = PuzzleSessionConfig::new(mode, Duration::from_secs(7))?;
        assert_eq!(custom.duration().as_duration(), Duration::from_secs(7));
        assert!(PuzzleSessionConfig::new(mode, Duration::ZERO).is_err());
        assert!(PuzzleSessionConfig::new(mode, Duration::MAX).is_err());
        for nanos in [1, u64::MAX] {
            assert_eq!(
                PuzzleSessionConfig::new(mode, Duration::from_nanos(nanos))?
                    .duration()
                    .as_nanos(),
                nanos
            );
        }
    }
    assert!(PuzzleSessionRevision::new(u64::MAX).next().is_err());
    Ok(())
}

#[test]
fn all_stale_context_components_are_rejected_before_any_transition()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["e2e4", "e7e5", "g1f3"]])?;
    let other = NonZeroU64::new(2).ok_or("Missing nonzero identity")?;
    let identities = [
        PuzzleSessionContext::new(
            PuzzleSessionId::new(other),
            session.attempt().id(),
            PuzzleFixture::identity()?,
            session.revision(),
        ),
        PuzzleSessionContext::new(
            session.id(),
            PuzzleAttemptId::new(other),
            PuzzleFixture::identity()?,
            session.revision(),
        ),
        PuzzleSessionContext::new(
            session.id(),
            session.attempt().id(),
            PuzzleDefinitionIdentity::new("fixture".into(), "v2".into(), "tactic-1".into())?,
            session.revision(),
        ),
        PuzzleSessionContext::new(
            session.id(),
            session.attempt().id(),
            PuzzleFixture::identity()?,
            PuzzleSessionRevision::new(1),
        ),
    ];
    let movement = session.attempt().position().parse_uci("e2e4")?;
    for context in identities {
        let before = session.clone();
        assert!(session.submit_move(&context, movement).is_err());
        assert!(session.request_hint(&context).is_err());
        assert!(session.retry_practice(&context).is_err());
        assert!(session.skip_attempt(&context).is_err());
        assert!(session.record_timeout(&context).is_err());
        let mut random = PuzzleRandomProviderSeeded::new(71);
        assert!(session.play_opponent_reply(&context, &mut random).is_err());
        assert_eq!(session, before);
    }
    let stale = session.context();
    session.request_hint(&stale)?;
    assert!(matches!(
        session.submit_move(&stale, movement),
        Err(PuzzleSessionError::StaleRevision { .. })
    ));
    Ok(())
}

#[test]
fn phase_guards_prevent_out_of_turn_input_retries_and_post_review_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSolutionFixture::session(&[&["e2e4", "e7e5", "g1f3"]])?;
    let mut random = PuzzleRandomProviderSeeded::new(19);
    let before = session.clone();
    assert!(session.retry_practice(&session.context()).is_err());
    assert!(
        session
            .play_opponent_reply(&session.context(), &mut random)
            .is_err()
    );
    assert_eq!(session, before);
    PuzzleSolutionFixture::movement(&mut session, "e2e4")?;
    let before = session.clone();
    assert!(session.request_hint(&session.context()).is_err());
    assert!(session.retry_practice(&session.context()).is_err());
    let opponent = session.attempt().position().parse_uci("e7e5")?;
    assert!(session.submit_move(&session.context(), opponent).is_err());
    assert_eq!(session, before);
    PuzzleSolutionFixture::reply(&mut session)?;
    PuzzleSolutionFixture::movement(&mut session, "g1f3")?;
    let before = session.clone();
    let move_after_review = session.attempt().position().parse_uci("b8c6")?;
    assert!(
        session
            .submit_move(&session.context(), move_after_review)
            .is_err()
    );
    assert!(session.request_hint(&session.context()).is_err());
    assert!(session.retry_practice(&session.context()).is_err());
    assert!(
        session
            .play_opponent_reply(&session.context(), &mut random)
            .is_err()
    );
    assert!(session.skip_attempt(&session.context()).is_err());
    assert!(session.record_timeout(&session.context()).is_err());
    assert_eq!(session, before);
    Ok(())
}
