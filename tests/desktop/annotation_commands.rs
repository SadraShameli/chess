use std::sync::Arc;

use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
};
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationWakeError, DesktopApplicationWakeProvider,
};
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorEpoch,
    DesktopGameEditorGeneration,
};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::game::enums::{GameId, GameRevision};
use crate::game::storage_repository::GameStorageRevision;
use crate::notation::pgn_import::NotationPgnImport;
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};

struct DesktopAnnotationCommandsClock;

impl ClockTimeProvider for DesktopAnnotationCommandsClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(41))
    }
}

struct DesktopAnnotationCommandsWake;

impl DesktopApplicationWakeProvider for DesktopAnnotationCommandsWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

#[tokio::test]
async fn command_retains_the_actual_admitted_application_outcome_and_bound_context()
-> Result<(), Box<dyn std::error::Error>> {
    let application = DesktopApplication::new(
        Arc::new(DesktopAnnotationCommandsClock),
        Arc::new(DesktopAnnotationCommandsWake),
    );
    let port = application.port();
    let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
    let receipt = port.try_submit(&request)?;
    let work = application.try_next()?.ok_or("missing admitted work")?;
    let mut node = 0_u128;
    let mut parser = NotationPgnImport::new(std::io::Cursor::new("1. e4 *"), move || {
        node += 1;
        uuid::Uuid::from_u128(node).to_string().parse()
    });
    let game = parser.next_game(&|| false)?.ok_or("missing notation")?;
    let root = game.tree().root();
    let id: GameId = "e707c2dd-3e4a-4a22-8159-47fa63696071".parse()?;
    let record =
        StorageLibraryRecord::new(id, StorageLibraryEnumContent::Notation(Box::new(game)))?;
    let context = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO,
        root,
        DesktopGameEditorBinding::new(
            GameStorageRevision::new(3),
            DesktopGameEditorEpoch::from_request_id(work.receipt().request_id()),
            DesktopGameEditorGeneration::ZERO,
        ),
    )?
    .context();
    let command = DesktopAnnotationCommands::new(
        work.receipt().clone(),
        context,
        DesktopAnnotationCommandsEnumAction::Snapshot,
    );
    assert!(command.receipt().shares_outcome(&receipt));
    assert_eq!(command.expected(), context);
    assert_eq!(
        command.receipt().received_at(),
        ClockInstant::from_nanos(41)
    );
    let quit = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
    port.try_submit(&quit)?;
    assert!(command.receipt().is_cancellation_requested());
    Ok(())
}
