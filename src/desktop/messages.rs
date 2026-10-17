use std::sync::{Arc, OnceLock};

use crate::clock::duration::ClockInstant;
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsEnumOperation, DesktopEditorCommandsError,
};
use crate::desktop::error::DesktopFailure;
use crate::desktop::file_handle::DesktopFileEnumPurpose;
use crate::desktop::file_handle::DesktopFileHandleOwner;
use crate::desktop::game_bridge::DesktopGameBridgeEnumOperation;
use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::library_service::{DesktopLibraryServiceError, DesktopLibraryServiceOperation};
use crate::desktop::puzzle_bridge::DesktopPuzzleBridgeEnumOperation;
use crate::desktop::puzzle_service::DesktopPuzzleServiceEnumOperation;
use crate::puzzle::persistence::PuzzlePersistenceEnumCommand;
use crate::puzzle::run::PuzzleRunEnumConfirmation;
use crate::puzzle::session::PuzzleSessionId;
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DesktopMessagesEntityId(DesktopRequestId);

impl DesktopMessagesEntityId {
    pub const fn for_editor(draft: crate::board::editor::BoardEditorDraftId) -> Self {
        Self(DesktopRequestId::for_editor(draft))
    }
    pub const fn for_game(game: crate::game::enums::GameId) -> Self {
        Self(DesktopRequestId::for_game(game))
    }
    pub fn new() -> Self {
        Self(DesktopRequestId::new())
    }

    pub fn file_handle_owner(self) -> DesktopFileHandleOwner {
        DesktopFileHandleOwner::from_request_id(self.0)
    }
}

impl Default for DesktopMessagesEntityId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopEnumCommand {
    Library(DesktopLibraryServiceOperation),
    Editor {
        owner: DesktopMessagesEntityId,
        operation: DesktopEditorCommandsEnumOperation,
    },
    Game(DesktopGameBridgeEnumOperation),
    Backup(crate::desktop::backup_bridge::DesktopBackupBridgeEnumOperation),
    Engine(crate::desktop::engine_commands::DesktopEngineCommandsOperation),
    Puzzle {
        owner: DesktopMessagesEntityId,
        operation: DesktopPuzzleServiceEnumOperation,
    },
    Bootstrap,
    Update(crate::desktop::update_commands::DesktopUpdateOperation),
    FileSelect {
        owner: DesktopMessagesEntityId,
        purpose: DesktopFileEnumPurpose,
    },
    StreamSubscribe,
    StreamAcknowledge {
        subscription_id: DesktopRequestId,
        sequence: DesktopRevision,
    },
    StreamResync {
        subscription_id: DesktopRequestId,
    },
    Stop {
        owner: DesktopMessagesEntityId,
    },
    Pause {
        owner: DesktopMessagesEntityId,
    },
    Quit,
}

impl DesktopEnumCommand {
    pub fn is_available_while_closing(&self) -> bool {
        match self {
            Self::Library(operation) => operation.is_available_while_closing(),
            Self::Puzzle { operation, .. } => operation.is_available_while_closing(),
            Self::Game(_)
            | Self::Backup(_)
            | Self::Editor { .. }
            | Self::Engine(_)
            | Self::Bootstrap
            | Self::Update(_)
            | Self::FileSelect { .. }
            | Self::StreamSubscribe
            | Self::StreamAcknowledge { .. }
            | Self::StreamResync { .. }
            | Self::Stop { .. }
            | Self::Pause { .. }
            | Self::Quit => false,
        }
    }

    pub fn is_control(&self) -> bool {
        self.control_key().is_some()
    }

    pub(crate) fn control_key(&self) -> Option<DesktopMessagesEnumControlKey> {
        match self {
            Self::Puzzle { owner, operation } => {
                if !operation.is_urgent() {
                    return None;
                }
                match operation {
                    DesktopPuzzleServiceEnumOperation::Interrupt { session, reason } => {
                        Some(match reason {
                            PuzzleTimerEnumPauseReason::FocusLost
                            | PuzzleTimerEnumPauseReason::WindowHidden => {
                                DesktopMessagesEnumControlKey::PuzzleHidden(*owner, *session)
                            }
                            PuzzleTimerEnumPauseReason::Manual
                            | PuzzleTimerEnumPauseReason::Sleep
                            | PuzzleTimerEnumPauseReason::Recovery
                            | PuzzleTimerEnumPauseReason::SaveBackpressure => {
                                DesktopMessagesEnumControlKey::PuzzlePause(*owner, *session)
                            }
                        })
                    }
                    DesktopPuzzleServiceEnumOperation::Session { expected, operation } => {
                        let session = expected.session_id();
                        match operation {
                            DesktopPuzzleBridgeEnumOperation::Command(PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed)) => Some(DesktopMessagesEnumControlKey::PuzzleEnd(*owner, session)),
                            DesktopPuzzleBridgeEnumOperation::Command(PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Hidden)) => Some(DesktopMessagesEnumControlKey::PuzzleHidden(*owner, session)),
                            DesktopPuzzleBridgeEnumOperation::Command(PuzzlePersistenceEnumCommand::Pause(_)) => Some(DesktopMessagesEnumControlKey::PuzzlePause(*owner, session)),
                            DesktopPuzzleBridgeEnumOperation::Command(_)
                            | DesktopPuzzleBridgeEnumOperation::RetrySave
                            | DesktopPuzzleBridgeEnumOperation::Snapshot
                            | DesktopPuzzleBridgeEnumOperation::Summary => None,
                        }
                    }
                    DesktopPuzzleServiceEnumOperation::Inventory { .. }
                    | DesktopPuzzleServiceEnumOperation::OpenSaved { .. }
                    | DesktopPuzzleServiceEnumOperation::DeleteHistory { .. } => None,
                }
            }
            Self::Engine(operation) => match operation.action() {
                crate::desktop::engine_commands::DesktopEngineCommandsEnumOperation::Stop { .. } => Some(DesktopMessagesEnumControlKey::EngineStop(operation.context().owner())),
                crate::desktop::engine_commands::DesktopEngineCommandsEnumOperation::Workbench(crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation::Cancel { .. }) => Some(DesktopMessagesEnumControlKey::WorkbenchCancel(operation.context().owner())),
                crate::desktop::engine_commands::DesktopEngineCommandsEnumOperation::Snapshot
                | crate::desktop::engine_commands::DesktopEngineCommandsEnumOperation::Start { .. }
                | crate::desktop::engine_commands::DesktopEngineCommandsEnumOperation::Configure(_)
                | crate::desktop::engine_commands::DesktopEngineCommandsEnumOperation::Retry
                | crate::desktop::engine_commands::DesktopEngineCommandsEnumOperation::PrepareTablebases { .. }
                | crate::desktop::engine_commands::DesktopEngineCommandsEnumOperation::LoadTablebases
                | crate::desktop::engine_commands::DesktopEngineCommandsEnumOperation::Workbench(crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation::Start { .. } | crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation::Result { .. } | crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation::PreviewExport { .. } | crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation::ApplyExport { .. }) => None,
            },
            Self::Game(DesktopGameBridgeEnumOperation::Session { game, action, .. }) => {
                let owner = DesktopMessagesEntityId::for_game(*game);
                if matches!(action, DesktopGameCommandsEnumAction::Pause { .. }) {
                    Some(DesktopMessagesEnumControlKey::GamePause(owner))
                } else if matches!(action, DesktopGameCommandsEnumAction::Stop) {
                    Some(DesktopMessagesEnumControlKey::GameStop(owner))
                } else {
                    None
                }
            }
            Self::Game(DesktopGameBridgeEnumOperation::Create { .. }) => None,
            Self::Stop { owner } => Some(DesktopMessagesEnumControlKey::Stop(*owner)),
            Self::Pause { owner } => Some(DesktopMessagesEnumControlKey::Pause(*owner)),
            Self::Quit => Some(DesktopMessagesEnumControlKey::Quit),
            Self::Library(_)
            | Self::Backup(_)
            | Self::Editor { .. }
            | Self::Bootstrap
            | Self::Update(_)
            | Self::FileSelect { .. }
            | Self::StreamSubscribe
            | Self::StreamAcknowledge { .. }
            | Self::StreamResync { .. } => None,
        }
    }

    pub fn mutation_entity(&self) -> Option<DesktopMessagesEntityId> {
        match self {
            Self::Library(operation) => operation.mutation_entity(),
            Self::Editor { owner, operation } => operation.is_mutation().then_some(*owner),
            Self::Puzzle { owner, operation } => match operation {
                DesktopPuzzleServiceEnumOperation::OpenSaved { .. }
                | DesktopPuzzleServiceEnumOperation::DeleteHistory { .. }
                | DesktopPuzzleServiceEnumOperation::Session {
                    operation:
                        DesktopPuzzleBridgeEnumOperation::Command(_)
                        | DesktopPuzzleBridgeEnumOperation::RetrySave,
                    ..
                } => Some(*owner),
                DesktopPuzzleServiceEnumOperation::Inventory { .. }
                | DesktopPuzzleServiceEnumOperation::Interrupt { .. }
                | DesktopPuzzleServiceEnumOperation::Session {
                    operation:
                        DesktopPuzzleBridgeEnumOperation::Snapshot
                        | DesktopPuzzleBridgeEnumOperation::Summary,
                    ..
                } => None,
            },
            Self::Engine(operation) => operation
                .action()
                .is_mutation()
                .then_some(operation.context().owner()),
            Self::Game(operation) => Some(operation.owner()),
            Self::FileSelect { owner, .. } => Some(*owner),
            Self::Bootstrap
            | Self::Backup(_)
            | Self::Update(_)
            | Self::StreamSubscribe
            | Self::StreamAcknowledge { .. }
            | Self::StreamResync { .. }
            | Self::Stop { .. }
            | Self::Pause { .. }
            | Self::Quit => None,
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum DesktopMessagesEnumControlKey {
    EngineStop(DesktopMessagesEntityId),
    WorkbenchCancel(DesktopMessagesEntityId),
    GamePause(DesktopMessagesEntityId),
    GameStop(DesktopMessagesEntityId),
    PuzzlePause(DesktopMessagesEntityId, PuzzleSessionId),
    PuzzleHidden(DesktopMessagesEntityId, PuzzleSessionId),
    PuzzleEnd(DesktopMessagesEntityId, PuzzleSessionId),
    Stop(DesktopMessagesEntityId),
    Pause(DesktopMessagesEntityId),
    Quit,
}

impl DesktopMessagesEnumControlKey {
    pub(crate) const PER_ENTITY_CAPACITY: usize = 9;

    pub(crate) fn owner(self) -> Option<DesktopMessagesEntityId> {
        match self {
            Self::EngineStop(target)
            | Self::WorkbenchCancel(target)
            | Self::Stop(target)
            | Self::Pause(target)
            | Self::GameStop(target)
            | Self::GamePause(target)
            | Self::PuzzlePause(target, _)
            | Self::PuzzleHidden(target, _)
            | Self::PuzzleEnd(target, _) => Some(target),
            Self::Quit => None,
        }
    }

    pub(crate) fn targets_mutation(self, operation: &DesktopEnumCommand) -> bool {
        match self {
            Self::PuzzlePause(owner, session)
            | Self::PuzzleHidden(owner, session)
            | Self::PuzzleEnd(owner, session) => matches!(
                operation,
                DesktopEnumCommand::Puzzle {
                    owner: target_owner,
                    operation: DesktopPuzzleServiceEnumOperation::Session { expected, .. },
                } if *target_owner == owner
                    && expected.session_id() == session
                    && operation.mutation_entity().is_some()
            ),
            Self::EngineStop(_)
            | Self::WorkbenchCancel(_)
            | Self::Stop(_)
            | Self::Pause(_)
            | Self::GameStop(_)
            | Self::GamePause(_) => self.owner() == operation.mutation_entity(),
            Self::Quit => false,
        }
    }

    pub(crate) fn stopped_result(self) -> DesktopMessagesEnumResult {
        match self {
            Self::EngineStop(_) => DesktopMessagesEnumResult::Stopped,
            Self::WorkbenchCancel(_) => DesktopMessagesEnumResult::Stopped,
            Self::Stop(_) | Self::GameStop(_) => DesktopMessagesEnumResult::Stopped,
            Self::Pause(_) | Self::GamePause(_) => DesktopMessagesEnumResult::Paused,
            Self::PuzzlePause(_, _) | Self::PuzzleHidden(_, _) => DesktopMessagesEnumResult::Paused,
            Self::PuzzleEnd(_, _) => DesktopMessagesEnumResult::Stopped,
            Self::Quit => DesktopMessagesEnumResult::QuitReady,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopCommand {
    operation: DesktopEnumCommand,
    expected_revision: Option<DesktopRevision>,
}

impl DesktopCommand {
    pub fn editor(
        owner: DesktopMessagesEntityId,
        operation: DesktopEditorCommandsEnumOperation,
    ) -> Self {
        let expected_revision = Some(Self::editor_expected_revision(&operation));
        Self::new(
            DesktopEnumCommand::Editor { owner, operation },
            expected_revision,
        )
    }

    pub fn editor_expected_revision(
        operation: &DesktopEditorCommandsEnumOperation,
    ) -> DesktopRevision {
        operation
            .expected()
            .map_or(DesktopRevision::ZERO, |context| {
                DesktopRevision::for_editor(context.revision())
            })
    }

    pub fn library(operation: DesktopLibraryServiceOperation) -> Self {
        let expected_revision = operation.expected_revision();
        Self::new(DesktopEnumCommand::Library(operation), expected_revision)
    }

    pub fn puzzle(
        owner: DesktopMessagesEntityId,
        operation: DesktopPuzzleServiceEnumOperation,
    ) -> Self {
        let expected_revision = Some(operation.expected_revision());
        Self::new(
            DesktopEnumCommand::Puzzle { owner, operation },
            expected_revision,
        )
    }

    pub fn new(operation: DesktopEnumCommand, expected_revision: Option<DesktopRevision>) -> Self {
        Self {
            operation,
            expected_revision,
        }
    }

    pub fn operation(&self) -> &DesktopEnumCommand {
        &self.operation
    }

    pub fn expected_revision(&self) -> Option<DesktopRevision> {
        self.expected_revision
    }
}

struct DesktopMessagesRequestIdentity {
    session: DesktopRequestId,
    sequence: u64,
    request_id: DesktopRequestId,
    command: DesktopCommand,
    received_at: OnceLock<(ClockInstant, tokio::time::Instant)>,
    admitted_sequence: OnceLock<u64>,
}

#[derive(Clone)]
pub struct DesktopRequest(Arc<DesktopMessagesRequestIdentity>);

impl DesktopRequest {
    pub(crate) fn new(
        session: DesktopRequestId,
        sequence: u64,
        request_id: DesktopRequestId,
        command: DesktopCommand,
    ) -> Self {
        Self(Arc::new(DesktopMessagesRequestIdentity {
            session,
            sequence,
            request_id,
            command,
            received_at: OnceLock::new(),
            admitted_sequence: OnceLock::new(),
        }))
    }

    pub fn request_id(&self) -> DesktopRequestId {
        self.0.request_id
    }

    pub fn command(&self) -> &DesktopCommand {
        &self.0.command
    }

    pub(crate) fn session(&self) -> DesktopRequestId {
        self.0.session
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.0.sequence
    }

    pub(crate) fn first_receipt(&self, now: ClockInstant) -> ClockInstant {
        self.0
            .received_at
            .get_or_init(|| (now, tokio::time::Instant::now()))
            .0
    }

    pub(crate) fn runtime_receipt(&self) -> Option<tokio::time::Instant> {
        self.0.received_at.get().map(|(_clock, runtime)| *runtime)
    }

    pub(crate) fn admitted_sequence(&self) -> Option<u64> {
        self.0.admitted_sequence.get().copied()
    }

    pub(crate) fn retain_admission(&self, sequence: u64) {
        self.0.admitted_sequence.get_or_init(|| sequence);
    }
}

#[derive(Clone, Debug)]
pub enum DesktopMessagesEnumResult {
    LibraryFailure(Arc<DesktopLibraryServiceError>),
    EditorFailure(Arc<DesktopEditorCommandsError>),
    Value {
        value: DesktopEnumValue,
        revision: Option<DesktopRevision>,
    },
    Failure(DesktopFailure),
    Cancelled,
    Stopped,
    Paused,
    QuitReady,
    ReconciliationRequired,
}
