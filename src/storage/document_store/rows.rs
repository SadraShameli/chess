use diesel::prelude::*;

use crate::storage::schema::documents;

#[derive(Queryable, Selectable)]
#[diesel(table_name = documents, check_for_backend(diesel::sqlite::Sqlite))]
pub(super) struct StorageDocumentRowContent {
    pub(super) revision: String,
    pub(super) body: String,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = documents, check_for_backend(diesel::sqlite::Sqlite))]
pub(super) struct StorageDocumentRowReceipt {
    pub(super) id: String,
    pub(super) revision: String,
}
