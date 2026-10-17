use crate::storage_sqlite_inspector::{StorageSqliteInspector, StorageSqliteInspectorError};
use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;

use rand::{SeedableRng, rngs::StdRng};
use tokio::time::Instant;

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::BoardStart;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::state::ClockState;
use crate::clock::time_control::ClockTimeControl;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumResult, DesktopAnnotationCommandsEnumScope,
};
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationReceipt, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use crate::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorContext,
    DesktopGameEditorEnumSource, DesktopGameEditorEpoch, DesktopGameEditorError,
    DesktopGameEditorPublication, DesktopGameEditorSnapshotEnumState,
};
use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryError, DesktopGameRegistryFactoryProvider,
    DesktopGameRegistryOptions, DesktopGameRegistryRestored,
};
use crate::desktop::game_supervisor::{
    DesktopGameSupervisor, DesktopGameSupervisorEditorReceipt, DesktopGameSupervisorEngineFactory,
    DesktopGameSupervisorEnumOrdinary, DesktopGameSupervisorEnumPersistence,
    DesktopGameSupervisorError, DesktopGameSupervisorOptions, DesktopGameSupervisorPort,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::engine::live_game::EngineLiveGame;
use crate::engine::process_provider::EngineNativeProcess;
use crate::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use crate::game::annotations::{GameAnnotationsEnumChange, GameAnnotationsEnumVariationRequest};
use crate::game::engine_provider::{GameEngineContext, GameEngineSearchConfig};
use crate::game::enums::{
    GameEnumEngineReadiness, GameEnumMarkingColor, GameEnumMode, GameEnumPhase, GameEnumSeat,
    GameId, GameMoveId,
};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::move_node::{GameMoveNodeMarking, GameMoveNodeNag};
use crate::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use crate::game::storage_repository::{
    GameStorageRepository, GameStorageRevision, GameStorageWallTimeError,
    GameStorageWallTimeProvider,
};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::library_played::StorageLibraryPlayedRepository;

type DesktopGameEditorRuntimeTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type DesktopGameEditorRuntimeActor = DesktopGameSupervisor<
    DesktopGameEditorRuntimeFactory,
    DesktopGameEditorRuntimeClock,
    DesktopGameEditorRuntimeProof,
>;

struct DesktopGameEditorRuntimeClock(AtomicU64);

impl DesktopGameEditorRuntimeClock {
    fn new() -> Self {
        Self(AtomicU64::new(50))
    }
}

impl ClockTimeProvider for DesktopGameEditorRuntimeClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}

struct DesktopGameEditorRuntimeWake;

impl DesktopApplicationWakeProvider for DesktopGameEditorRuntimeWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

struct DesktopGameEditorRuntimeWall {
    millis: AtomicI64,
    failing: AtomicBool,
    reads: AtomicUsize,
}

impl GameStorageWallTimeProvider for DesktopGameEditorRuntimeWall {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.failing.load(Ordering::SeqCst) {
            return Err(GameStorageWallTimeError::Unavailable);
        }
        Ok(GameLibraryOrganizationTimestamp::new(
            self.millis.load(Ordering::SeqCst),
        )?)
    }
}

struct DesktopGameEditorRuntimeProof;

impl GameAdjudicationMateProofProvider for DesktopGameEditorRuntimeProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

struct DesktopGameEditorRuntimeFactory;

impl DesktopGameSupervisorEngineFactory for DesktopGameEditorRuntimeFactory {
    type Engine = EngineLiveGame<EngineNativeProcess, DesktopGameEditorRuntimeClock>;
    type Error = std::io::Error;

    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        Err(std::io::Error::other("Local editor requested an engine"))
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl DesktopGameRegistryFactoryProvider for DesktopGameEditorRuntimeFactory {
    type Factory = Self;
    type Error = std::io::Error;

    fn factory(&mut self, _game: GameId) -> Result<Self::Factory, Self::Error> {
        Ok(Self)
    }
}

struct DesktopGameEditorRuntimeFixture {
    directory: tempfile::TempDir,
    store: StorageDocumentStore,
    executor: Arc<CapacityExecutor>,
    budget: CapacityBudget,
    wall: Arc<DesktopGameEditorRuntimeWall>,
    application: DesktopApplication<DesktopGameEditorRuntimeClock, DesktopGameEditorRuntimeWake>,
    actor: DesktopGameEditorRuntimeActor,
    port: DesktopGameSupervisorPort,
}

impl DesktopGameEditorRuntimeFixture {
    fn session(id: u128, analysis: bool) -> DesktopGameEditorRuntimeTestResult<GameSession> {
        let mut session = GameSession::new(GameSessionOptions {
            id: uuid::Uuid::from_u128(id).to_string().parse()?,
            root: uuid::Uuid::from_u128(1).to_string().parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?;
        if analysis {
            session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
        }
        Ok(session)
    }

    async fn new() -> DesktopGameEditorRuntimeTestResult<Self> {
        Self::with_session(Self::session(0x460, true)?).await
    }

    async fn with_session(session: GameSession) -> DesktopGameEditorRuntimeTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("editor.sqlite"), budget.clone())
                .await?;
        let wall = Arc::new(DesktopGameEditorRuntimeWall {
            millis: AtomicI64::new(1_000),
            failing: AtomicBool::new(false),
            reads: AtomicUsize::new(0),
        });
        let (actor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
            session,
            storage_revision: GameStorageRevision::INITIAL,
            storage: store.client(),
            executor: Arc::clone(&executor),
            clock: Arc::new(DesktopGameEditorRuntimeClock::new()),
            wall_time: Arc::<DesktopGameEditorRuntimeWall>::clone(&wall),
            proof: Arc::new(DesktopGameEditorRuntimeProof),
            factory: DesktopGameEditorRuntimeFactory,
            search_config: GameEngineSearchConfig::initial_default(),
        })
        .await?;
        Ok(Self {
            directory,
            store,
            executor,
            budget,
            wall,
            application: DesktopApplication::new(
                Arc::new(DesktopGameEditorRuntimeClock::new()),
                Arc::new(DesktopGameEditorRuntimeWake),
            ),
            actor,
            port,
        })
    }

    fn receipt(&self) -> DesktopGameEditorRuntimeTestResult<DesktopApplicationReceipt> {
        let port = self.application.port();
        let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        let receipt = port.try_submit(&request)?;
        let work = self
            .application
            .try_next()?
            .ok_or("missing application work")?;
        assert!(receipt.shares_outcome(work.receipt()));
        Ok(receipt)
    }

    fn snapshot(
        result: DesktopAnnotationCommandsEnumResult,
    ) -> DesktopGameEditorRuntimeTestResult<DesktopGameEditorPublication> {
        match result {
            DesktopAnnotationCommandsEnumResult::Snapshot(snapshot)
            | DesktopAnnotationCommandsEnumResult::Edited { snapshot, .. } => Ok(snapshot),
            DesktopAnnotationCommandsEnumResult::Tree(_)
            | DesktopAnnotationCommandsEnumResult::Children(_)
            | DesktopAnnotationCommandsEnumResult::Variation(_)
            | DesktopAnnotationCommandsEnumResult::Scope(_)
            | DesktopAnnotationCommandsEnumResult::PendingProjection { .. } => {
                Err("expected editor snapshot".into())
            }
        }
    }

    async fn drive(
        &mut self,
        reply: &mut DesktopGameSupervisorEditorReceipt,
    ) -> Result<DesktopAnnotationCommandsEnumResult, Arc<DesktopGameSupervisorError>> {
        tokio::select! {
            biased;
            result = reply.wait() => result,
            result = self.actor.run_loop() => {
                Err(Arc::new(result.err().unwrap_or(DesktopGameSupervisorError::Closed)))
            }
        }
    }

    async fn open(&mut self) -> DesktopGameEditorRuntimeTestResult<DesktopGameEditorPublication> {
        let mut reply = self.port.try_open_editor(self.receipt()?)?;
        Self::snapshot(self.drive(&mut reply).await?)
    }

    async fn action(
        &mut self,
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
    ) -> DesktopGameEditorRuntimeTestResult<DesktopAnnotationCommandsEnumResult> {
        let mut reply = self.port.try_submit_editor(DesktopAnnotationCommands::new(
            self.receipt()?,
            expected,
            action,
        ))?;
        Ok(self.drive(&mut reply).await?)
    }

    async fn notes(
        &mut self,
        expected: DesktopGameEditorContext,
        notes: &str,
    ) -> DesktopGameEditorRuntimeTestResult<DesktopGameEditorPublication> {
        let result = self
            .action(
                expected,
                DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: notes.to_owned(),
                    nags: vec![GameMoveNodeNag::new(1)],
                },
            )
            .await?;
        Self::snapshot(result)
    }

    async fn navigate(
        &mut self,
        expected: DesktopGameEditorContext,
        selected: GameMoveId,
    ) -> DesktopGameEditorRuntimeTestResult<DesktopGameEditorPublication> {
        Self::snapshot(
            self.action(
                expected,
                DesktopAnnotationCommandsEnumAction::Navigate { selected },
            )
            .await?,
        )
    }

    async fn append(
        &mut self,
        expected: DesktopGameEditorContext,
        id: u128,
        uci: &str,
    ) -> DesktopGameEditorRuntimeTestResult<DesktopAnnotationCommandsEnumResult> {
        let state = self.actor.session.take()?;
        let movement = state
            .session
            .tree()
            .node(expected.selected())?
            .position()
            .parse_uci(uci)?;
        self.actor.session.replace(state)?;
        self.action(
            expected,
            DesktopAnnotationCommandsEnumAction::AppendStudyBranch {
                node: uuid::Uuid::from_u128(id).to_string().parse()?,
                movement,
            },
        )
        .await
    }

    async fn preview(
        &mut self,
        expected: DesktopGameEditorContext,
        request: GameAnnotationsEnumVariationRequest,
    ) -> DesktopGameEditorRuntimeTestResult<
        crate::desktop::annotation_commands::DesktopAnnotationCommandsVariationPreview,
    > {
        match self
            .action(
                expected,
                DesktopAnnotationCommandsEnumAction::PreviewVariation(request),
            )
            .await?
        {
            DesktopAnnotationCommandsEnumResult::Variation(preview) => Ok(preview),
            DesktopAnnotationCommandsEnumResult::Snapshot(_)
            | DesktopAnnotationCommandsEnumResult::Edited { .. }
            | DesktopAnnotationCommandsEnumResult::Tree(_)
            | DesktopAnnotationCommandsEnumResult::Children(_)
            | DesktopAnnotationCommandsEnumResult::Scope(_)
            | DesktopAnnotationCommandsEnumResult::PendingProjection { .. } => {
                Err("expected variation preview".into())
            }
        }
    }

    async fn sql<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut StorageSqliteInspector) -> Result<T, StorageSqliteInspectorError>
        + Send
        + std::panic::UnwindSafe
        + 'static,
    ) -> DesktopGameEditorRuntimeTestResult<T> {
        let path = self.directory.path().join("editor.sqlite");
        Ok(self
            .executor
            .submit_waiting(move |_cancellation| operation(&mut StorageSqliteInspector::open(path)?))
            .await?
            .join()
            .await??)
    }

    async fn modified(&self) -> DesktopGameEditorRuntimeTestResult<Option<i64>> {
        self.sql(|connection| {
            connection.optional_integer("SELECT modified_ms AS value FROM library_entries")
        })
        .await
    }

    async fn fail_storage(&self, enabled: bool) -> DesktopGameEditorRuntimeTestResult {
        self.sql(move |connection| connection.batch(if enabled {
            "CREATE TRIGGER editor_failure BEFORE UPDATE ON library_entries BEGIN SELECT RAISE(ABORT, 'controlled editor failure'); END;"
        } else { "DROP TRIGGER editor_failure;" })).await
    }

    async fn flush(&mut self) -> DesktopGameEditorRuntimeTestResult {
        while self.actor.saving.is_some() || !self.actor.writes.is_empty() {
            self.actor.begin_save(true);
            let result = DesktopGameEditorRuntimeActor::save_ready(&mut self.actor.saving).await;
            self.actor.saving = None;
            self.actor.finish_save(result)?;
        }
        Ok(())
    }

    async fn close(mut self) -> DesktopGameEditorRuntimeTestResult {
        self.actor
            .shutdown_retained(Instant::now() + Duration::from_secs(5))
            .await?;
        drop((self.actor, self.port));
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_shared| "executor remains owned")?
            .shutdown()
            .await?;
        self.directory.close()?;
        Ok(())
    }

    async fn compute_queued_editor(&mut self) -> DesktopGameEditorRuntimeTestResult {
        let DesktopGameSupervisorEnumOrdinary::Editor(envelope) = self.actor.ordinary.try_recv()?
        else {
            return Err("expected admitted editor envelope".into());
        };
        self.actor.handle_editor(envelope);
        self.actor
            .observe_computation(Instant::now() + Duration::from_secs(5))
            .await?;
        Ok(())
    }

    async fn source_open(
        &mut self,
    ) -> DesktopGameEditorRuntimeTestResult<DesktopGameEditorPublication> {
        let mut reply = self.port.try_open_editor(self.receipt()?)?;
        self.compute_queued_editor().await?;
        Self::snapshot(reply.wait().await?)
    }

    async fn finish_one_save(&mut self) -> DesktopGameEditorRuntimeTestResult {
        self.actor.begin_save(false);
        let result = DesktopGameEditorRuntimeActor::save_ready(&mut self.actor.saving).await;
        self.actor.saving = None;
        self.actor.finish_save(result)?;
        Ok(())
    }
}

#[tokio::test]
async fn first_open_uses_admitted_epoch_and_focus_keeps_authoritative_source()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let receipt = fixture.receipt()?;
    let mut reply = fixture.port.try_open_editor(receipt.clone())?;
    assert_eq!(reply.request_id(), receipt.request_id());
    assert!(reply.application_receipt().shares_outcome(&receipt));
    let first = DesktopGameEditorRuntimeFixture::snapshot(fixture.drive(&mut reply).await?)?;
    assert_eq!(first.context().epoch().request_id(), receipt.request_id());
    assert!(matches!(
        first.context().source(),
        DesktopGameEditorEnumSource::Played { .. }
    ));
    assert!(matches!(
        first.state(),
        DesktopGameEditorSnapshotEnumState::Played {
            phase: GameEnumPhase::Analysis,
            ..
        }
    ));
    let second = fixture.open().await?;
    assert_eq!(first.context(), second.context());
    fixture.close().await
}

#[tokio::test]
async fn new_actor_for_same_game_uses_fresh_first_open_epoch() -> DesktopGameEditorRuntimeTestResult
{
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let first = fixture.open().await?;
    fixture.flush().await?;
    let recovered = StorageLibraryPlayedRepository::new(fixture.store.client(), &fixture.executor)
        .load(first.context().game())
        .await?
        .ok_or("missing saved game")?;
    let revision = recovered.storage_revision();
    let session = recovered.into_session();
    fixture
        .actor
        .shutdown_retained(Instant::now() + Duration::from_secs(5))
        .await?;
    let (actor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
        session,
        storage_revision: revision,
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::new(DesktopGameEditorRuntimeClock::new()),
        wall_time: Arc::<DesktopGameEditorRuntimeWall>::clone(&fixture.wall),
        proof: Arc::new(DesktopGameEditorRuntimeProof),
        factory: DesktopGameEditorRuntimeFactory,
        search_config: GameEngineSearchConfig::initial_default(),
    })
    .await?;
    fixture.actor = actor;
    fixture.port = port;
    let second = fixture.open().await?;
    assert_ne!(first.context().epoch(), second.context().epoch());
    assert_eq!(first.context().game(), second.context().game());
    fixture.close().await
}

#[tokio::test]
async fn notes_nags_and_markings_save_separately_with_durable_bound_receipts()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let first = fixture.open().await?;
    fixture.wall.millis.store(2_000, Ordering::SeqCst);
    let notes = fixture.notes(first.context(), "plain <text>").await?;
    assert_eq!(notes.annotations().notes(), "plain <text>");
    assert_eq!(notes.annotations().nags(), &[GameMoveNodeNag::new(1)]);
    assert!(notes.annotations().markings().is_empty());
    assert!(
        notes.context().storage_revision().value() > first.context().storage_revision().value()
    );
    fixture.wall.millis.store(3_000, Ordering::SeqCst);
    let marking = GameMoveNodeMarking::circle(GameEnumMarkingColor::Green, "e4".parse()?);
    let saved = DesktopGameEditorRuntimeFixture::snapshot(
        fixture
            .action(
                notes.context(),
                DesktopAnnotationCommandsEnumAction::SaveMarkings {
                    markings: vec![marking],
                },
            )
            .await?,
    )?;
    assert_eq!(saved.annotations().notes(), notes.annotations().notes());
    assert_eq!(saved.annotations().nags(), notes.annotations().nags());
    assert_eq!(saved.annotations().markings(), &[marking]);
    assert_eq!(fixture.modified().await?, Some(3_000));
    let recovered = StorageLibraryPlayedRepository::new(fixture.store.client(), &fixture.executor)
        .load(saved.context().game())
        .await?
        .ok_or("missing durable played game")?;
    let revision = recovered.storage_revision();
    let session = recovered.into_session();
    assert_eq!(revision, saved.context().storage_revision());
    assert_eq!(
        session
            .tree()
            .node(saved.context().selected())?
            .annotations(),
        saved.annotations()
    );
    fixture.close().await
}

#[tokio::test]
async fn equal_fen_foreign_game_epoch_view_and_storage_contexts_are_rejected()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let snapshot = fixture.open().await?;
    for (game, epoch, storage) in [
        (
            0x461,
            snapshot.context().epoch(),
            snapshot.context().storage_revision(),
        ),
        (
            0x460,
            DesktopGameEditorEpoch::from_request_id(DesktopRequestId::new()),
            snapshot.context().storage_revision(),
        ),
        (
            0x460,
            snapshot.context().epoch(),
            GameStorageRevision::new(999),
        ),
    ] {
        let session = DesktopGameEditorRuntimeFixture::session(game, true)?;
        let context = DesktopGameEditor::played(
            &session,
            DesktopGameEditorBinding::new(storage, epoch, snapshot.context().generation()),
        )?
        .context();
        let mut reply = fixture
            .port
            .try_submit_editor(DesktopAnnotationCommands::new(
                fixture.receipt()?,
                context,
                DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "foreign".to_owned(),
                    nags: Vec::new(),
                },
            ))?;
        assert!(
            matches!(fixture.drive(&mut reply).await, Err(error) if matches!(error.as_ref(), DesktopGameSupervisorError::Editor(DesktopGameEditorError::StaleContext)))
        );
    }
    let branch = DesktopGameEditorRuntimeFixture::snapshot(
        fixture.append(snapshot.context(), 2, "e2e4").await?,
    )?;
    let selected = fixture.navigate(branch.context(), branch.root()).await?;
    let mut stale = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            branch.context(),
            DesktopAnnotationCommandsEnumAction::Snapshot,
        ))?;
    assert!(fixture.drive(&mut stale).await.is_err());
    assert_eq!(fixture.open().await?.context(), selected.context());
    fixture.close().await
}

#[tokio::test]
async fn ordinary_and_editor_share_finite_admission_while_controls_remain_reserved()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let initial = fixture.open().await?;
    for index in 0..DesktopGameSupervisorPort::ORDINARY_CAPACITY {
        if index % 2 == 0 {
            fixture.port.try_open_editor(fixture.receipt()?)?;
        } else {
            fixture.port.try_submit(DesktopGameCommandsRequest {
                request_id: DesktopRequestId::new(),
                expected_revision: fixture.port.snapshot().snapshot.revision(),
                received_at: ClockInstant::from_nanos(50),
                action: DesktopGameCommandsEnumAction::Navigate {
                    node: initial.root(),
                },
            })?;
        }
    }
    assert!(matches!(
        fixture.port.try_open_editor(fixture.receipt()?),
        Err(DesktopGameSupervisorError::Busy)
    ));
    let pause = fixture.port.try_submit(DesktopGameCommandsRequest {
        request_id: DesktopRequestId::new(),
        expected_revision: fixture.port.snapshot().snapshot.revision(),
        received_at: ClockInstant::from_nanos(50),
        action: DesktopGameCommandsEnumAction::Stop,
    })?;
    tokio::select! {
        biased;
        result = pause.wait() => { result?; }
        result = fixture.actor.run_loop() => { result?; return Err("supervisor exited before reserved control".into()); }
    }
    fixture.close().await
}

#[tokio::test]
async fn variation_confirmation_retains_real_scope_and_foreign_requests_keep_proof()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    let e4 = DesktopGameEditorRuntimeFixture::snapshot(
        fixture.append(root.context(), 2, "e2e4").await?,
    )?;
    let initial = fixture.navigate(e4.context(), e4.root()).await?;
    let d4 = DesktopGameEditorRuntimeFixture::snapshot(
        fixture.append(initial.context(), 3, "d2d4").await?,
    )?;
    let preview = fixture
        .preview(
            d4.context(),
            GameAnnotationsEnumVariationRequest::Promote {
                tip: d4.context().selected(),
            },
        )
        .await?;
    let scope = fixture
        .action(
            d4.context(),
            DesktopAnnotationCommandsEnumAction::VariationScope {
                token: preview.token(),
                scope: DesktopAnnotationCommandsEnumScope::SelectedPath,
                offset: 0,
            },
        )
        .await?;
    assert!(
        matches!(scope, DesktopAnnotationCommandsEnumResult::Scope(page) if page.nodes().last() == Some(&d4.context().selected()) && page.nodes().len() <= 100)
    );
    let mut foreign = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            d4.context(),
            DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: DesktopRequestId::new(),
            },
        ))?;
    assert!(fixture.drive(&mut foreign).await.is_err());
    let foreign_source = DesktopGameEditorRuntimeFixture::session(0x461, true)?;
    let foreign_context = DesktopGameEditor::played(
        &foreign_source,
        DesktopGameEditorBinding::new(
            d4.context().storage_revision(),
            d4.context().epoch(),
            d4.context().generation(),
        ),
    )?
    .context();
    let mut foreign_context_reply =
        fixture
            .port
            .try_submit_editor(DesktopAnnotationCommands::new(
                fixture.receipt()?,
                foreign_context,
                DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                    token: preview.token(),
                },
            ))?;
    assert!(fixture.drive(&mut foreign_context_reply).await.is_err());
    fixture.wall.failing.store(true, Ordering::SeqCst);
    let mut failed = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            d4.context(),
            DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: preview.token(),
            },
        ))?;
    assert!(
        matches!(fixture.drive(&mut failed).await, Err(error) if matches!(error.as_ref(), DesktopGameSupervisorError::WallTime(_)))
    );
    fixture.wall.failing.store(false, Ordering::SeqCst);
    let promoted = DesktopGameEditorRuntimeFixture::snapshot(
        fixture
            .action(
                d4.context(),
                DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                    token: preview.token(),
                },
            )
            .await?,
    )?;
    assert_eq!(promoted.mainline_tip(), d4.context().selected());
    let deletion = fixture
        .preview(
            promoted.context(),
            GameAnnotationsEnumVariationRequest::Delete {
                root: e4.context().selected(),
            },
        )
        .await?;
    assert_eq!(deletion.deleted_count(), 1);
    let deleted = DesktopGameEditorRuntimeFixture::snapshot(
        fixture
            .action(
                promoted.context(),
                DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                    token: deletion.token(),
                },
            )
            .await?,
    )?;
    assert_eq!(deleted.mainline_tip(), promoted.mainline_tip());
    fixture.close().await
}

#[tokio::test]
async fn identical_branch_notes_navigation_and_checkpoint_preserve_modified_time()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    fixture.wall.millis.store(2_000, Ordering::SeqCst);
    let branch = DesktopGameEditorRuntimeFixture::snapshot(
        fixture.append(root.context(), 2, "e2e4").await?,
    )?;
    let initial = fixture.navigate(branch.context(), branch.root()).await?;
    fixture.wall.millis.store(9_000, Ordering::SeqCst);
    let reused = fixture.append(initial.context(), 3, "e2e4").await?;
    assert!(matches!(
        &reused,
        DesktopAnnotationCommandsEnumResult::Edited {
            change: GameAnnotationsEnumChange::Unchanged,
            ..
        }
    ));
    let reused = DesktopGameEditorRuntimeFixture::snapshot(reused)?;
    assert_eq!(reused.context().selected(), branch.context().selected());
    assert_eq!(fixture.modified().await?, Some(2_000));
    let empty = DesktopGameEditorRuntimeFixture::snapshot(
        fixture
            .action(
                reused.context(),
                DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: String::new(),
                    nags: Vec::new(),
                },
            )
            .await?,
    )?;
    let checkpoint = DesktopGameEditorRuntimeFixture::snapshot(
        fixture
            .action(
                empty.context(),
                DesktopAnnotationCommandsEnumAction::Checkpoint,
            )
            .await?,
    )?;
    assert_eq!(fixture.modified().await?, Some(2_000));
    assert_eq!(checkpoint.annotations().notes(), "");
    fixture.close().await
}

#[tokio::test]
async fn failed_utc_keeps_source_and_retryable_draft_unchanged()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    fixture.wall.failing.store(true, Ordering::SeqCst);
    let mut reply = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "retry me".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    assert!(
        matches!(fixture.drive(&mut reply).await, Err(error) if matches!(error.as_ref(), DesktopGameSupervisorError::WallTime(_)))
    );
    let unchanged = fixture.open().await?;
    assert_eq!(unchanged.context(), root.context());
    assert_eq!(unchanged.annotations(), root.annotations());
    fixture.wall.failing.store(false, Ordering::SeqCst);
    fixture.notes(root.context(), "retry me").await?;
    fixture.close().await
}

#[tokio::test]
async fn storage_failure_retains_original_utc_and_retry_saves_newest_owned_source()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    fixture.fail_storage(true).await?;
    fixture.wall.millis.store(2_000, Ordering::SeqCst);
    let mut failed = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "retained".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    assert!(fixture.drive(&mut failed).await.is_err());
    fixture.fail_storage(false).await?;
    fixture.wall.failing.store(true, Ordering::SeqCst);
    let current = fixture.open().await?;
    assert_eq!(current.annotations().notes(), "retained");
    let retried = DesktopGameEditorRuntimeFixture::snapshot(
        fixture
            .action(
                current.context(),
                DesktopAnnotationCommandsEnumAction::RetrySave,
            )
            .await?,
    )?;
    assert_eq!(retried.annotations().notes(), "retained");
    assert_eq!(fixture.modified().await?, Some(2_000));
    assert!(matches!(
        fixture.port.snapshot().persistence,
        DesktopGameSupervisorEnumPersistence::Current
    ));
    fixture.close().await
}

#[tokio::test]
async fn cancelled_wait_keeps_accepted_result_and_application_correlation()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    let application = fixture.receipt()?;
    let command = || {
        DesktopAnnotationCommands::new(
            application.clone(),
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "once".to_owned(),
                nags: Vec::new(),
            },
        )
    };
    let mut reply = fixture.port.try_submit_editor(command())?;
    {
        let mut waiting = Box::pin(reply.wait());
        poll_fn(|context| {
            assert!(waiting.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    drop(reply);
    let mut retained = fixture.port.try_submit_editor(command())?;
    let saved = DesktopGameEditorRuntimeFixture::snapshot(fixture.drive(&mut retained).await?)?;
    let durable = retained
        .storage_receipt()
        .ok_or("missing actual storage receipt")?;
    assert_eq!(durable.game(), saved.context().game());
    assert_eq!(
        durable.game_revision(),
        fixture.port.snapshot().snapshot.revision()
    );
    assert_eq!(
        durable.storage_revision(),
        saved.context().storage_revision()
    );
    let revision = saved.context().storage_revision();
    let mut again = fixture.port.try_submit_editor(command())?;
    let duplicate = DesktopGameEditorRuntimeFixture::snapshot(again.wait().await?)?;
    assert_eq!(duplicate.context().storage_revision(), revision);
    assert!(again.application_receipt().shares_outcome(&application));
    fixture.close().await
}

#[tokio::test]
async fn cancellation_before_acceptance_has_no_edit_or_epoch_initialization()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let receipt = fixture.receipt()?;
    receipt.cancellation().request();
    let mut reply = fixture.port.try_open_editor(receipt)?;
    assert!(fixture.drive(&mut reply).await.is_err());
    let fresh = fixture.receipt()?;
    let mut opened = fixture.port.try_open_editor(fresh.clone())?;
    let snapshot = DesktopGameEditorRuntimeFixture::snapshot(fixture.drive(&mut opened).await?)?;
    assert_eq!(snapshot.context().epoch().request_id(), fresh.request_id());
    fixture.close().await
}

#[tokio::test]
async fn active_phase_rejects_annotations_and_never_changes_live_history()
-> DesktopGameEditorRuntimeTestResult {
    let mut session = DesktopGameEditorRuntimeFixture::session(0x460, false)?;
    session.start(
        session.revision(),
        ClockInstant::from_nanos(0),
        GameEnumEngineReadiness::NotRequired,
        &DesktopGameEditorRuntimeProof,
    )?;
    let mut fixture = DesktopGameEditorRuntimeFixture::with_session(session).await?;
    let root = fixture.open().await?;
    let mut reply = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "forbidden".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    assert!(
        matches!(fixture.drive(&mut reply).await, Err(error) if matches!(error.as_ref(), DesktopGameSupervisorError::Session(crate::game::session::GameSessionError::WrongPhase(GameEnumPhase::Active))))
    );
    assert_eq!(fixture.open().await?.context(), root.context());
    fixture.close().await
}

#[tokio::test]
async fn registry_forwards_existing_game_and_refuses_absent_game()
-> DesktopGameEditorRuntimeTestResult {
    let fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let session = DesktopGameEditorRuntimeFixture::session(0x900, true)?;
    let id = session.id();
    let (mut registry, port) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
        factory_provider: DesktopGameEditorRuntimeFactory,
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::new(DesktopGameEditorRuntimeClock::new()),
        wall_time: Arc::<DesktopGameEditorRuntimeWall>::clone(&fixture.wall),
        proof: Arc::new(DesktopGameEditorRuntimeProof),
        random: StdRng::seed_from_u64(42),
    });
    registry
        .restore(DesktopGameRegistryRestored {
            session,
            storage_revision: GameStorageRevision::INITIAL,
        })
        .await?;
    let mut opened = port.try_open_editor(id, fixture.receipt()?)?;
    let snapshot = DesktopGameEditorRuntimeFixture::snapshot(opened.wait().await?)?;
    assert_eq!(snapshot.context().game(), id);
    let mut edited = port.try_submit_editor(DesktopAnnotationCommands::new(
        fixture.receipt()?,
        snapshot.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "registry".to_owned(),
            nags: Vec::new(),
        },
    ))?;
    assert_eq!(
        DesktopGameEditorRuntimeFixture::snapshot(edited.wait().await?)?
            .annotations()
            .notes(),
        "registry"
    );
    assert!(matches!(
        port.try_open_editor(
            "00000000-0000-0000-0000-000000009999".parse()?,
            fixture.receipt()?
        ),
        Err(DesktopGameRegistryError::UnknownGame)
    ));
    registry
        .shutdown_retained(Instant::now() + Duration::from_secs(5))
        .await?;
    drop((registry, port));
    fixture.close().await
}

#[tokio::test]
async fn older_completion_keeps_newer_edit_watermark_and_original_timestamp()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    fixture.wall.millis.store(2_000, Ordering::SeqCst);
    let mut first = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "first".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    fixture.compute_queued_editor().await?;
    fixture.actor.begin_save(false);
    let current = fixture.source_open().await?;
    fixture.wall.millis.store(3_000, Ordering::SeqCst);
    let mut newest = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            current.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "newest".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    fixture.compute_queued_editor().await?;
    let state = fixture.actor.session.take()?;
    let accepted = state.accepted_edit.ok_or("missing accepted edit")?;
    fixture.actor.session.replace(state)?;
    fixture.finish_one_save().await?;
    let state = fixture.actor.session.take()?;
    assert_eq!(state.accepted_edit, Some(accepted));
    assert_eq!(
        state
            .session
            .tree()
            .node(state.session.view())?
            .annotations()
            .notes(),
        "newest"
    );
    fixture.actor.session.replace(state)?;
    assert_eq!(fixture.modified().await?, Some(2_000));
    fixture
        .actor
        .observe_editor_projections(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_eq!(
        DesktopGameEditorRuntimeFixture::snapshot(first.wait().await?)?
            .annotations()
            .notes(),
        "newest"
    );
    fixture.wall.failing.store(true, Ordering::SeqCst);
    fixture.finish_one_save().await?;
    fixture
        .actor
        .observe_editor_projections(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_eq!(
        DesktopGameEditorRuntimeFixture::snapshot(newest.wait().await?)?
            .annotations()
            .notes(),
        "newest"
    );
    assert_eq!(fixture.modified().await?, Some(3_000));
    fixture.close().await
}

#[tokio::test]
async fn cancelled_computation_wait_retains_the_actual_session_owner()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    let mut reply = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "compute retained".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    let DesktopGameSupervisorEnumOrdinary::Editor(envelope) = fixture.actor.ordinary.try_recv()?
    else {
        return Err("missing editor work".into());
    };
    fixture.actor.handle_editor(envelope);
    let lease = fixture.budget.try_lease(1)?;
    {
        let mut observing = Box::pin(
            fixture
                .actor
                .observe_computation(Instant::now() + Duration::from_secs(5)),
        );
        poll_fn(|context| {
            assert!(observing.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    assert!(fixture.actor.computing.is_some());
    let state = fixture.actor.session.take()?;
    assert_eq!(
        state
            .session
            .tree()
            .node(root.root())?
            .annotations()
            .notes(),
        ""
    );
    fixture.actor.session.replace(state)?;
    drop(lease);
    fixture
        .actor
        .observe_computation(Instant::now() + Duration::from_secs(5))
        .await?;
    fixture.finish_one_save().await?;
    fixture
        .actor
        .observe_editor_projections(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_eq!(
        DesktopGameEditorRuntimeFixture::snapshot(reply.wait().await?)?
            .annotations()
            .notes(),
        "compute retained"
    );
    fixture.close().await
}

#[tokio::test]
async fn cancelled_preparation_wait_preserves_accepted_source_until_real_save()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    let mut reply = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "prepare retained".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    fixture.compute_queued_editor().await?;
    fixture.actor.begin_save(false);
    assert!(matches!(
        reply.pending(),
        Some(
            crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumPending::Saving {
                computing: false
            }
        )
    ));
    reply.application_receipt().cancellation().request();
    let lease = fixture.budget.try_lease(1)?;
    {
        let mut observing = Box::pin(DesktopGameEditorRuntimeActor::save_ready(
            &mut fixture.actor.saving,
        ));
        poll_fn(|context| {
            assert!(observing.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    assert!(fixture.actor.saving_context.is_some());
    let state = fixture.actor.session.take()?;
    assert!(state.accepted_edit.is_some());
    assert_eq!(
        state
            .session
            .tree()
            .node(root.root())?
            .annotations()
            .notes(),
        "prepare retained"
    );
    fixture.actor.session.replace(state)?;
    drop(lease);
    fixture.finish_one_save().await?;
    fixture
        .actor
        .observe_editor_projections(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_eq!(
        DesktopGameEditorRuntimeFixture::snapshot(reply.wait().await?)?
            .annotations()
            .notes(),
        "prepare retained"
    );
    fixture.close().await
}

#[tokio::test]
async fn cancelled_commit_wait_retains_the_real_sql_mutation_and_receipt()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    let mut reply = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "commit retained".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    fixture.compute_queued_editor().await?;
    let mut connection =
        StorageSqliteInspector::open(fixture.directory.path().join("editor.sqlite"))?;
    connection.batch("BEGIN IMMEDIATE;")?;
    fixture.actor.begin_save(false);
    {
        let mut observing = Box::pin(DesktopGameEditorRuntimeActor::save_ready(
            &mut fixture.actor.saving,
        ));
        poll_fn(|context| {
            assert!(observing.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    fixture
        .executor
        .submit_waiting(|_cancellation| ())
        .await?
        .join()
        .await?;
    {
        let mut observing = Box::pin(DesktopGameEditorRuntimeActor::save_ready(
            &mut fixture.actor.saving,
        ));
        poll_fn(|context| {
            assert!(observing.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    assert!(fixture.actor.saving_context.is_some());
    connection.batch("ROLLBACK;")?;
    fixture.finish_one_save().await?;
    fixture
        .actor
        .observe_editor_projections(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_eq!(
        DesktopGameEditorRuntimeFixture::snapshot(reply.wait().await?)?
            .annotations()
            .notes(),
        "commit retained"
    );
    drop(connection);
    fixture.close().await
}

#[tokio::test]
async fn cancelled_post_save_projection_retains_pending_and_rejects_obsolete_context()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    let mut reply = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "project retained".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    fixture.compute_queued_editor().await?;
    let before_save = fixture.source_open().await?;
    fixture.finish_one_save().await?;
    assert!(matches!(
        reply.try_result(),
        Some(Ok(
            DesktopAnnotationCommandsEnumResult::PendingProjection { .. }
        ))
    ));
    let lease = fixture.budget.try_lease(1)?;
    fixture.actor.project_editor()?;
    {
        let mut observing = Box::pin(
            fixture
                .actor
                .observe_computation(Instant::now() + Duration::from_secs(5)),
        );
        poll_fn(|context| {
            assert!(observing.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    assert!(matches!(
        reply.try_result(),
        Some(Ok(
            DesktopAnnotationCommandsEnumResult::PendingProjection { .. }
        ))
    ));
    let mut obsolete = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            before_save.context(),
            DesktopAnnotationCommandsEnumAction::Snapshot,
        ))?;
    assert!(obsolete.try_result().is_none());
    drop(lease);
    fixture
        .actor
        .observe_computation(Instant::now() + Duration::from_secs(5))
        .await?;
    let published = DesktopGameEditorRuntimeFixture::snapshot(reply.wait().await?)?;
    assert!(
        published.context().storage_revision().value()
            > before_save.context().storage_revision().value()
    );
    assert!(fixture.drive(&mut obsolete).await.is_err());
    fixture.close().await
}

#[tokio::test]
async fn accepted_edit_under_publication_pressure_resumes_without_edit_replay_or_new_utc()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    let reservation = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - 65_536)?;
    fixture.wall.millis.store(2_000, Ordering::SeqCst);
    let application = fixture.receipt()?;
    let command = || {
        DesktopAnnotationCommands::new(
            application.clone(),
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "a".repeat(131_072),
                nags: Vec::new(),
            },
        )
    };
    let mut reply = fixture.port.try_submit_editor(command())?;
    let error = fixture
        .drive(&mut reply)
        .await
        .err()
        .ok_or("publication unexpectedly admitted")?;
    assert!(matches!(
        error.as_ref(),
        DesktopGameSupervisorError::EditorPublicationPending { .. }
    ));
    assert!(matches!(
        reply.pending(),
        Some(
            crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumPending::Publishing { .. }
        )
    ));
    let DesktopGameSupervisorError::EditorPublicationPending {
        context: accepted_context,
        ..
    } = error.as_ref()
    else {
        return Err("wrong publication failure".into());
    };
    let mut foreign = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            *accepted_context,
            DesktopAnnotationCommandsEnumAction::Snapshot,
        ))?;
    assert!(
        matches!(fixture.drive(&mut foreign).await, Err(error) if matches!(error.as_ref(), DesktopGameSupervisorError::Busy))
    );
    let state = fixture.actor.session.take()?;
    let accepted = state.accepted_edit.ok_or("accepted edit was lost")?;
    let revision = state.session.revision();
    assert_eq!(
        state
            .session
            .tree()
            .node(root.root())?
            .annotations()
            .notes()
            .len(),
        131_072
    );
    fixture.actor.session.replace(state)?;
    let foreign = fixture.receipt()?;
    let mut stale = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            foreign,
            root.context(),
            DesktopAnnotationCommandsEnumAction::Snapshot,
        ))?;
    assert!(fixture.drive(&mut stale).await.is_err());
    drop(reservation);
    fixture.wall.failing.store(true, Ordering::SeqCst);
    let mut resumed = fixture.port.try_submit_editor(command())?;
    let saved = DesktopGameEditorRuntimeFixture::snapshot(fixture.drive(&mut resumed).await?)?;
    let state = fixture.actor.session.take()?;
    assert_eq!(state.session.revision(), revision);
    assert!(state.accepted_edit.is_none());
    fixture.actor.session.replace(state)?;
    assert_eq!(saved.annotations().notes().len(), 131_072);
    assert_eq!(
        fixture.modified().await?,
        Some(accepted.timestamp().value())
    );
    fixture.close().await
}

#[tokio::test]
async fn editor_generation_and_revision_overflow_are_checked_before_mutation()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    fixture.open().await?;
    let mut state = fixture.actor.session.take()?;
    state.editor.generation =
        crate::desktop::game_editor::DesktopGameEditorGeneration::new(u64::MAX);
    fixture.actor.session.replace(state)?;
    let root = fixture.source_open().await?;
    let mut failed = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "overflow".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    assert!(fixture.drive(&mut failed).await.is_err());
    let mut state = fixture.actor.session.take()?;
    assert_eq!(
        state
            .session
            .tree()
            .node(root.root())?
            .annotations()
            .notes(),
        ""
    );
    state.editor.generation = crate::desktop::game_editor::DesktopGameEditorGeneration::ZERO;
    let mut restored = state.session.restoration(ClockInstant::from_nanos(50))?;
    restored.revision = u64::MAX.to_string().parse()?;
    state.session = GameSession::restore(restored)?;
    fixture.actor.session.replace(state)?;
    let root = fixture.source_open().await?;
    let mut failed = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveMarkings {
                markings: vec![GameMoveNodeMarking::circle(
                    GameEnumMarkingColor::Blue,
                    "a1".parse()?,
                )],
            },
        ))?;
    assert!(fixture.drive(&mut failed).await.is_err());
    let state = fixture.actor.session.take()?;
    assert!(
        state
            .session
            .tree()
            .node(root.root())?
            .annotations()
            .markings()
            .is_empty()
    );
    fixture.actor.session.replace(state)?;
    fixture.close().await
}

#[tokio::test]
async fn tree_children_and_confirmation_scope_are_paged_and_source_bound()
-> DesktopGameEditorRuntimeTestResult {
    let mut session = DesktopGameEditorRuntimeFixture::session(0x460, true)?;
    for index in 0..205_u128 {
        let uci = match index % 4 {
            0 => "g1f3",
            1 => "g8f6",
            2 => "f3g1",
            _ => "f6g8",
        };
        let movement = session
            .tree()
            .node(session.view())?
            .position()
            .parse_uci(uci)?;
        session.append_study_variation(
            session.revision(),
            uuid::Uuid::from_u128(index + 2).to_string().parse()?,
            movement,
        )?;
    }
    let mut fixture = DesktopGameEditorRuntimeFixture::with_session(session).await?;
    let snapshot = fixture.open().await?;
    let page = fixture
        .action(
            snapshot.context(),
            DesktopAnnotationCommandsEnumAction::TreePage { cursor: None },
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Tree(first) = page else {
        return Err("missing tree page".into());
    };
    assert_eq!(first.nodes().len(), 100);
    let second = fixture
        .action(
            snapshot.context(),
            DesktopAnnotationCommandsEnumAction::TreePage {
                cursor: first.next_cursor(),
            },
        )
        .await?;
    assert!(
        matches!(second, DesktopAnnotationCommandsEnumResult::Tree(page) if page.nodes().len() == 100)
    );
    let children = fixture
        .action(
            snapshot.context(),
            DesktopAnnotationCommandsEnumAction::ChildrenPage {
                parent: snapshot.root(),
                cursor: None,
            },
        )
        .await?;
    let DesktopAnnotationCommandsEnumResult::Children(children) = children else {
        return Err("missing children".into());
    };
    let preview = fixture
        .preview(
            snapshot.context(),
            GameAnnotationsEnumVariationRequest::Delete {
                root: children.nodes()[0].id(),
            },
        )
        .await?;
    assert_eq!(preview.deleted_count(), 205);
    for (offset, length) in [(0, 100), (100, 100), (200, 5)] {
        let scope = fixture
            .action(
                snapshot.context(),
                DesktopAnnotationCommandsEnumAction::VariationScope {
                    token: preview.token(),
                    scope: DesktopAnnotationCommandsEnumScope::DeletedNodes,
                    offset,
                },
            )
            .await?;
        assert!(
            matches!(scope, DesktopAnnotationCommandsEnumResult::Scope(page) if page.nodes().len() == length)
        );
    }
    let moved = fixture
        .navigate(snapshot.context(), snapshot.root())
        .await?;
    let mut stale = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            moved.context(),
            DesktopAnnotationCommandsEnumAction::TreePage {
                cursor: first.next_cursor(),
            },
        ))?;
    assert!(fixture.drive(&mut stale).await.is_err());
    fixture.close().await
}

#[tokio::test]
async fn completed_played_history_survives_study_promotion_and_cannot_be_deleted()
-> DesktopGameEditorRuntimeTestResult {
    let mut session = DesktopGameEditorRuntimeFixture::session(0x460, false)?;
    let now = ClockInstant::from_nanos(0);
    session.start(
        session.revision(),
        now,
        GameEnumEngineReadiness::NotRequired,
        &DesktopGameEditorRuntimeProof,
    )?;
    let movement = session
        .tree()
        .node(session.live())?
        .position()
        .parse_uci("e2e4")?;
    session.accept_move(
        crate::game::session::GameSessionMoveRequest {
            expected_revision: session.revision(),
            seat: GameEnumSeat::Human,
            color: BoardEnumColor::White,
            node: uuid::Uuid::from_u128(2).to_string().parse()?,
            movement,
            received_at: now,
        },
        &DesktopGameEditorRuntimeProof,
    )?;
    session.resign(
        crate::game::result::GameResultResignRequest {
            expected_revision: session.revision(),
            resigning: BoardEnumColor::Black,
            received_at: now,
            engine: crate::game::pause::GamePauseEnumEngineBarrier::NoEngine,
        },
        &DesktopGameEditorRuntimeProof,
    )?;
    let live = session.live();
    let played = session.live_moves()?;
    let outcome = session.outcome();
    let mut fixture = DesktopGameEditorRuntimeFixture::with_session(session).await?;
    let snapshot = fixture.open().await?;
    let root = fixture
        .navigate(snapshot.context(), snapshot.root())
        .await?;
    let alternative = DesktopGameEditorRuntimeFixture::snapshot(
        fixture.append(root.context(), 3, "d2d4").await?,
    )?;
    let preview = fixture
        .preview(
            alternative.context(),
            GameAnnotationsEnumVariationRequest::Promote {
                tip: alternative.context().selected(),
            },
        )
        .await?;
    let promoted = DesktopGameEditorRuntimeFixture::snapshot(
        fixture
            .action(
                alternative.context(),
                DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                    token: preview.token(),
                },
            )
            .await?,
    )?;
    assert_ne!(promoted.mainline_tip(), live);
    let mut forbidden = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            promoted.context(),
            DesktopAnnotationCommandsEnumAction::PreviewVariation(
                GameAnnotationsEnumVariationRequest::Delete { root: live },
            ),
        ))?;
    assert!(fixture.drive(&mut forbidden).await.is_err());
    let state = fixture.actor.session.take()?;
    assert_eq!(state.session.live(), live);
    assert_eq!(state.session.live_moves()?, played);
    assert_eq!(state.session.outcome(), outcome);
    fixture.actor.session.replace(state)?;
    fixture.close().await
}

#[tokio::test]
async fn failed_older_save_and_drained_newer_write_retry_the_newest_original_edit()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    fixture.wall.millis.store(2_000, Ordering::SeqCst);
    let mut first = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            root.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "older".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    fixture.compute_queued_editor().await?;
    fixture.actor.begin_save(false);
    let current = fixture.source_open().await?;
    fixture.wall.millis.store(3_000, Ordering::SeqCst);
    let mut newer = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            current.context(),
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "newest retained".to_owned(),
                nags: Vec::new(),
            },
        ))?;
    fixture.compute_queued_editor().await?;
    fixture.fail_storage(true).await?;
    fixture.finish_one_save().await?;
    assert!(first.wait().await.is_err());
    assert!(newer.wait().await.is_err());
    assert!(first.storage_receipt().is_none());
    assert!(newer.storage_receipt().is_none());
    fixture.fail_storage(false).await?;
    fixture.wall.failing.store(true, Ordering::SeqCst);
    let current = fixture.open().await?;
    assert_eq!(current.annotations().notes(), "newest retained");
    let retried = DesktopGameEditorRuntimeFixture::snapshot(
        fixture
            .action(
                current.context(),
                DesktopAnnotationCommandsEnumAction::RetrySave,
            )
            .await?,
    )?;
    assert_eq!(retried.annotations().notes(), "newest retained");
    assert_eq!(fixture.modified().await?, Some(3_000));
    fixture.close().await
}

#[tokio::test]
async fn due_live_clock_is_adjudicated_before_queued_editor_snapshot_work()
-> DesktopGameEditorRuntimeTestResult {
    let mut session = DesktopGameEditorRuntimeFixture::session(0x460, false)?;
    let mut restored = session.restoration(ClockInstant::from_nanos(0))?;
    restored.session.clock = ClockState::new(
        ClockTimeControl::sudden_death(crate::clock::duration::ClockDuration::from_millis(100)?)?,
        BoardEnumColor::White,
    );
    restored.clock = restored
        .session
        .clock
        .snapshot(ClockInstant::from_nanos(0))?;
    restored.root_clock = Some(restored.clock.clone());
    session = GameSession::restore(restored)?;
    session.start(
        session.revision(),
        ClockInstant::from_nanos(0),
        GameEnumEngineReadiness::NotRequired,
        &DesktopGameEditorRuntimeProof,
    )?;
    let mut fixture = DesktopGameEditorRuntimeFixture::with_session(session).await?;
    let before = fixture.open().await?;
    fixture.actor.clock.0.store(500_000_000, Ordering::SeqCst);
    let mut queued = fixture
        .port
        .try_submit_editor(DesktopAnnotationCommands::new(
            fixture.receipt()?,
            before.context(),
            DesktopAnnotationCommandsEnumAction::Snapshot,
        ))?;
    assert!(fixture.drive(&mut queued).await.is_err());
    assert_eq!(
        fixture.port.snapshot().snapshot.phase(),
        GameEnumPhase::Completed
    );
    fixture.close().await
}

#[derive(Debug, PartialEq, Eq)]
struct DesktopGameEditorRuntimeReviewSource {
    revision: crate::game::enums::GameRevision,
    accepted: Option<crate::game::storage_repository::GameStorageContentEdit>,
    notes: String,
    publishing: bool,
}

impl DesktopGameEditorRuntimeFixture {
    const REVIEW_TIMEOUT: Duration = Duration::from_secs(5);

    async fn poll_review_scheduler(&mut self) -> Poll<Result<(), DesktopGameSupervisorError>> {
        let mut running = Box::pin(tokio::task::coop::unconstrained(self.actor.run_loop()));
        poll_fn(|context| Poll::Ready(running.as_mut().poll(context))).await
    }

    fn review_source(
        &self,
        selected: GameMoveId,
    ) -> DesktopGameEditorRuntimeTestResult<DesktopGameEditorRuntimeReviewSource> {
        let state = self.actor.session.take()?;
        let observed = DesktopGameEditorRuntimeReviewSource {
            revision: state.session.revision(),
            accepted: state.accepted_edit,
            notes: state
                .session
                .tree()
                .node(selected)?
                .annotations()
                .notes()
                .to_owned(),
            publishing: state.editor.is_publishing(),
        };
        self.actor.session.replace(state)?;
        Ok(observed)
    }

    async fn release_review(self) -> DesktopGameEditorRuntimeTestResult {
        let work_observed = self.actor.computing.is_none()
            && self.actor.computing_response.is_none()
            && self.actor.computing_editor.is_none()
            && self.actor.saving.is_none()
            && self.actor.saving_context.is_none()
            && self.actor.completed_save.is_none()
            && self.actor.writes.is_empty()
            && self.actor.editor_projections.is_empty()
            && self.actor.deferred_ordinary.is_none()
            && self.actor.deferred_controls.is_empty()
            && self.actor.factory_request.is_none();
        let factory_observed = self.actor.factory_shutdown_complete;
        drop((self.actor, self.port, self.application));
        let memory = self
            .store
            .client()
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET);
        let memory_released = memory.is_ok();
        drop(memory);
        let storage_cleanup = self.store.close().await;
        let mut executor = Arc::try_unwrap(self.executor)
            .map_err(|_shared| "review executor remains owned after actor cleanup")?;
        let executor_cleanup = executor
            .shutdown_retained(Instant::now() + Self::REVIEW_TIMEOUT)
            .await;
        let executor_observed = executor.is_shutdown_complete();
        drop(executor);
        let capacity_released = self.budget.available() == self.budget.workers();
        let directory_cleanup = self.directory.close();
        storage_cleanup?;
        executor_cleanup?;
        directory_cleanup?;
        assert!(work_observed, "review left unobserved supervisor work");
        assert!(factory_observed, "review did not observe factory cleanup");
        assert!(executor_observed, "review did not observe executor cleanup");
        assert!(capacity_released, "review retained injected CPU capacity");
        assert!(memory_released, "review retained receiving memory");
        Ok(())
    }
}

#[tokio::test]
async fn deferred_editor_open_adjudicates_due_clock_before_returned_publication()
-> DesktopGameEditorRuntimeTestResult {
    let mut session = GameSession::new(GameSessionOptions {
        id: uuid::Uuid::from_u128(0x462).to_string().parse()?,
        root: uuid::Uuid::from_u128(1).to_string().parse()?,
        start: BoardStart::standard(),
        seats: GameSeats::new(
            GameEnumMode::LocalTwoPlayer,
            GameEnumSeat::Human,
            GameEnumSeat::Human,
        )?,
        clock: ClockState::new(
            ClockTimeControl::sudden_death(crate::clock::duration::ClockDuration::from_millis(
                60_000,
            )?)?,
            BoardEnumColor::White,
        ),
        metadata: GameMetadata::default(),
    })?;
    session.start(
        session.revision(),
        ClockInstant::from_nanos(0),
        GameEnumEngineReadiness::NotRequired,
        &DesktopGameEditorRuntimeProof,
    )?;
    let mut fixture = DesktopGameEditorRuntimeFixture::with_session(session).await?;
    let application = fixture.receipt()?;
    let request_id = application.request_id();
    let mut opened = fixture.port.try_open_editor(application.clone())?;
    let lease = fixture.budget.try_lease(1)?;
    let blocked = fixture.poll_review_scheduler().await;
    let deferred = matches!(
        &fixture.actor.deferred_ordinary,
        Some(DesktopGameSupervisorEnumOrdinary::Editor(envelope))
            if envelope.application.shares_outcome(&application)
                && matches!(
                    envelope.operation,
                    crate::desktop::game_supervisor::editor::DesktopGameSupervisorEditorEnumOperation::Open
                )
    );
    let save_unresolved = fixture.actor.saving.is_some()
        && fixture.actor.saving_context.is_some()
        && fixture.actor.completed_save.is_none()
        && fixture.actor.storage_revision == GameStorageRevision::INITIAL;
    let unpublished = opened.try_result().is_none();
    fixture
        .actor
        .clock
        .0
        .store(120_000_000_000, Ordering::SeqCst);
    let now = fixture.actor.clock.now()?;
    let clock_due = fixture
        .port
        .snapshot()
        .snapshot
        .clock()
        .view(now)?
        .deadline()
        .is_some_and(|deadline| now.has_reached(deadline));
    let deferred_while_due = fixture.actor.deferred_ordinary.is_some()
        && fixture.actor.saving.is_some()
        && opened.try_result().is_none();
    drop(lease);
    fixture.finish_one_save().await?;
    let saved_before_dispatch = fixture.actor.storage_revision != GameStorageRevision::INITIAL
        && fixture.actor.saving.is_none()
        && fixture.actor.writes.is_empty()
        && fixture.actor.deferred_ordinary.is_some();
    let result = tokio::time::timeout(
        DesktopGameEditorRuntimeFixture::REVIEW_TIMEOUT,
        fixture.drive(&mut opened),
    )
    .await;
    let publication: DesktopGameEditorRuntimeTestResult<_> = match result {
        Ok(Ok(DesktopAnnotationCommandsEnumResult::Snapshot(snapshot))) => {
            Ok((snapshot.context(), snapshot.state().clone()))
        }
        Ok(Ok(_)) => Err("Open returned an outcome without a snapshot publication".into()),
        Ok(Err(error)) => Err(error.into()),
        Err(error) => Err(error.into()),
    };
    let actual_receipt = opened.application_receipt().shares_outcome(&application);
    let shutdown = fixture
        .actor
        .shutdown_retained(Instant::now() + DesktopGameEditorRuntimeFixture::REVIEW_TIMEOUT)
        .await;
    drop((opened, application));
    fixture.release_review().await?;
    assert!(
        matches!(blocked, Poll::Pending),
        "scheduler returned before controlled save release: {blocked:?}"
    );
    assert!(deferred, "Open did not enter the real deferred slot");
    assert!(
        save_unresolved,
        "Open was not blocked by an unresolved save"
    );
    assert!(unpublished, "Open published before the save drained");
    assert!(clock_due, "the injected live deadline was not due");
    assert!(
        deferred_while_due,
        "Open was not deferred when the deadline became due"
    );
    assert!(
        saved_before_dispatch,
        "the original save did not drain before Open dispatch"
    );
    assert!(actual_receipt);
    assert!(shutdown.is_ok(), "supervisor cleanup failed: {shutdown:?}");
    let (context, state) = publication?;
    assert_eq!(context.epoch().request_id(), request_id);
    assert!(
        matches!(
            state,
            DesktopGameEditorSnapshotEnumState::Played {
                phase: GameEnumPhase::Completed,
                outcome: Some(outcome),
                ..
            } if outcome.reason() == crate::game::enums::GameEnumOutcomeReason::Flagfall
                && outcome.result() == crate::game::enums::GameEnumResult::Victory(BoardEnumColor::Black)
        ),
        "deferred Open returned a publication before adjudicating its due flag: {state:?}"
    );
    Ok(())
}

#[tokio::test]
async fn shutdown_retries_accepted_publication_after_receiving_memory_is_released()
-> DesktopGameEditorRuntimeTestResult {
    let mut fixture = DesktopGameEditorRuntimeFixture::new().await?;
    let root = fixture.open().await?;
    let expected = root.context();
    let selected = root.root();
    let reads_before = fixture.wall.reads.load(Ordering::SeqCst);
    let reservation = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - 65_536)?;
    fixture.wall.millis.store(2_000, Ordering::SeqCst);
    let application = fixture.receipt()?;
    let notes = "s".repeat(131_072);
    let command = || {
        DesktopAnnotationCommands::new(
            application.clone(),
            expected,
            DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: notes.clone(),
                nags: Vec::new(),
            },
        )
    };
    let mut original = fixture.port.try_submit_editor(command())?;
    let witness = original.clone();
    let pressure = fixture.drive(&mut original).await.map(drop);
    let accepted = fixture.review_source(selected)?;
    let reads_accepted = fixture.wall.reads.load(Ordering::SeqCst);
    let publishing = matches!(
        original.pending(),
        Some(
            crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumPending::Publishing { .. }
        )
    );
    let unsaved = original.storage_receipt().is_none() && witness.storage_receipt().is_none();
    fixture.wall.millis.store(9_000, Ordering::SeqCst);
    fixture.wall.failing.store(true, Ordering::SeqCst);
    let first_shutdown = fixture
        .actor
        .shutdown_retained(Instant::now() + DesktopGameEditorRuntimeFixture::REVIEW_TIMEOUT)
        .await;
    let retained = fixture.review_source(selected)?;
    let reads_retained = fixture.wall.reads.load(Ordering::SeqCst);
    drop(reservation);
    let second_shutdown = fixture
        .actor
        .shutdown_retained(Instant::now() + DesktopGameEditorRuntimeFixture::REVIEW_TIMEOUT)
        .await;
    let mut retry = None;
    let mut retry_admission_error = None;
    let mut retry_result = None;
    if second_shutdown.is_err() {
        match fixture.port.try_submit_editor(command()) {
            Ok(mut receipt) => {
                retry_result = Some(
                    tokio::time::timeout(
                        DesktopGameEditorRuntimeFixture::REVIEW_TIMEOUT,
                        fixture.drive(&mut receipt),
                    )
                    .await
                    .map(|result| result.map(drop)),
                );
                retry = Some(receipt);
            }
            Err(error) => retry_admission_error = Some(error),
        }
    }
    let final_shutdown = fixture
        .actor
        .shutdown_retained(Instant::now() + DesktopGameEditorRuntimeFixture::REVIEW_TIMEOUT)
        .await;
    let after = fixture.review_source(selected)?;
    let current = fixture.actor.session.is_current();
    let reads_after = fixture.wall.reads.load(Ordering::SeqCst);
    let receipt = original.storage_receipt();
    let witness_receipt = witness.storage_receipt();
    let correlated = original.application_receipt().shares_outcome(&application)
        && witness.application_receipt().shares_outcome(&application)
        && retry.as_ref().is_none_or(|retry| {
            retry.application_receipt().shares_outcome(&application)
                && retry.storage_receipt() == receipt
        });
    let publication = original.try_result().and_then(|result| match result {
        Ok(DesktopAnnotationCommandsEnumResult::Edited { change, snapshot }) => Some((
            change,
            snapshot.context(),
            snapshot.annotations().notes().to_owned(),
        )),
        _ => None,
    });
    let durable = StorageLibraryPlayedRepository::new(fixture.store.client(), &fixture.executor)
        .load(expected.game())
        .await?
        .ok_or("the initial durable game disappeared")?;
    let durable_revision = durable.session().revision();
    let durable_storage_revision = durable.storage_revision();
    let durable_notes = durable
        .session()
        .tree()
        .node(selected)?
        .annotations()
        .notes()
        .to_owned();
    let modified = fixture.modified().await?;
    drop((root, original, witness, retry, durable, application));
    fixture.release_review().await?;
    assert!(
        matches!(
            &pressure,
            Err(error) if matches!(
                error.as_ref(),
                DesktopGameSupervisorError::EditorPublicationPending { source, .. }
                    if matches!(source.as_ref(), crate::desktop::game_editor::DesktopGameEditorPublicationError::Memory(crate::storage::document_store::StorageDocumentError::Saturated))
            )
        ),
        "setup did not fail from real receiving memory pressure: {pressure:?}"
    );
    assert!(publishing && unsaved && accepted.publishing);
    assert_eq!(accepted.notes, notes);
    assert_eq!(reads_accepted, reads_before + 1);
    let edit = accepted.accepted.ok_or("the note edit was not accepted")?;
    assert_eq!(edit.revision(), accepted.revision);
    assert_eq!(edit.timestamp().value(), 2_000);
    assert_eq!(
        edit.cause(),
        crate::game::storage_repository::GameStorageEnumEditCause::Annotation
    );
    assert!(
        first_shutdown.is_err(),
        "shutdown acknowledged an unpublished accepted edit"
    );
    assert_eq!(
        retained, accepted,
        "shutdown changed the retained accepted edit"
    );
    assert_eq!(reads_retained, reads_accepted);
    assert!(correlated);
    assert!(
        final_shutdown.is_ok(),
        "released memory did not let shutdown finish the accepted publication: first={first_shutdown:?}, second={second_shutdown:?}, original_receipt_retry={retry_admission_error:?}, original_receipt_result={retry_result:?}, final={final_shutdown:?}"
    );
    assert_eq!(reads_after, reads_accepted, "recovery obtained another UTC");
    assert_eq!(
        after.revision, accepted.revision,
        "recovery replayed the edit"
    );
    assert_eq!(after.notes, accepted.notes);
    assert!(current && after.accepted.is_none() && !after.publishing);
    let receipt = receipt.ok_or("the original accepted edit has no actual storage receipt")?;
    assert_eq!(witness_receipt, Some(receipt));
    assert_eq!(receipt.game(), expected.game());
    assert_eq!(receipt.game_revision(), accepted.revision);
    assert_eq!(receipt.storage_revision(), durable_storage_revision);
    assert_eq!(durable_revision, accepted.revision);
    assert_eq!(durable_notes, accepted.notes);
    assert_eq!(modified, Some(edit.timestamp().value()));
    let (change, context, notes) =
        publication.ok_or("the original receipt has no edited publication")?;
    assert_eq!(change, GameAnnotationsEnumChange::Changed);
    assert_eq!(context.game(), expected.game());
    assert_eq!(
        context.source(),
        DesktopGameEditorEnumSource::Played {
            revision: accepted.revision
        }
    );
    assert_eq!(context.selected(), expected.selected());
    assert_eq!(context.epoch(), expected.epoch());
    assert_eq!(context.generation(), expected.generation().next()?);
    assert_eq!(context.storage_revision(), receipt.storage_revision());
    assert_eq!(notes, accepted.notes);
    Ok(())
}
