use std::sync::{Arc, Mutex, PoisonError};

use thiserror::Error;

use crate::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesError, DesktopPreferencesRevision,
};
use crate::desktop::preferences_import::DesktopPreferencesCodec;
use crate::storage::preferences_repository::{StoragePreferencesError, StoragePreferencesRepository};

#[derive(Debug, Error)]
pub enum DesktopPreferencesStoreError {
    #[error(transparent)]
    Storage(#[from] StoragePreferencesError),
    #[error(transparent)]
    Preferences(#[from] DesktopPreferencesError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPreferencesLoad {
    preferences: DesktopPreferences,
    revision: DesktopPreferencesRevision,
    warning: Option<String>,
}

impl DesktopPreferencesLoad {
    pub const fn defaults() -> Self {
        Self {
            preferences: DesktopPreferences::new_default(),
            revision: DesktopPreferencesRevision::INITIAL,
            warning: None,
        }
    }

    pub const fn preferences(&self) -> DesktopPreferences {
        self.preferences
    }

    pub const fn revision(&self) -> DesktopPreferencesRevision {
        self.revision
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}

type DesktopPreferencesStoreOutcome =
    Option<Result<DesktopPreferencesRevision, DesktopPreferencesStoreError>>;

pub struct DesktopPreferencesStore {
    repository: Arc<StoragePreferencesRepository>,
    runtime: tokio::runtime::Handle,
    outcome: Arc<Mutex<DesktopPreferencesStoreOutcome>>,
}

impl DesktopPreferencesStore {
    pub fn new(repository: StoragePreferencesRepository, runtime: tokio::runtime::Handle) -> Self {
        Self {
            repository: Arc::new(repository),
            runtime,
            outcome: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn load(&self) -> DesktopPreferencesLoad {
        match self.repository.load().await {
            Ok(None) => DesktopPreferencesLoad::defaults(),
            Ok(Some(record)) => {
                let revision = DesktopPreferencesRevision::new(record.revision());
                match DesktopPreferencesCodec::load(record.body().as_bytes()) {
                    Ok(preferences) => DesktopPreferencesLoad {
                        preferences,
                        revision,
                        warning: None,
                    },
                    Err(error) => DesktopPreferencesLoad {
                        preferences: DesktopPreferences::default(),
                        revision,
                        warning: Some(format!(
                            "Saved settings could not be read and defaults are in use: {error}"
                        )),
                    },
                }
            }
            Err(error) => DesktopPreferencesLoad {
                preferences: DesktopPreferences::default(),
                revision: DesktopPreferencesRevision::INITIAL,
                warning: Some(format!(
                    "Saved settings are unavailable and defaults are in use: {error}"
                )),
            },
        }
    }

    pub fn save(&self, expected: DesktopPreferencesRevision, preferences: DesktopPreferences) {
        let repository = Arc::clone(&self.repository);
        let outcome = Arc::clone(&self.outcome);
        self.runtime.spawn(async move {
            let result = Self::persist(&repository, expected, preferences).await;
            *outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        });
    }

    async fn persist(
        repository: &StoragePreferencesRepository,
        expected: DesktopPreferencesRevision,
        preferences: DesktopPreferences,
    ) -> Result<DesktopPreferencesRevision, DesktopPreferencesStoreError> {
        let body = DesktopPreferencesCodec::encode(preferences)?;
        let saved = repository
            .save(expected.value(), DesktopPreferencesCodec::CURRENT_VERSION, body)
            .await?;
        Ok(DesktopPreferencesRevision::new(saved))
    }

    pub fn take_outcome(&self) -> DesktopPreferencesStoreOutcome {
        self.outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}
