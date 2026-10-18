use std::num::NonZeroUsize;

use chess::desktop::report_commands::{
    DesktopReportCommands, DesktopReportCommandsError, DesktopReportEnumConfirmation,
};
use chess::report::job::{ReportJobEnumFreshness, ReportJobEnumState};
use chess::report::scheduler::ReportScheduler;
use chess::report::scheduler::policy::{ReportSchedulerDemand, ReportSchedulerEnumActivity};
use chess::report::scoring::ReportScoringEnumCoverage;
use chess::report::service::{ReportService, ReportServiceEnumOutcome, ReportServiceEnumTrigger};

use super::fixture::{ReportFixture, ReportFixtureResult};

fn commands() -> ReportFixtureResult<DesktopReportCommands> {
    Ok(DesktopReportCommands::new(ReportService::new(
        ReportScheduler::new(
            NonZeroUsize::new(8).ok_or("empty queue")?,
            ReportSchedulerDemand::new(
                0,
                ReportSchedulerEnumActivity::Idle,
                ReportSchedulerEnumActivity::Idle,
                ReportSchedulerEnumActivity::Idle,
            ),
        ),
    )))
}

#[tokio::test]
async fn start_list_read_and_cancel_expose_typed_partial_state() -> ReportFixtureResult {
    let fixture = ReportFixture::new().await?;
    let mut commands = commands()?;
    let id = match commands.start(
        ReportServiceEnumTrigger::PlayedCompletion,
        fixture.job_with_id(&["e2e4", "e7e5"], 700)?,
    )? {
        ReportServiceEnumOutcome::Created(id) => id,
        other @ ReportServiceEnumOutcome::Existing(_)
        | other @ ReportServiceEnumOutcome::Declined(_) => {
            return Err(format!("unexpected {other:?}").into());
        }
    };
    let entries = commands.list()?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, id);
    assert_eq!(entries[0].state, ReportJobEnumState::Queued);
    let view = commands.read(id)?;
    assert_eq!(view.coverage, ReportScoringEnumCoverage::Partial);
    assert_eq!(view.freshness, ReportJobEnumFreshness::Current);
    assert_eq!(view.findings.len(), 2);
    assert_eq!(view.findings[0].played_uci, "e2e4");
    assert!(view.findings.iter().all(|finding| finding.label.is_none()));
    assert!(view.accuracy.iter().all(|player| player.display.is_none()));
    commands.cancel(id)?;
    assert_eq!(commands.read(id)?.state, ReportJobEnumState::Cancelled);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
async fn replacement_needs_a_preview_and_explicit_confirmation() -> ReportFixtureResult {
    let fixture = ReportFixture::new().await?;
    let mut commands = commands()?;
    let previous = match commands.start(
        ReportServiceEnumTrigger::PlayedCompletion,
        fixture.job_with_id(&["e2e4", "e7e5"], 710)?,
    )? {
        ReportServiceEnumOutcome::Created(id) => id,
        other @ ReportServiceEnumOutcome::Existing(_)
        | other @ ReportServiceEnumOutcome::Declined(_) => {
            return Err(format!("unexpected {other:?}").into());
        }
    };
    assert!(matches!(
        commands.confirm_replacement(previous, DesktopReportEnumConfirmation::Confirmed),
        Err(DesktopReportCommandsError::NoPreview)
    ));
    commands.cancel(previous)?;
    let proposed = fixture.job_with_id(&["e2e4", "e7e5"], 711)?;
    let proposed_id = proposed.id();
    commands.preview_replacement(previous, proposed)?;
    assert!(commands.pending_replacement(previous).is_some());
    assert!(matches!(
        commands.confirm_replacement(previous, DesktopReportEnumConfirmation::Unconfirmed),
        Err(DesktopReportCommandsError::ConfirmationRequired)
    ));
    assert!(commands.pending_replacement(previous).is_some());
    assert_eq!(commands.list()?.len(), 1);
    let replaced =
        commands.confirm_replacement(previous, DesktopReportEnumConfirmation::Confirmed)?;
    assert_eq!(replaced, proposed_id);
    let entries = commands.list()?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, proposed_id);
    assert!(commands.pending_replacement(previous).is_none());
    fixture.close().await?;
    Ok(())
}
