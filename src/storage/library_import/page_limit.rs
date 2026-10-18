use diesel::SqliteConnection;
use diesel::connection::{AnsiTransactionManager, TransactionManager};
use diesel::prelude::*;
use diesel::sql_types::BigInt;

use crate::storage::library_repository::StorageLibraryError;

#[derive(QueryableByName)]
struct StorageLibraryImportPageCount {
    #[diesel(sql_type = BigInt)]
    max_page_count: i64,
}

pub(crate) struct StorageLibraryImportPageLimit {
    previous: u64,
}

impl StorageLibraryImportPageLimit {
    pub(crate) fn is_autocommit(
        connection: &mut SqliteConnection,
    ) -> Result<bool, StorageLibraryError> {
        Ok(
            AnsiTransactionManager::transaction_manager_status_mut(connection)
                .transaction_depth()?
                .is_none(),
        )
    }

    fn set(connection: &mut SqliteConnection, requested: u64) -> Result<(), StorageLibraryError> {
        let row: StorageLibraryImportPageCount =
            diesel::sql_query(format!("PRAGMA main.max_page_count = {requested}"))
                .get_result(connection)?;
        let actual = u64::try_from(row.max_page_count)?;
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
        if scope.is_some() || Self::is_autocommit(connection)? || ceiling == 0 {
            return Err(StorageLibraryError::ImportLayout);
        }
        let row: StorageLibraryImportPageCount =
            diesel::sql_query("SELECT max_page_count FROM pragma_max_page_count")
                .get_result(connection)?;
        let previous = u64::try_from(row.max_page_count)?;
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
        let restored = if Self::is_autocommit(connection)? {
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
