use diesel::dsl::{AsExprOf, sql};
use diesel::expression::{SqlLiteral, UncheckedBind};
use diesel::sql_types::{Bool, Text};

use crate::game::library_organization::GameLibraryOrganizationFolderId;

type StorageLibraryQueryFoldersPredicate =
    SqlLiteral<Bool, UncheckedBind<SqlLiteral<Bool>, AsExprOf<String, Text>>>;

pub(crate) struct StorageLibraryQueryFolders;

impl StorageLibraryQueryFolders {
    pub(crate) const PREFIX: &'static str =
        "WITH RECURSIVE folders(id) AS (SELECT folder_id FROM library_folders WHERE folder_id = ";
    pub(crate) const SUFFIX: &'static str =
        " UNION SELECT f.folder_id FROM library_folders f JOIN folders p ON f.parent_id = p.id) ";

    pub(crate) fn predicate(
        id: GameLibraryOrganizationFolderId,
    ) -> StorageLibraryQueryFoldersPredicate {
        sql::<Bool>(&format!(
            "\"library_entries\".\"folder_id\" IN ({}",
            Self::PREFIX
        ))
        .bind::<Text, _>(id.to_string())
        .sql(&format!("{}SELECT id FROM folders)", Self::SUFFIX))
    }
}
