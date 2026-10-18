use diesel::SqliteConnection;
use diesel::prelude::*;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::storage::document_store::{StorageDocumentClient, StorageDocumentError};
use crate::storage::schema::preferences;

#[derive(Debug, Error)]
pub enum StoragePreferencesError {
    #[error(transparent)]
    Document(#[from] StorageDocumentError),
    #[error(transparent)]
    Diesel(#[from] diesel::result::Error),
    #[error("The preferences reply channel closed: {0}")]
    Channel(#[from] oneshot::error::RecvError),
    #[error("The stored preferences revision is invalid: {0}")]
    Revision(#[from] std::num::ParseIntError),
    #[error("The preferences body exceeds {0} bytes")]
    BodyTooLarge(usize),
    #[error("The preferences revision {expected} is stale; the stored revision is {stored}")]
    Stale { expected: u64, stored: u64 },
    #[error("The preferences schema version exceeds its supported range")]
    SchemaVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePreferencesRecord {
    revision: u64,
    schema_version: u32,
    body: String,
}

impl StoragePreferencesRecord {
    pub const MAX_BODY_BYTES: usize = 1024 * 1024;

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn body(&self) -> &str {
        &self.body
    }
}

#[derive(Debug)]
pub(crate) enum StoragePreferencesEnumRequest {
    Load,
    Save {
        expected: u64,
        schema_version: u32,
        body: String,
    },
}

#[derive(Debug)]
pub(crate) enum StoragePreferencesEnumResponse {
    Loaded(Option<StoragePreferencesRecord>),
    Saved(u64),
}

pub(crate) struct StoragePreferencesTransaction;

impl StoragePreferencesTransaction {
    pub(crate) fn execute(
        connection: &mut SqliteConnection,
        request: StoragePreferencesEnumRequest,
    ) -> Result<StoragePreferencesEnumResponse, StoragePreferencesError> {
        match request {
            StoragePreferencesEnumRequest::Load => connection
                .transaction(Self::load)
                .map(StoragePreferencesEnumResponse::Loaded),
            StoragePreferencesEnumRequest::Save {
                expected,
                schema_version,
                body,
            } => connection
                .transaction(|connection| Self::save(connection, expected, schema_version, &body))
                .map(StoragePreferencesEnumResponse::Saved),
        }
    }

    fn load(
        connection: &mut SqliteConnection,
    ) -> Result<Option<StoragePreferencesRecord>, StoragePreferencesError> {
        let row: Option<(i32, String, String)> = preferences::table
            .select((
                preferences::schema_version,
                preferences::revision,
                preferences::body,
            ))
            .first(connection)
            .optional()?;
        row.map(|(schema_version, revision, body)| {
            Ok(StoragePreferencesRecord {
                revision: revision.parse()?,
                schema_version: u32::try_from(schema_version)
                    .map_err(|_error| StoragePreferencesError::SchemaVersion)?,
                body,
            })
        })
        .transpose()
    }

    fn save(
        connection: &mut SqliteConnection,
        expected: u64,
        schema_version: u32,
        body: &str,
    ) -> Result<u64, StoragePreferencesError> {
        if body.len() > StoragePreferencesRecord::MAX_BODY_BYTES {
            return Err(StoragePreferencesError::BodyTooLarge(
                StoragePreferencesRecord::MAX_BODY_BYTES,
            ));
        }
        let stored = Self::load(connection)?.map_or(0, |record| record.revision);
        if stored != expected {
            return Err(StoragePreferencesError::Stale { expected, stored });
        }
        let next = expected
            .checked_add(1)
            .ok_or(StoragePreferencesError::Stale { expected, stored })?;
        let version = i32::try_from(schema_version)
            .map_err(|_error| StoragePreferencesError::SchemaVersion)?;
        let revision = next.to_string();
        diesel::insert_into(preferences::table)
            .values((
                preferences::singleton.eq(Some(1)),
                preferences::schema_version.eq(version),
                preferences::revision.eq(&revision),
                preferences::body.eq(body),
            ))
            .on_conflict(preferences::singleton)
            .do_update()
            .set((
                preferences::schema_version.eq(version),
                preferences::revision.eq(&revision),
                preferences::body.eq(body),
            ))
            .execute(connection)?;
        Ok(next)
    }
}

pub struct StoragePreferencesRepository {
    client: StorageDocumentClient,
}

impl StoragePreferencesRepository {
    pub const fn new(client: StorageDocumentClient) -> Self {
        Self { client }
    }

    pub async fn load(&self) -> Result<Option<StoragePreferencesRecord>, StoragePreferencesError> {
        match self
            .client
            .preferences(StoragePreferencesEnumRequest::Load)
            .await?
        {
            StoragePreferencesEnumResponse::Loaded(record) => Ok(record),
            StoragePreferencesEnumResponse::Saved(_) => Err(StoragePreferencesError::Stale {
                expected: 0,
                stored: 0,
            }),
        }
    }

    pub async fn save(
        &self,
        expected: u64,
        schema_version: u32,
        body: String,
    ) -> Result<u64, StoragePreferencesError> {
        match self
            .client
            .preferences(StoragePreferencesEnumRequest::Save {
                expected,
                schema_version,
                body,
            })
            .await?
        {
            StoragePreferencesEnumResponse::Saved(revision) => Ok(revision),
            StoragePreferencesEnumResponse::Loaded(_) => Err(StoragePreferencesError::Stale {
                expected,
                stored: 0,
            }),
        }
    }
}
