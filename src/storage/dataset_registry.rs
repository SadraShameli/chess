use diesel::SqliteConnection;
use diesel::prelude::*;
use thiserror::Error;

use crate::package::digest::PackageDigest;
use crate::storage::dataset_package::{
    StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
};
use crate::storage::document_store::{StorageDocumentClient, StorageDocumentError};
use crate::storage::schema::{dataset_leases, datasets};

#[derive(Debug, Error)]
pub enum StorageDatasetRegistryError {
    #[error(transparent)]
    Diesel(#[from] diesel::result::Error),
    #[error("A dataset with this kind, name and version is already installed")]
    Duplicate,
    #[error("The dataset {0:?} is not installed")]
    Unknown(StorageDatasetId),
    #[error(
        "The dataset is referenced by {leases} active runs, queries, reports or history entries"
    )]
    InUse { leases: usize },
    #[error("A replacement must keep the kind and name and use a different version")]
    ReplacementMismatch,
    #[error("The saved dataset registry rows are inconsistent")]
    Corrupt,
    #[error(transparent)]
    Document(#[from] StorageDocumentError),
    #[error("The dataset registry reply channel closed: {0}")]
    Channel(#[from] tokio::sync::oneshot::error::RecvError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StorageDatasetId(i32);

impl StorageDatasetId {
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StorageDatasetLeaseId(i32);

impl StorageDatasetLeaseId {
    pub const fn value(self) -> i32 {
        self.0
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::EnumString,
    strum::AsRefStr,
    strum::Display,
    strum::EnumIter,
    diesel::AsExpression,
    diesel::FromSqlRow,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
pub enum StorageDatasetRegistryEnumKind {
    #[strum(serialize = "corpus")]
    Corpus,
    #[strum(serialize = "book")]
    Book,
    #[strum(serialize = "puzzles")]
    Puzzles,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::AsRefStr,
    strum::Display,
    strum::EnumIter,
    diesel::AsExpression,
    diesel::FromSqlRow,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
pub enum StorageDatasetRegistryEnumOrigin {
    #[strum(serialize = "starter")]
    Starter,
    #[strum(serialize = "personal")]
    Personal,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::AsRefStr,
    strum::Display,
    strum::EnumIter,
    diesel::AsExpression,
    diesel::FromSqlRow,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
pub enum StorageDatasetRegistryEnumHolder {
    #[strum(serialize = "run")]
    Run,
    #[strum(serialize = "query")]
    Query,
    #[strum(serialize = "report")]
    Report,
    #[strum(serialize = "history")]
    History,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetRegistryNew {
    pub kind: StorageDatasetRegistryEnumKind,
    pub origin: StorageDatasetRegistryEnumOrigin,
    pub name: String,
    pub version: String,
    pub format: StorageDatasetPackageEnumFormat,
    pub license: String,
    pub checksum: PackageDigest,
    pub location: String,
    pub validation: StorageDatasetPackageEnumValidation,
    pub records: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetRegistryEntry {
    pub id: StorageDatasetId,
    pub kind: StorageDatasetRegistryEnumKind,
    pub origin: StorageDatasetRegistryEnumOrigin,
    pub name: String,
    pub version: String,
    pub format: StorageDatasetPackageEnumFormat,
    pub license: String,
    pub checksum: PackageDigest,
    pub location: String,
    pub validation: StorageDatasetPackageEnumValidation,
    pub records: Option<u32>,
    pub superseded: bool,
}

type StorageDatasetRegistryRow = (
    i32,
    StorageDatasetRegistryEnumKind,
    StorageDatasetRegistryEnumOrigin,
    String,
    String,
    StorageDatasetPackageEnumFormat,
    String,
    String,
    String,
    StorageDatasetPackageEnumValidation,
    Option<i32>,
    i32,
);

pub struct StorageDatasetRegistry;

impl StorageDatasetRegistry {
    fn entry(
        row: StorageDatasetRegistryRow,
    ) -> Result<StorageDatasetRegistryEntry, StorageDatasetRegistryError> {
        let (
            id,
            kind,
            origin,
            name,
            version,
            format,
            license,
            checksum,
            location,
            validation,
            records,
            superseded,
        ) = row;
        Ok(StorageDatasetRegistryEntry {
            id: StorageDatasetId(id),
            kind,
            origin,
            name,
            version,
            format,
            license,
            checksum: checksum
                .parse()
                .map_err(|_error| StorageDatasetRegistryError::Corrupt)?,
            location,
            validation,
            records: records
                .map(|count| {
                    u32::try_from(count).map_err(|_error| StorageDatasetRegistryError::Corrupt)
                })
                .transpose()?,
            superseded: superseded != 0,
        })
    }

    pub fn register(
        connection: &mut SqliteConnection,
        dataset: &StorageDatasetRegistryNew,
    ) -> Result<StorageDatasetId, StorageDatasetRegistryError> {
        connection.immediate_transaction(|connection| Self::insert(connection, dataset))
    }

    fn insert(
        connection: &mut SqliteConnection,
        dataset: &StorageDatasetRegistryNew,
    ) -> Result<StorageDatasetId, StorageDatasetRegistryError> {
        let records = dataset
            .records
            .map(|count| {
                i32::try_from(count).map_err(|_error| StorageDatasetRegistryError::Corrupt)
            })
            .transpose()?;
        let inserted = diesel::insert_into(datasets::table)
            .values((
                datasets::kind.eq(dataset.kind),
                datasets::origin.eq(dataset.origin),
                datasets::name.eq(&dataset.name),
                datasets::version.eq(&dataset.version),
                datasets::format.eq(dataset.format),
                datasets::license.eq(&dataset.license),
                datasets::checksum.eq(dataset.checksum.to_string()),
                datasets::location.eq(&dataset.location),
                datasets::validation.eq(dataset.validation),
                datasets::records.eq(records),
            ))
            .execute(connection);
        match inserted {
            Ok(_) => {}
            Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            )) => return Err(StorageDatasetRegistryError::Duplicate),
            Err(error) => return Err(error.into()),
        }
        let id = datasets::table
            .filter(datasets::kind.eq(dataset.kind))
            .filter(datasets::name.eq(&dataset.name))
            .filter(datasets::version.eq(&dataset.version))
            .select(datasets::dataset_id)
            .first::<i32>(connection)?;
        Ok(StorageDatasetId(id))
    }

    pub fn list(
        connection: &mut SqliteConnection,
    ) -> Result<Vec<StorageDatasetRegistryEntry>, StorageDatasetRegistryError> {
        datasets::table
            .order((
                datasets::kind.asc(),
                datasets::name.asc(),
                datasets::version.asc(),
            ))
            .select((
                datasets::dataset_id,
                datasets::kind,
                datasets::origin,
                datasets::name,
                datasets::version,
                datasets::format,
                datasets::license,
                datasets::checksum,
                datasets::location,
                datasets::validation,
                datasets::records,
                datasets::superseded,
            ))
            .load::<StorageDatasetRegistryRow>(connection)?
            .into_iter()
            .map(Self::entry)
            .collect()
    }

    pub fn lease_count(
        connection: &mut SqliteConnection,
        dataset: StorageDatasetId,
    ) -> Result<usize, StorageDatasetRegistryError> {
        let count = dataset_leases::table
            .filter(dataset_leases::dataset_id.eq(dataset.0))
            .count()
            .get_result::<i64>(connection)?;
        usize::try_from(count).map_err(|_error| StorageDatasetRegistryError::Corrupt)
    }

    fn require(
        connection: &mut SqliteConnection,
        dataset: StorageDatasetId,
    ) -> Result<(), StorageDatasetRegistryError> {
        let exists = diesel::select(diesel::dsl::exists(
            datasets::table.filter(datasets::dataset_id.eq(dataset.0)),
        ))
        .get_result::<bool>(connection)?;
        if exists {
            Ok(())
        } else {
            Err(StorageDatasetRegistryError::Unknown(dataset))
        }
    }

    pub fn lease(
        connection: &mut SqliteConnection,
        dataset: StorageDatasetId,
        holder: StorageDatasetRegistryEnumHolder,
        reference: &str,
    ) -> Result<StorageDatasetLeaseId, StorageDatasetRegistryError> {
        connection.immediate_transaction(|connection| {
            Self::insert_lease(connection, dataset, holder, reference)
        })
    }

    fn insert_lease(
        connection: &mut SqliteConnection,
        dataset: StorageDatasetId,
        holder: StorageDatasetRegistryEnumHolder,
        reference: &str,
    ) -> Result<StorageDatasetLeaseId, StorageDatasetRegistryError> {
        Self::require(connection, dataset)?;
        diesel::insert_into(dataset_leases::table)
            .values((
                dataset_leases::dataset_id.eq(dataset.0),
                dataset_leases::holder.eq(holder),
                dataset_leases::holder_ref.eq(reference),
            ))
            .execute(connection)?;
        let id = dataset_leases::table
            .select(diesel::dsl::max(dataset_leases::lease_id))
            .first::<Option<i32>>(connection)?
            .ok_or(StorageDatasetRegistryError::Corrupt)?;
        Ok(StorageDatasetLeaseId(id))
    }

    pub fn release(
        connection: &mut SqliteConnection,
        lease: StorageDatasetLeaseId,
    ) -> Result<(), StorageDatasetRegistryError> {
        diesel::delete(dataset_leases::table.filter(dataset_leases::lease_id.eq(lease.0)))
            .execute(connection)?;
        Ok(())
    }

    pub fn remove(
        connection: &mut SqliteConnection,
        dataset: StorageDatasetId,
    ) -> Result<(), StorageDatasetRegistryError> {
        connection.immediate_transaction(|connection| {
            Self::require(connection, dataset)?;
            let leases = Self::lease_count(connection, dataset)?;
            if leases > 0 {
                return Err(StorageDatasetRegistryError::InUse { leases });
            }
            diesel::delete(datasets::table.filter(datasets::dataset_id.eq(dataset.0)))
                .execute(connection)?;
            Ok(())
        })
    }

    pub fn replace(
        connection: &mut SqliteConnection,
        old: StorageDatasetId,
        replacement: &StorageDatasetRegistryNew,
    ) -> Result<StorageDatasetId, StorageDatasetRegistryError> {
        connection.immediate_transaction(|connection| {
            let (kind, name, version) = datasets::table
                .filter(datasets::dataset_id.eq(old.0))
                .select((datasets::kind, datasets::name, datasets::version))
                .first::<(StorageDatasetRegistryEnumKind, String, String)>(connection)
                .optional()?
                .ok_or(StorageDatasetRegistryError::Unknown(old))?;
            if kind != replacement.kind
                || name != replacement.name
                || version == replacement.version
            {
                return Err(StorageDatasetRegistryError::ReplacementMismatch);
            }
            let created = Self::insert(connection, replacement)?;
            if Self::lease_count(connection, old)? == 0 {
                diesel::delete(datasets::table.filter(datasets::dataset_id.eq(old.0)))
                    .execute(connection)?;
            } else {
                diesel::update(datasets::table.filter(datasets::dataset_id.eq(old.0)))
                    .set(datasets::superseded.eq(1))
                    .execute(connection)?;
            }
            Ok(created)
        })
    }

    pub fn purge_released(
        connection: &mut SqliteConnection,
    ) -> Result<usize, StorageDatasetRegistryError> {
        connection.immediate_transaction(|connection| {
            let candidates = datasets::table
                .filter(datasets::superseded.eq(1))
                .select(datasets::dataset_id)
                .load::<i32>(connection)?;
            let mut purged = 0;
            for id in candidates {
                if Self::lease_count(connection, StorageDatasetId(id))? == 0 {
                    diesel::delete(datasets::table.filter(datasets::dataset_id.eq(id)))
                        .execute(connection)?;
                    purged += 1;
                }
            }
            Ok(purged)
        })
    }
}

#[derive(Debug)]
pub(crate) enum StorageDatasetRegistryEnumRequest {
    List,
    LeaseActive {
        kind: StorageDatasetRegistryEnumKind,
        holder: StorageDatasetRegistryEnumHolder,
        reference: String,
    },
    Release(Vec<StorageDatasetLeaseId>),
}

#[derive(Debug)]
pub(crate) enum StorageDatasetRegistryEnumResponse {
    Entries(Vec<StorageDatasetRegistryEntry>),
    Leases(Vec<StorageDatasetLeaseId>),
    Released,
}

impl StorageDatasetRegistry {
    pub(crate) fn execute(
        connection: &mut SqliteConnection,
        request: StorageDatasetRegistryEnumRequest,
    ) -> Result<StorageDatasetRegistryEnumResponse, StorageDatasetRegistryError> {
        match request {
            StorageDatasetRegistryEnumRequest::List => Ok(
                StorageDatasetRegistryEnumResponse::Entries(Self::list(connection)?),
            ),
            StorageDatasetRegistryEnumRequest::LeaseActive {
                kind,
                holder,
                reference,
            } => connection.immediate_transaction(|connection| {
                let active = datasets::table
                    .filter(datasets::kind.eq(kind))
                    .filter(datasets::superseded.eq(0))
                    .select(datasets::dataset_id)
                    .load::<i32>(connection)?;
                let mut leases = Vec::with_capacity(active.len());
                for id in active {
                    leases.push(Self::insert_lease(
                        connection,
                        StorageDatasetId(id),
                        holder,
                        &reference,
                    )?);
                }
                Ok(StorageDatasetRegistryEnumResponse::Leases(leases))
            }),
            StorageDatasetRegistryEnumRequest::Release(leases) => {
                connection.immediate_transaction(|connection| {
                    for lease in leases {
                        Self::release(connection, lease)?;
                    }
                    Ok(StorageDatasetRegistryEnumResponse::Released)
                })
            }
        }
    }
}

#[derive(Clone)]
pub struct StorageDatasetRepository {
    client: StorageDocumentClient,
}

impl StorageDatasetRepository {
    pub const fn new(client: StorageDocumentClient) -> Self {
        Self { client }
    }

    pub async fn list(
        &self,
    ) -> Result<Vec<StorageDatasetRegistryEntry>, StorageDatasetRegistryError> {
        match self
            .client
            .dataset_registry(StorageDatasetRegistryEnumRequest::List)
            .await?
        {
            StorageDatasetRegistryEnumResponse::Entries(entries) => Ok(entries),
            StorageDatasetRegistryEnumResponse::Leases(_)
            | StorageDatasetRegistryEnumResponse::Released => {
                Err(StorageDatasetRegistryError::Corrupt)
            }
        }
    }

    pub async fn lease_active(
        &self,
        kind: StorageDatasetRegistryEnumKind,
        holder: StorageDatasetRegistryEnumHolder,
        reference: String,
    ) -> Result<Vec<StorageDatasetLeaseId>, StorageDatasetRegistryError> {
        match self
            .client
            .dataset_registry(StorageDatasetRegistryEnumRequest::LeaseActive {
                kind,
                holder,
                reference,
            })
            .await?
        {
            StorageDatasetRegistryEnumResponse::Leases(leases) => Ok(leases),
            StorageDatasetRegistryEnumResponse::Entries(_)
            | StorageDatasetRegistryEnumResponse::Released => {
                Err(StorageDatasetRegistryError::Corrupt)
            }
        }
    }

    pub async fn release(
        &self,
        leases: Vec<StorageDatasetLeaseId>,
    ) -> Result<(), StorageDatasetRegistryError> {
        match self
            .client
            .dataset_registry(StorageDatasetRegistryEnumRequest::Release(leases))
            .await?
        {
            StorageDatasetRegistryEnumResponse::Released => Ok(()),
            StorageDatasetRegistryEnumResponse::Entries(_)
            | StorageDatasetRegistryEnumResponse::Leases(_) => {
                Err(StorageDatasetRegistryError::Corrupt)
            }
        }
    }
}
