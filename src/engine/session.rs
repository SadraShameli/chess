use std::time::Duration;

use thiserror::Error;
use tokio::time::timeout;

use crate::engine::capabilities::{EngineCapabilities, EngineCapabilitiesError, EngineIdentity};
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessProvider,
};

#[derive(Debug, Error)]
pub enum EngineSessionError {
    #[error(transparent)]
    Process(#[from] EngineProcessError),
    #[error(transparent)]
    Capabilities(#[from] EngineCapabilitiesError),
    #[error("Engine {0} handshake exceeded five seconds")]
    Timeout(&'static str),
    #[error("The engine sent an unexpected startup response")]
    UnexpectedResponse,
    #[error("Engine startup failed ({startup}); cleanup also failed ({cleanup})")]
    Cleanup {
        startup: Box<Self>,
        cleanup: EngineProcessError,
    },
}

pub struct EngineSession<P: EngineProcess> {
    process: P,
    identity: EngineIdentity,
    package: EnginePackageSelected,
}

impl<P: EngineProcess> EngineSession<P> {
    pub const HANDSHAKE: Duration = Duration::from_secs(5);

    pub async fn start(
        provider: &impl EngineProcessProvider<Process = P>,
        package: EnginePackageSelected,
    ) -> Result<Self, EngineSessionError> {
        let mut process = provider.spawn(&package)?;
        let result = Self::handshake(&mut process).await;
        match result {
            Ok(identity) => Ok(Self {
                process,
                identity,
                package,
            }),
            Err(startup) => match process.shutdown().await {
                Ok(()) => Err(startup),
                Err(cleanup) => Err(EngineSessionError::Cleanup {
                    startup: Box::new(startup),
                    cleanup,
                }),
            },
        }
    }

    pub fn identity(&self) -> &EngineIdentity {
        &self.identity
    }

    pub fn package(&self) -> &EnginePackageSelected {
        &self.package
    }

    pub async fn shutdown(mut self) -> Result<(), EngineSessionError> {
        self.process.shutdown().await?;
        Ok(())
    }

    async fn handshake(process: &mut P) -> Result<EngineIdentity, EngineSessionError> {
        let identity = timeout(Self::HANDSHAKE, Self::identify(process))
            .await
            .map_err(|_elapsed| EngineSessionError::Timeout("uci"))??;
        timeout(Self::HANDSHAKE, Self::ready(process))
            .await
            .map_err(|_elapsed| EngineSessionError::Timeout("readiness"))??;
        Ok(identity)
    }

    async fn identify(process: &mut P) -> Result<EngineIdentity, EngineSessionError> {
        process.send("uci").await?;
        let mut name = None;
        let mut author = None;
        let mut capabilities = EngineCapabilities::default();
        loop {
            let line = process.receive().await?;
            if line.pipe() == EngineProcessEnumPipe::Stderr {
                continue;
            }
            let text = line.text();
            if let Some(value) = text.strip_prefix("id name ") {
                if name.replace(value.to_owned()).is_some() {
                    return Err(EngineSessionError::UnexpectedResponse);
                }
            } else if let Some(value) = text.strip_prefix("id author ") {
                if author.replace(value.to_owned()).is_some() {
                    return Err(EngineSessionError::UnexpectedResponse);
                }
            } else if text.starts_with("option ") {
                capabilities.discover(text)?;
            } else if text == "uciok" {
                return Ok(EngineIdentity::new(
                    name.ok_or(EngineCapabilitiesError::IdentityMismatch)?,
                    author.ok_or(EngineCapabilitiesError::IdentityMismatch)?,
                    capabilities,
                )?);
            } else {
                return Err(EngineSessionError::UnexpectedResponse);
            }
        }
    }

    async fn ready(process: &mut P) -> Result<(), EngineSessionError> {
        process.send("isready").await?;
        loop {
            let line = process.receive().await?;
            if line.pipe() == EngineProcessEnumPipe::Stderr {
                continue;
            }
            if line.text() == "readyok" {
                return Ok(());
            }
            return Err(EngineSessionError::UnexpectedResponse);
        }
    }
}
