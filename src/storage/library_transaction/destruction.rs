use diesel::dsl::{exists, not, sql};
use diesel::sql_types::{BigInt, Text};
use diesel::sqlite::SqliteConnection;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};

use crate::game::library_organization::GameLibraryOrganizationFolderId;
use crate::storage::library_query::StorageLibraryQuery;
use crate::storage::library_query::folders::StorageLibraryQueryFolders;
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::library_transaction::StorageLibraryTransaction;
use crate::storage::library_transaction::organization::{
    StorageLibraryTransactionOrganization, StorageLibraryTransactionOrganizationEnumScope,
};
use crate::storage::library_transaction::rows::StorageLibraryTransactionRows;
use crate::storage::schema::{documents, library_entries, library_folders};
use crate::storage::trash::{
    StorageTrashConfirmedRequest, StorageTrashEnumAppliedAction, StorageTrashEnumLocation,
    StorageTrashPlan, StorageTrashPlanOptions, StorageTrashPreviewRequest, StorageTrashReceipt,
    StorageTrashRestoreRequest,
};

pub(super) struct StorageLibraryTransactionDestruction;

impl StorageLibraryTransactionDestruction {
    pub(super) fn preview(
        transaction: &mut SqliteConnection,
        action: StorageTrashPreviewRequest,
    ) -> Result<StorageTrashPlan, StorageLibraryError> {
        let catalog = StorageLibraryTransaction::catalog(transaction)?;
        let (content, organization, folders, games) = match action {
            StorageTrashPreviewRequest::RemoveFolder(id) => {
                let folder = StorageLibraryTransactionOrganization::folder(transaction, id)?;
                let (folders, games): (i64, i64) = sql::<(BigInt, BigInt)>(StorageLibraryQueryFolders::PREFIX)
                    .bind::<Text, _>(id.to_string())
                    .sql(StorageLibraryQueryFolders::SUFFIX)
                    .sql("SELECT (SELECT COUNT(*) FROM folders), (SELECT COUNT(*) FROM library_entries WHERE folder_id IN (SELECT id FROM folders))")
                    .get_result(transaction)?;
                (
                    None,
                    folder.revision(),
                    u64::try_from(folders)?,
                    u64::try_from(games)?,
                )
            }
            StorageTrashPreviewRequest::MoveToTrash(id)
            | StorageTrashPreviewRequest::PermanentlyDelete(id) => {
                let game = StorageLibraryTransactionRows::state(transaction, id)?
                    .ok_or(StorageLibraryError::MissingGame(id))?;
                match (action, game.location()) {
                    (
                        StorageTrashPreviewRequest::MoveToTrash(_),
                        StorageTrashEnumLocation::Library,
                    )
                    | (
                        StorageTrashPreviewRequest::PermanentlyDelete(_),
                        StorageTrashEnumLocation::Trash,
                    ) => {}
                    (
                        StorageTrashPreviewRequest::MoveToTrash(_),
                        StorageTrashEnumLocation::Trash,
                    ) => return Err(StorageLibraryError::InTrash(id)),
                    (
                        StorageTrashPreviewRequest::PermanentlyDelete(_),
                        StorageTrashEnumLocation::Library,
                    ) => return Err(StorageLibraryError::InLibrary(id)),
                    (StorageTrashPreviewRequest::RemoveFolder(_), _) => {
                        return Err(StorageLibraryError::Protocol);
                    }
                }
                (Some(game.content()), game.organization().revision(), 0, 1)
            }
        };
        Ok(StorageTrashPlan::new(StorageTrashPlanOptions {
            action,
            catalog,
            content,
            organization,
            folders,
            games,
        }))
    }
    pub(super) fn apply(
        transaction: &mut SqliteConnection,
        request: &StorageTrashConfirmedRequest,
    ) -> Result<StorageTrashReceipt, StorageLibraryError> {
        let plan = request.plan();
        let current = Self::preview(transaction, plan.action())?;
        if &current != plan {
            return Err(StorageLibraryError::StalePlan);
        }
        let action = match plan.action() {
            StorageTrashPreviewRequest::RemoveFolder(id) => {
                let affected = StorageLibraryTransactionOrganization::touch(
                    transaction,
                    StorageLibraryTransactionOrganizationEnumScope::FolderSubtree(id),
                    request.timestamp(),
                )?;
                if affected != plan.affected_game_count() {
                    return Err(StorageLibraryError::StalePlan);
                }
                diesel::update(
                    library_entries::table.filter(StorageLibraryQueryFolders::predicate(id)),
                )
                .set(library_entries::folder_id.eq(None::<String>))
                .execute(transaction)?;
                Self::remove_folders(transaction, id, plan.affected_folder_count())?;
                StorageTrashEnumAppliedAction::RemovedFolder(id)
            }
            StorageTrashPreviewRequest::MoveToTrash(id) => {
                let closed = request
                    .closed_game()
                    .ok_or(StorageLibraryError::LifecycleRequired)?;
                if closed.id() != id || Some(closed.content_revision()) != plan.content_revision() {
                    return Err(StorageLibraryError::LifecycleRequired);
                }
                StorageLibraryTransactionOrganization::touch(
                    transaction,
                    StorageLibraryTransactionOrganizationEnumScope::Game(id),
                    request.timestamp(),
                )?;
                diesel::update(library_entries::table.find(id.as_uuid().to_string()))
                    .set((
                        library_entries::location.eq(StorageTrashEnumLocation::Trash.sql()),
                        library_entries::trashed_ms.eq(Some(request.timestamp().value())),
                    ))
                    .execute(transaction)?;
                StorageTrashEnumAppliedAction::Trashed(id)
            }
            StorageTrashPreviewRequest::PermanentlyDelete(id) => {
                let closed = request
                    .closed_game()
                    .ok_or(StorageLibraryError::LifecycleRequired)?;
                if closed.id() != id || Some(closed.content_revision()) != plan.content_revision() {
                    return Err(StorageLibraryError::LifecycleRequired);
                }
                diesel::delete(documents::table.find(id.as_uuid().to_string()))
                    .execute(transaction)?;
                StorageTrashEnumAppliedAction::Deleted(id)
            }
        };
        Ok(StorageTrashReceipt::new(
            action,
            StorageLibraryTransaction::bump_catalog(transaction)?,
            plan.affected_game_count(),
        ))
    }
    pub(super) fn restore(
        transaction: &mut SqliteConnection,
        request: &StorageTrashRestoreRequest,
    ) -> Result<StorageTrashReceipt, StorageLibraryError> {
        let target = request.target();
        let game = StorageLibraryTransactionRows::state(transaction, target.id())?
            .ok_or(StorageLibraryError::MissingGame(target.id()))?;
        game.organization().revision().check(target.expected())?;
        if game.location() != StorageTrashEnumLocation::Trash {
            return Err(StorageLibraryError::InLibrary(target.id()));
        }
        StorageLibraryTransactionOrganization::touch(
            transaction,
            StorageLibraryTransactionOrganizationEnumScope::Game(target.id()),
            request.timestamp(),
        )?;
        diesel::update(library_entries::table.find(target.id().as_uuid().to_string()))
            .set((
                library_entries::location.eq(StorageTrashEnumLocation::Library.sql()),
                library_entries::trashed_ms.eq(None::<i64>),
            ))
            .execute(transaction)?;
        Ok(StorageTrashReceipt::new(
            StorageTrashEnumAppliedAction::Restored(target.id()),
            StorageLibraryTransaction::bump_catalog(transaction)?,
            1,
        ))
    }
    fn remove_folders(
        transaction: &mut SqliteConnection,
        root: GameLibraryOrganizationFolderId,
        expected: u64,
    ) -> Result<(), StorageLibraryError> {
        let mut removed = 0_u64;
        loop {
            let leaves: Vec<String> = sql::<Text>(StorageLibraryQueryFolders::PREFIX)
                .bind::<Text, _>(root.to_string())
                .sql(StorageLibraryQueryFolders::SUFFIX)
                .sql("SELECT f.folder_id FROM library_folders f JOIN folders target ON target.id = f.folder_id WHERE NOT EXISTS(SELECT 1 FROM library_folders child WHERE child.parent_id = f.folder_id) ORDER BY f.folder_id LIMIT ")
                .bind::<BigInt, _>(i64::try_from(StorageLibraryQuery::PAGE_SIZE)?)
                .load(transaction)?;
            if leaves.is_empty() {
                break;
            }
            for leaf in leaves {
                let leaf: GameLibraryOrganizationFolderId = leaf.parse()?;
                let child = diesel::alias!(library_folders as child_folders);
                let key = leaf.to_string();
                let count = diesel::delete(library_folders::table.find(&key).filter(not(exists(
                    child.filter(child.field(library_folders::parent_id).eq(&key)),
                ))))
                .execute(transaction)?;
                if count != 1 {
                    return Err(StorageLibraryError::StalePlan);
                }
                removed = removed
                    .checked_add(1)
                    .ok_or(StorageLibraryError::StalePlan)?;
            }
        }
        if removed != expected {
            return Err(StorageLibraryError::StalePlan);
        }
        Ok(())
    }
}
