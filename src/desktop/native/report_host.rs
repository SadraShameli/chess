use std::num::NonZeroUsize;

use thiserror::Error;
use tokio::sync::Mutex;

use crate::board::position::BoardPosition;
use crate::desktop::opening_commands::{
    DesktopOpeningCommands, DesktopOpeningCommandsError, DesktopOpeningExplorerView,
};
use crate::desktop::report_commands::{DesktopReportCommands, DesktopReportCommandsError};
use crate::game::enums::GameId;
use crate::report::job::ReportJob;
use crate::report::scheduler::ReportScheduler;
use crate::report::scheduler::policy::{ReportSchedulerDemand, ReportSchedulerEnumActivity};
use crate::report::service::{ReportService, ReportServiceEnumOutcome};
use crate::report::source::{ReportJobSource, ReportJobSourceError};
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::opening_repository::StorageOpeningRepository;

#[derive(Debug, Error)]
pub enum DesktopNativeReportHostError {
    #[error(transparent)]
    Reports(#[from] DesktopReportCommandsError),
    #[error(transparent)]
    Openings(#[from] DesktopOpeningCommandsError),
}

pub struct DesktopNativeReportHost {
    reports: Mutex<DesktopReportCommands>,
    openings: StorageOpeningRepository,
}

impl DesktopNativeReportHost {
    pub const QUEUE_LIMIT: NonZeroUsize = NonZeroUsize::MIN.saturating_add(31);

    pub fn new(client: StorageDocumentClient) -> Self {
        let scheduler = ReportScheduler::new(
            Self::QUEUE_LIMIT,
            ReportSchedulerDemand::new(
                0,
                ReportSchedulerEnumActivity::Idle,
                ReportSchedulerEnumActivity::Idle,
                ReportSchedulerEnumActivity::Idle,
            ),
        );
        Self {
            reports: Mutex::new(DesktopReportCommands::new(ReportService::new(scheduler))),
            openings: StorageOpeningRepository::new(client),
        }
    }

    pub async fn game_completed(
        &self,
        job: ReportJob,
    ) -> Result<ReportServiceEnumOutcome, DesktopNativeReportHostError> {
        Ok(self.reports.lock().await.observe_game_completed(job)?)
    }

    pub async fn game_line_changed(
        &self,
        game: GameId,
        capture: impl FnMut() -> Result<ReportJobSource, ReportJobSourceError>,
    ) -> Result<usize, DesktopNativeReportHostError> {
        Ok(self
            .reports
            .lock()
            .await
            .observe_game_line(game, capture)?)
    }

    pub async fn live_games_changed(
        &self,
        running: u32,
    ) -> Result<(), DesktopNativeReportHostError> {
        let mut reports = self.reports.lock().await;
        reports
            .service_mut()
            .scheduler_mut()
            .update_demand(ReportSchedulerDemand::new(
                running,
                ReportSchedulerEnumActivity::Idle,
                ReportSchedulerEnumActivity::Idle,
                ReportSchedulerEnumActivity::Idle,
            ))
            .map_err(DesktopReportCommandsError::from)?;
        Ok(())
    }

    pub async fn explore(
        &self,
        position: &BoardPosition,
    ) -> Result<DesktopOpeningExplorerView, DesktopNativeReportHostError> {
        Ok(DesktopOpeningCommands::explore_indexed(&self.openings, position).await?)
    }
}
