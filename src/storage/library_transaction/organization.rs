use diesel::SqliteConnection;
use diesel::prelude::*;

use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationEnumTarget,
    GameLibraryOrganizationError, GameLibraryOrganizationFolderId,
    GameLibraryOrganizationFolderTarget, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationReceipt, GameLibraryOrganizationRequest,
    GameLibraryOrganizationRevision, GameLibraryOrganizationTagId,
    GameLibraryOrganizationTagTarget, GameLibraryOrganizationTimestamp,
};
use crate::storage::library_query::{
    StorageLibraryMillis, StorageLibraryQuery, library_coalesce_millis, library_max_millis,
};
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::library_transaction::StorageLibraryTransaction;
use crate::storage::library_transaction::rows::{
    StorageLibraryTransactionEntryState, StorageLibraryTransactionRows,
};
use crate::storage::schema::{library_entries, library_entry_tags, library_folders, library_tags};

#[derive(Clone, Copy)]
pub(super) enum StorageLibraryTransactionOrganizationEnumScope {
    FolderSubtree(GameLibraryOrganizationFolderId),
    Tag(GameLibraryOrganizationTagId),
    Game(GameId),
}

impl StorageLibraryTransactionOrganizationEnumScope {
    fn page(
        self,
        connection: &mut SqliteConnection,
        after: Option<&str>,
    ) -> Result<Vec<(String, String)>, StorageLibraryError> {
        let mut query = library_entries::table
            .select((
                library_entries::game_id,
                library_entries::organization_revision,
            ))
            .order(library_entries::game_id.asc())
            .limit(i64::try_from(StorageLibraryQuery::PAGE_SIZE)?)
            .into_boxed();
        query = match self {
            Self::FolderSubtree(id) => {
                let subtree = StorageLibraryQuery::folder_subtree(connection, id.to_string())?;
                query.filter(library_entries::folder_id.eq_any(subtree))
            }
            Self::Tag(id) => query.filter(
                library_entries::game_id.eq_any(
                    library_entry_tags::table
                        .select(library_entry_tags::game_id)
                        .filter(library_entry_tags::tag_id.eq(id.to_string())),
                ),
            ),
            Self::Game(id) => query.filter(library_entries::game_id.eq(id.as_uuid().to_string())),
        };
        if let Some(after) = after {
            query = query.filter(library_entries::game_id.gt(after.to_owned()));
        }
        Ok(query.load(connection)?)
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
        connection: &mut SqliteConnection,
        id: GameLibraryOrganizationFolderId,
    ) -> Result<StorageLibraryTransactionOrganizationFolder, StorageLibraryError> {
        let row: Option<(String, String, Option<String>)> = library_folders::table
            .filter(library_folders::folder_id.eq(id.to_string()))
            .select((
                library_folders::revision,
                library_folders::name,
                library_folders::parent_id,
            ))
            .first(connection)
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
        connection: &mut SqliteConnection,
        target: GameLibraryOrganizationFolderTarget,
    ) -> Result<StorageLibraryTransactionOrganizationFolder, StorageLibraryError> {
        let folder = Self::folder(connection, target.id())?;
        folder.revision.check(target.expected())?;
        Ok(folder)
    }
    fn tag(
        connection: &mut SqliteConnection,
        target: GameLibraryOrganizationTagTarget,
    ) -> Result<(GameLibraryOrganizationRevision, String), StorageLibraryError> {
        let row: Option<(String, String)> = library_tags::table
            .filter(library_tags::tag_id.eq(target.id().to_string()))
            .select((library_tags::revision, library_tags::name))
            .first(connection)
            .optional()?;
        let (revision, name) = row.ok_or(StorageLibraryError::MissingTag(target.id()))?;
        let revision: GameLibraryOrganizationRevision = revision.parse()?;
        revision.check(target.expected())?;
        Ok((revision, name))
    }
    fn game(
        connection: &mut SqliteConnection,
        target: GameLibraryOrganizationGameTarget,
    ) -> Result<StorageLibraryTransactionEntryState, StorageLibraryError> {
        let game = StorageLibraryTransactionRows::state(connection, target.id())?
            .ok_or(StorageLibraryError::MissingGame(target.id()))?;
        game.organization().revision().check(target.expected())?;
        Ok(game)
    }
    fn parent(
        connection: &mut SqliteConnection,
        target: Option<GameLibraryOrganizationFolderTarget>,
    ) -> Result<Option<GameLibraryOrganizationFolderId>, StorageLibraryError> {
        target
            .map(|target| {
                Self::folder_target(connection, target)?;
                Ok(target.id())
            })
            .transpose()
    }
    pub(super) fn touch(
        connection: &mut SqliteConnection,
        scope: StorageLibraryTransactionOrganizationEnumScope,
        timestamp: GameLibraryOrganizationTimestamp,
    ) -> Result<u64, StorageLibraryError> {
        let mut after: Option<String> = None;
        let mut count = 0_u64;
        loop {
            let page = scope.page(connection, after.as_deref())?;
            if page.is_empty() {
                break;
            }
            for (id, revision) in &page {
                let _validated_id: GameId = id.parse()?;
                let revision: GameLibraryOrganizationRevision = revision.parse()?;
                let next = revision.next()?;
                let stamp = StorageLibraryMillis::new(timestamp.value());
                diesel::update(library_entries::table.filter(library_entries::game_id.eq(id)))
                    .set((
                        library_entries::organization_revision.eq(next.to_string()),
                        library_entries::modified_ms.eq(library_max_millis(
                            library_coalesce_millis(
                                library_entries::modified_ms,
                                stamp.expression(),
                            ),
                            stamp.expression(),
                        )
                        .nullable()),
                    ))
                    .execute(connection)?;
                count = count
                    .checked_add(1)
                    .ok_or(GameLibraryOrganizationError::Exhausted)?;
            }
            after = page.last().map(|(id, _revision)| id.clone());
        }
        Ok(count)
    }
    pub(super) fn execute(
        connection: &mut SqliteConnection,
        request: &GameLibraryOrganizationRequest,
    ) -> Result<GameLibraryOrganizationReceipt, StorageLibraryError> {
        let timestamp = request.timestamp();
        let receipt = match request.mutation() {
            GameLibraryOrganizationEnumMutation::CreateFolder(id, parent, name) => {
                let parent = Self::parent(connection, *parent)?;
                let exists: bool = diesel::select(diesel::dsl::exists(
                    library_folders::table.filter(library_folders::folder_id.eq(id.to_string())),
                ))
                .get_result(connection)?;
                if exists {
                    return Err(StorageLibraryError::FolderExists);
                }
                diesel::insert_into(library_folders::table)
                    .values((
                        library_folders::folder_id.eq(id.to_string()),
                        library_folders::parent_id.eq(parent.map(|id| id.to_string())),
                        library_folders::revision.eq("0"),
                        library_folders::name.eq(name.as_str()),
                    ))
                    .execute(connection)?;
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Folder(*id),
                    GameLibraryOrganizationRevision::ZERO,
                    0,
                    true,
                )
            }
            GameLibraryOrganizationEnumMutation::RenameFolder(target, name) => {
                let folder = Self::folder_target(connection, *target)?;
                let changed = folder.name != name.as_str();
                let revision = if changed {
                    folder.revision.next()?
                } else {
                    folder.revision
                };
                let affected = if changed {
                    let count = Self::touch(
                        connection,
                        StorageLibraryTransactionOrganizationEnumScope::FolderSubtree(target.id()),
                        timestamp,
                    )?;
                    diesel::update(
                        library_folders::table
                            .filter(library_folders::folder_id.eq(target.id().to_string())),
                    )
                    .set((
                        library_folders::name.eq(name.as_str()),
                        library_folders::revision.eq(revision.to_string()),
                    ))
                    .execute(connection)?;
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
                let folder = Self::folder_target(connection, *target)?;
                let parent = Self::parent(connection, *parent)?;
                if let Some(parent) = parent {
                    let subtree =
                        StorageLibraryQuery::folder_subtree(connection, target.id().to_string())?;
                    if subtree.contains(&parent.to_string()) {
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
                        connection,
                        StorageLibraryTransactionOrganizationEnumScope::FolderSubtree(target.id()),
                        timestamp,
                    )?;
                    diesel::update(
                        library_folders::table
                            .filter(library_folders::folder_id.eq(target.id().to_string())),
                    )
                    .set((
                        library_folders::parent_id.eq(parent.map(|id| id.to_string())),
                        library_folders::revision.eq(revision.to_string()),
                    ))
                    .execute(connection)?;
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
                let exists: bool = diesel::select(diesel::dsl::exists(
                    library_tags::table.filter(library_tags::tag_id.eq(id.to_string())),
                ))
                .get_result(connection)?;
                if exists {
                    return Err(StorageLibraryError::TagExists);
                }
                diesel::insert_into(library_tags::table)
                    .values((
                        library_tags::tag_id.eq(id.to_string()),
                        library_tags::revision.eq("0"),
                        library_tags::name.eq(name.as_str()),
                    ))
                    .execute(connection)?;
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Tag(*id),
                    GameLibraryOrganizationRevision::ZERO,
                    0,
                    true,
                )
            }
            GameLibraryOrganizationEnumMutation::RenameTag(target, name) => {
                let (previous, current_name) = Self::tag(connection, *target)?;
                let changed = current_name != name.as_str();
                let revision = if changed { previous.next()? } else { previous };
                let affected = if changed {
                    let count = Self::touch(
                        connection,
                        StorageLibraryTransactionOrganizationEnumScope::Tag(target.id()),
                        timestamp,
                    )?;
                    diesel::update(
                        library_tags::table
                            .filter(library_tags::tag_id.eq(target.id().to_string())),
                    )
                    .set((
                        library_tags::name.eq(name.as_str()),
                        library_tags::revision.eq(revision.to_string()),
                    ))
                    .execute(connection)?;
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
                let (previous, _name) = Self::tag(connection, *target)?;
                let affected = Self::touch(
                    connection,
                    StorageLibraryTransactionOrganizationEnumScope::Tag(target.id()),
                    timestamp,
                )?;
                diesel::delete(
                    library_tags::table.filter(library_tags::tag_id.eq(target.id().to_string())),
                )
                .execute(connection)?;
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Tag(target.id()),
                    previous.next()?,
                    affected,
                    true,
                )
            }
            GameLibraryOrganizationEnumMutation::SetFavorite(target, favorite) => {
                let game = Self::game(connection, *target)?;
                let changed = game.organization().is_favorite() != *favorite;
                Self::assign_game(connection, *target, timestamp, changed, |connection| {
                    diesel::update(
                        library_entries::table
                            .filter(library_entries::game_id.eq(target.id().as_uuid().to_string())),
                    )
                    .set(library_entries::favorite.eq(i32::from(*favorite)))
                    .execute(connection)?;
                    Ok(())
                })?
            }
            GameLibraryOrganizationEnumMutation::MoveToFolder(target, parent) => {
                let game = Self::game(connection, *target)?;
                let folder = Self::parent(connection, *parent)?;
                let changed = game.organization().folder() != folder;
                Self::assign_game(connection, *target, timestamp, changed, |connection| {
                    diesel::update(
                        library_entries::table
                            .filter(library_entries::game_id.eq(target.id().as_uuid().to_string())),
                    )
                    .set(library_entries::folder_id.eq(folder.map(|id| id.to_string())))
                    .execute(connection)?;
                    Ok(())
                })?
            }
            GameLibraryOrganizationEnumMutation::ReplaceTags(target, tags) => {
                let game = Self::game(connection, *target)?;
                for id in tags.ids() {
                    let exists: bool = diesel::select(diesel::dsl::exists(
                        library_tags::table.filter(library_tags::tag_id.eq(id.to_string())),
                    ))
                    .get_result(connection)?;
                    if !exists {
                        return Err(StorageLibraryError::MissingTag(*id));
                    }
                }
                let changed = game.organization().tags() != tags;
                Self::assign_game(connection, *target, timestamp, changed, |connection| {
                    diesel::delete(
                        library_entry_tags::table.filter(
                            library_entry_tags::game_id.eq(target.id().as_uuid().to_string()),
                        ),
                    )
                    .execute(connection)?;
                    for id in tags.ids() {
                        diesel::insert_into(library_entry_tags::table)
                            .values((
                                library_entry_tags::game_id.eq(target.id().as_uuid().to_string()),
                                library_entry_tags::tag_id.eq(id.to_string()),
                            ))
                            .execute(connection)?;
                    }
                    Ok(())
                })?
            }
        };
        if receipt.is_changed() {
            StorageLibraryTransaction::diesel_bump_catalog(connection)?;
        }
        Ok(receipt)
    }
    fn assign_game<F: FnOnce(&mut SqliteConnection) -> Result<(), StorageLibraryError>>(
        connection: &mut SqliteConnection,
        target: GameLibraryOrganizationGameTarget,
        timestamp: GameLibraryOrganizationTimestamp,
        changed: bool,
        assign: F,
    ) -> Result<GameLibraryOrganizationReceipt, StorageLibraryError> {
        let revision = if changed {
            assign(connection)?;
            Self::touch(
                connection,
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
