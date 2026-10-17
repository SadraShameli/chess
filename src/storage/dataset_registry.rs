use diesel::SqliteConnection;
use diesel::prelude::*;
use thiserror::Error;

use crate::package::digest::PackageDigest;
use crate::storage::dataset_package::{
    StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
};
use crate::storage::schema::{dataset_leases, datasets};

#[derive(Debug, Error)]
pub enum StorageDatasetRegistryError {
    #[error(transparent)]
    Diesel(#[from] diesel::result::Error),
    #[error("A dataset with this kind, name and version is already installed")]
    Duplicate,
    #[error("The dataset {0:?} is not installed")]
    Unknown(StorageDatasetId),
    #[error("The dataset is referenced by {leases} active runs, queries, reports or history entries")]
    InUse { leases: usize },
    #[error("A replacement must keep the kind and name and use a different version")]
    ReplacementMismatch,
    #[error("The saved dataset registry rows are inconsistent")]
    Corrupt,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StorageDatasetRegistryEnumKind {
    Corpus,
    Book,
    Puzzles,
}

impl StorageDatasetRegistryEnumKind {
    const fn text(self) -> &'static str {
        match self {
            Self::Corpus => "corpus",
            Self::Book => "book",
            Self::Puzzles => "puzzles",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageDatasetRegistryError> {
        match value {
            "corpus" => Ok(Self::Corpus),
            "book" => Ok(Self::Book),
            "puzzles" => Ok(Self::Puzzles),
            _ => Err(StorageDatasetRegistryError::Corrupt),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageDatasetRegistryEnumOrigin {
    Starter,
    Personal,
}

impl StorageDatasetRegistryEnumOrigin {
    const fn text(self) -> &'static str {
        match self {
            Self::Starter => "starter",
            Self::Personal => "personal",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageDatasetRegistryError> {
        match value {
            "starter" => Ok(Self::Starter),
            "personal" => Ok(Self::Personal),
            _ => Err(StorageDatasetRegistryError::Corrupt),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageDatasetRegistryEnumHolder {
    Run,
    Query,
    Report,
    History,
}

impl StorageDatasetRegistryEnumHolder {
    const fn text(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Query => "query",
            Self::Report => "report",
            Self::History => "history",
        }
    }
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
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<i32>,
    i32,
);

pub struct StorageDatasetRegistry;

impl StorageDatasetRegistry {
    const fn format_text(format: StorageDatasetPackageEnumFormat) -> &'static str {
        match format {
            StorageDatasetPackageEnumFormat::Pgn => "pgn",
            StorageDatasetPackageEnumFormat::Polyglot => "polyglot",
            StorageDatasetPackageEnumFormat::PuzzleCsv => "puzzle_csv",
            StorageDatasetPackageEnumFormat::PreparedPuzzleCatalogV1 => {
                "prepared_puzzle_catalog_v1"
            }
        }
    }

    fn parse_format(
        value: &str,
    ) -> Result<StorageDatasetPackageEnumFormat, StorageDatasetRegistryError> {
        match value {
            "pgn" => Ok(StorageDatasetPackageEnumFormat::Pgn),
            "polyglot" => Ok(StorageDatasetPackageEnumFormat::Polyglot),
            "puzzle_csv" => Ok(StorageDatasetPackageEnumFormat::PuzzleCsv),
            "prepared_puzzle_catalog_v1" => {
                Ok(StorageDatasetPackageEnumFormat::PreparedPuzzleCatalogV1)
            }
            _ => Err(StorageDatasetRegistryError::Corrupt),
        }
    }

    const fn validation_text(validation: StorageDatasetPackageEnumValidation) -> &'static str {
        match validation {
            StorageDatasetPackageEnumValidation::Unchecked => "unchecked",
            StorageDatasetPackageEnumValidation::IntegrityVerified => "integrity_verified",
        }
    }

    fn parse_validation(
        value: &str,
    ) -> Result<StorageDatasetPackageEnumValidation, StorageDatasetRegistryError> {
        match value {
            "unchecked" => Ok(StorageDatasetPackageEnumValidation::Unchecked),
            "integrity_verified" => Ok(StorageDatasetPackageEnumValidation::IntegrityVerified),
            _ => Err(StorageDatasetRegistryError::Corrupt),
        }
    }

    fn entry(row: StorageDatasetRegistryRow) -> Result<StorageDatasetRegistryEntry, StorageDatasetRegistryError> {
        let (id, kind, origin, name, version, format, license, checksum, location, validation, records, superseded) =
            row;
        Ok(StorageDatasetRegistryEntry {
            id: StorageDatasetId(id),
            kind: StorageDatasetRegistryEnumKind::parse(&kind)?,
            origin: StorageDatasetRegistryEnumOrigin::parse(&origin)?,
            name,
            version,
            format: Self::parse_format(&format)?,
            license,
            checksum: checksum
                .parse()
                .map_err(|_error| StorageDatasetRegistryError::Corrupt)?,
            location,
            validation: Self::parse_validation(&validation)?,
            records: records
                .map(|count| u32::try_from(count).map_err(|_error| StorageDatasetRegistryError::Corrupt))
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
            .map(|count| i32::try_from(count).map_err(|_error| StorageDatasetRegistryError::Corrupt))
            .transpose()?;
        let inserted = diesel::insert_into(datasets::table)
            .values((
                datasets::kind.eq(dataset.kind.text()),
                datasets::origin.eq(dataset.origin.text()),
                datasets::name.eq(&dataset.name),
                datasets::version.eq(&dataset.version),
                datasets::format.eq(Self::format_text(dataset.format)),
                datasets::license.eq(&dataset.license),
                datasets::checksum.eq(dataset.checksum.to_string()),
                datasets::location.eq(&dataset.location),
                datasets::validation.eq(Self::validation_text(dataset.validation)),
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
            .filter(datasets::kind.eq(dataset.kind.text()))
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
            .order((datasets::kind.asc(), datasets::name.asc(), datasets::version.asc()))
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
            Self::require(connection, dataset)?;
            diesel::insert_into(dataset_leases::table)
                .values((
                    dataset_leases::dataset_id.eq(dataset.0),
                    dataset_leases::holder.eq(holder.text()),
                    dataset_leases::holder_ref.eq(reference),
                ))
                .execute(connection)?;
            let id = dataset_leases::table
                .select(diesel::dsl::max(dataset_leases::lease_id))
                .first::<Option<i32>>(connection)?
                .ok_or(StorageDatasetRegistryError::Corrupt)?;
            Ok(StorageDatasetLeaseId(id))
        })
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
                .first::<(String, String, String)>(connection)
                .optional()?
                .ok_or(StorageDatasetRegistryError::Unknown(old))?;
            if kind != replacement.kind.text()
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
