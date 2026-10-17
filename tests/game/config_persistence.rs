use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::board::start::BoardStart;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::game_bridge::{
    DesktopGameBridgeEnumOperation, DesktopGameBridgeProvider, DesktopGameBridgeRequest,
};
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::messages::DesktopMessagesEntityId;
use chess::game::configuration::{
    GameConfiguration, GameConfigurationEnumOpponent, GameConfigurationEnumStart,
    GameConfigurationOptions,
};
use chess::game::engine_provider::{GameEngineRequestError, GameEngineSearchConfig};
use chess::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameEnumSeat};
use chess::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionError, GameSessionOptions,
};
use chess::game::storage_repository::GameStorageRepository;
use chess::storage::game_document::{
    StorageGameDocument, StorageGameDocumentError, StorageGameDocumentRepository,
};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::desktop_game_registry_fixture::{
    DesktopGameRegistryFixtureProof, DesktopGameRegistryFixtureRuntime,
};

type GameConfigPersistenceTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct GameConfigPersistenceEnvelope {
    schema_version: u32,
    game: GameConfigPersistenceRecord,
}

#[derive(Deserialize)]
struct GameConfigPersistenceRecord {
    play_options: GameConfigPersistenceOptions,
}

#[derive(Deserialize, Serialize)]
struct GameConfigPersistenceOptions {
    untimed_move_time_nanos: u64,
    move_overhead_nanos: u64,
    is_auto_flip_enabled: bool,
}

impl GameConfigPersistenceOptions {
    fn field(&self) -> GameConfigPersistenceTestResult<String> {
        Ok(format!(
            ",\"play_options\":{}",
            serde_json::to_string(self)?
        ))
    }
}

struct GameConfigPersistenceFixture;

impl GameConfigPersistenceFixture {
    fn session() -> GameConfigPersistenceTestResult<GameSession> {
        Ok(GameSession::new(GameSessionOptions {
            id: "118e68ba-2d4f-4bbf-844f-412549cfbe07".parse()?,
            root: "74760178-d2de-48f6-815c-3703c6f5741a".parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?)
    }

    fn selected() -> GameConfigPersistenceTestResult<GameEngineSearchConfig> {
        Ok(GameEngineSearchConfig::new(
            ClockDuration::from_millis(4_321)?,
            ClockDuration::from_millis(37)?,
        )?)
    }

    fn default_json() -> GameConfigPersistenceTestResult<String> {
        Ok(StorageGameDocument::capture(&Self::session()?, ClockInstant::from_nanos(0))?.json()?)
    }

    fn without_play_options(json: &str) -> GameConfigPersistenceTestResult<String> {
        let envelope: GameConfigPersistenceEnvelope = serde_json::from_str(json)?;
        let field = envelope.game.play_options.field()?;
        if !json.contains(&field) {
            return Err("the fixture has no complete serialized play options".into());
        }
        Ok(json.replacen(&field, "", 1))
    }
}

#[tokio::test]
async fn selected_game_search_budget_and_auto_flip_survive_registry_save_and_reload()
-> GameConfigPersistenceTestResult {
    let runtime =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let (mut registry, port) = runtime.registry();
    let worker = tokio::spawn(async move {
        let result = registry.run().await;
        (registry, result)
    });
    let selected = GameConfigPersistenceFixture::selected()?;
    let snapshot = port
        .execute(DesktopGameBridgeRequest {
            request_id: DesktopRequestId::new(),
            received_at: ClockInstant::from_nanos(0),
            operation: DesktopGameBridgeEnumOperation::Create {
                owner: DesktopMessagesEntityId::new(),
                configuration: GameConfiguration::new(
                    GameConfigurationEnumStart::Standard,
                    GameConfigurationEnumOpponent::LocalTwoPlayer,
                    ClockTimeControl::untimed(),
                    GameConfigurationOptions::new(selected, true, false, true),
                ),
            },
        })
        .await
        .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
    port.shutdown_all(Instant::now() + Duration::from_secs(3))?
        .wait()
        .await?;
    let (registry, result) = worker.await?;
    result?;
    let repository = StorageGameDocumentRepository::new(runtime.store.client(), &runtime.executor);
    let restored = repository
        .load(snapshot.id())
        .await?
        .ok_or("the selected game was not saved")?;
    let json =
        StorageGameDocument::capture(restored.session(), ClockInstant::from_nanos(0))?.json()?;
    assert_eq!(restored.session().engine_search_config(), selected);
    assert!(restored.session().is_auto_flip_enabled());
    drop((registry, port, repository, restored));
    runtime.close().await?;
    let saved: GameConfigPersistenceEnvelope = serde_json::from_str(&json)?;
    assert_eq!(saved.schema_version, StorageGameDocument::SCHEMA_VERSION);
    assert_eq!(
        saved.game.play_options.untimed_move_time_nanos,
        selected.untimed_move_time().as_nanos()
    );
    assert_eq!(
        saved.game.play_options.move_overhead_nanos,
        selected.move_overhead().as_nanos()
    );
    assert!(saved.game.play_options.is_auto_flip_enabled);
    Ok(())
}

#[test]
fn session_play_options_require_the_current_setup_revision() -> GameConfigPersistenceTestResult {
    let mut session = GameConfigPersistenceFixture::session()?;
    let selected = GameConfigPersistenceFixture::selected()?;
    let initial_revision = session.revision();
    let initial_config = session.engine_search_config();
    assert!(matches!(
        session.configure_play_options(initial_revision.next()?, selected, true),
        Err(GameSessionError::Revision(_))
    ));
    assert_eq!(session.revision(), initial_revision);
    assert_eq!(session.engine_search_config(), initial_config);
    assert!(!session.is_auto_flip_enabled());
    session.configure_play_options(initial_revision, selected, true)?;
    assert_eq!(session.engine_search_config(), selected);
    assert!(session.is_auto_flip_enabled());
    session.start(
        session.revision(),
        ClockInstant::from_nanos(0),
        GameEnumEngineReadiness::Ready,
        &DesktopGameRegistryFixtureProof::default(),
    )?;
    let active_revision = session.revision();
    assert!(matches!(
        session.configure_play_options(active_revision, initial_config, false),
        Err(GameSessionError::WrongPhase(_))
    ));
    assert_eq!(session.revision(), active_revision);
    assert_eq!(session.engine_search_config(), selected);
    assert!(session.is_auto_flip_enabled());
    Ok(())
}

#[test]
fn session_restoration_keeps_selected_play_options() -> GameConfigPersistenceTestResult {
    let mut session = GameConfigPersistenceFixture::session()?;
    let selected = GameConfigPersistenceFixture::selected()?;
    session.configure_play_options(session.revision(), selected, true)?;
    let restored = GameSession::restore(session.restoration(ClockInstant::from_nanos(0))?)?;
    assert_eq!(restored.revision(), session.revision());
    assert_eq!(restored.engine_search_config(), selected);
    assert!(restored.is_auto_flip_enabled());
    Ok(())
}

#[test]
fn current_game_schema_rejects_missing_play_options() -> GameConfigPersistenceTestResult {
    let json = GameConfigPersistenceFixture::default_json()?;
    let incomplete = GameConfigPersistenceFixture::without_play_options(&json)?;
    assert!(matches!(
        StorageGameDocument::restore(&incomplete),
        Err(StorageGameDocumentError::MissingPlayOptions)
    ));
    Ok(())
}

#[test]
fn stored_search_budget_is_revalidated_before_game_restoration() -> GameConfigPersistenceTestResult
{
    let json = GameConfigPersistenceFixture::default_json()?;
    let mut envelope: GameConfigPersistenceEnvelope = serde_json::from_str(&json)?;
    let original = envelope.game.play_options.field()?;
    envelope.game.play_options.untimed_move_time_nanos = 0;
    let invalid = json.replacen(&original, &envelope.game.play_options.field()?, 1);
    assert!(matches!(
        StorageGameDocument::restore(&invalid),
        Err(StorageGameDocumentError::EngineRequest(
            GameEngineRequestError::EmptyBudget
        ))
    ));
    Ok(())
}
