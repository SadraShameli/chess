use rusqlite::Connection;

use crate::storage::library_repository::StorageLibraryError;

pub(crate) struct StorageLibraryImportPageLimit {
    previous: u64,
}

impl StorageLibraryImportPageLimit {
    fn set(connection: &Connection, requested: u64) -> Result<(), StorageLibraryError> {
        let actual = u64::try_from(connection.query_row(
            &format!("PRAGMA main.max_page_count = {requested}"),
            [],
            |row| row.get::<_, i64>(0),
        )?)?;
        if actual != requested {
            return Err(StorageLibraryError::ImportPageCeiling { requested, actual });
        }
        Ok(())
    }

    pub(crate) fn install(
        connection: &Connection,
        ceiling: u64,
        scope: &mut Option<Self>,
    ) -> Result<(), StorageLibraryError> {
        if scope.is_some() || connection.is_autocommit() || ceiling == 0 {
            return Err(StorageLibraryError::ImportLayout);
        }
        let previous = u64::try_from(connection.pragma_query_value(
            None,
            "max_page_count",
            |row| row.get::<_, i64>(0),
        )?)?;
        *scope = Some(Self { previous });
        Self::set(connection, previous.min(ceiling))
    }

    pub(crate) fn finish<T>(
        connection: &Connection,
        scope: Option<Self>,
        primary: Result<T, StorageLibraryError>,
    ) -> Result<T, StorageLibraryError> {
        let Some(scope) = scope else {
            return primary;
        };
        let restored = if connection.is_autocommit() {
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
