use std::num::NonZeroUsize;

use chess::game::enums::GameRevision;
use chess::report::job::{ReportJobEnumFreshness, ReportJobEnumState};
use chess::report::scheduler::ReportScheduler;
use chess::report::scheduler::policy::{ReportSchedulerDemand, ReportSchedulerEnumActivity};
use chess::report::service::{ReportService, ReportServiceEnumOutcome, ReportServiceEnumTrigger};
use chess::report::source::{ReportJobSource, ReportJobSourceLimits};

use super::fixture::{ReportFixture, ReportFixtureResult};

fn service() -> ReportFixtureResult<ReportService> {
    Ok(ReportService::new(ReportScheduler::new(
        NonZeroUsize::new(8).ok_or("empty queue")?,
        ReportSchedulerDemand::new(
            0,
            ReportSchedulerEnumActivity::Idle,
            ReportSchedulerEnumActivity::Idle,
            ReportSchedulerEnumActivity::Idle,
        ),
    )))
}

#[tokio::test]
async fn only_played_completion_and_explicit_analysis_create_reports() -> ReportFixtureResult {
    let fixture = ReportFixture::new().await?;
    let mut service = service()?;
    for (index, trigger) in [
        ReportServiceEnumTrigger::Reopen,
        ReportServiceEnumTrigger::Import,
        ReportServiceEnumTrigger::MetadataEdit,
    ]
    .into_iter()
    .enumerate()
    {
        let job = fixture.job_with_id(&["e2e4"], 100 + u128::try_from(index)?)?;
        assert!(matches!(
            service.request(trigger, job)?,
            ReportServiceEnumOutcome::Declined(_)
        ));
    }
    assert!(service.scheduler().is_empty());
    for (index, trigger) in [
        ReportServiceEnumTrigger::PlayedCompletion,
        ReportServiceEnumTrigger::ExplicitImported,
        ReportServiceEnumTrigger::ExplicitVariation,
    ]
    .into_iter()
    .enumerate()
    {
        let moves: &[&str] = [&["e2e4"][..], &["d2d4"][..], &["c2c4"][..]][index];
        let job = fixture.job_with_id(moves, 200 + u128::try_from(index)?)?;
        assert!(matches!(
            service.request(trigger, job)?,
            ReportServiceEnumOutcome::Created(_)
        ));
    }
    assert_eq!(service.scheduler().len(), 3);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn the_same_current_line_is_never_queued_twice() -> ReportFixtureResult {
    let fixture = ReportFixture::new().await?;
    let mut service = service()?;
    let first = match service.request(
        ReportServiceEnumTrigger::PlayedCompletion,
        fixture.job_with_id(&["e2e4", "e7e5"], 300)?,
    )? {
        ReportServiceEnumOutcome::Created(id) => id,
        other @ ReportServiceEnumOutcome::Existing(_)
        | other @ ReportServiceEnumOutcome::Declined(_) => {
            return Err(format!("unexpected {other:?}").into());
        }
    };
    let again = service.request(
        ReportServiceEnumTrigger::ExplicitImported,
        fixture.job_with_id(&["e2e4", "e7e5"], 301)?,
    )?;
    assert!(matches!(again, ReportServiceEnumOutcome::Existing(id) if id == first));
    assert_eq!(service.scheduler().len(), 1);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn changed_lines_outdate_old_reports_but_metadata_only_edits_do_not() -> ReportFixtureResult {
    let fixture = ReportFixture::new().await?;
    let mut service = service()?;
    let tree = ReportFixture::tree(&["e2e4", "e7e5"])?;
    let source = ReportFixture::source(&tree)?;
    let game = source.game();
    let id = match service.request(
        ReportServiceEnumTrigger::PlayedCompletion,
        fixture.job_with_source(source, 400)?,
    )? {
        ReportServiceEnumOutcome::Created(id) => id,
        other @ ReportServiceEnumOutcome::Existing(_)
        | other @ ReportServiceEnumOutcome::Declined(_) => {
            return Err(format!("unexpected {other:?}").into());
        }
    };
    let tip = tree.nodes().last().ok_or("missing tip")?.id();
    let limits = ReportJobSourceLimits::new(128, 8 * 1024 * 1024)?;
    let newer = GameRevision::ZERO.next()?;
    let same = service.observe_game(game, || {
        ReportJobSource::capture(game, newer, &tree, tip, limits)
    })?;
    assert_eq!(same, 0);
    let job = service.scheduler().job(id)?;
    assert_eq!(job.freshness(), ReportJobEnumFreshness::Current);
    assert_eq!(job.state(), ReportJobEnumState::Queued);
    let changed_tree = ReportFixture::tree(&["e2e4", "c7c5"])?;
    let changed_tip = changed_tree.nodes().last().ok_or("missing tip")?.id();
    let outdated = service.observe_game(game, || {
        ReportJobSource::capture(game, newer, &changed_tree, changed_tip, limits)
    })?;
    assert_eq!(outdated, 1);
    let job = service.scheduler().job(id)?;
    assert_eq!(job.freshness(), ReportJobEnumFreshness::Outdated);
    assert_eq!(job.state(), ReportJobEnumState::Paused);
    let replacement = service.request(
        ReportServiceEnumTrigger::ExplicitVariation,
        fixture.job_with_source(
            ReportJobSource::capture(game, newer, &changed_tree, changed_tip, limits)?,
            401,
        )?,
    )?;
    assert!(matches!(replacement, ReportServiceEnumOutcome::Created(_)));
    assert_eq!(service.scheduler().len(), 2);
    fixture.close().await?;
    Ok(())
}
