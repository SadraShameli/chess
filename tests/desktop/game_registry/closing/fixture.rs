use crate::storage_sqlite_inspector::StorageSqliteInspector;
use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::{Notify, Semaphore};
use tokio::time::Instant;

use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryCloseCertificate, DesktopGameRegistryCloseReceipt,
    DesktopGameRegistryFactoryProvider, DesktopGameRegistryOptions,
};
use crate::desktop::game_supervisor::DesktopGameSupervisorEngineFactory;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestClock, DesktopLibraryServiceTestFactory,
    DesktopLibraryServiceTestProof, DesktopLibraryServiceTestResult, DesktopLibraryServiceTestWall,
};
use crate::game::engine_provider::GameEngineContext;
use crate::game::enums::GameId;

pub(super) type DesktopGameRegistryClosingTestRegistry = DesktopGameRegistry<
    DesktopGameRegistryClosingFactories,
    DesktopLibraryServiceTestClock,
    DesktopLibraryServiceTestProof,
    StdRng,
>;

pub(super) struct DesktopGameRegistryClosingGate {
    pub(super) entered: Notify,
    pub(super) release: Semaphore,
}

impl DesktopGameRegistryClosingGate {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Notify::new(),
            release: Semaphore::new(0),
        })
    }
}

pub(super) struct DesktopGameRegistryClosingFactory {
    inner: DesktopLibraryServiceTestFactory,
    gate: Option<Arc<DesktopGameRegistryClosingGate>>,
}

impl DesktopGameSupervisorEngineFactory for DesktopGameRegistryClosingFactory {
    type Engine = <DesktopLibraryServiceTestFactory as DesktopGameSupervisorEngineFactory>::Engine;
    type Error = std::io::Error;

    async fn create(&mut self, context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        self.inner.create(context).await
    }

    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), Self::Error> {
        if let Some(gate) = &self.gate {
            gate.entered.notify_one();
            gate.release
                .acquire()
                .await
                .map_err(std::io::Error::other)?
                .forget();
            self.gate = None;
        }
        self.inner.shutdown_retained(deadline).await
    }
}

pub(super) struct DesktopGameRegistryClosingFactories {
    pub(super) fail: Arc<AtomicBool>,
    factory: DesktopLibraryServiceTestFactory,
    gate: Option<Arc<DesktopGameRegistryClosingGate>>,
}

impl DesktopGameRegistryFactoryProvider for DesktopGameRegistryClosingFactories {
    type Factory = DesktopGameRegistryClosingFactory;
    type Error = std::io::Error;

    fn factory(&mut self, game: GameId) -> Result<Self::Factory, Self::Error> {
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(std::io::Error::other("injected source constructor failure"));
        }
        Ok(DesktopGameRegistryClosingFactory {
            inner: self.factory.factory(game)?,
            gate: self.gate.as_ref().map(Arc::clone),
        })
    }
}

pub(super) struct DesktopGameRegistryClosingFixture;

impl DesktopGameRegistryClosingFixture {
    pub(super) fn failing_registry(
        fixture: &DesktopLibraryServiceFixture,
    ) -> (
        DesktopGameRegistryClosingTestRegistry,
        crate::desktop::game_registry::DesktopGameRegistryPort,
    ) {
        Self::registry(fixture, true, None)
    }

    pub(super) fn gated_registry(
        fixture: &DesktopLibraryServiceFixture,
        gate: Arc<DesktopGameRegistryClosingGate>,
    ) -> (
        DesktopGameRegistryClosingTestRegistry,
        crate::desktop::game_registry::DesktopGameRegistryPort,
    ) {
        Self::registry(fixture, false, Some(gate))
    }

    fn registry(
        fixture: &DesktopLibraryServiceFixture,
        fail: bool,
        gate: Option<Arc<DesktopGameRegistryClosingGate>>,
    ) -> (
        DesktopGameRegistryClosingTestRegistry,
        crate::desktop::game_registry::DesktopGameRegistryPort,
    ) {
        DesktopGameRegistry::new(DesktopGameRegistryOptions {
            factory_provider: DesktopGameRegistryClosingFactories {
                fail: Arc::new(AtomicBool::new(fail)),
                gate,
                factory: DesktopLibraryServiceTestFactory {
                    factories: Arc::clone(&fixture.factories),
                    shutdowns: Arc::clone(&fixture.shutdowns),
                },
            },
            storage: fixture.store.client(),
            executor: Arc::clone(&fixture.executor),
            clock: Arc::new(DesktopLibraryServiceTestClock),
            wall_time: Arc::<DesktopLibraryServiceTestWall>::clone(&fixture.wall),
            proof: Arc::new(DesktopLibraryServiceTestProof),
            random: StdRng::seed_from_u64(11),
        })
    }

    pub(super) async fn document(
        fixture: &DesktopLibraryServiceFixture,
        game: GameId,
    ) -> DesktopLibraryServiceTestResult<String> {
        let path = fixture.directory.path().join("library.sqlite");
        Ok(fixture
            .executor
            .submit_waiting(move |_cancellation| {
                StorageSqliteInspector::open(path)?.text(&format!(
                    "SELECT body AS value FROM documents WHERE id = '{}'",
                    String::from(game)
                ))
            })
            .await?
            .join()
            .await??)
    }

    pub(super) async fn drive(
        fixture: &mut DesktopLibraryServiceFixture,
        receipt: &mut DesktopGameRegistryCloseReceipt,
    ) -> DesktopLibraryServiceTestResult<DesktopGameRegistryCloseCertificate> {
        let mut running = Box::pin(fixture.registry.run());
        let mut waiting = Box::pin(receipt.wait());
        let result = tokio::select! {
            result = &mut waiting => result.map_err(|error| error.to_string().into()),
            result = &mut running => Err(format!("registry stopped during close: {result:?}").into()),
        };
        drop((waiting, running));
        result
    }

    pub(super) async fn poll(fixture: &mut DesktopLibraryServiceFixture) {
        let mut running = Box::pin(fixture.registry.run());
        poll_fn(|context| {
            assert!(running.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }

    pub(super) async fn released(
        fixture: &mut DesktopLibraryServiceFixture,
        receipt: &mut DesktopGameRegistryCloseReceipt,
    ) -> DesktopLibraryServiceTestResult {
        let mut running = Box::pin(fixture.registry.run());
        let result = tokio::select! {
            result = receipt.wait_released() => result.map_err(|error| error.to_string().into()),
            result = &mut running => Err(format!("registry stopped before barrier release: {result:?}").into()),
        };
        drop(running);
        result
    }
}
