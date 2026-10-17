use rusqlite::{OptionalExtension, Transaction, params};

use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationEnumTarget,
    GameLibraryOrganizationError, GameLibraryOrganizationFolderId,
    GameLibraryOrganizationFolderTarget, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationReceipt, GameLibraryOrganizationRequest,
    GameLibraryOrganizationRevision, GameLibraryOrganizationTagId,
    GameLibraryOrganizationTagTarget, GameLibraryOrganizationTimestamp,
};
use crate::storage::library_query::StorageLibraryQuery;
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::library_transaction::StorageLibraryTransaction;
use crate::storage::library_transaction::rows::{
    StorageLibraryTransactionEntryState, StorageLibraryTransactionRows,
};

#[derive(Clone, Copy)]
pub(super) enum StorageLibraryTransactionOrganizationEnumScope {
    FolderSubtree(GameLibraryOrganizationFolderId),
    Tag(GameLibraryOrganizationTagId),
    Game(GameId),
}

impl StorageLibraryTransactionOrganizationEnumScope {
    fn predicate(self) -> &'static str {
        match self {
            Self::FolderSubtree(_) => {
                "e.folder_id IN (WITH RECURSIVE folders(id) AS (SELECT folder_id FROM library_folders WHERE folder_id = ?1 UNION SELECT f.folder_id FROM library_folders f JOIN folders p ON f.parent_id = p.id) SELECT id FROM folders)"
            }
            Self::Tag(_) => {
                "EXISTS(SELECT 1 FROM library_entry_tags t WHERE t.game_id = e.game_id AND t.tag_id = ?1)"
            }
            Self::Game(_) => "e.game_id = ?1",
        }
    }
    fn key(self) -> String {
        match self {
            Self::FolderSubtree(id) => id.to_string(),
            Self::Tag(id) => id.to_string(),
            Self::Game(id) => id.as_uuid().to_string(),
        }
    }
}

pub(super) struct StorageLibraryTransactionOrganizationFolder {
    revision: GameLibraryOrganizationRevision,
    name: String,
    parent: Option<GameLibraryOrganizationFolderId>,
}

impl StorageLibraryTransactionOrganizationFolder {
    pub(super) const fn revision(&self) -> GameLibraryOrganizationRevision {
        self.revision
    }
}

pub(super) struct StorageLibraryTransactionOrganization;

impl StorageLibraryTransactionOrganization {
    pub(super) fn folder(
        transaction: &Transaction<'_>,
        id: GameLibraryOrganizationFolderId,
    ) -> Result<StorageLibraryTransactionOrganizationFolder, StorageLibraryError> {
        let row: Option<(String, String, Option<String>)> = transaction
            .query_row(
                "SELECT revision, name, parent_id FROM library_folders WHERE folder_id = ?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (revision, name, parent) = row.ok_or(StorageLibraryError::MissingFolder(id))?;
        crate::game::library_organization::GameLibraryOrganizationName::new(name.clone())?;
        Ok(StorageLibraryTransactionOrganizationFolder {
            revision: revision.parse()?,
            name,
            parent: parent.as_deref().map(str::parse).transpose()?,
        })
    }
    fn folder_target(
        transaction: &Transaction<'_>,
        target: GameLibraryOrganizationFolderTarget,
    ) -> Result<StorageLibraryTransactionOrganizationFolder, StorageLibraryError> {
        let folder = Self::folder(transaction, target.id())?;
        folder.revision.check(target.expected())?;
        Ok(folder)
    }
    fn tag(
        transaction: &Transaction<'_>,
        target: GameLibraryOrganizationTagTarget,
    ) -> Result<(GameLibraryOrganizationRevision, String), StorageLibraryError> {
        let row: Option<(String, String)> = transaction
            .query_row(
                "SELECT revision, name FROM library_tags WHERE tag_id = ?1",
                [target.id().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (revision, name) = row.ok_or(StorageLibraryError::MissingTag(target.id()))?;
        let revision: GameLibraryOrganizationRevision = revision.parse()?;
        revision.check(target.expected())?;
        Ok((revision, name))
    }
    fn game(
        transaction: &Transaction<'_>,
        target: GameLibraryOrganizationGameTarget,
    ) -> Result<StorageLibraryTransactionEntryState, StorageLibraryError> {
        let game = StorageLibraryTransactionRows::state(transaction, target.id())?
            .ok_or(StorageLibraryError::MissingGame(target.id()))?;
        game.organization().revision().check(target.expected())?;
        Ok(game)
    }
    fn parent(
        transaction: &Transaction<'_>,
        target: Option<GameLibraryOrganizationFolderTarget>,
    ) -> Result<Option<GameLibraryOrganizationFolderId>, StorageLibraryError> {
        target
            .map(|target| {
                Self::folder_target(transaction, target)?;
                Ok(target.id())
            })
            .transpose()
    }
    pub(super) fn touch(
        transaction: &Transaction<'_>,
        scope: StorageLibraryTransactionOrganizationEnumScope,
        timestamp: GameLibraryOrganizationTimestamp,
    ) -> Result<u64, StorageLibraryError> {
        let sql = format!(
            "SELECT e.game_id, e.organization_revision FROM library_entries e WHERE {} AND (?2 IS NULL OR e.game_id > ?2) ORDER BY e.game_id LIMIT ?3",
            scope.predicate()
        );
        let mut after: Option<String> = None;
        let mut count = 0_u64;
        loop {
            let page: Vec<(String, String)> = {
                let mut statement = transaction.prepare(&sql)?;
                statement
                    .query_map(
                        params![
                            scope.key(),
                            after,
                            i64::try_from(StorageLibraryQuery::PAGE_SIZE)?
                        ],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )?
                    .collect::<Result<_, _>>()?
            };
            if page.is_empty() {
                break;
            }
            for (id, revision) in &page {
                let _validated_id: GameId = id.parse()?;
                let revision: GameLibraryOrganizationRevision = revision.parse()?;
                let next = revision.next()?;
                transaction.execute("UPDATE library_entries SET organization_revision = ?1, modified_ms = MAX(COALESCE(modified_ms, ?2), ?2) WHERE game_id = ?3", params![next.to_string(), timestamp.value(), id])?;
                count = count
                    .checked_add(1)
                    .ok_or(GameLibraryOrganizationError::Exhausted)?;
            }
            after = page.last().map(|(id, _revision)| id.clone());
        }
        Ok(count)
    }
    pub(super) fn execute(
        transaction: &Transaction<'_>,
        request: &GameLibraryOrganizationRequest,
    ) -> Result<GameLibraryOrganizationReceipt, StorageLibraryError> {
        let timestamp = request.timestamp();
        let receipt = match request.mutation() {
            GameLibraryOrganizationEnumMutation::CreateFolder(id, parent, name) => {
                let parent = Self::parent(transaction, *parent)?;
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM library_folders WHERE folder_id = ?1)",
                    [id.to_string()],
                    |row| row.get(0),
                )?;
                if exists {
                    return Err(StorageLibraryError::FolderExists);
                }
                transaction.execute("INSERT INTO library_folders(folder_id, parent_id, revision, name) VALUES (?1, ?2, '0', ?3)", params![id.to_string(), parent.map(|id| id.to_string()), name.as_str()])?;
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Folder(*id),
                    GameLibraryOrganizationRevision::ZERO,
                    0,
                    true,
                )
            }
            GameLibraryOrganizationEnumMutation::RenameFolder(target, name) => {
                let folder = Self::folder_target(transaction, *target)?;
                let changed = folder.name != name.as_str();
                let revision = if changed {
                    folder.revision.next()?
                } else {
                    folder.revision
                };
                let affected = if changed {
                    let count = Self::touch(
                        transaction,
                        StorageLibraryTransactionOrganizationEnumScope::FolderSubtree(target.id()),
                        timestamp,
                    )?;
                    transaction.execute(
                        "UPDATE library_folders SET name = ?1, revision = ?2 WHERE folder_id = ?3",
                        params![name.as_str(), revision.to_string(), target.id().to_string()],
                    )?;
                    count
                } else {
                    0
                };
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Folder(target.id()),
                    revision,
                    affected,
                    changed,
                )
            }
            GameLibraryOrganizationEnumMutation::ReparentFolder(target, parent) => {
                let folder = Self::folder_target(transaction, *target)?;
                let parent = Self::parent(transaction, *parent)?;
                if let Some(parent) = parent {
                    let cycle: bool = transaction.query_row("WITH RECURSIVE folders(id) AS (SELECT folder_id FROM library_folders WHERE folder_id = ?1 UNION SELECT f.folder_id FROM library_folders f JOIN folders p ON f.parent_id = p.id) SELECT EXISTS(SELECT 1 FROM folders WHERE id = ?2)", params![target.id().to_string(), parent.to_string()], |row| row.get(0))?;
                    if cycle {
                        return Err(GameLibraryOrganizationError::FolderCycle.into());
                    }
                }
                let changed = folder.parent != parent;
                let revision = if changed {
                    folder.revision.next()?
                } else {
                    folder.revision
                };
                let affected = if changed {
                    let count = Self::touch(
                        transaction,
                        StorageLibraryTransactionOrganizationEnumScope::FolderSubtree(target.id()),
                        timestamp,
                    )?;
                    transaction.execute("UPDATE library_folders SET parent_id = ?1, revision = ?2 WHERE folder_id = ?3", params![parent.map(|id| id.to_string()), revision.to_string(), target.id().to_string()])?;
                    count
                } else {
                    0
                };
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Folder(target.id()),
                    revision,
                    affected,
                    changed,
                )
            }
            GameLibraryOrganizationEnumMutation::CreateTag(id, name) => {
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM library_tags WHERE tag_id = ?1)",
                    [id.to_string()],
                    |row| row.get(0),
                )?;
                if exists {
                    return Err(StorageLibraryError::TagExists);
                }
                transaction.execute(
                    "INSERT INTO library_tags(tag_id, revision, name) VALUES (?1, '0', ?2)",
                    params![id.to_string(), name.as_str()],
                )?;
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Tag(*id),
                    GameLibraryOrganizationRevision::ZERO,
                    0,
                    true,
                )
            }
            GameLibraryOrganizationEnumMutation::RenameTag(target, name) => {
                let (previous, current_name) = Self::tag(transaction, *target)?;
                let changed = current_name != name.as_str();
                let revision = if changed { previous.next()? } else { previous };
                let affected = if changed {
                    let count = Self::touch(
                        transaction,
                        StorageLibraryTransactionOrganizationEnumScope::Tag(target.id()),
                        timestamp,
                    )?;
                    transaction.execute(
                        "UPDATE library_tags SET name = ?1, revision = ?2 WHERE tag_id = ?3",
                        params![name.as_str(), revision.to_string(), target.id().to_string()],
                    )?;
                    count
                } else {
                    0
                };
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Tag(target.id()),
                    revision,
                    affected,
                    changed,
                )
            }
            GameLibraryOrganizationEnumMutation::DeleteTag(target) => {
                let (previous, _name) = Self::tag(transaction, *target)?;
                let affected = Self::touch(
                    transaction,
                    StorageLibraryTransactionOrganizationEnumScope::Tag(target.id()),
                    timestamp,
                )?;
                transaction.execute(
                    "DELETE FROM library_tags WHERE tag_id = ?1",
                    [target.id().to_string()],
                )?;
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Tag(target.id()),
                    previous.next()?,
                    affected,
                    true,
                )
            }
            GameLibraryOrganizationEnumMutation::SetFavorite(target, favorite) => {
                let game = Self::game(transaction, *target)?;
                let changed = game.organization().is_favorite() != *favorite;
                Self::assign_game(transaction, *target, timestamp, changed, || {
                    transaction.execute(
                        "UPDATE library_entries SET favorite = ?1 WHERE game_id = ?2",
                        params![i64::from(*favorite), target.id().as_uuid().to_string()],
                    )?;
                    Ok(())
                })?
            }
            GameLibraryOrganizationEnumMutation::MoveToFolder(target, parent) => {
                let game = Self::game(transaction, *target)?;
                let folder = Self::parent(transaction, *parent)?;
                let changed = game.organization().folder() != folder;
                Self::assign_game(transaction, *target, timestamp, changed, || {
                    transaction.execute(
                        "UPDATE library_entries SET folder_id = ?1 WHERE game_id = ?2",
                        params![
                            folder.map(|id| id.to_string()),
                            target.id().as_uuid().to_string()
                        ],
                    )?;
                    Ok(())
                })?
            }
            GameLibraryOrganizationEnumMutation::ReplaceTags(target, tags) => {
                let game = Self::game(transaction, *target)?;
                for id in tags.ids() {
                    let exists: bool = transaction.query_row(
                        "SELECT EXISTS(SELECT 1 FROM library_tags WHERE tag_id = ?1)",
                        [id.to_string()],
                        |row| row.get(0),
                    )?;
                    if !exists {
                        return Err(StorageLibraryError::MissingTag(*id));
                    }
                }
                let changed = game.organization().tags() != tags;
                Self::assign_game(transaction, *target, timestamp, changed, || {
                    transaction.execute(
                        "DELETE FROM library_entry_tags WHERE game_id = ?1",
                        [target.id().as_uuid().to_string()],
                    )?;
                    for id in tags.ids() {
                        transaction.execute(
                            "INSERT INTO library_entry_tags(game_id, tag_id) VALUES (?1, ?2)",
                            params![target.id().as_uuid().to_string(), id.to_string()],
                        )?;
                    }
                    Ok(())
                })?
            }
        };
        if receipt.is_changed() {
            StorageLibraryTransaction::bump_catalog(transaction)?;
        }
        Ok(receipt)
    }
    fn assign_game<F: FnOnce() -> Result<(), StorageLibraryError>>(
        transaction: &Transaction<'_>,
        target: GameLibraryOrganizationGameTarget,
        timestamp: GameLibraryOrganizationTimestamp,
        changed: bool,
        assign: F,
    ) -> Result<GameLibraryOrganizationReceipt, StorageLibraryError> {
        let revision = if changed {
            assign()?;
            Self::touch(
                transaction,
                StorageLibraryTransactionOrganizationEnumScope::Game(target.id()),
                timestamp,
            )?;
            target.expected().next()?
        } else {
            target.expected()
        };
        Ok(GameLibraryOrganizationReceipt::new(
            GameLibraryOrganizationEnumTarget::Game(target.id()),
            revision,
            u64::from(changed),
            changed,
        ))
    }
}
