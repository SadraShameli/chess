use std::path::Path;

use diesel::connection::SimpleConnection;
use diesel::sql_types::{BigInt, Nullable, Text};
use diesel::{Connection, QueryableByName, RunQueryDsl, SqliteConnection};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum StorageSqliteInspectorError {
    #[error("inspector connection failed: {0}")]
    Connection(#[from] diesel::ConnectionError),
    #[error("inspector query failed: {0}")]
    Query(#[from] diesel::result::Error),
}

#[derive(QueryableByName)]
struct StorageSqliteInspectorText {
    #[diesel(sql_type = Nullable<Text>)]
    value: Option<String>,
}

#[derive(QueryableByName)]
struct StorageSqliteInspectorInteger {
    #[diesel(sql_type = Nullable<BigInt>)]
    value: Option<i64>,
}

pub(crate) struct StorageSqliteInspector {
    connection: SqliteConnection,
}

impl StorageSqliteInspector {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, StorageSqliteInspectorError> {
        Ok(Self {
            connection: SqliteConnection::establish(&path.as_ref().to_string_lossy())?,
        })
    }

    pub(crate) fn batch(&mut self, sql: &str) -> Result<(), StorageSqliteInspectorError> {
        Ok(self.connection.batch_execute(sql)?)
    }

    pub(crate) fn execute(&mut self, sql: &str) -> Result<usize, StorageSqliteInspectorError> {
        Ok(diesel::sql_query(sql).execute(&mut self.connection)?)
    }

    pub(crate) fn optional_text(
        &mut self,
        sql: &str,
    ) -> Result<Option<String>, StorageSqliteInspectorError> {
        let rows =
            diesel::sql_query(sql).load::<StorageSqliteInspectorText>(&mut self.connection)?;
        Ok(rows.into_iter().next().and_then(|row| row.value))
    }

    pub(crate) fn texts(&mut self, sql: &str) -> Result<Vec<String>, StorageSqliteInspectorError> {
        let rows =
            diesel::sql_query(sql).load::<StorageSqliteInspectorText>(&mut self.connection)?;
        Ok(rows.into_iter().filter_map(|row| row.value).collect())
    }

    pub(crate) fn text(&mut self, sql: &str) -> Result<String, StorageSqliteInspectorError> {
        self.optional_text(sql)?
            .ok_or(diesel::result::Error::NotFound)
            .map_err(Into::into)
    }

    pub(crate) fn optional_integer(
        &mut self,
        sql: &str,
    ) -> Result<Option<i64>, StorageSqliteInspectorError> {
        let rows =
            diesel::sql_query(sql).load::<StorageSqliteInspectorInteger>(&mut self.connection)?;
        Ok(rows.into_iter().next().and_then(|row| row.value))
    }

    pub(crate) fn integer(&mut self, sql: &str) -> Result<i64, StorageSqliteInspectorError> {
        self.optional_integer(sql)?
            .ok_or(diesel::result::Error::NotFound)
            .map_err(Into::into)
    }
}
