use crate::board::enums::BoardEnumColor;
use crate::clock::duration::ClockInstant;
use crate::clock::state::{ClockStateEnumStatus, ClockStateError};
use crate::desktop::game_snapshot::{DesktopGameSnapshot, DesktopGameSnapshotEnumAdmission};
use crate::game::enums::{GameEnumPhase, GameId};
use crate::presentation::game::play::PresentationGamePlayClock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::Display)]
pub enum PresentationGameOverviewEnumStatus {
    #[strum(serialize = "Running")]
    Running,
    #[strum(serialize = "Waiting for engine capacity")]
    WaitingForCapacity,
    #[strum(serialize = "Paused")]
    Paused,
    #[strum(serialize = "Finished")]
    Finished,
}

impl PresentationGameOverviewEnumStatus {
    pub fn label(self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationGameOverviewRow {
    id: GameId,
    ordinal: usize,
    turn: BoardEnumColor,
    clock: String,
    status: PresentationGameOverviewEnumStatus,
    unsaved: bool,
    resources: Option<String>,
}

impl PresentationGameOverviewRow {
    pub const fn id(&self) -> GameId {
        self.id
    }

    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    pub const fn turn(&self) -> BoardEnumColor {
        self.turn
    }

    pub fn clock(&self) -> &str {
        &self.clock
    }

    pub const fn status(&self) -> PresentationGameOverviewEnumStatus {
        self.status
    }

    pub const fn is_unsaved(&self) -> bool {
        self.unsaved
    }

    pub fn resources(&self) -> Option<&str> {
        self.resources.as_deref()
    }

    pub fn tab_label(&self) -> String {
        let mut label = format!("Game {}", self.ordinal);
        if self.unsaved {
            label.push_str(" *");
        }
        label
    }

    pub fn summary(&self) -> String {
        let turn = match self.turn {
            BoardEnumColor::White => "White",
            BoardEnumColor::Black => "Black",
        };
        let mut summary = format!(
            "{}: {} to move, {}, {}",
            self.tab_label(),
            turn,
            self.clock,
            self.status.label()
        );
        if self.unsaved {
            summary.push_str(", unsaved changes");
        }
        if let Some(resources) = &self.resources {
            summary.push_str(", ");
            summary.push_str(resources);
        }
        summary
    }
}

pub struct PresentationGameOverview;

impl PresentationGameOverview {
    pub fn rows<'a>(
        snapshots: impl IntoIterator<Item = &'a DesktopGameSnapshot>,
        now: ClockInstant,
    ) -> Result<Vec<PresentationGameOverviewRow>, ClockStateError> {
        let mut rows = Vec::new();
        for (index, snapshot) in snapshots.into_iter().enumerate() {
            rows.push(Self::row(index.saturating_add(1), snapshot, now)?);
        }
        Ok(rows)
    }

    pub fn row(
        ordinal: usize,
        snapshot: &DesktopGameSnapshot,
        now: ClockInstant,
    ) -> Result<PresentationGameOverviewRow, ClockStateError> {
        let view = snapshot.clock().view(now)?;
        let turn = view.active();
        let clock = PresentationGamePlayClock::text(view.side(turn).displayed());
        let status = if snapshot.phase() == GameEnumPhase::Completed {
            PresentationGameOverviewEnumStatus::Finished
        } else if snapshot.admission() == DesktopGameSnapshotEnumAdmission::WaitingForCapacity {
            PresentationGameOverviewEnumStatus::WaitingForCapacity
        } else if snapshot.phase() == GameEnumPhase::Active
            && view.status() == ClockStateEnumStatus::Running
        {
            PresentationGameOverviewEnumStatus::Running
        } else {
            PresentationGameOverviewEnumStatus::Paused
        };
        let resources = snapshot.allocation().map(|allocation| {
            format!(
                "{} threads, {} MiB hash",
                allocation.workers(),
                allocation.hash_mib()
            )
        });
        Ok(PresentationGameOverviewRow {
            id: snapshot.id(),
            ordinal,
            turn,
            clock,
            status,
            unsaved: snapshot.has_unsaved_changes(),
            resources,
        })
    }
}
