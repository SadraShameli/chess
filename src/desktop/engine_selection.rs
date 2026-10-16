use thiserror::Error;
use tokio::sync::watch;

use crate::desktop::engine_commands::DesktopEngineCommandsSelectionProvider;
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::game_snapshot::{
    DesktopGameSnapshot, DesktopGameSnapshotEnumAnalysis,
    DesktopGameSnapshotEnumAnalysisUnavailable,
};
use crate::engine::search::identity::EngineSearchContext;
use crate::engine::search::position::EngineSearchPosition;
use crate::game::enums::{GameEnumPhase, GameId};

#[derive(Debug, Error)]
pub enum DesktopEngineSelectionError {
    #[error("The authoritative selected-game source is disconnected")]
    Disconnected,
    #[error("Select an authoritative analysis or completed game before using the engine")]
    Missing,
    #[error("The selected game or position revision has changed")]
    Stale,
    #[error("Engine assistance is unavailable in setup, active or interrupted live play")]
    LivePlay,
    #[error("The selected position history exceeds the engine command limit")]
    OversizedHistory,
    #[error(
        "The supplied engine position does not match the selected node and its complete original history"
    )]
    Position,
}

impl From<DesktopEngineSelectionError> for DesktopFailure {
    fn from(error: DesktopEngineSelectionError) -> Self {
        let code = match error {
            DesktopEngineSelectionError::Stale => DesktopEnumFailureCode::StaleRevision,
            DesktopEngineSelectionError::Missing
            | DesktopEngineSelectionError::Disconnected
            | DesktopEngineSelectionError::LivePlay
            | DesktopEngineSelectionError::OversizedHistory => {
                DesktopEnumFailureCode::ResourceRefusal
            }
            DesktopEngineSelectionError::Position => DesktopEnumFailureCode::InvalidInput,
        };
        Self::new(code, error.to_string(), true)
    }
}

pub struct DesktopEngineSelection {
    snapshots: watch::Receiver<Vec<DesktopGameSnapshot>>,
    selected: watch::Receiver<Option<GameId>>,
}

impl DesktopEngineSelection {
    pub const fn new(
        snapshots: watch::Receiver<Vec<DesktopGameSnapshot>>,
        selected: watch::Receiver<Option<GameId>>,
    ) -> Self {
        Self {
            snapshots,
            selected,
        }
    }

    fn selected(
        &self,
        context: EngineSearchContext,
        position: Option<&EngineSearchPosition>,
    ) -> Result<(), DesktopEngineSelectionError> {
        if self.selected.has_changed().is_err() || self.snapshots.has_changed().is_err() {
            return Err(DesktopEngineSelectionError::Disconnected);
        }
        let selected = self.selected.borrow();
        let game = selected.ok_or(DesktopEngineSelectionError::Missing)?;
        if game.as_uuid() != context.game().value() {
            return Err(DesktopEngineSelectionError::Stale);
        }
        let snapshots = self.snapshots.borrow();
        let snapshot = snapshots
            .iter()
            .find(|snapshot| snapshot.id() == game)
            .ok_or(DesktopEngineSelectionError::Missing)?;
        if snapshot.revision().value() != context.revision().value() {
            return Err(DesktopEngineSelectionError::Stale);
        }
        match snapshot.phase() {
            GameEnumPhase::Analysis | GameEnumPhase::Completed => {}
            GameEnumPhase::Setup | GameEnumPhase::Active | GameEnumPhase::Interrupted => {
                return Err(DesktopEngineSelectionError::LivePlay);
            }
        }
        match snapshot.analysis() {
            DesktopGameSnapshotEnumAnalysis::Available(projection)
                if projection.node() == snapshot.view() =>
            {
                if let Some(position) = position {
                    let authoritative = projection.position();
                    if position.initial() != authoritative.initial()
                        || position.history() != authoritative.history()
                        || position.current() != authoritative.current()
                    {
                        return Err(DesktopEngineSelectionError::Position);
                    }
                }
                Ok(())
            }
            DesktopGameSnapshotEnumAnalysis::Available(_) => {
                Err(DesktopEngineSelectionError::Stale)
            }
            DesktopGameSnapshotEnumAnalysis::Unavailable(
                DesktopGameSnapshotEnumAnalysisUnavailable::LivePlay,
            ) => Err(DesktopEngineSelectionError::LivePlay),
            DesktopGameSnapshotEnumAnalysis::Unavailable(
                DesktopGameSnapshotEnumAnalysisUnavailable::OversizedHistory,
            ) => Err(DesktopEngineSelectionError::OversizedHistory),
        }
    }
}

impl DesktopEngineCommandsSelectionProvider for DesktopEngineSelection {
    fn authorize_context(&self, context: EngineSearchContext) -> Result<(), DesktopFailure> {
        self.selected(context, None)?;
        Ok(())
    }

    fn authorize(
        &self,
        context: EngineSearchContext,
        position: &EngineSearchPosition,
    ) -> Result<(), DesktopFailure> {
        self.selected(context, Some(position))?;
        Ok(())
    }
}
