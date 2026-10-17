use std::future::Future;
use std::pin::Pin;

use tokio::sync::watch;

use crate::clock::duration::ClockInstant;
use crate::desktop::error::DesktopFailure;
use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::game::configuration::GameConfiguration;
use crate::game::enums::{GameEnumPhase, GameId, GameRevision};
use crate::game::pause::GamePauseEnumCause;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopGameBridgeEnumOperation {
    Create {
        owner: DesktopMessagesEntityId,
        configuration: GameConfiguration,
    },
    Session {
        game: GameId,
        revision: GameRevision,
        action: DesktopGameCommandsEnumAction,
    },
    Close {
        game: GameId,
        revision: GameRevision,
    },
}

impl DesktopGameBridgeEnumOperation {
    pub fn pause_all<'a>(
        snapshots: impl IntoIterator<Item = &'a DesktopGameSnapshot>,
    ) -> Vec<Self> {
        snapshots
            .into_iter()
            .filter(|snapshot| snapshot.phase() == GameEnumPhase::Active)
            .map(|snapshot| Self::Session {
                game: snapshot.id(),
                revision: snapshot.revision(),
                action: DesktopGameCommandsEnumAction::Pause {
                    cause: GamePauseEnumCause::Manual,
                },
            })
            .collect()
    }

    pub fn owner(&self) -> DesktopMessagesEntityId {
        match self {
            Self::Create { owner, .. } => *owner,
            Self::Session { game, .. } | Self::Close { game, .. } => {
                DesktopMessagesEntityId::for_game(*game)
            }
        }
    }

    pub const fn revision(&self) -> DesktopRevision {
        match self {
            Self::Create { .. } => DesktopRevision::ZERO,
            Self::Session { revision, .. } | Self::Close { revision, .. } => {
                DesktopRevision::for_game(*revision)
            }
        }
    }
}

pub struct DesktopGameBridgeRequest {
    pub request_id: DesktopRequestId,
    pub received_at: ClockInstant,
    pub operation: DesktopGameBridgeEnumOperation,
}

pub type DesktopGameBridgeFuture =
    Pin<Box<dyn Future<Output = Result<DesktopGameSnapshot, DesktopFailure>> + Send + 'static>>;

pub trait DesktopGameBridgeProvider: Send + Sync {
    fn execute(&self, request: DesktopGameBridgeRequest) -> DesktopGameBridgeFuture;
    fn snapshots(&self) -> watch::Receiver<Vec<DesktopGameSnapshot>>;
}
