use crate::engine::search::identity::EngineSearchTag;
use crate::report::job::ReportJobId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportSchedulerEnumActivity {
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportSchedulerEnumBlocker {
    LiveGames,
    LiveSearch,
    InteractiveAnalysis,
    Workbench,
    Report,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportSchedulerEnumDecision {
    Idle,
    Blocked(ReportSchedulerEnumBlocker),
    Ready(ReportJobId),
    Dispatching(ReportJobId),
    AwaitingTerminal(EngineSearchTag),
    Stop(EngineSearchTag),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportSchedulerDemand {
    running_live_games: u32,
    live_search: ReportSchedulerEnumActivity,
    interactive_analysis: ReportSchedulerEnumActivity,
    workbench: ReportSchedulerEnumActivity,
}

impl ReportSchedulerDemand {
    pub const fn new(
        running_live_games: u32,
        live_search: ReportSchedulerEnumActivity,
        interactive_analysis: ReportSchedulerEnumActivity,
        workbench: ReportSchedulerEnumActivity,
    ) -> Self {
        Self {
            running_live_games,
            live_search,
            interactive_analysis,
            workbench,
        }
    }

    pub(super) const fn blocker(self) -> Option<ReportSchedulerEnumBlocker> {
        if self.running_live_games > 0 {
            Some(ReportSchedulerEnumBlocker::LiveGames)
        } else if matches!(self.live_search, ReportSchedulerEnumActivity::Active) {
            Some(ReportSchedulerEnumBlocker::LiveSearch)
        } else if matches!(self.workbench, ReportSchedulerEnumActivity::Active) {
            Some(ReportSchedulerEnumBlocker::Workbench)
        } else if matches!(
            self.interactive_analysis,
            ReportSchedulerEnumActivity::Active
        ) {
            Some(ReportSchedulerEnumBlocker::InteractiveAnalysis)
        } else {
            None
        }
    }
}
