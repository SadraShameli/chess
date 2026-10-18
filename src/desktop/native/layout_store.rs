use serde_json::{Map, Value};
use thiserror::Error;

use crate::desktop::layout::DesktopLayout;
use crate::storage::preferences_repository::{
    StoragePreferencesError, StoragePreferencesRepository,
};

#[derive(Debug, Error)]
pub enum DesktopNativeLayoutStoreError {
    #[error(transparent)]
    Preferences(#[from] StoragePreferencesError),
    #[error("The stored preferences are not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("The stored preferences are not a JSON object")]
    Shape,
    #[error("The preferences revision kept changing while the layout was saved")]
    Contention,
}

pub struct DesktopNativeLayoutStore {
    repository: StoragePreferencesRepository,
}

impl DesktopNativeLayoutStore {
    pub const KEY: &'static str = "layout";
    pub const SCHEMA_VERSION: u32 = 1;
    const ATTEMPTS: u8 = 3;

    pub const fn new(repository: StoragePreferencesRepository) -> Self {
        Self { repository }
    }

    pub fn extract(body: &str) -> Result<Option<DesktopLayout>, DesktopNativeLayoutStoreError> {
        let Value::Object(mut object) = serde_json::from_str::<Value>(body)? else {
            return Err(DesktopNativeLayoutStoreError::Shape);
        };
        object
            .remove(Self::KEY)
            .map(|value| {
                serde_json::from_value::<DesktopLayout>(value)
                    .map_err(DesktopNativeLayoutStoreError::from)
            })
            .transpose()
    }

    pub fn merge(
        body: Option<&str>,
        layout: &DesktopLayout,
    ) -> Result<String, DesktopNativeLayoutStoreError> {
        let mut object = match body {
            Some(body) => match serde_json::from_str::<Value>(body)? {
                Value::Object(object) => object,
                Value::Null
                | Value::Bool(_)
                | Value::Number(_)
                | Value::String(_)
                | Value::Array(_) => return Err(DesktopNativeLayoutStoreError::Shape),
            },
            None => Map::new(),
        };
        object.insert(Self::KEY.to_owned(), serde_json::to_value(layout)?);
        Ok(serde_json::to_string(&Value::Object(object))?)
    }

    pub async fn load(&self) -> Result<DesktopLayout, DesktopNativeLayoutStoreError> {
        match self.repository.load().await? {
            Some(record) => Ok(Self::extract(record.body())?.unwrap_or_default()),
            None => Ok(DesktopLayout::default()),
        }
    }

    pub async fn save(&self, layout: &DesktopLayout) -> Result<(), DesktopNativeLayoutStoreError> {
        for _attempt in 0..Self::ATTEMPTS {
            let record = self.repository.load().await?;
            let expected = record.as_ref().map_or(0, |record| record.revision());
            let version = record
                .as_ref()
                .map_or(Self::SCHEMA_VERSION, |record| record.schema_version());
            let body = Self::merge(record.as_ref().map(|record| record.body()), layout)?;
            match self.repository.save(expected, version, body).await {
                Ok(_revision) => return Ok(()),
                Err(StoragePreferencesError::Stale { .. }) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(DesktopNativeLayoutStoreError::Contention)
    }
}
