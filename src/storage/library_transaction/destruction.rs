use rusqlite::{Transaction, params};

use crate::game::library_organization::GameLibraryOrganizationFolderId;
use crate::storage::library_query::StorageLibraryQuery;
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::library_transaction::StorageLibraryTransaction;
use crate::storage::library_transaction::organization::{
    StorageLibraryTransactionOrganization, StorageLibraryTransactionOrganizationEnumScope,
};
use crate::storage::library_transaction::rows::StorageLibraryTransactionRows;
use crate::storage::trash::{
    StorageTrashConfirmedRequest, StorageTrashEnumAppliedAction, StorageTrashEnumLocation,
    StorageTrashPlan, StorageTrashPlanOptions, StorageTrashPreviewRequest, StorageTrashReceipt,
    StorageTrashRestoreRequest,
};

pub(super) struct StorageLibraryTransactionDestruction;

impl StorageLibraryTransactionDestruction {
    pub(super) fn preview(
        transaction: &Transaction<'_>,
        action: StorageTrashPreviewRequest,
    ) -> Result<StorageTrashPlan, StorageLibraryError> {
        let catalog = StorageLibraryTransaction::catalog(transaction)?;
        let (content, organization, folders, games) = match action {
            StorageTrashPreviewRequest::RemoveFolder(id) => {
                let folder = StorageLibraryTransactionOrganization::folder(transaction, id)?;
                let (folders, games): (i64, i64) = transaction.query_row(
                    "WITH RECURSIVE folders(id) AS (SELECT folder_id FROM library_folders WHERE folder_id = ?1 UNION SELECT f.folder_id FROM library_folders f JOIN folders p ON f.parent_id = p.id) SELECT (SELECT COUNT(*) FROM folders), (SELECT COUNT(*) FROM library_entries WHERE folder_id IN (SELECT id FROM folders))",
                    [id.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))?;
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
        transaction: &Transaction<'_>,
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
                transaction.execute("WITH RECURSIVE folders(id) AS (SELECT folder_id FROM library_folders WHERE folder_id = ?1 UNION SELECT f.folder_id FROM library_folders f JOIN folders p ON f.parent_id = p.id) UPDATE library_entries SET folder_id = NULL WHERE folder_id IN (SELECT id FROM folders)", [id.to_string()])?;
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
                transaction.execute(
                    "UPDATE library_entries SET location = ?1, trashed_ms = ?2 WHERE game_id = ?3",
                    params![
                        StorageTrashEnumLocation::Trash.sql(),
                        request.timestamp().value(),
                        id.as_uuid().to_string()
                    ],
                )?;
                StorageTrashEnumAppliedAction::Trashed(id)
            }
            StorageTrashPreviewRequest::PermanentlyDelete(id) => {
                let closed = request
                    .closed_game()
                    .ok_or(StorageLibraryError::LifecycleRequired)?;
                if closed.id() != id || Some(closed.content_revision()) != plan.content_revision() {
                    return Err(StorageLibraryError::LifecycleRequired);
                }
                transaction.execute(
                    "DELETE FROM documents WHERE id = ?1",
                    [id.as_uuid().to_string()],
                )?;
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
        transaction: &Transaction<'_>,
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
        transaction.execute(
            "UPDATE library_entries SET location = ?1, trashed_ms = NULL WHERE game_id = ?2",
            params![
                StorageTrashEnumLocation::Library.sql(),
                target.id().as_uuid().to_string()
            ],
        )?;
        Ok(StorageTrashReceipt::new(
            StorageTrashEnumAppliedAction::Restored(target.id()),
            StorageLibraryTransaction::bump_catalog(transaction)?,
            1,
        ))
    }
    fn remove_folders(
        transaction: &Transaction<'_>,
        root: GameLibraryOrganizationFolderId,
        expected: u64,
    ) -> Result<(), StorageLibraryError> {
        let mut removed = 0_u64;
        loop {
            let leaves: Vec<String> = {
                let mut statement = transaction.prepare("WITH RECURSIVE folders(id) AS (SELECT folder_id FROM library_folders WHERE folder_id = ?1 UNION SELECT f.folder_id FROM library_folders f JOIN folders p ON f.parent_id = p.id) SELECT f.folder_id FROM library_folders f JOIN folders target ON target.id = f.folder_id WHERE NOT EXISTS(SELECT 1 FROM library_folders child WHERE child.parent_id = f.folder_id) ORDER BY f.folder_id LIMIT ?2")?;
                statement
                    .query_map(
                        params![
                            root.to_string(),
                            i64::try_from(StorageLibraryQuery::PAGE_SIZE)?
                        ],
                        |row| row.get(0),
                    )?
                    .collect::<Result<_, _>>()?
            };
            if leaves.is_empty() {
                break;
            }
            for leaf in leaves {
                let leaf: GameLibraryOrganizationFolderId = leaf.parse()?;
                let count = transaction.execute("DELETE FROM library_folders WHERE folder_id = ?1 AND NOT EXISTS(SELECT 1 FROM library_folders child WHERE child.parent_id = ?1)", [leaf.to_string()])?;
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
