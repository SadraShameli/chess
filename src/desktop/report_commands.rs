use std::collections::HashMap;

use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::engine::analysis::EngineAnalysisEnumScore;
use crate::game::enums::{GameId, GameMoveId};
use crate::report::job::{
    ReportJob, ReportJobEnumFreshness, ReportJobEnumState, ReportJobError, ReportJobId,
};
use crate::report::replacement::{ReportJobReplacementError, ReportJobReplacementPreview};
use crate::report::scheduler::ReportSchedulerError;
use crate::report::scoring::{
    ReportScoringEnumAssessment, ReportScoringEnumCoverage, ReportScoringEnumLabel,
    ReportScoringEnumPolicyVersion,
};
use crate::report::service::{
    ReportService, ReportServiceEnumOutcome, ReportServiceEnumTrigger, ReportServiceError,
};
use crate::report::source::{ReportJobSource, ReportJobSourceError};

#[derive(Debug, Error)]
pub enum DesktopReportCommandsError {
    #[error(transparent)]
    Service(#[from] ReportServiceError),
    #[error(transparent)]
    Scheduler(#[from] ReportSchedulerError),
    #[error(transparent)]
    Job(#[from] ReportJobError),
    #[error(transparent)]
    Replacement(#[from] ReportJobReplacementError),
    #[error("Replacing a report requires explicit confirmation of its preview")]
    ConfirmationRequired,
    #[error("No replacement preview is pending for this report")]
    NoPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopReportEnumConfirmation {
    Unconfirmed,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopReportEnumEvaluation {
    Centipawns(i64),
    Mate(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesktopReportFinding {
    pub index: u32,
    pub played_uci: String,
    pub label: Option<ReportScoringEnumLabel>,
    pub forced: bool,
    pub missed_mate: bool,
    pub loss: Option<f64>,
    pub after_evaluation: Option<DesktopReportEnumEvaluation>,
    pub before_node: GameMoveId,
    pub after_node: GameMoveId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesktopReportAccuracy {
    pub color: BoardEnumColor,
    pub display: Option<String>,
    pub scored_moves: u32,
    pub forced_moves: u32,
    pub unscored_moves: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesktopReportView {
    pub id: ReportJobId,
    pub game: GameId,
    pub state: ReportJobEnumState,
    pub freshness: ReportJobEnumFreshness,
    pub completed: u32,
    pub total: u32,
    pub policy: ReportScoringEnumPolicyVersion,
    pub coverage: ReportScoringEnumCoverage,
    pub accuracy: [DesktopReportAccuracy; 2],
    pub findings: Vec<DesktopReportFinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopReportListEntry {
    pub id: ReportJobId,
    pub game: GameId,
    pub state: ReportJobEnumState,
    pub freshness: ReportJobEnumFreshness,
    pub completed: u32,
    pub total: u32,
}

pub struct DesktopReportCommands {
    service: ReportService,
    previews: HashMap<ReportJobId, ReportJobReplacementPreview>,
}

impl DesktopReportCommands {
    pub fn new(service: ReportService) -> Self {
        Self {
            service,
            previews: HashMap::new(),
        }
    }

    pub const fn service_mut(&mut self) -> &mut ReportService {
        &mut self.service
    }

    pub fn start(
        &mut self,
        trigger: ReportServiceEnumTrigger,
        job: ReportJob,
    ) -> Result<ReportServiceEnumOutcome, DesktopReportCommandsError> {
        Ok(self.service.request(trigger, job)?)
    }

    pub fn observe_game_completed(
        &mut self,
        job: ReportJob,
    ) -> Result<ReportServiceEnumOutcome, DesktopReportCommandsError> {
        self.start(ReportServiceEnumTrigger::PlayedCompletion, job)
    }

    pub fn observe_game_line(
        &mut self,
        game: GameId,
        capture: impl FnMut() -> Result<ReportJobSource, ReportJobSourceError>,
    ) -> Result<usize, DesktopReportCommandsError> {
        Ok(self.service.observe_game(game, capture)?)
    }

    pub fn cancel(&mut self, id: ReportJobId) -> Result<(), DesktopReportCommandsError> {
        Ok(self.service.scheduler_mut().cancel(id)?)
    }

    pub fn list(&mut self) -> Result<Vec<DesktopReportListEntry>, DesktopReportCommandsError> {
        let mut entries = Vec::new();
        for id in self.service.ids() {
            let job = self.service.scheduler().job(id)?;
            entries.push(DesktopReportListEntry {
                id,
                game: job.source().game(),
                state: job.state(),
                freshness: job.freshness(),
                completed: job.progress().completed(),
                total: job.progress().total(),
            });
        }
        Ok(entries)
    }

    pub fn read(&self, id: ReportJobId) -> Result<DesktopReportView, DesktopReportCommandsError> {
        let job = self.service.scheduler().job(id)?;
        let summary = job.summary()?;
        let mut findings = Vec::new();
        for index in 0..job.progress().total().saturating_sub(1) {
            let finding = job.finding(index)?;
            let variant = finding.before().position().variant();
            let (label, loss, missed_mate) = match finding.assessment() {
                ReportScoringEnumAssessment::Scored(scored) => (
                    Some(scored.label()),
                    Some(scored.loss()),
                    scored.is_missed_mate(),
                ),
                ReportScoringEnumAssessment::Unscored { .. } => (None, None, false),
            };
            findings.push(DesktopReportFinding {
                index,
                played_uci: finding.played_move().uci(variant),
                label,
                forced: finding.is_forced(),
                missed_mate,
                loss,
                after_evaluation: finding
                    .after_result()
                    .and_then(|result| result.score())
                    .map(|score| match score.reported() {
                        EngineAnalysisEnumScore::Centipawns(_) => {
                            DesktopReportEnumEvaluation::Centipawns(score.white_value())
                        }
                        EngineAnalysisEnumScore::Mate(_) => {
                            DesktopReportEnumEvaluation::Mate(score.white_value())
                        }
                    }),
                before_node: finding.before_node(),
                after_node: finding.after_node(),
            });
        }
        let accuracy = [BoardEnumColor::White, BoardEnumColor::Black].map(|color| {
            let player = summary.player(color);
            DesktopReportAccuracy {
                color,
                display: player.display_accuracy(),
                scored_moves: player.scored_moves(),
                forced_moves: player.forced_moves(),
                unscored_moves: player.unscored_moves(),
            }
        });
        Ok(DesktopReportView {
            id,
            game: job.source().game(),
            state: job.state(),
            freshness: job.freshness(),
            completed: job.progress().completed(),
            total: job.progress().total(),
            policy: summary.policy(),
            coverage: summary.coverage(),
            accuracy,
            findings,
        })
    }

    pub fn preview_replacement(
        &mut self,
        previous: ReportJobId,
        proposed: ReportJob,
    ) -> Result<(), DesktopReportCommandsError> {
        let preview =
            ReportJobReplacementPreview::new(self.service.scheduler().job(previous)?, proposed)?;
        self.previews.insert(previous, preview);
        Ok(())
    }

    pub fn pending_replacement(&self, previous: ReportJobId) -> Option<&ReportJob> {
        self.previews
            .get(&previous)
            .map(ReportJobReplacementPreview::proposed)
    }

    pub fn discard_replacement(&mut self, previous: ReportJobId) {
        self.previews.remove(&previous);
    }

    pub fn confirm_replacement(
        &mut self,
        previous: ReportJobId,
        confirmation: DesktopReportEnumConfirmation,
    ) -> Result<ReportJobId, DesktopReportCommandsError> {
        if confirmation != DesktopReportEnumConfirmation::Confirmed {
            return Err(DesktopReportCommandsError::ConfirmationRequired);
        }
        let preview = self
            .previews
            .remove(&previous)
            .ok_or(DesktopReportCommandsError::NoPreview)?;
        let replacement = preview.confirm(self.service.scheduler().job(previous)?)?;
        Ok(self.service.replace(previous, replacement)?)
    }
}
