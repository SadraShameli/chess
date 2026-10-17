use std::num::NonZeroUsize;

use thiserror::Error;
use tokio::sync::{Mutex, watch};

use crate::board::position::BoardPosition;
use crate::capacity::lease::CapacityLease;
use crate::desktop::dataset_commands::{
    DesktopDatasetCommands, DesktopDatasetRow, DesktopDatasetStarter,
};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::opening_commands::{
    DesktopOpeningCommands, DesktopOpeningCommandsError, DesktopOpeningExplorerView,
};
use crate::desktop::report_commands::DesktopReportEnumConfirmation;
use crate::desktop::report_commands::{
    DesktopReportCommands, DesktopReportCommandsError, DesktopReportView,
};
use crate::engine::process_provider::EngineProcess;
use crate::engine::search::identity::EngineSearchTag;
use crate::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumEvent, EngineSearchSessionError,
};
use crate::engine::search::stop::{EngineSearchStop, EngineSearchStopError};
use crate::engine::session::EngineSession;
use crate::game::enums::{GameEnumPhase, GameId, GameMoveId, GameRevision};
use crate::game::tree::GameTree;
use crate::presentation::opening::controller::PresentationOpeningIntent;
use crate::presentation::report::controller::PresentationReportIntent;
use crate::presentation::settings::datasets::PresentationSettingsDatasetsIntent;
use crate::presentation::study::PresentationStudy;
use crate::report::configuration::{
    ReportJobBudget, ReportJobConfiguration, ReportJobConfigurationError,
};
use crate::report::job::ReportJob;
use crate::report::job::{ReportJobError, ReportJobId};
use crate::report::scheduler::ReportScheduler;
use crate::report::scheduler::ReportSchedulerError;
use crate::report::scheduler::policy::ReportSchedulerEnumDecision;
use crate::report::scheduler::policy::{ReportSchedulerDemand, ReportSchedulerEnumActivity};
use crate::report::scoring::ReportScoringEnumPolicyVersion;
use crate::report::service::{ReportService, ReportServiceEnumOutcome};
use crate::report::source::ReportJobSourceLimits;
use crate::report::source::{ReportJobSource, ReportJobSourceError};
use crate::storage::dataset_integrity::StorageDatasetIntegrityReport;
use crate::storage::dataset_registry::{
    StorageDatasetRegistryEnumHolder, StorageDatasetRegistryEnumKind, StorageDatasetRegistryError,
    StorageDatasetRepository,
};
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::opening_repository::StorageOpeningRepository;

#[derive(Debug, Error)]
pub enum DesktopNativeReportHostError {
    #[error("Reanalysis needs the finished game to still be open")]
    ReanalysisUnavailable,
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
    #[error(transparent)]
    Stop(#[from] EngineSearchStopError),
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
    pub failures: Vec<String>,
}

pub struct DesktopNativeReportHost {
    reports: Mutex<DesktopReportCommands>,
    openings: StorageOpeningRepository,
    datasets: StorageDatasetRepository,
    failures: std::sync::Mutex<Vec<String>>,
    previews_ready: std::sync::Mutex<Vec<ReportJobId>>,
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
            failures: std::sync::Mutex::new(Vec::new()),
            previews_ready: std::sync::Mutex::new(Vec::new()),
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
        Ok(self.reports.lock().await.observe_game_line(game, capture)?)
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
        sample_offset: usize,
    ) -> Result<DesktopOpeningExplorerView, DesktopNativeReportHostError> {
        let leases = self
            .datasets
            .lease_active(
                StorageDatasetRegistryEnumKind::Corpus,
                StorageDatasetRegistryEnumHolder::Query,
                "explorer".to_owned(),
            )
            .await?;
        let outcome =
            DesktopOpeningCommands::explore_indexed(&self.openings, position, sample_offset).await;
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

    pub fn running_games(snapshots: &[DesktopGameSnapshot]) -> u32 {
        let running = snapshots
            .iter()
            .filter(|snapshot| snapshot.phase() == GameEnumPhase::Active)
            .count();
        u32::try_from(running).unwrap_or(u32::MAX)
    }

    async fn stop_requested(&self, tag: EngineSearchTag) -> bool {
        matches!(
            self.reports.lock().await.service_mut().scheduler_mut().decision(),
            ReportSchedulerEnumDecision::Stop(requested) if requested == tag
        )
    }

    pub async fn run_next<P: EngineProcess>(
        &self,
        session: &mut EngineSearchSession<P>,
        lease: CapacityLease,
        live: &watch::Receiver<Vec<DesktopGameSnapshot>>,
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
        let mut applied = None;
        let terminal = loop {
            let running = Self::running_games(&live.borrow());
            if applied != Some(running) {
                self.live_games_changed(running).await?;
                applied = Some(running);
            }
            if self.stop_requested(tag).await {
                break session
                    .stop(EngineSearchStop::received_at(
                        tag,
                        tokio::time::Instant::now(),
                    )?)
                    .await?;
            }
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

    pub fn record_failure(&self, message: String) {
        if let Ok(mut failures) = self.failures.lock() {
            failures.push(message);
        }
    }

    pub fn take_failures(&self) -> Vec<String> {
        self.failures
            .lock()
            .map(|mut failures| std::mem::take(&mut *failures))
            .unwrap_or_default()
    }

    pub async fn drain<P: EngineProcess>(
        &self,
        session: &mut EngineSearchSession<P>,
        lease: impl Fn() -> Option<CapacityLease>,
        live: &watch::Receiver<Vec<DesktopGameSnapshot>>,
    ) -> Result<usize, DesktopNativeReportHostError> {
        let mut completed = 0_usize;
        while let Some(lease) = lease() {
            if !self.run_next(session, lease, live).await? {
                break;
            }
            completed += 1;
        }
        Ok(completed)
    }

    pub async fn process(&self, input: DesktopNativeStudyInput) -> DesktopNativeStudyOutput {
        let mut output = DesktopNativeStudyOutput {
            starters: input.starters.clone(),
            ..DesktopNativeStudyOutput::default()
        };
        {
            let mut reports = self.reports.lock().await;
            for intent in input.report_intents {
                match intent {
                    PresentationReportIntent::Cancel(id) => {
                        if let Err(error) = reports.cancel(id) {
                            output
                                .failures
                                .push(format!("The report could not be cancelled: {error}"));
                        }
                    }
                    PresentationReportIntent::PreviewReanalysis(id) => output.reanalysis.push(id),
                    PresentationReportIntent::ConfirmReplacement(id) => {
                        match reports
                            .confirm_replacement(id, DesktopReportEnumConfirmation::Confirmed)
                        {
                            Ok(_) => output.replacement_resolved = true,
                            Err(error) => output
                                .failures
                                .push(format!("The report could not be replaced: {error}")),
                        }
                    }
                    PresentationReportIntent::DiscardReplacement(id) => {
                        reports.discard_replacement(id);
                        output.replacement_resolved = true;
                    }
                }
            }
            match reports.list() {
                Ok(listed) => {
                    let target = input
                        .current_report
                        .filter(|id| listed.iter().any(|entry| entry.id == *id))
                        .or_else(|| listed.first().map(|entry| entry.id));
                    if let Some(id) = target {
                        match reports.read(id) {
                            Ok(view) => output.report = Some(view),
                            Err(error) => output
                                .failures
                                .push(format!("The report could not be read: {error}")),
                        }
                    }
                }
                Err(error) => output
                    .failures
                    .push(format!("The reports could not be listed: {error}")),
            }
        }
        match self.explore(&input.position, input.sample_offset).await {
            Ok(view) => output.explorer = Some(view),
            Err(error) => output
                .failures
                .push(format!("The opening explorer is unavailable: {error}")),
        }
        match self.datasets.list().await {
            Ok(entries) => {
                output.rows = Some(DesktopDatasetCommands::rows_from_entries(
                    entries,
                    &input.integrity,
                ));
            }
            Err(error) => output
                .failures
                .push(format!("The data list is unavailable: {error}")),
        }
        output.previews_ready = self.take_previews_ready();
        output.failures.extend(self.take_failures());
        output
    }

    pub fn apply(
        output: DesktopNativeStudyOutput,
        study: &mut PresentationStudy,
    ) -> DesktopNativeStudyPending {
        if let Some(view) = output.report {
            study.report_mut().observe(view);
        }
        if output.replacement_resolved {
            study.report_mut().set_replacement_preview(false);
        }
        let current = study.report_mut().view().map(|view| view.id);
        if current.is_some_and(|id| output.previews_ready.contains(&id)) {
            study.report_mut().set_replacement_preview(true);
        }
        if let Some(explorer) = output.explorer {
            study.opening_mut().observe_explorer(explorer);
        }
        if let Some(rows) = output.rows {
            study.set_datasets(output.starters, rows);
        }
        study.report_mut().observe_failures(output.failures.clone());
        DesktopNativeStudyPending {
            opening: study.take_opening_intents(),
            datasets: study.take_dataset_intents(),
            reanalysis: output.reanalysis,
            failures: output.failures,
        }
    }

    pub async fn pump(
        &self,
        study: &mut PresentationStudy,
        position: &BoardPosition,
        starters: &[DesktopDatasetStarter],
        integrity: &StorageDatasetIntegrityReport,
    ) -> DesktopNativeStudyPending {
        let input = DesktopNativeStudyInput {
            report_intents: study.take_report_intents(),
            current_report: study.report_mut().view().map(|view| view.id),
            position: position.clone(),
            sample_offset: 0,
            starters: starters.to_vec(),
            integrity: integrity.clone(),
        };
        Self::apply(self.process(input).await, study)
    }

    pub async fn preview_reanalysis<P: EngineProcess>(
        &self,
        previous: ReportJobId,
        engine: &EngineSession<P>,
        snapshots: &[DesktopGameSnapshot],
    ) -> Result<(), DesktopNativeReportHostError> {
        let game = {
            let mut reports = self.reports.lock().await;
            reports
                .list()?
                .into_iter()
                .find(|entry| entry.id == previous)
                .map(|entry| entry.game)
                .ok_or(DesktopNativeReportHostError::ReanalysisUnavailable)?
        };
        let snapshot = snapshots
            .iter()
            .find(|snapshot| snapshot.id() == game && snapshot.completed_tree().is_some())
            .ok_or(DesktopNativeReportHostError::ReanalysisUnavailable)?;
        let tree = snapshot
            .completed_tree()
            .ok_or(DesktopNativeReportHostError::ReanalysisUnavailable)?;
        let source = ReportJobSource::capture(
            game,
            snapshot.revision(),
            tree,
            snapshot.live(),
            ReportJobSourceLimits::new(10_000, 256 * 1024 * 1024)?,
        )?;
        let configuration = ReportJobConfiguration::capture(
            engine,
            ReportJobBudget::default_per_position()?,
            ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        )?;
        let job = ReportJob::new(
            ReportJobId::try_from(uuid::Uuid::new_v4())?,
            source,
            configuration,
        )?;
        self.reports
            .lock()
            .await
            .preview_replacement(previous, job)?;
        if let Ok(mut ready) = self.previews_ready.lock() {
            ready.push(previous);
        }
        Ok(())
    }

    fn take_previews_ready(&self) -> Vec<ReportJobId> {
        self.previews_ready
            .lock()
            .map(|mut ready| std::mem::take(&mut *ready))
            .unwrap_or_default()
    }
}

pub struct DesktopNativeStudyInput {
    pub report_intents: Vec<PresentationReportIntent>,
    pub current_report: Option<ReportJobId>,
    pub position: BoardPosition,
    pub sample_offset: usize,
    pub starters: Vec<DesktopDatasetStarter>,
    pub integrity: StorageDatasetIntegrityReport,
}

#[derive(Default)]
pub struct DesktopNativeStudyOutput {
    pub report: Option<DesktopReportView>,
    pub replacement_resolved: bool,
    pub previews_ready: Vec<ReportJobId>,
    pub explorer: Option<DesktopOpeningExplorerView>,
    pub rows: Option<Vec<DesktopDatasetRow>>,
    pub starters: Vec<DesktopDatasetStarter>,
    pub reanalysis: Vec<ReportJobId>,
    pub failures: Vec<String>,
}
