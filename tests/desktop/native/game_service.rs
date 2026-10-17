use std::sync::{Arc, Mutex, atomic::Ordering};
use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::board::start::BoardStart;
use chess::capacity::phase::CapacityPhase;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::error::DesktopFailure;
use chess::desktop::game_bridge::DesktopGameBridgeProvider;
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::game_registry::{
    DesktopGameRegistryFactoryProvider, DesktopGameRegistryOptions,
};
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::native::game_runtime::DesktopNativeGameRuntimeError;
use chess::desktop::native::game_service::{
    DesktopNativeGameService, DesktopNativeGameServiceError,
};
use chess::desktop::native::library_startup::DesktopNativeLibraryStartupEnumReadiness;
use chess::game::enums::{
    GameEnumEngineReadiness, GameEnumMode, GameEnumPhase, GameEnumSeat, GameId, GameMoveId,
};
use chess::game::pause::GamePauseEnumCause;
use chess::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionEnumInterruption, GameSessionMoveRequest,
    GameSessionOptions,
};
use chess::game::storage_repository::{
    GameStorageRepository, GameStorageRevision, GameStorageSaveRequest,
};
use chess::game::tree::GameTree;
use chess::notation::game::{NotationGame, NotationGameParts};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::storage::game_document::{StorageGameDocument, StorageGameDocumentRepository};
use chess::storage::library_record::StorageLibraryEnumContent;
use tokio::sync::watch;
use tokio::time::{Instant, timeout, timeout_at};

use crate::desktop_game_registry_fixture::{
    DesktopGameRegistryFixture, DesktopGameRegistryFixtureFactories,
    DesktopGameRegistryFixtureFactory, DesktopGameRegistryFixtureGate,
    DesktopGameRegistryFixtureProof, DesktopGameRegistryFixtureRuntime,
    DesktopGameRegistryTestResult,
};

#[derive(Clone, Copy)]
pub(crate) enum DesktopNativeGameServiceFixtureEnumDocument {
    Notation,
    Unrelated,
    CorruptPlayed,
    FuturePlayed,
}

impl DesktopNativeGameServiceFixtureEnumDocument {
    pub(crate) fn body(self, id: GameId) -> Result<String, Box<dyn std::error::Error>> {
        match self {
            Self::Notation => {
                let root = "20000000-0000-4000-8000-000000000002".parse()?;
                let tip = "20000000-0000-4000-8000-000000000003".parse()?;
                let start = BoardStart::standard();
                let mut tree = GameTree::new(root, start.position().clone(), None);
                let movement = start.position().parse_uci("c2c4")?;
                tree.append(root, tip, movement, None)?;
                let notation = NotationGame::new(NotationGameParts {
                    start,
                    tree,
                    mainline_tip: tip,
                    metadata: GameMetadata::default(),
                    result: NotationGameIdentityEnumResult::Unfinished,
                    result_header: None,
                    time_control: None,
                    clocks: Vec::new(),
                    warnings: Vec::new(),
                })?;
                Ok(StorageGameDocument::capture_notation(id, &notation)?.json()?)
            }
            Self::Unrelated => Ok(r#"{"workspace":{"selected_panel":"library"}}"#.to_owned()),
            Self::CorruptPlayed => {
                Ok(r#"{"schema_version":1,"kind":"played","game":null}"#.to_owned())
            }
            Self::FuturePlayed => {
                Ok(r#"{"schema_version":99,"kind":"played","game":null}"#.to_owned())
            }
        }
    }
}

struct DesktopNativeGameServiceFixtureFactories {
    inner: DesktopGameRegistryFixtureFactories,
    games: Arc<Mutex<Vec<GameId>>>,
}

impl DesktopGameRegistryFactoryProvider for DesktopNativeGameServiceFixtureFactories {
    type Factory = DesktopGameRegistryFixtureFactory;
    type Error = std::io::Error;

    fn factory(&mut self, game: GameId) -> Result<Self::Factory, Self::Error> {
        let factory = self.inner.factory(game)?;
        self.games
            .lock()
            .map_err(|_poisoned| std::io::Error::other("factory admissions poisoned"))?
            .push(game);
        Ok(factory)
    }
}

pub(crate) struct DesktopNativeGameServiceFixture;

impl DesktopNativeGameServiceFixture {
    fn failure(error: DesktopFailure) -> std::io::Error {
        std::io::Error::other(error.message().to_owned())
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(3)
    }

    async fn wait_failure(
        failures: &mut watch::Receiver<Option<Arc<DesktopNativeGameRuntimeError>>>,
    ) -> Result<Arc<DesktopNativeGameRuntimeError>, Box<dyn std::error::Error>> {
        timeout(Duration::from_secs(3), failures.wait_for(Option::is_some))
            .await??
            .clone()
            .ok_or_else(|| "missing published runtime failure".into())
    }

    pub(crate) fn played(
        id: GameId,
        root: GameMoveId,
        node: GameMoveId,
        uci: &str,
    ) -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session = GameSession::new(GameSessionOptions {
            id,
            root,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(
                ClockTimeControl::sudden_death(ClockDuration::from_millis(60_000)?)?,
                BoardEnumColor::White,
            ),
            metadata: GameMetadata::default(),
        })?;
        let proof = DesktopGameRegistryFixtureProof::default();
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::NotRequired,
            &proof,
        )?;
        let movement = session.start_position().position().parse_uci(uci)?;
        session.accept_move(
            GameSessionMoveRequest {
                expected_revision: session.revision(),
                seat: GameEnumSeat::Human,
                color: BoardEnumColor::White,
                node,
                movement,
                received_at: ClockInstant::from_millis(500)?,
            },
            &proof,
        )?;
        Ok(session)
    }

    fn assert_played(
        snapshot: &DesktopGameSnapshot,
        session: &GameSession,
        saved_at: ClockInstant,
    ) -> DesktopGameRegistryTestResult {
        assert_eq!(snapshot.id(), session.id());
        assert_eq!(snapshot.revision(), session.revision());
        assert_eq!(snapshot.phase(), GameEnumPhase::Interrupted);
        assert_eq!(
            snapshot.interruption(),
            Some(GameSessionEnumInterruption::Paused(
                GamePauseEnumCause::Relaunch
            ))
        );
        assert_eq!(snapshot.outcome(), None);
        assert_eq!(snapshot.seats(), session.seats());
        assert_eq!(snapshot.root(), session.tree().root());
        assert_eq!(snapshot.live(), session.live());
        assert_eq!(snapshot.view(), session.view());
        assert_eq!(snapshot.metadata(), session.metadata());
        let live = session.tree().node(session.live())?;
        assert_eq!(snapshot.board().fen(), live.position().fen());
        assert_eq!(snapshot.live_turn(), BoardEnumColor::Black);
        assert_eq!(snapshot.moves().len(), 1);
        let movement = snapshot.moves().first().ok_or("missing recovered move")?;
        assert_eq!(movement.id(), live.id());
        assert_eq!(Some(movement.san()), live.san());
        assert!(!snapshot.clock().is_running());
        let expected = ClockState::recover(&session.clock().snapshot(saved_at)?, [false; 2])?;
        assert_eq!(snapshot.clock(), &expected);
        assert_eq!(
            snapshot.clock().view(ClockInstant::from_millis(9_000)?)?,
            expected.view(saved_at)?
        );
        Ok(())
    }

    async fn mixed_document_recovery(
        content: DesktopNativeGameServiceFixtureEnumDocument,
    ) -> DesktopGameRegistryTestResult {
        let document_id: GameId = "20000000-0000-4000-8000-000000000001".parse()?;
        let body = content.body(document_id)?;
        let saved_at = ClockInstant::from_millis(2_000)?;
        let fixture =
            DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default())
                .await?;
        let games = Arc::new(Mutex::new(Vec::new()));
        let options = fixture.options();
        let mut service = DesktopNativeGameService::new(DesktopGameRegistryOptions {
            factory_provider: DesktopNativeGameServiceFixtureFactories {
                inner: options.factory_provider,
                games: Arc::clone(&games),
            },
            storage: options.storage,
            executor: options.executor,
            clock: options.clock,
            wall_time: crate::game_storage_wall_time_fixture::GameStorageWallTimeFixture::shared(),
            proof: options.proof,
            random: options.random,
        });
        let recovery = async {
            let played = [
                Self::played(
                    "10000000-0000-4000-8000-000000000001".parse()?,
                    "10000000-0000-4000-8000-000000000002".parse()?,
                    "10000000-0000-4000-8000-000000000003".parse()?,
                    "e2e4",
                )?,
                Self::played(
                    "30000000-0000-4000-8000-000000000001".parse()?,
                    "30000000-0000-4000-8000-000000000002".parse()?,
                    "30000000-0000-4000-8000-000000000003".parse()?,
                    "d2d4",
                )?,
            ];
            let repository =
                StorageGameDocumentRepository::new(fixture.store.client(), &fixture.executor);
            for session in &played {
                repository
                    .save(GameStorageSaveRequest::capture(
                        session,
                        saved_at,
                        GameStorageRevision::INITIAL,
                    )?)
                    .await?;
            }
            let saved = fixture
                .store
                .client()
                .save(
                    document_id.as_uuid(),
                    GameStorageRevision::INITIAL.value(),
                    body.clone(),
                )
                .await?;
            fixture.clock.advance_to(saved_at)?;
            let restoration = service.restore().await;
            let readiness = service.library_readiness();
            let snapshots = service
                .start()
                .map(|port| port.snapshots().borrow().clone());
            Ok::<_, Box<dyn std::error::Error>>((played, saved, restoration, snapshots, readiness))
        }
        .await;
        let shutdown = service.shutdown_retained(Self::deadline()).await;
        let complete = service.is_shutdown_complete();
        drop(service);
        let preserved = fixture.store.client().read(document_id.as_uuid()).await;
        let closed = fixture.close().await;
        shutdown?;
        closed?;
        assert!(complete);
        let (played, saved, restoration, snapshots, readiness) = recovery?;
        assert!(
            restoration.is_ok(),
            "non-played content must not prevent played-game recovery: {restoration:?}"
        );
        let snapshots = snapshots?;
        assert_eq!(snapshots.len(), played.len());
        let admissions = games
            .lock()
            .map_err(|_poisoned| "factory admissions poisoned")?;
        assert_eq!(admissions.len(), played.len());
        assert!(!admissions.contains(&document_id));
        for session in &played {
            assert!(admissions.contains(&session.id()));
            let snapshot = snapshots
                .iter()
                .find(|snapshot| snapshot.id() == session.id())
                .ok_or("played game was omitted from mixed-content recovery")?;
            Self::assert_played(snapshot, session, saved_at)?;
        }
        assert!(
            snapshots
                .iter()
                .all(|snapshot| snapshot.id() != document_id)
        );
        let preserved = preserved?.ok_or("non-played document was removed during recovery")?;
        assert_eq!(saved.id(), document_id.as_uuid());
        assert_eq!(preserved.revision(), saved.revision());
        assert_eq!(preserved.body(), body);
        match content {
            DesktopNativeGameServiceFixtureEnumDocument::Notation => {
                let record = StorageGameDocument::recover_record(preserved.body())?;
                assert_eq!(record.id(), document_id);
                assert_eq!(record.mode(), None);
                let StorageLibraryEnumContent::Notation(notation) = record.into_content() else {
                    return Err("recovery synthesized played content for notation".into());
                };
                assert!(notation.clocks().is_empty());
                assert!(
                    notation
                        .tree()
                        .nodes()
                        .all(|node| { node.clock().is_none() && node.before_clock().is_none() })
                );
            }
            DesktopNativeGameServiceFixtureEnumDocument::Unrelated => {
                assert!(matches!(
                    readiness,
                    DesktopNativeLibraryStartupEnumReadiness::Ready(_)
                ));
            }
            DesktopNativeGameServiceFixtureEnumDocument::CorruptPlayed
            | DesktopNativeGameServiceFixtureEnumDocument::FuturePlayed => {
                let DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(summary) = readiness
                else {
                    return Err("an unresolved played document was certified ready".into());
                };
                assert_eq!(summary.issue_count(), 1);
                let issue = summary
                    .issues()
                    .first()
                    .ok_or("missing played recovery issue")?;
                assert_eq!(issue.id(), document_id.as_uuid());
                assert_eq!(issue.content_revision().value(), saved.revision());
            }
        }
        Ok(())
    }
}

#[tokio::test]
async fn clockless_notation_does_not_block_played_recovery_or_create_a_runtime_game()
-> DesktopGameRegistryTestResult {
    DesktopNativeGameServiceFixture::mixed_document_recovery(
        DesktopNativeGameServiceFixtureEnumDocument::Notation,
    )
    .await
}

#[tokio::test]
async fn unrelated_generic_document_does_not_block_played_game_recovery()
-> DesktopGameRegistryTestResult {
    DesktopNativeGameServiceFixture::mixed_document_recovery(
        DesktopNativeGameServiceFixtureEnumDocument::Unrelated,
    )
    .await
}

#[tokio::test]
async fn corrupt_played_record_is_reported_while_other_played_sessions_recover()
-> DesktopGameRegistryTestResult {
    DesktopNativeGameServiceFixture::mixed_document_recovery(
        DesktopNativeGameServiceFixtureEnumDocument::CorruptPlayed,
    )
    .await
}

#[tokio::test]
async fn future_played_record_is_reported_while_other_played_sessions_recover()
-> DesktopGameRegistryTestResult {
    DesktopNativeGameServiceFixture::mixed_document_recovery(
        DesktopNativeGameServiceFixtureEnumDocument::FuturePlayed,
    )
    .await
}

#[tokio::test]
async fn canceled_shutdown_preserves_the_move_and_restores_its_saved_paused_game()
-> DesktopGameRegistryTestResult {
    let gate = Arc::new(DesktopGameRegistryFixtureGate::default());
    let fixture = DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof(Some(
        Arc::clone(&gate),
    )))
    .await?;
    let mut service = DesktopNativeGameService::new(fixture.options());
    service.restore().await?;
    let port = service.start()?;
    let mut failures = port.failures();
    let first = port
        .execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameServiceFixture::failure)?;
    gate.armed.store(true, Ordering::SeqCst);
    let movement = tokio::spawn(port.execute(DesktopGameRegistryFixture::session(
        &first,
        DesktopGameCommandsEnumAction::Play {
            node: "f9fbc8bb-9899-4f60-ad4a-eed15d34ef90".parse()?,
            origin: "e2".parse()?,
            destination: "e4".parse()?,
            promotion: None,
            color: BoardEnumColor::White,
        },
    )));
    gate.entered.notified().await;
    assert!(service.shutdown_retained(Instant::now()).await.is_err());
    DesktopNativeGameServiceFixture::wait_failure(&mut failures).await?;
    assert!(!service.is_shutdown_complete());
    gate.release()?;
    movement
        .await?
        .map_err(DesktopNativeGameServiceFixture::failure)?;
    service
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    assert!(service.is_shutdown_complete());
    drop((service, port, failures));

    let mut restored = DesktopNativeGameService::new(fixture.options());
    restored.restore().await?;
    restored.restore().await?;
    let port = restored.start()?;
    let snapshots = port.snapshots().borrow().clone();
    assert_eq!(snapshots.len(), 1);
    let snapshot = snapshots.first().ok_or("missing restored game")?;
    assert_eq!(snapshot.id(), first.id());
    assert_eq!(snapshot.phase(), GameEnumPhase::Interrupted);
    assert_eq!(snapshot.moves().len(), 1);
    assert_eq!(snapshot.moves()[0].san(), "e4");
    assert_eq!(snapshot.live_turn(), BoardEnumColor::Black);
    assert!(!snapshot.clock().is_running());
    restored
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    assert!(restored.is_shutdown_complete());
    drop((restored, port));
    fixture.close().await
}

#[tokio::test]
async fn runtime_failure_is_observed_once_and_completed_worker_is_not_polled_again()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let mut service = DesktopNativeGameService::new(fixture.options());
    let port = service.start()?;
    let mut failures = port.failures();
    port.execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameServiceFixture::failure)?;
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    assert!(
        port.registry_port()
            .shutdown_all(Instant::now())?
            .wait()
            .await
            .is_err()
    );
    let failure = DesktopNativeGameServiceFixture::wait_failure(&mut failures).await?;
    assert!(!service.is_shutdown_complete());
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    let outcome = service
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await;
    assert!(matches!(
        outcome,
        Err(DesktopNativeGameServiceError::Runtime(ref error)) if Arc::ptr_eq(error, &failure)
    ));
    assert!(service.is_shutdown_complete());
    service.shutdown_retained(Instant::now()).await?;
    drop((service, port, failures, phase));
    fixture.close().await
}

#[tokio::test]
async fn canceled_restore_retains_its_registry_and_resumes_without_duplicate_installation()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let mut original = DesktopNativeGameService::new(fixture.options());
    let port = original.start()?;
    for _ in 0..2 {
        port.execute(DesktopGameRegistryFixture::create())
            .await
            .map_err(DesktopNativeGameServiceFixture::failure)?;
    }
    original
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    fixture.admitted.notified().await;
    drop((original, port));

    let mut service = DesktopNativeGameService::new(fixture.options());
    {
        let restoration = service.restore();
        tokio::pin!(restoration);
        tokio::select! {
            biased;
            () = fixture.admitted.notified() => {},
            result = &mut restoration => {
                result?;
                return Err("restoration completed before the cancellation gate".into());
            }
        }
    }
    assert!(matches!(
        service.start(),
        Err(DesktopNativeGameServiceError::Restoring)
    ));
    service.restore().await?;
    service.restore().await?;
    let port = service.start()?;
    assert_eq!(port.snapshots().borrow().len(), 2);
    assert!(
        port.snapshots()
            .borrow()
            .iter()
            .all(|game| game.phase() == GameEnumPhase::Interrupted && !game.clock().is_running())
    );
    service
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    assert!(service.is_shutdown_complete());
    drop((service, port));
    fixture.close().await
}

#[tokio::test]
async fn shutdown_retains_blocked_restoration_until_the_capacity_phase_allows_cleanup()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let mut original = DesktopNativeGameService::new(fixture.options());
    let port = original.start()?;
    port.execute(DesktopGameRegistryFixture::create())
        .await
        .map_err(DesktopNativeGameServiceFixture::failure)?;
    original
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    drop((original, port));

    let mut service = DesktopNativeGameService::new(fixture.options());
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    assert!(timeout_at(Instant::now(), service.restore()).await.is_err());
    assert!(service.shutdown_retained(Instant::now()).await.is_err());
    assert!(!service.is_shutdown_complete());
    assert!(matches!(
        service.start(),
        Err(DesktopNativeGameServiceError::Closing)
    ));
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    service
        .shutdown_retained(DesktopNativeGameServiceFixture::deadline())
        .await?;
    assert!(service.is_shutdown_complete());
    service.shutdown_retained(Instant::now()).await?;
    drop((service, phase));
    fixture.close().await
}
