#[path = "fixture.rs"]
mod fixture;

#[path = "analysis_update_review.rs"]
mod analysis_update_review;

#[path = "service.rs"]
mod service;

use chess::board::position::BoardPosition;
use chess::game::enums::GameRevision;
use chess::report::job::{ReportJobEnumFreshness, ReportJobEnumState, ReportJobError};
use chess::report::source::{ReportJobSource, ReportJobSourceLimits};

use fixture::{ReportFixture, ReportFixtureResult};

#[tokio::test]
async fn root_only_completed_game_has_no_invented_player_accuracy() -> ReportFixtureResult {
    let fixture = ReportFixture::new().await?;
    let mut job = fixture.job(&[])?;
    job.resume()?;
    let attempt = job.start_position(0)?;
    job.record_unavailable(&attempt)?;
    let summary = job.summary()?;
    assert_eq!(job.progress().total(), 1);
    assert_eq!(
        summary
            .player(chess::board::enums::BoardEnumColor::White)
            .accuracy(),
        None
    );
    assert_eq!(
        summary
            .player(chess::board::enums::BoardEnumColor::Black)
            .accuracy(),
        None
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn cumulative_job_memory_rejects_attempt_without_advancing_progress() -> ReportFixtureResult {
    let fixture = ReportFixture::new().await?;
    let tree = ReportFixture::tree(&["e2e4"])?;
    let normal = fixture.job(&["e2e4"])?;
    let limits = ReportJobSourceLimits::new(2, normal.memory_charge_bytes())?;
    let source = ReportJobSource::capture(
        ReportFixture::game(1)?,
        GameRevision::ZERO,
        &tree,
        ReportFixture::node(2)?,
        limits,
    )?;
    let mut limited = fixture.job_with_source(source, 21)?;
    limited.resume()?;
    assert!(limited.start_position(0).is_err());
    assert_eq!(limited.progress().completed(), 0);
    assert_eq!(limited.active_search(), None);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn terminal_draw_outcome_is_node_bound_and_resignation_is_not_a_proven_mate()
-> ReportFixtureResult {
    use chess::board::enums::BoardEnumColor;
    use chess::game::enums::{GameEnumOutcomeReason, GameEnumResult};
    use chess::game::outcome::GameOutcome;
    let mut fixture = ReportFixture::new().await?;
    let mut job = fixture.job(&["e2e4"])?;
    job.resume()?;
    fixture
        .complete(&mut job, 0, "info score cp 400 pv e2e4")
        .await?;
    let attempt = job.start_position(1)?;
    let wrong_node = GameOutcome::new(
        GameEnumResult::Draw,
        GameEnumOutcomeReason::Agreement,
        ReportFixture::node(99)?,
    )?;
    assert!(job.record_outcome(&attempt, wrong_node).is_err());
    let resignation = GameOutcome::new(
        GameEnumResult::Victory(BoardEnumColor::White),
        GameEnumOutcomeReason::Resignation,
        ReportFixture::node(2)?,
    )?;
    assert!(job.record_outcome(&attempt, resignation).is_err());
    let draw = GameOutcome::new(
        GameEnumResult::Draw,
        GameEnumOutcomeReason::Agreement,
        ReportFixture::node(2)?,
    )?;
    job.record_outcome(&attempt, draw)?;
    assert_eq!(job.state(), ReportJobEnumState::Completed);
    assert_eq!(
        job.summary()?
            .player(BoardEnumColor::White)
            .display_accuracy()
            .as_deref(),
        Some("76.9")
    );
    fixture.close().await?;
    Ok(())
}

#[test]
fn line_capture_preserves_real_nodes_positions_and_metadata_only_revisions() -> ReportFixtureResult
{
    let tree = ReportFixture::tree(&["e2e4", "e7e5", "g1f3"])?;
    let source = ReportFixture::source(&tree)?;
    let path = tree.path_to(ReportFixture::node(4)?)?;
    assert_eq!(source.positions().len(), 4);
    for (captured, node) in source.positions().iter().zip(&path) {
        assert_eq!(captured.node(), *node);
        assert_eq!(captured.position(), tree.node(*node)?.position());
    }
    let newer = ReportJobSource::capture(
        source.game(),
        GameRevision::ZERO.next()?,
        &tree,
        source.tip(),
        source.limits(),
    )?;
    assert_ne!(source.revision(), newer.revision());
    assert!(source.is_same_line(&newer));
    assert_eq!(source.digest(), newer.digest());
    Ok(())
}

#[test]
fn line_identity_rejects_changed_nodes_moves_root_counters_and_other_games() -> ReportFixtureResult
{
    let tree = ReportFixture::tree(&["e2e4", "e7e5"])?;
    let source = ReportFixture::source(&tree)?;
    let mut changed = tree.clone();
    changed.replace_branch(
        ReportFixture::node(3)?,
        ReportFixture::node(30)?,
        changed
            .node(ReportFixture::node(2)?)?
            .position()
            .parse_uci("c7c5")?,
    )?;
    let other = ReportJobSource::capture(
        source.game(),
        source.revision(),
        &changed,
        ReportFixture::node(30)?,
        source.limits(),
    )?;
    assert!(!source.is_same_line(&other));
    let other_game = ReportJobSource::capture(
        ReportFixture::game(2)?,
        source.revision(),
        &tree,
        source.tip(),
        source.limits(),
    )?;
    assert!(!source.is_same_line(&other_game));
    let root = BoardPosition::from_fen(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 1 2",
        BoardPosition::standard().variant(),
    )?;
    let shifted = ReportFixture::tree_from(root, &["e2e4", "e7e5"])?;
    let shifted = ReportFixture::source(&shifted)?;
    assert!(!source.is_same_line(&shifted));
    assert_ne!(source.positions()[0], shifted.positions()[0]);
    Ok(())
}

#[tokio::test]
async fn counter_preserving_source_dispatches_full_canonical_initial_fen() -> ReportFixtureResult {
    let fixture = ReportFixture::new().await?;
    let initial = BoardPosition::from_fen(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 1 2",
        BoardPosition::standard().variant(),
    )?;
    let fen = initial.fen();
    let tree = ReportFixture::tree_from(initial, &["e2e4"])?;
    let mut job = fixture.job_with_source(ReportFixture::source(&tree)?, 21)?;
    job.resume()?;
    let attempt = job.start_position(0)?;
    assert_eq!(attempt.position().initial().fen(), fen);
    assert_eq!(attempt.position().command(), format!("position fen {fen}"));
    fixture.close().await?;
    Ok(())
}

#[test]
fn source_position_and_cumulative_memory_limits_fail_before_admission() -> ReportFixtureResult {
    let tree = ReportFixture::tree(&["e2e4", "e7e5"])?;
    for limits in [
        ReportJobSourceLimits::new(2, 1_000_000)?,
        ReportJobSourceLimits::new(4, 1)?,
    ] {
        assert!(
            ReportJobSource::capture(
                ReportFixture::game(1)?,
                GameRevision::ZERO,
                &tree,
                ReportFixture::node(3)?,
                limits
            )
            .is_err()
        );
    }
    assert!(ReportJobSourceLimits::new(0, 1).is_err());
    assert!(ReportJobSourceLimits::new(1, 0).is_err());
    Ok(())
}

#[tokio::test]
async fn lifecycle_retains_completed_positions_and_restarts_interrupted_full_budget()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut job = fixture.job(&["e2e4", "e7e5"])?;
    assert_eq!(
        job.position_state(0)?,
        chess::report::job::ReportJobEnumPositionState::Pending
    );
    job.resume()?;
    fixture
        .complete(&mut job, 0, "info score cp 400 pv e2e4")
        .await?;
    let attempt = job.start_position(1)?;
    assert_eq!(
        job.position_state(1)?,
        chess::report::job::ReportJobEnumPositionState::Preparing
    );
    let first_budget = attempt.limit();
    job.pause()?;
    assert_eq!(job.progress().completed(), 1);
    assert_eq!(
        job.position_state(0)?,
        chess::report::job::ReportJobEnumPositionState::Completed
    );
    assert_eq!(job.state(), ReportJobEnumState::Paused);
    job.resume()?;
    let restarted = job.start_position(1)?;
    assert_ne!(attempt.sequence(), restarted.sequence());
    assert_eq!(restarted.limit(), first_budget);
    assert!(matches!(
        job.record_unavailable(&attempt),
        Err(ReportJobError::StaleAttempt)
    ));
    job.cancel()?;
    assert_eq!(job.progress().completed(), 1);
    assert_eq!(job.state(), ReportJobEnumState::Cancelled);
    assert!(job.resume().is_err());
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn actual_engine_context_budget_history_and_configuration_must_match() -> ReportFixtureResult
{
    let mut fixture = ReportFixture::new().await?;
    let mut job = fixture.job(&["e2e4"])?;
    job.resume()?;
    let attempt = job.start_position(0)?;
    let wrong = fixture
        .start(
            &attempt,
            Some(chess::engine::search::limit::EngineSearchLimit::depth(1)?),
        )
        .await?;
    assert!(job.bind_engine(&attempt, fixture.session()).is_err());
    fixture.finish(wrong, "info score cp 0 pv e2e4").await?;
    job.pause()?;
    job.resume()?;
    fixture
        .complete(&mut job, 0, "info score cp 0 pv e2e4")
        .await?;
    assert_eq!(job.progress().completed(), 1);
    assert_eq!(job.progress().total(), 2);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn adjacent_findings_use_played_move_and_separate_top_candidate_flag() -> ReportFixtureResult
{
    let mut fixture = ReportFixture::new().await?;
    let mut job = fixture.job(&["e2e4"])?;
    job.resume()?;
    fixture
        .complete(&mut job, 0, "info score cp 400 pv e2e4 e7e5")
        .await?;
    assert!(job.finding(0)?.is_best());
    assert!(matches!(
        job.finding(0)?.assessment(),
        chess::report::scoring::ReportScoringEnumAssessment::Unscored { .. }
    ));
    fixture
        .complete(&mut job, 1, "info score cp 0 pv e7e5")
        .await?;
    let finding = job.finding(0)?;
    assert!(finding.is_best());
    assert_eq!(finding.before_node(), ReportFixture::node(1)?);
    assert_eq!(finding.after_node(), ReportFixture::node(2)?);
    assert_eq!(
        finding.played_move(),
        BoardPosition::standard().parse_uci("e2e4")?
    );
    assert_eq!(job.state(), ReportJobEnumState::Completed);
    assert_eq!(
        job.summary()?
            .player(chess::board::enums::BoardEnumColor::White)
            .display_accuracy()
            .as_deref(),
        Some("76.9")
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn bound_pause_and_cancel_retain_search_until_observed_terminal_and_reject_old_tags()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut job = fixture.job(&["e2e4"])?;
    job.resume()?;
    let first = job.start_position(0)?;
    let first_tag = fixture.start(&first, None).await?;
    let first_binding = job.bind_engine(&first, fixture.session())?;
    job.pause()?;
    assert_eq!(job.state(), ReportJobEnumState::Pausing);
    assert_eq!(job.active_search(), Some(first_tag));
    assert!(job.resume().is_err());
    assert!(job.recover_after_relaunch().is_err());
    let first_terminal = fixture.finish(first_tag, "info score cp 0 pv e2e4").await?;
    job.record_terminal(&first_binding, &first_terminal)?;
    assert_eq!(job.state(), ReportJobEnumState::Paused);
    assert_eq!(job.progress().completed(), 0);
    job.resume()?;
    let second = job.start_position(0)?;
    let second_tag = fixture.start(&second, None).await?;
    let second_binding = job.bind_engine(&second, fixture.session())?;
    let second_terminal = fixture
        .finish(second_tag, "info score cp 0 pv e2e4")
        .await?;
    assert!(
        job.record_terminal(&second_binding, &first_terminal)
            .is_err()
    );
    assert_eq!(job.active_search(), Some(second_tag));
    job.cancel()?;
    assert_eq!(job.state(), ReportJobEnumState::Cancelling);
    assert_eq!(job.active_search(), Some(second_tag));
    job.record_terminal(&second_binding, &second_terminal)?;
    assert_eq!(job.state(), ReportJobEnumState::Cancelled);
    assert_eq!(job.active_search(), None);
    assert_eq!(job.progress().completed(), 0);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn equal_current_board_with_different_history_and_context_cannot_bind() -> ReportFixtureResult
{
    use chess::engine::search::identity::{EngineSearchContext, EngineSearchRevision};
    use chess::engine::search::position::EngineSearchPosition;

    let mut fixture = ReportFixture::new().await?;
    let mut job = fixture.job(&["e2e4"])?;
    job.resume()?;
    let attempt = job.start_position(1)?;
    let no_history = EngineSearchPosition::new(
        attempt.position().current().clone(),
        std::iter::empty::<&str>(),
    )?;
    assert_eq!(no_history.current(), attempt.position().current());
    let tag = fixture
        .start_with(attempt.context(), no_history, attempt.limit())
        .await?;
    assert!(job.bind_engine(&attempt, fixture.session()).is_err());
    fixture.finish(tag, "info score cp 0 pv e7e5").await?;
    let context = EngineSearchContext::new(attempt.context().game(), EngineSearchRevision::new(9));
    let tag = fixture
        .start_with(context, attempt.position().clone(), attempt.limit())
        .await?;
    assert!(job.bind_engine(&attempt, fixture.session()).is_err());
    fixture.finish(tag, "info score cp 0 pv e7e5").await?;
    fixture.configure_hash(32).await?;
    let tag = fixture.start(&attempt, None).await?;
    assert!(job.bind_engine(&attempt, fixture.session()).is_err());
    fixture.finish(tag, "info score cp 0 pv e7e5").await?;
    assert_eq!(job.progress().completed(), 0);
    job.cancel()?;
    fixture.close().await?;
    Ok(())
}

#[test]
fn finite_budgets_reuse_engine_validation_and_keep_three_second_default() -> ReportFixtureResult {
    use chess::clock::duration::ClockDuration;
    use chess::report::configuration::ReportJobBudget;

    assert_eq!(
        ReportJobBudget::default_per_position()?.limit().command(),
        "go movetime 3000"
    );
    assert_eq!(ReportJobBudget::depth(27)?.limit().command(), "go depth 27");
    assert_eq!(
        ReportJobBudget::nodes(12_345)?.limit().command(),
        "go nodes 12345"
    );
    assert!(ReportJobBudget::move_time(ClockDuration::ZERO).is_err());
    assert!(ReportJobBudget::move_time(ClockDuration::from_nanos(1_000_001)).is_err());
    assert!(ReportJobBudget::depth(128).is_err());
    assert!(ReportJobBudget::nodes(2_147_483_648).is_err());
    Ok(())
}

#[tokio::test]
async fn relaunch_is_paused_and_outdated_source_cannot_accept_new_results() -> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut job = fixture.job(&["e2e4", "e7e5"])?;
    job.resume()?;
    fixture
        .complete(&mut job, 0, "info score cp 0 pv e2e4")
        .await?;
    let old_attempt = job.start_position(1)?;
    job.recover_after_relaunch()?;
    assert_eq!(job.state(), ReportJobEnumState::Paused);
    assert_eq!(job.progress().completed(), 1);
    assert!(job.record_unavailable(&old_attempt).is_err());
    let changed = ReportFixture::source(&ReportFixture::tree(&["d2d4", "d7d5"])?)?;
    job.observe_source(&changed)?;
    assert_eq!(job.freshness(), ReportJobEnumFreshness::Outdated);
    assert!(job.resume().is_err());
    assert_eq!(job.progress().completed(), 1);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn replacement_preview_preserves_old_report_until_explicit_fresh_confirmation()
-> ReportFixtureResult {
    use chess::report::replacement::ReportJobReplacementPreview;
    let fixture = ReportFixture::new().await?;
    let mut previous = fixture.job(&["e2e4"])?;
    let replacement = fixture.job_with_id(&["e2e4"], 21)?;
    let preview = ReportJobReplacementPreview::new(&previous, replacement)?;
    assert_eq!(previous.id().value(), uuid::Uuid::from_u128(20));
    assert_eq!(previous.progress().completed(), 0);
    assert_eq!(preview.proposed().id().value(), uuid::Uuid::from_u128(21));
    previous.resume()?;
    assert!(preview.confirm(&previous).is_err());
    let preview = ReportJobReplacementPreview::new(&previous, fixture.job_with_id(&["e2e4"], 22)?)?;
    let accepted = preview.confirm(&previous)?;
    assert_eq!(accepted.id().value(), uuid::Uuid::from_u128(22));
    assert_eq!(previous.id().value(), uuid::Uuid::from_u128(20));
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn unavailable_completion_does_not_invent_score_and_score_without_pv_can_still_score()
-> ReportFixtureResult {
    let mut fixture = ReportFixture::new().await?;
    let mut job = fixture.job(&["e2e4"])?;
    job.resume()?;
    fixture.complete(&mut job, 0, "info score cp 400").await?;
    fixture.complete(&mut job, 1, "info score cp 0").await?;
    assert_eq!(
        job.summary()?
            .player(chess::board::enums::BoardEnumColor::White)
            .display_accuracy()
            .as_deref(),
        Some("76.9")
    );
    assert!(!job.finding(0)?.is_best());
    let mut unavailable = fixture.job_with_id(&["e2e4"], 21)?;
    unavailable.resume()?;
    let attempt = unavailable.start_position(0)?;
    unavailable.record_unavailable(&attempt)?;
    let attempt = unavailable.start_position(1)?;
    unavailable.record_unavailable(&attempt)?;
    assert_eq!(unavailable.progress().completed(), 2);
    assert_eq!(
        unavailable
            .summary()?
            .player(chess::board::enums::BoardEnumColor::White)
            .accuracy(),
        None
    );
    fixture.close().await?;
    Ok(())
}
