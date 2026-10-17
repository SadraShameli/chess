use std::num::NonZeroUsize;

use thiserror::Error;
use tokio::sync::Mutex;

use crate::board::position::BoardPosition;
use crate::desktop::opening_commands::{
    DesktopOpeningCommands, DesktopOpeningCommandsError, DesktopOpeningExplorerView,
};
use crate::desktop::report_commands::{DesktopReportCommands, DesktopReportCommandsError};
use crate::capacity::lease::CapacityLease;
use crate::desktop::dataset_commands::{DesktopDatasetCommands, DesktopDatasetStarter};
use crate::desktop::report_commands::DesktopReportEnumConfirmation;
use crate::presentation::opening::controller::PresentationOpeningIntent;
use crate::presentation::report::controller::PresentationReportIntent;
use crate::presentation::settings::datasets::PresentationSettingsDatasetsIntent;
use crate::presentation::study::PresentationStudy;
use crate::storage::dataset_integrity::StorageDatasetIntegrityReport;
use crate::engine::process_provider::EngineProcess;
use crate::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumEvent, EngineSearchSessionError,
};
use crate::engine::session::EngineSession;
use crate::game::enums::{GameId, GameMoveId, GameRevision};
use crate::game::tree::GameTree;
use crate::report::configuration::{ReportJobBudget, ReportJobConfiguration, ReportJobConfigurationError};
use crate::report::job::{ReportJobError, ReportJobId};
use crate::report::scheduler::ReportSchedulerError;
use crate::report::scheduler::policy::ReportSchedulerEnumDecision;
use crate::report::scoring::ReportScoringEnumPolicyVersion;
use crate::report::source::ReportJobSourceLimits;
use crate::report::job::ReportJob;
use crate::report::scheduler::ReportScheduler;
use crate::report::scheduler::policy::{ReportSchedulerDemand, ReportSchedulerEnumActivity};
use crate::report::service::{ReportService, ReportServiceEnumOutcome};
use crate::report::source::{ReportJobSource, ReportJobSourceError};
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::dataset_registry::{
    StorageDatasetRegistryEnumHolder, StorageDatasetRegistryEnumKind, StorageDatasetRegistryError,
    StorageDatasetRepository,
};
use crate::storage::opening_repository::StorageOpeningRepository;

#[derive(Debug, Error)]
pub enum DesktopNativeReportHostError {
    #[error(transparent)]
    Reports(#[from] DesktopReportCommandsError),
    #[error(transparent)]
    Openings(#[from] DesktopOpeningCommandsError),
    #[error(transparent)]
    Source(#[from] ReportJobSourceError),
    #[error(transparent)]
    Configuration(#[from] ReportJobConfigurationError),
    #[error(transparent)]
    Job(#[from] ReportJobError),
    #[error(transparent)]
    Scheduler(#[from] ReportSchedulerError),
    #[error(transparent)]
    Search(#[from] EngineSearchSessionError),
    #[error(transparent)]
    Datasets(#[from] StorageDatasetRegistryError),
}

pub struct DesktopNativeReportHostFinishedGame<'a, P: EngineProcess> {
    pub id: ReportJobId,
    pub game: GameId,
    pub revision: GameRevision,
    pub tree: &'a GameTree,
    pub tip: GameMoveId,
    pub engine: &'a EngineSession<P>,
    pub limits: ReportJobSourceLimits,
}

pub struct DesktopNativeStudyPending {
    pub opening: Vec<PresentationOpeningIntent>,
    pub datasets: Vec<PresentationSettingsDatasetsIntent>,
    pub reanalysis: Vec<ReportJobId>,
}

pub struct DesktopNativeReportHost {
    reports: Mutex<DesktopReportCommands>,
    openings: StorageOpeningRepository,
    datasets: StorageDatasetRepository,
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
            openings: StorageOpeningRepository::new(client.clone()),
            datasets: StorageDatasetRepository::new(client),
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
        let leases = self
            .datasets
            .lease_active(
                StorageDatasetRegistryEnumKind::Corpus,
                StorageDatasetRegistryEnumHolder::Query,
                "explorer".to_owned(),
            )
            .await?;
        let outcome = DesktopOpeningCommands::explore_indexed(&self.openings, position).await;
        self.datasets.release(leases).await?;
        Ok(outcome?)
    }

    pub async fn game_finished<P: EngineProcess>(
        &self,
        finished: DesktopNativeReportHostFinishedGame<'_, P>,
    ) -> Result<ReportServiceEnumOutcome, DesktopNativeReportHostError> {
        let source = ReportJobSource::capture(
            finished.game,
            finished.revision,
            finished.tree,
            finished.tip,
            finished.limits,
        )?;
        let configuration = ReportJobConfiguration::capture(
            finished.engine,
            ReportJobBudget::default_per_position()?,
            ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        )?;
        let job = ReportJob::new(finished.id, source, configuration)?;
        self.game_completed(job).await
    }

    pub async fn run_next<P: EngineProcess>(
        &self,
        session: &mut EngineSearchSession<P>,
        lease: CapacityLease,
    ) -> Result<bool, DesktopNativeReportHostError> {
        let tag = {
            let mut reports = self.reports.lock().await;
            let scheduler = reports.service_mut().scheduler_mut();
            if !matches!(scheduler.decision(), ReportSchedulerEnumDecision::Ready(_)) {
                return Ok(false);
            }
            let mut dispatch = scheduler.prepare(session, lease)?;
            let tag = match dispatch.start(session).await {
                Ok(tag) => tag,
                Err(error) => {
                    scheduler.settle_dispatch(&mut dispatch, session)?;
                    return Err(error.into());
                }
            };
            scheduler.bind(session)?;
            tag
        };
        let terminal = loop {
            if let EngineSearchSessionEnumEvent::Finished(terminal) =
                session.next_event(tag).await?
            {
                break terminal;
            }
        };
        self.reports
            .lock()
            .await
            .service_mut()
            .scheduler_mut()
            .record_terminal(&terminal)?;
        Ok(true)
    }

    pub async fn drain<P: EngineProcess>(
        &self,
        session: &mut EngineSearchSession<P>,
        lease: impl Fn() -> Option<CapacityLease>,
    ) -> Result<usize, DesktopNativeReportHostError> {
        let mut completed = 0_usize;
        while let Some(lease) = lease() {
            if !self.run_next(session, lease).await? {
                break;
            }
            completed += 1;
        }
        Ok(completed)
    }

    pub async fn pump(
        &self,
        study: &mut PresentationStudy,
        position: &BoardPosition,
        starters: &[DesktopDatasetStarter],
        integrity: &StorageDatasetIntegrityReport,
    ) -> Result<DesktopNativeStudyPending, DesktopNativeReportHostError> {
        let mut reanalysis = Vec::new();
        {
            let mut reports = self.reports.lock().await;
            for intent in study.take_report_intents() {
                match intent {
                    PresentationReportIntent::Cancel(id) => reports.cancel(id)?,
                    PresentationReportIntent::PreviewReanalysis(id) => reanalysis.push(id),
                    PresentationReportIntent::ConfirmReplacement(id) => {
                        reports.confirm_replacement(id, DesktopReportEnumConfirmation::Confirmed)?;
                        study.report_mut().set_replacement_preview(false);
                    }
                    PresentationReportIntent::DiscardReplacement(id) => {
                        reports.discard_replacement(id);
                        study.report_mut().set_replacement_preview(false);
                    }
                }
            }
            let current = study.report_mut().view().map(|view| view.id);
            let listed = reports.list()?;
            let target = current
                .filter(|id| listed.iter().any(|entry| entry.id == *id))
                .or_else(|| listed.first().map(|entry| entry.id));
            if let Some(id) = target {
                study.report_mut().observe(reports.read(id)?);
            }
        }
        study
            .opening_mut()
            .observe_explorer(self.explore(position).await?);
        let rows = DesktopDatasetCommands::rows_from_entries(self.datasets.list().await?, integrity);
        study.set_datasets(starters.to_vec(), rows);
        Ok(DesktopNativeStudyPending {
            opening: study.take_opening_intents(),
            datasets: study.take_dataset_intents(),
            reanalysis,
        })
    }
}
