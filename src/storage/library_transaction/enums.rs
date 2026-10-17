use diesel::dsl::sql;
use diesel::expression::SqlLiteral;
use diesel::sql_types::{Bool, Nullable, Text};
use diesel::sqlite::SqliteConnection;
use diesel::{Column, Connection};

use crate::game::library_organization::GameLibraryOrganizationName;
use crate::storage::schema::{library_folders, library_tags};

#[derive(Clone, Copy)]
pub(super) enum StorageLibraryTransactionEnumAccess {
    Read,
    Write,
}

#[derive(Clone, Copy)]
pub(super) enum StorageLibraryTransactionMetadataEnumText {
    FolderId,
    ParentId,
    TagId,
    Revision,
    Name,
}

impl StorageLibraryTransactionMetadataEnumText {
    fn column(self) -> &'static str {
        match self {
            Self::FolderId => library_folders::folder_id::NAME,
            Self::ParentId => library_folders::parent_id::NAME,
            Self::TagId => library_tags::tag_id::NAME,
            Self::Revision => library_folders::revision::NAME,
            Self::Name => library_folders::name::NAME,
        }
    }

    pub(super) fn bounded(self) -> SqlLiteral<Nullable<Text>> {
        let column = self.column();
        sql(&format!(
            "CASE WHEN length(CAST(\"{column}\" AS BLOB)) <= {} THEN \"{column}\" END",
            GameLibraryOrganizationName::MAX_BYTES
        ))
    }

    pub(super) fn is_bounded(self) -> SqlLiteral<Bool> {
        let column = self.column();
        sql(&format!(
            "\"{column}\" IS NULL OR length(CAST(\"{column}\" AS BLOB)) <= {}",
            GameLibraryOrganizationName::MAX_BYTES
        ))
    }
}

impl StorageLibraryTransactionEnumAccess {
    pub(super) fn run<T>(
        self,
        connection: &mut SqliteConnection,
        operation: impl FnOnce(&mut SqliteConnection) -> diesel::QueryResult<T>,
    ) -> diesel::QueryResult<T> {
        match self {
            Self::Read => connection.transaction(operation),
            Self::Write => connection.immediate_transaction(operation),
        }
    }
}
