use diesel::backend::Backend;
use diesel::deserialize::{self, FromSql};
use diesel::serialize::{self, Output, ToSql};
use diesel::sql_types::Text;
use diesel::sqlite::Sqlite;
use thiserror::Error;

use crate::board::enums::BoardEnumVariant;
use crate::game::enums::GameEnumMode;
use crate::storage::dataset_package::{
    StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
};
use crate::storage::dataset_registry::{
    StorageDatasetRegistryEnumHolder, StorageDatasetRegistryEnumKind,
    StorageDatasetRegistryEnumOrigin,
};
use crate::storage::game_document::StorageGameDocumentEnumContentKind;
use crate::storage::library_prepared::{StorageLibraryEnumResult, StorageLibraryEnumStartKind};
use crate::storage::report_repository::{StorageReportEnumFreshness, StorageReportEnumState};
use crate::storage::trash::StorageTrashEnumLocation;

#[derive(Debug, Error)]
#[error("stored text is not a {column} spelling")]
pub(crate) struct StorageTextColumnError {
    column: &'static str,
}

macro_rules! storage_text_column {
    ($kind:ty) => {
        impl ToSql<Text, Sqlite> for $kind {
            fn to_sql<'output>(
                &'output self,
                out: &mut Output<'output, '_, Sqlite>,
            ) -> serialize::Result {
                <str as ToSql<Text, Sqlite>>::to_sql(self.as_ref(), out)
            }
        }

        impl FromSql<Text, Sqlite> for $kind {
            fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
                let text = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
                text.parse().map_err(|_error| {
                    StorageTextColumnError {
                        column: stringify!($kind),
                    }
                    .into()
                })
            }
        }
    };
}

storage_text_column!(StorageGameDocumentEnumContentKind);
storage_text_column!(BoardEnumVariant);
storage_text_column!(GameEnumMode);
storage_text_column!(StorageLibraryEnumStartKind);
storage_text_column!(StorageLibraryEnumResult);
storage_text_column!(StorageTrashEnumLocation);
storage_text_column!(StorageReportEnumState);
storage_text_column!(StorageReportEnumFreshness);
storage_text_column!(StorageDatasetPackageEnumFormat);
storage_text_column!(StorageDatasetPackageEnumValidation);
storage_text_column!(StorageDatasetRegistryEnumKind);
storage_text_column!(StorageDatasetRegistryEnumOrigin);
storage_text_column!(StorageDatasetRegistryEnumHolder);
