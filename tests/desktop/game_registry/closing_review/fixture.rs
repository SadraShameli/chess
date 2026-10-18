use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::{Notify, Semaphore};
use tokio::time::Instant;

use crate::clock::duration::ClockInstant;
use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumResult;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_bridge::{
    DesktopGameBridgeEnumOperation, DesktopGameBridgeProvider, DesktopGameBridgeRequest,
};
use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::game_editor::{
    DesktopGameEditorContext, DesktopGameEditorPublication, DesktopGameEditorSnapshotEnumState,
};
use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryCloseCertificate,
    DesktopGameRegistryCloseEnumObservation, DesktopGameRegistryCloseReceipt,
    DesktopGameRegistryError, DesktopGameRegistryFactoryProvider, DesktopGameRegistryOptions,
    DesktopGameRegistryPort, DesktopGameRegistrySavedOpenReceipt,
};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::game_supervisor::DesktopGameSupervisorEngineFactory;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestClock, DesktopLibraryServiceTestFactory,
    DesktopLibraryServiceTestProof, DesktopLibraryServiceTestResult, DesktopLibraryServiceTestWall,
};
use crate::game::engine_provider::GameEngineContext;
use crate::game::enums::{GameEnumPhase, GameId};
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_repository::{StorageLibraryRecovered, StorageLibraryRepository};

type DesktopGameRegistryClosingReviewRegistry = DesktopGameRegistry<
    DesktopGameRegistryClosingReviewFactories,
    DesktopLibraryServiceTestClock,
    DesktopLibraryServiceTestProof,
    StdRng,
>;

pub(super) struct DesktopGameRegistryClosingReviewGate {
    pub(super) entered: Notify,
    pub(super) release: Semaphore,
    pub(super) finished: Notify,
}

impl DesktopGameRegistryClosingReviewGate {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Notify::new(),
            release: Semaphore::new(0),
            finished: Notify::new(),
        })
    }
}

struct DesktopGameRegistryClosingReviewFactory {
    inner: DesktopLibraryServiceTestFactory,
    gate: Option<Arc<DesktopGameRegistryClosingReviewGate>>,
}

impl DesktopGameSupervisorEngineFactory for DesktopGameRegistryClosingReviewFactory {
    type Engine = <DesktopLibraryServiceTestFactory as DesktopGameSupervisorEngineFactory>::Engine;
    type Error = std::io::Error;

    async fn create(&mut self, context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        self.inner.create(context).await
    }

    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), Self::Error> {
        if let Some(gate) = self.gate.take() {
            gate.entered.notify_one();
            gate.release
                .acquire()
                .await
                .map_err(std::io::Error::other)?
                .forget();
            self.inner.shutdown_retained(deadline).await?;
            gate.finished.notify_one();
            Ok(())
        } else {
            self.inner.shutdown_retained(deadline).await
        }
    }
}

struct DesktopGameRegistryClosingReviewFactories {
    inner: DesktopLibraryServiceTestFactory,
    gate: Option<Arc<DesktopGameRegistryClosingReviewGate>>,
    target: GameId,
}

impl DesktopGameRegistryFactoryProvider for DesktopGameRegistryClosingReviewFactories {
    type Factory = DesktopGameRegistryClosingReviewFactory;
    type Error = std::io::Error;

    fn factory(&mut self, game: GameId) -> Result<Self::Factory, Self::Error> {
        Ok(DesktopGameRegistryClosingReviewFactory {
            inner: self.inner.factory(game)?,
            gate: if game == self.target {
                self.gate.take()
            } else {
                None
            },
        })
    }
}

#[derive(Debug)]
pub(super) struct DesktopGameRegistryClosingReviewOpenObservation {
    context: Option<DesktopGameEditorContext>,
    paused: bool,
    application_correlated: bool,
    editor_correlated: bool,
    failure: Option<Arc<DesktopGameRegistryError>>,
}

impl DesktopGameRegistryClosingReviewOpenObservation {
    pub(super) fn capture(
        receipt: &DesktopGameRegistrySavedOpenReceipt,
        application: &DesktopApplicationReceipt,
        result: Result<DesktopAnnotationCommandsEnumResult, Arc<DesktopGameRegistryError>>,
    ) -> Self {
        let (context, paused, failure) = match result {
            Ok(DesktopAnnotationCommandsEnumResult::Snapshot(snapshot)) => (
                Some(snapshot.context()),
                matches!(
                    snapshot.state(),
                    DesktopGameEditorSnapshotEnumState::Played {
                        phase: GameEnumPhase::Setup | GameEnumPhase::Interrupted,
                        ..
                    }
                ),
                None,
            ),
            Err(error) => (None, false, Some(error)),
            Ok(
                DesktopAnnotationCommandsEnumResult::Tree(_)
                | DesktopAnnotationCommandsEnumResult::Children(_)
                | DesktopAnnotationCommandsEnumResult::Edited { .. }
                | DesktopAnnotationCommandsEnumResult::Variation(_)
                | DesktopAnnotationCommandsEnumResult::Scope(_)
                | DesktopAnnotationCommandsEnumResult::PendingProjection { .. },
            ) => (None, false, None),
        };
        Self {
            context,
            paused,
            application_correlated: receipt.application_receipt().shares_outcome(application),
            editor_correlated: receipt
                .editor_receipt()
                .is_some_and(|editor| editor.application_receipt().shares_outcome(application)),
            failure,
        }
    }

    pub(super) fn assert_opened(&self, game: GameId, stored: GameStorageRevision) {
        assert!(self.failure.is_none(), "saved open failed: {self:?}");
        assert_eq!(self.context.map(DesktopGameEditorContext::game), Some(game));
        assert_eq!(
            self.context.map(DesktopGameEditorContext::storage_revision),
            Some(stored)
        );
        assert!(self.paused);
        assert!(self.application_correlated);
        assert!(self.editor_correlated);
    }
}

pub(super) struct DesktopGameRegistryClosingReviewFixture {
    pub(super) library: DesktopLibraryServiceFixture,
    registry: DesktopGameRegistryClosingReviewRegistry,
    pub(super) games: DesktopGameRegistryPort,
}

impl DesktopGameRegistryClosingReviewFixture {
    pub(super) const TARGET: u128 = 100;
    pub(super) const OTHER: u128 = 200;
    const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);

    pub(super) async fn new(
        gate: Option<Arc<DesktopGameRegistryClosingReviewGate>>,
    ) -> DesktopLibraryServiceTestResult<Self> {
        let library = DesktopLibraryServiceFixture::new().await?;
        let (registry, games) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
            factory_provider: DesktopGameRegistryClosingReviewFactories {
                inner: DesktopLibraryServiceTestFactory {
                    factories: Arc::clone(&library.factories),
                    shutdowns: Arc::clone(&library.shutdowns),
                },
                gate,
                target: DesktopLibraryServiceFixture::id(Self::TARGET)?,
            },
            storage: library.store.client(),
            executor: Arc::clone(&library.executor),
            clock: Arc::new(DesktopLibraryServiceTestClock),
            wall_time: Arc::<DesktopLibraryServiceTestWall>::clone(&library.wall),
            proof: Arc::new(DesktopLibraryServiceTestProof),
            random: StdRng::seed_from_u64(11),
        });
        Ok(Self {
            library,
            registry,
            games,
        })
    }

    pub(super) async fn drive<T>(
        &mut self,
        future: impl Future<Output = T>,
    ) -> DesktopLibraryServiceTestResult<T> {
        tokio::select! {
            result = future => Ok(result),
            result = self.registry.run() => {
                Err(format!("registry stopped before receipt: {result:?}").into())
            }
        }
    }

    pub(super) async fn poll(&mut self) -> DesktopLibraryServiceTestResult {
        let mut running = Box::pin(self.registry.run());
        poll_fn(|context| match running.as_mut().poll(context) {
            Poll::Pending => Poll::Ready(Ok(())),
            Poll::Ready(result) => Poll::Ready(Err(format!(
                "registry stopped while work was retained: {result:?}"
            )
            .into())),
        })
        .await
    }

    pub(super) async fn open_target(
        &mut self,
        stored: GameStorageRevision,
    ) -> DesktopLibraryServiceTestResult<DesktopGameEditorPublication> {
        let mut receipt = self.games.try_open_saved_editor(
            DesktopLibraryServiceFixture::id(Self::TARGET)?,
            stored,
            self.library.receipt()?,
        )?;
        let DesktopAnnotationCommandsEnumResult::Snapshot(snapshot) =
            self.drive(receipt.wait()).await??
        else {
            return Err("target opening did not publish an editor".into());
        };
        Ok(snapshot)
    }

    fn target_snapshot(&self) -> DesktopLibraryServiceTestResult<DesktopGameSnapshot> {
        let game = DesktopLibraryServiceFixture::id(Self::TARGET)?;
        self.games
            .snapshots()
            .borrow()
            .iter()
            .find(|snapshot| snapshot.id() == game)
            .cloned()
            .ok_or_else(|| "missing target snapshot".into())
    }

    pub(super) async fn target_action(
        &self,
        action: DesktopGameCommandsEnumAction,
    ) -> DesktopLibraryServiceTestResult<DesktopGameSnapshot> {
        let snapshot = self.target_snapshot()?;
        let application = self.library.receipt()?;
        Ok(self
            .games
            .execute(DesktopGameBridgeRequest {
                request_id: application.request_id(),
                received_at: ClockInstant::from_nanos(50),
                operation: DesktopGameBridgeEnumOperation::Session {
                    game: snapshot.id(),
                    revision: snapshot.revision(),
                    action,
                },
            })
            .await?)
    }

    pub(super) fn target_close(
        &self,
        stored: GameStorageRevision,
    ) -> DesktopLibraryServiceTestResult<DesktopGameRegistryCloseReceipt> {
        let snapshot = self.target_snapshot()?;
        let target = self
            .games
            .try_close_target(snapshot.id(), snapshot.revision(), stored)?;
        Ok(self.games.try_close_game(target)?)
    }

    pub(super) fn target_is_blocked(
        &self,
        stored: GameStorageRevision,
    ) -> DesktopLibraryServiceTestResult<bool> {
        Ok(matches!(
            self.games.try_open_saved_editor(
                DesktopLibraryServiceFixture::id(Self::TARGET)?,
                stored,
                self.library.receipt()?,
            ),
            Err(DesktopGameRegistryError::Busy)
        ))
    }

    pub(super) async fn stored_target(
        &self,
    ) -> DesktopLibraryServiceTestResult<StorageLibraryRecovered> {
        StorageLibraryRepository::new(self.library.store.client(), &self.library.executor)
            .load(DesktopLibraryServiceFixture::id(Self::TARGET)?)
            .await?
            .ok_or_else(|| "missing stored target".into())
    }

    pub(super) async fn release(
        &mut self,
        receipt: &mut DesktopGameRegistryCloseReceipt,
        certificate: DesktopGameRegistryCloseCertificate,
    ) -> DesktopLibraryServiceTestResult {
        self.games.try_finish_close(
            &certificate,
            DesktopGameRegistryCloseEnumObservation::CancelConfirmed,
        )?;
        self.drive(receipt.wait_released()).await??;
        Ok(())
    }

    pub(super) async fn close(mut self) -> DesktopLibraryServiceTestResult {
        self.registry
            .shutdown_retained(Instant::now() + Self::CLEANUP_TIMEOUT)
            .await?;
        let stopped = self.registry.is_shutdown_complete();
        drop((self.registry, self.games));
        self.library.close().await?;
        if !stopped {
            return Err("review registry retained unfinished shutdown".into());
        }
        Ok(())
    }
}
