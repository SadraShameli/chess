use std::num::NonZeroU32;
use std::path::PathBuf;

use thiserror::Error;

use crate::engine::options::{
    EngineOptionsEnumOutcome, EngineOptionsEnumTablebases, EngineOptionsEnumValue,
    EngineOptionsSetting, EngineOptionsSyzygyPath,
};
use crate::engine::process_provider::EngineProcess;
use crate::engine::session::{EngineSession, EngineSessionError};
use crate::engine::tablebase::location::EngineTablebaseLocation;

#[derive(Debug, Error)]
pub enum EngineTablebaseStatusError {
    #[error("The engine configuration result did not contain an explicit tablebase load outcome")]
    UnexpectedOutcome,
    #[error("The engine tablebase outcome belongs to a different configured alias path")]
    PathMismatch,
    #[error(transparent)]
    Session(#[from] EngineSessionError),
}

#[derive(Debug)]
pub enum EngineTablebaseStatusEnumState {
    Configured,
    Loaded { pieces: NonZeroU32 },
    Unavailable,
    Failed(EngineTablebaseStatusError),
}

#[derive(Debug)]
pub struct EngineTablebaseStatus {
    source_directories: Vec<PathBuf>,
    engine_path: EngineOptionsSyzygyPath,
    state: EngineTablebaseStatusEnumState,
}

impl EngineTablebaseStatus {
    pub fn configured(location: &EngineTablebaseLocation) -> Self {
        Self {
            source_directories: location.source_directories().to_vec(),
            engine_path: location.engine_path().clone(),
            state: EngineTablebaseStatusEnumState::Configured,
        }
    }

    pub fn source_directories(&self) -> &[PathBuf] {
        &self.source_directories
    }

    pub const fn engine_path(&self) -> &EngineOptionsSyzygyPath {
        &self.engine_path
    }

    pub const fn state(&self) -> &EngineTablebaseStatusEnumState {
        &self.state
    }

    pub async fn configure<P: EngineProcess>(
        &mut self,
        session: &mut EngineSession<P>,
    ) -> &EngineTablebaseStatusEnumState {
        let setting = EngineOptionsSetting::new(
            EngineOptionsEnumValue::SyzygyPath(self.engine_path.clone()),
            session.identity().capabilities(),
        );
        let result = match setting {
            Ok(setting) => session.configure_option(setting).await,
            Err(error) => Err(EngineSessionError::Options(error)),
        };
        self.record_configuration(result);
        self.state()
    }

    pub fn record_configuration(
        &mut self,
        result: Result<EngineOptionsEnumOutcome, EngineSessionError>,
    ) {
        self.state = match result {
            Ok(EngineOptionsEnumOutcome::Tablebases(EngineOptionsEnumTablebases::Loaded {
                path,
                pieces,
            })) if path == self.engine_path => EngineTablebaseStatusEnumState::Loaded { pieces },
            Ok(EngineOptionsEnumOutcome::Tablebases(
                EngineOptionsEnumTablebases::Unavailable { path },
            )) if path == self.engine_path => EngineTablebaseStatusEnumState::Unavailable,
            Ok(EngineOptionsEnumOutcome::Tablebases(
                EngineOptionsEnumTablebases::Loaded { .. }
                | EngineOptionsEnumTablebases::Unavailable { .. },
            )) => EngineTablebaseStatusEnumState::Failed(EngineTablebaseStatusError::PathMismatch),
            Ok(EngineOptionsEnumOutcome::Applied(_))
            | Ok(EngineOptionsEnumOutcome::Tablebases(EngineOptionsEnumTablebases::Unconfigured)) => {
                EngineTablebaseStatusEnumState::Failed(
                    EngineTablebaseStatusError::UnexpectedOutcome,
                )
            }
            Err(error) => {
                EngineTablebaseStatusEnumState::Failed(EngineTablebaseStatusError::Session(error))
            }
        };
    }
}
