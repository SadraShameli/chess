use std::convert::Infallible;
use std::sync::Arc;

use thiserror::Error;
use tokio::time::Instant;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::game_registry::DesktopGameRegistryFactoryProvider;
use crate::desktop::game_supervisor::DesktopGameSupervisorEngineFactory;
use crate::desktop::native::game_engine::{
    DesktopNativeGameEngine, DesktopNativeGameEngineError, DesktopNativeGameEngineFactory,
    DesktopNativeGameEngineFactoryOptions, DesktopNativeGameEngineResources,
};
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::{EngineNativeProcess, EngineNativeProcessProvider};
use crate::game::engine_provider::GameEngineContext;
use crate::game::enums::GameId;

#[derive(Debug, Error)]
pub enum DesktopNativeGameFactoriesError<E: std::error::Error + Send + Sync + 'static> {
    #[error("The packaged Reckless engine is unavailable; repair its package and restart Chess")]
    Unavailable,
    #[error(transparent)]
    Engine(#[from] DesktopNativeGameEngineError<E>),
}

pub struct DesktopNativeGameFactories<C: ClockTimeProvider, R: DesktopNativeGameEngineResources> {
    package: Option<EnginePackageSelected>,
    clock: Arc<C>,
    resources: Arc<R>,
}

impl<C: ClockTimeProvider, R: DesktopNativeGameEngineResources> DesktopNativeGameFactories<C, R> {
    pub const fn new(
        package: Option<EnginePackageSelected>,
        clock: Arc<C>,
        resources: Arc<R>,
    ) -> Self {
        Self {
            package,
            clock,
            resources,
        }
    }
}

impl<C: ClockTimeProvider + 'static, R: DesktopNativeGameEngineResources + 'static>
    DesktopGameRegistryFactoryProvider for DesktopNativeGameFactories<C, R>
{
    type Factory = DesktopNativeGameFactoriesFactory<C, R>;
    type Error = Infallible;

    fn factory(&mut self, _game: GameId) -> Result<Self::Factory, Self::Error> {
        Ok(DesktopNativeGameFactoriesFactory {
            engine: self.package.as_ref().map(|package| {
                DesktopNativeGameEngineFactory::new(DesktopNativeGameEngineFactoryOptions {
                    processes: EngineNativeProcessProvider,
                    package: package.clone(),
                    clock: Arc::clone(&self.clock),
                    resources: Arc::clone(&self.resources),
                })
            }),
        })
    }
}

pub struct DesktopNativeGameFactoriesFactory<
    C: ClockTimeProvider,
    R: DesktopNativeGameEngineResources,
> {
    engine: Option<DesktopNativeGameEngineFactory<EngineNativeProcessProvider, C, R>>,
}

impl<C: ClockTimeProvider + 'static, R: DesktopNativeGameEngineResources + 'static>
    DesktopGameSupervisorEngineFactory for DesktopNativeGameFactoriesFactory<C, R>
{
    type Engine = DesktopNativeGameEngine<EngineNativeProcess, C, R>;
    type Error = DesktopNativeGameFactoriesError<R::Error>;

    async fn create(&mut self, context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        match &mut self.engine {
            Some(engine) => Ok(engine.create(context).await?),
            None => Err(DesktopNativeGameFactoriesError::Unavailable),
        }
    }

    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), Self::Error> {
        match &mut self.engine {
            Some(engine) => Ok(engine.shutdown_retained(deadline).await?),
            None => Ok(()),
        }
    }
}
