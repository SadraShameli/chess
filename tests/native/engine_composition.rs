use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use chess::capacity::phase::CapacityPhaseEnumState;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::application::DesktopApplicationError;
use chess::desktop::dto::DesktopEnumValue;
use chess::desktop::engine_bridge::DesktopEngineBridgeEnumResult;
use chess::desktop::engine_commands::{
    DesktopEngineCommandsContext, DesktopEngineCommandsEnumOperation,
    DesktopEngineCommandsOperation,
};
use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
};
use chess::desktop::native::bootstrap::DesktopNativeBootstrap;
use chess::desktop::native::host::{DesktopNativeHostEvent, DesktopNativeHostServices};
use chess::game::configuration::{
    GameConfiguration, GameConfigurationEnumOpponent, GameConfigurationEnumStart,
    GameConfigurationOptions,
};
use chess::presentation::engine_panels::PresentationEnginePanels;

use chess::board::enums::BoardEnumColor;
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::session::EngineSearchSessionEnumCompletion;

pub(super) struct NativeEngineCompositionHarness;

type NativeEngineCompositionResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

impl NativeEngineCompositionHarness {
    const DEADLINE: Duration = Duration::from_secs(30);

    pub(super) fn run() -> NativeEngineCompositionResult {
        let directory = tempfile::tempdir()?;
        let mut observed = None;
        let mut budget = None;
        DesktopNativeBootstrap::run_with_host(
            [
                OsString::from("--profile-directory"),
                directory.path().join("profile").into_os_string(),
            ],
            |mut services| {
                budget = Some(services.budget.clone());
                let runtime = services.runtime.clone();
                observed = Some(runtime.block_on(async {
                    tokio::time::timeout(Self::DEADLINE, Self::exercise(&mut services)).await?
                }));
                Ok(())
            },
        )?;
        observed.ok_or("native composition did not receive production host services")??;
        let budget = budget.ok_or("native composition did not observe its shared budget")?;
        if budget.available() != budget.workers() {
            return Err("production root retained a CPU lease after shutdown".into());
        }
        Ok(())
    }

    async fn command(
        services: &DesktopNativeHostServices,
        command: DesktopEnumCommand,
    ) -> NativeEngineCompositionResult<DesktopEnumValue> {
        let revision = match &command {
            DesktopEnumCommand::Editor { operation, .. } => {
                Some(DesktopCommand::editor_expected_revision(operation))
            }
            DesktopEnumCommand::Library(operation) => operation.expected_revision(),
            DesktopEnumCommand::Puzzle { operation, .. } => Some(operation.expected_revision()),
            DesktopEnumCommand::Engine(operation) => Some(operation.context().configuration()),
            DesktopEnumCommand::Game(operation) => Some(operation.revision()),
            DesktopEnumCommand::FileSelect { .. } => {
                Some(chess::desktop::identity::DesktopRevision::ZERO)
            }
            DesktopEnumCommand::Bootstrap
            | DesktopEnumCommand::StreamSubscribe
            | DesktopEnumCommand::StreamAcknowledge { .. }
            | DesktopEnumCommand::StreamResync { .. }
            | DesktopEnumCommand::Stop { .. }
            | DesktopEnumCommand::Pause { .. }
            | DesktopEnumCommand::Quit => None,
        };
        let request = services
            .port
            .request(DesktopCommand::new(command, revision))?;
        let receipt = loop {
            match services.port.try_submit(&request) {
                Err(DesktopApplicationError::Busy) => tokio::task::yield_now().await,
                result => break result?,
            }
        };
        loop {
            services.wake.acknowledge();
            services.port.acknowledge_wake();
            if let Some(response) = receipt.response() {
                return match response {
                    DesktopMessagesEnumResult::Value { value, .. } => Ok(value.clone()),
                    DesktopMessagesEnumResult::Failure(failure) => Err(failure.clone().into()),
                    DesktopMessagesEnumResult::LibraryFailure(failure) => {
                        Err(Arc::clone(failure).into())
                    }
                    DesktopMessagesEnumResult::EditorFailure(failure) => {
                        Err(Arc::clone(failure).into())
                    }
                    DesktopMessagesEnumResult::Cancelled
                    | DesktopMessagesEnumResult::Stopped
                    | DesktopMessagesEnumResult::Paused
                    | DesktopMessagesEnumResult::QuitReady
                    | DesktopMessagesEnumResult::ReconciliationRequired => {
                        Err("unexpected native composition command outcome".into())
                    }
                };
            }
            services.wake.notified().await;
        }
    }

    fn game(value: DesktopEnumValue) -> NativeEngineCompositionResult<DesktopGameSnapshot> {
        if let DesktopEnumValue::Game(snapshot) = value {
            Ok(*snapshot)
        } else {
            Err("expected authoritative game snapshot".into())
        }
    }

    async fn exercise(services: &mut DesktopNativeHostServices) -> NativeEngineCompositionResult {
        let mut connection = services
            .engine_panels
            .take()
            .ok_or("packaged native analysis child was not admitted")?;
        let generation = services
            .capacity
            .lock()
            .map_err(|error| error.to_string())?
            .generation()
            .ok_or("missing framework preparation generation")?;
        services
            .events
            .send_replace(DesktopNativeHostEvent::FrameworkReady { generation });
        loop {
            services.wake.acknowledge();
            let state = services
                .capacity
                .lock()
                .map_err(|error| error.to_string())?
                .state();
            if state == CapacityPhaseEnumState::Steady {
                break;
            }
            services.wake.notified().await;
        }
        let created = Self::game(
            Self::command(
                services,
                DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Create {
                    owner: DesktopMessagesEntityId::new(),
                    configuration: GameConfiguration::new(
                        GameConfigurationEnumStart::Standard,
                        GameConfigurationEnumOpponent::LocalTwoPlayer,
                        ClockTimeControl::untimed(),
                        GameConfigurationOptions::initial_default(),
                    ),
                }),
            )
            .await?,
        )?;
        let completed = Self::game(
            Self::command(
                services,
                DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
                    game: created.id(),
                    revision: created.revision(),
                    action: DesktopGameCommandsEnumAction::Resign {
                        side: BoardEnumColor::White,
                    },
                }),
            )
            .await?,
        )?;
        let selection = PresentationEnginePanels::selection_from(&completed)?
            .ok_or("completed game lacks its authoritative history")?;
        connection.selected.send_replace(Some(completed.id()));
        let context = DesktopEngineCommandsContext::new(
            connection.owner,
            connection.initial.generation(),
            connection.initial.configuration(),
            selection.context(),
        );
        let result = Self::command(
            services,
            DesktopEnumCommand::Engine(DesktopEngineCommandsOperation::new(
                context,
                DesktopEngineCommandsEnumOperation::Start {
                    position: selection.position().clone(),
                    limit: EngineSearchLimit::infinite(),
                },
            )),
        )
        .await?;
        let DesktopEnumValue::Engine(result) = result else {
            return Err("expected correlated engine Start result".into());
        };
        let DesktopEngineBridgeEnumResult::Started(metadata) = *result else {
            return Err("native engine did not start analysis".into());
        };
        if metadata.tag().generation() != connection.initial.generation() {
            return Err("analysis reused another process generation".into());
        }
        loop {
            let searching = connection
                .telemetry
                .borrow_and_update()
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.metadata().tag() == metadata.tag()
                        && snapshot.latest_update().is_some()
                });
            if searching {
                break;
            }
            connection.telemetry.changed().await?;
        }
        let result = Self::command(
            services,
            DesktopEnumCommand::Engine(DesktopEngineCommandsOperation::new(
                context,
                DesktopEngineCommandsEnumOperation::Stop {
                    tag: metadata.tag(),
                },
            )),
        )
        .await?;
        let DesktopEnumValue::Engine(result) = result else {
            return Err("expected correlated engine Stop result".into());
        };
        let DesktopEngineBridgeEnumResult::Stopped(terminal) = *result else {
            return Err("native engine did not observe its final stop".into());
        };
        if !matches!(
            terminal.completion(),
            EngineSearchSessionEnumCompletion::Stopped
        ) || terminal.snapshot().metadata().tag() != metadata.tag()
        {
            return Err(format!(
                "Native stop returned {:?} with tag {:?}; expected {:?}",
                terminal.completion(),
                terminal.snapshot().metadata().tag(),
                metadata.tag(),
            )
            .into());
        }
        if completed.revision().value() != selection.context().revision().value() {
            return Err("analysis changed the game revision".into());
        }
        Ok(())
    }
}
