use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::capacity::lease::CapacityLease;
use crate::capacity::phase::CapacityPhase;
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::native::report_host::{
    DesktopNativeReportHost, DesktopNativeReportHostError, DesktopNativeReportHostFinishedGame,
};
use crate::engine::process_provider::EngineProcess;
use crate::engine::search::session::EngineSearchSession;
use crate::game::enums::{GameEnumPhase, GameId};
use crate::report::job::ReportJobId;
use crate::report::source::ReportJobSourceLimits;

pub struct DesktopNativeReportWorker;

pub struct DesktopNativeReportWorkerOptions<P: EngineProcess> {
    pub host: Arc<DesktopNativeReportHost>,
    pub session: EngineSearchSession<P>,
    pub games: watch::Receiver<Vec<DesktopGameSnapshot>>,
    pub capacity: Arc<Mutex<CapacityPhase>>,
    pub done: watch::Receiver<bool>,
    pub reanalysis: mpsc::UnboundedReceiver<ReportJobId>,
}

impl DesktopNativeReportWorker {
    pub const TICK: Duration = Duration::from_millis(500);
    pub const SOURCE_POSITIONS: u32 = 10_000;
    pub const SOURCE_BYTES: u64 = 256 * 1024 * 1024;
    pub const SHUTDOWN: Duration = Duration::from_secs(3);

    pub async fn run<P: EngineProcess>(options: DesktopNativeReportWorkerOptions<P>) {
        let DesktopNativeReportWorkerOptions {
            host,
            mut session,
            mut games,
            capacity,
            mut done,
            mut reanalysis,
        } = options;
        let mut reported: HashSet<GameId> = HashSet::new();
        let mut reanalysis_open = true;
        loop {
            if *done.borrow() {
                break;
            }
            let changed = tokio::select! {
                biased;
                outcome = done.changed() => {
                    if outcome.is_err() {
                        break;
                    }
                    continue;
                }
                request = reanalysis.recv(), if reanalysis_open => {
                    match request {
                        Some(id) => {
                            let snapshots = games.borrow().clone();
                            if let Err(error) = host
                                .preview_reanalysis(id, session.engine(), &snapshots)
                                .await
                            {
                                host.record_failure(format!(
                                    "Reanalysis could not be prepared: {error}"
                                ));
                            }
                        }
                        None => reanalysis_open = false,
                    }
                    continue;
                }
                outcome = games.changed() => outcome.is_ok(),
                () = tokio::time::sleep(Self::TICK) => false,
            };
            if changed {
                let snapshots = games.borrow_and_update().clone();
                if let Err(error) = Self::observe(&host, &session, &snapshots, &mut reported).await
                {
                    host.record_failure(error.to_string());
                }
            }
            let drained = host
                .drain(&mut session, || Self::lease(&capacity), &games)
                .await;
            if let Err(error) = drained {
                host.record_failure(error.to_string());
                break;
            }
        }
        if let Err(error) = session
            .shutdown_retained(tokio::time::Instant::now() + Self::SHUTDOWN)
            .await
        {
            host.record_failure(error.to_string());
        }
    }

    fn lease(capacity: &Mutex<CapacityPhase>) -> Option<CapacityLease> {
        capacity
            .try_lock()
            .ok()
            .and_then(|phase| phase.try_lease_workload(1).ok())
    }

    async fn observe<P: EngineProcess>(
        host: &DesktopNativeReportHost,
        session: &EngineSearchSession<P>,
        snapshots: &[DesktopGameSnapshot],
        reported: &mut HashSet<GameId>,
    ) -> Result<(), DesktopNativeReportHostError> {
        host.live_games_changed(DesktopNativeReportHost::running_games(snapshots))
            .await?;
        for snapshot in snapshots {
            if snapshot.phase() != GameEnumPhase::Completed || reported.contains(&snapshot.id()) {
                continue;
            }
            let Some(tree) = snapshot.completed_tree() else {
                continue;
            };
            reported.insert(snapshot.id());
            host.game_finished(DesktopNativeReportHostFinishedGame {
                id: ReportJobId::try_from(Uuid::new_v4())?,
                game: snapshot.id(),
                revision: snapshot.revision(),
                tree,
                tip: snapshot.live(),
                engine: session.engine(),
                limits: ReportJobSourceLimits::new(Self::SOURCE_POSITIONS, Self::SOURCE_BYTES)?,
            })
            .await?;
        }
        Ok(())
    }
}
