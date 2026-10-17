use diesel::sqlite::SqliteConnection;

use crate::storage::database::StorageDatabase;
use crate::storage::database::enums::StorageDatabaseEnumIntegerProperty;

use crate::storage::library_repository::StorageLibraryError;

pub(crate) struct StorageLibraryImportPageLimit {
    previous: u64,
}

impl StorageLibraryImportPageLimit {
    fn set(connection: &mut SqliteConnection, requested: u64) -> Result<(), StorageLibraryError> {
        let actual = StorageDatabase::set_max_page_count(connection, requested)?;
        if actual != requested {
            return Err(StorageLibraryError::ImportPageCeiling { requested, actual });
        }
        Ok(())
    }

    pub(crate) fn install(
        connection: &mut SqliteConnection,
        ceiling: u64,
        scope: &mut Option<Self>,
    ) -> Result<(), StorageLibraryError> {
        if scope.is_some() || StorageDatabase::is_resolved(connection) || ceiling == 0 {
            return Err(StorageLibraryError::ImportLayout);
        }
        let previous = u64::try_from(StorageDatabase::integer(
            connection,
            StorageDatabaseEnumIntegerProperty::MaxPageCount,
        )?)?;
        *scope = Some(Self { previous });
        Self::set(connection, previous.min(ceiling))
    }

    pub(crate) fn finish<T>(
        connection: &mut SqliteConnection,
        scope: Option<Self>,
        primary: Result<T, StorageLibraryError>,
    ) -> Result<T, StorageLibraryError> {
        let Some(scope) = scope else {
            return primary;
        };
        let restored = if StorageDatabase::is_resolved(connection) {
            Self::set(connection, scope.previous)
        } else {
            Err(StorageLibraryError::ImportScopeTransaction)
        };
        match (primary, restored) {
            (result, Ok(())) => result,
            (Err(primary), Err(restore)) => Err(StorageLibraryError::ImportScopeCleanup {
                primary: Box::new(primary),
                restore: Box::new(restore),
            }),
            (Ok(_), Err(restore)) => Err(StorageLibraryError::ImportScopeCommittedCleanup {
                restore: Box::new(restore),
            }),
        }
    }
}
