use diesel::SqliteConnection;
use diesel::prelude::*;

use crate::game::library_organization::GameLibraryOrganizationFolderId;
use crate::storage::library_query::StorageLibraryQuery;
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
        connection: &mut SqliteConnection,
        action: StorageTrashPreviewRequest,
    ) -> Result<StorageTrashPlan, StorageLibraryError> {
        let catalog = StorageLibraryTransaction::diesel_catalog(connection)?;
        let (content, organization, folders, games) = match action {
            StorageTrashPreviewRequest::RemoveFolder(id) => {
                let folder = StorageLibraryTransactionOrganization::folder(connection, id)?;
                let subtree = StorageLibraryQuery::folder_subtree(connection, id.to_string())?;
                let folders = i64::try_from(subtree.len())?;
                let games: i64 = library_entries::table
                    .filter(library_entries::folder_id.eq_any(subtree))
                    .count()
                    .get_result(connection)?;
                (
                    None,
                    folder.revision(),
                    u64::try_from(folders)?,
                    u64::try_from(games)?,
                )
            }
            StorageTrashPreviewRequest::MoveToTrash(id)
            | StorageTrashPreviewRequest::PermanentlyDelete(id) => {
                let game = StorageLibraryTransactionRows::state(connection, id)?
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
        connection: &mut SqliteConnection,
        request: &StorageTrashConfirmedRequest,
    ) -> Result<StorageTrashReceipt, StorageLibraryError> {
        let plan = request.plan();
        let current = Self::preview(connection, plan.action())?;
        if &current != plan {
            return Err(StorageLibraryError::StalePlan);
        }
        let action = match plan.action() {
            StorageTrashPreviewRequest::RemoveFolder(id) => {
                let affected = StorageLibraryTransactionOrganization::touch(
                    connection,
                    StorageLibraryTransactionOrganizationEnumScope::FolderSubtree(id),
                    request.timestamp(),
                )?;
                if affected != plan.affected_game_count() {
                    return Err(StorageLibraryError::StalePlan);
                }
                let subtree = StorageLibraryQuery::folder_subtree(connection, id.to_string())?;
                diesel::update(
                    library_entries::table.filter(library_entries::folder_id.eq_any(subtree)),
                )
                .set(library_entries::folder_id.eq(None::<String>))
                .execute(connection)?;
                Self::remove_folders(connection, id, plan.affected_folder_count())?;
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
                    connection,
                    StorageLibraryTransactionOrganizationEnumScope::Game(id),
                    request.timestamp(),
                )?;
                diesel::update(
                    library_entries::table
                        .filter(library_entries::game_id.eq(id.as_uuid().to_string())),
                )
                .set((
                    library_entries::location.eq(StorageTrashEnumLocation::Trash.sql()),
                    library_entries::trashed_ms.eq(Some(request.timestamp().value())),
                ))
                .execute(connection)?;
                StorageTrashEnumAppliedAction::Trashed(id)
            }
            StorageTrashPreviewRequest::PermanentlyDelete(id) => {
                let closed = request
                    .closed_game()
                    .ok_or(StorageLibraryError::LifecycleRequired)?;
                if closed.id() != id || Some(closed.content_revision()) != plan.content_revision() {
                    return Err(StorageLibraryError::LifecycleRequired);
                }
                diesel::delete(documents::table.filter(documents::id.eq(id.as_uuid().to_string())))
                    .execute(connection)?;
                StorageTrashEnumAppliedAction::Deleted(id)
            }
        };
        Ok(StorageTrashReceipt::new(
            action,
            StorageLibraryTransaction::diesel_bump_catalog(connection)?,
            plan.affected_game_count(),
        ))
    }
    pub(super) fn restore(
        connection: &mut SqliteConnection,
        request: &StorageTrashRestoreRequest,
    ) -> Result<StorageTrashReceipt, StorageLibraryError> {
        let target = request.target();
        let game = StorageLibraryTransactionRows::state(connection, target.id())?
            .ok_or(StorageLibraryError::MissingGame(target.id()))?;
        game.organization().revision().check(target.expected())?;
        if game.location() != StorageTrashEnumLocation::Trash {
            return Err(StorageLibraryError::InLibrary(target.id()));
        }
        StorageLibraryTransactionOrganization::touch(
            connection,
            StorageLibraryTransactionOrganizationEnumScope::Game(target.id()),
            request.timestamp(),
        )?;
        diesel::update(
            library_entries::table
                .filter(library_entries::game_id.eq(target.id().as_uuid().to_string())),
        )
        .set((
            library_entries::location.eq(StorageTrashEnumLocation::Library.sql()),
            library_entries::trashed_ms.eq(None::<i64>),
        ))
        .execute(connection)?;
        Ok(StorageTrashReceipt::new(
            StorageTrashEnumAppliedAction::Restored(target.id()),
            StorageLibraryTransaction::diesel_bump_catalog(connection)?,
            1,
        ))
    }
    fn remove_folders(
        connection: &mut SqliteConnection,
        root: GameLibraryOrganizationFolderId,
        expected: u64,
    ) -> Result<(), StorageLibraryError> {
        let subtree = StorageLibraryQuery::folder_subtree(connection, root.to_string())?;
        let mut removed = 0_u64;
        loop {
            let parents: Vec<String> = library_folders::table
                .filter(library_folders::folder_id.eq_any(&subtree))
                .filter(library_folders::parent_id.is_not_null())
                .select(library_folders::parent_id.assume_not_null())
                .load(connection)?;
            let leaves: Vec<String> = library_folders::table
                .filter(library_folders::folder_id.eq_any(&subtree))
                .filter(library_folders::folder_id.ne_all(&parents))
                .select(library_folders::folder_id)
                .order(library_folders::folder_id.asc())
                .limit(i64::try_from(StorageLibraryQuery::PAGE_SIZE)?)
                .load(connection)?;
            if leaves.is_empty() {
                break;
            }
            for leaf in leaves {
                let leaf: GameLibraryOrganizationFolderId = leaf.parse()?;
                let has_child: bool = diesel::select(diesel::dsl::exists(
                    library_folders::table.filter(library_folders::parent_id.eq(leaf.to_string())),
                ))
                .get_result(connection)?;
                if has_child {
                    return Err(StorageLibraryError::StalePlan);
                }
                let count = diesel::delete(
                    library_folders::table.filter(library_folders::folder_id.eq(leaf.to_string())),
                )
                .execute(connection)?;
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
