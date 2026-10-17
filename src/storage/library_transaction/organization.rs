use diesel::dsl::{case_when, exists, sql};
use diesel::sql_types::{Bool, Text};
use diesel::sqlite::{Sqlite, SqliteConnection};
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationEnumTarget,
    GameLibraryOrganizationError, GameLibraryOrganizationFolderId,
    GameLibraryOrganizationFolderTarget, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationName, GameLibraryOrganizationReceipt, GameLibraryOrganizationRequest,
    GameLibraryOrganizationRevision, GameLibraryOrganizationTagId,
    GameLibraryOrganizationTagTarget, GameLibraryOrganizationTimestamp,
};
use crate::storage::library_query::StorageLibraryQuery;
use crate::storage::library_query::folders::StorageLibraryQueryFolders;
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
    fn selection(self) -> library_entries::BoxedQuery<'static, Sqlite> {
        let query = library_entries::table.into_boxed::<Sqlite>();
        match self {
            Self::FolderSubtree(id) => query.filter(StorageLibraryQueryFolders::predicate(id)),
            Self::Tag(id) => query.filter(exists(
                library_entry_tags::table
                    .filter(library_entry_tags::game_id.eq(library_entries::game_id))
                    .filter(library_entry_tags::tag_id.eq(id.to_string())),
            )),
            Self::Game(id) => query.filter(library_entries::game_id.eq(id.as_uuid().to_string())),
        }
    }
}

pub(super) struct StorageLibraryTransactionOrganizationFolder {
    revision: GameLibraryOrganizationRevision,
    name: GameLibraryOrganizationName,
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
        transaction: &mut SqliteConnection,
        id: GameLibraryOrganizationFolderId,
    ) -> Result<StorageLibraryTransactionOrganizationFolder, StorageLibraryError> {
        let row: Option<(String, String, Option<String>)> = library_folders::table
            .find(id.to_string())
            .select((
                library_folders::revision,
                library_folders::name,
                library_folders::parent_id,
            ))
            .first(transaction)
            .optional()?;
        let (revision, name, parent) = row.ok_or(StorageLibraryError::MissingFolder(id))?;
        Ok(StorageLibraryTransactionOrganizationFolder {
            revision: revision.parse()?,
            name: GameLibraryOrganizationName::new(name)?,
            parent: parent.as_deref().map(str::parse).transpose()?,
        })
    }

    fn folder_target(
        transaction: &mut SqliteConnection,
        target: GameLibraryOrganizationFolderTarget,
    ) -> Result<StorageLibraryTransactionOrganizationFolder, StorageLibraryError> {
        let folder = Self::folder(transaction, target.id())?;
        folder.revision.check(target.expected())?;
        Ok(folder)
    }

    fn tag(
        transaction: &mut SqliteConnection,
        target: GameLibraryOrganizationTagTarget,
    ) -> Result<(GameLibraryOrganizationRevision, String), StorageLibraryError> {
        let row: Option<(String, String)> = library_tags::table
            .find(target.id().to_string())
            .select((library_tags::revision, library_tags::name))
            .first(transaction)
            .optional()?;
        let (revision, name) = row.ok_or(StorageLibraryError::MissingTag(target.id()))?;
        let revision: GameLibraryOrganizationRevision = revision.parse()?;
        revision.check(target.expected())?;
        Ok((revision, name))
    }

    fn game(
        transaction: &mut SqliteConnection,
        target: GameLibraryOrganizationGameTarget,
    ) -> Result<StorageLibraryTransactionEntryState, StorageLibraryError> {
        let game = StorageLibraryTransactionRows::state(transaction, target.id())?
            .ok_or(StorageLibraryError::MissingGame(target.id()))?;
        game.organization().revision().check(target.expected())?;
        Ok(game)
    }

    fn parent(
        transaction: &mut SqliteConnection,
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
        transaction: &mut SqliteConnection,
        scope: StorageLibraryTransactionOrganizationEnumScope,
        timestamp: GameLibraryOrganizationTimestamp,
    ) -> Result<u64, StorageLibraryError> {
        let mut after: Option<String> = None;
        let mut count = 0_u64;
        loop {
            let mut query = scope.selection();
            if let Some(after) = &after {
                query = query.filter(library_entries::game_id.gt(after.clone()));
            }
            let page: Vec<(String, String)> = query
                .order(library_entries::game_id.asc())
                .limit(i64::try_from(StorageLibraryQuery::PAGE_SIZE)?)
                .select((
                    library_entries::game_id,
                    library_entries::organization_revision,
                ))
                .load(transaction)?;
            if page.is_empty() {
                break;
            }
            for (id, revision) in &page {
                let _validated_id: GameId = id.parse()?;
                let revision: GameLibraryOrganizationRevision = revision.parse()?;
                let next = revision.next()?;
                let modified = case_when(
                    library_entries::modified_ms.gt(Some(timestamp.value())),
                    library_entries::modified_ms,
                )
                .otherwise(Some(timestamp.value()));
                diesel::update(library_entries::table.find(id))
                    .set((
                        library_entries::organization_revision.eq(next.to_string()),
                        library_entries::modified_ms.eq(modified),
                    ))
                    .execute(transaction)?;
                count = count
                    .checked_add(1)
                    .ok_or(GameLibraryOrganizationError::Exhausted)?;
            }
            after = page.last().map(|(id, _revision)| id.clone());
        }
        Ok(count)
    }

    pub(super) fn execute(
        transaction: &mut SqliteConnection,
        request: &GameLibraryOrganizationRequest,
    ) -> Result<GameLibraryOrganizationReceipt, StorageLibraryError> {
        let timestamp = request.timestamp();
        let receipt = match request.mutation() {
            GameLibraryOrganizationEnumMutation::CreateFolder(id, parent, name) => {
                let parent = Self::parent(transaction, *parent)?;
                let found: bool =
                    diesel::select(exists(library_folders::table.find(id.to_string())))
                        .get_result(transaction)?;
                if found {
                    return Err(StorageLibraryError::FolderExists);
                }
                diesel::insert_into(library_folders::table)
                    .values((
                        library_folders::folder_id.eq(id.to_string()),
                        library_folders::parent_id.eq(parent.map(|id| id.to_string())),
                        library_folders::revision
                            .eq(GameLibraryOrganizationRevision::ZERO.to_string()),
                        library_folders::name.eq(name.as_str()),
                    ))
                    .execute(transaction)?;
                GameLibraryOrganizationReceipt::new(
                    GameLibraryOrganizationEnumTarget::Folder(*id),
                    GameLibraryOrganizationRevision::ZERO,
                    0,
                    true,
                )
            }
            GameLibraryOrganizationEnumMutation::RenameFolder(target, name) => {
                let folder = Self::folder_target(transaction, *target)?;
                let changed = folder.name.as_str() != name.as_str();
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
                    diesel::update(library_folders::table.find(target.id().to_string()))
                        .set((
                            library_folders::name.eq(name.as_str()),
                            library_folders::revision.eq(revision.to_string()),
                        ))
                        .execute(transaction)?;
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
                    let cycle: bool = diesel::select(
                        sql::<Bool>(&format!("EXISTS({}", StorageLibraryQueryFolders::PREFIX))
                            .bind::<Text, _>(target.id().to_string())
                            .sql(StorageLibraryQueryFolders::SUFFIX)
                            .sql("SELECT 1 FROM folders WHERE id = ")
                            .bind::<Text, _>(parent.to_string())
                            .sql(")"),
                    )
                    .get_result(transaction)?;
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
                    diesel::update(library_folders::table.find(target.id().to_string()))
                        .set((
                            library_folders::parent_id.eq(parent.map(|id| id.to_string())),
                            library_folders::revision.eq(revision.to_string()),
                        ))
                        .execute(transaction)?;
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
                let found: bool = diesel::select(exists(library_tags::table.find(id.to_string())))
                    .get_result(transaction)?;
                if found {
                    return Err(StorageLibraryError::TagExists);
                }
                diesel::insert_into(library_tags::table)
                    .values((
                        library_tags::tag_id.eq(id.to_string()),
                        library_tags::revision
                            .eq(GameLibraryOrganizationRevision::ZERO.to_string()),
                        library_tags::name.eq(name.as_str()),
                    ))
                    .execute(transaction)?;
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
                    diesel::update(library_tags::table.find(target.id().to_string()))
                        .set((
                            library_tags::name.eq(name.as_str()),
                            library_tags::revision.eq(revision.to_string()),
                        ))
                        .execute(transaction)?;
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
                diesel::delete(library_tags::table.find(target.id().to_string()))
                    .execute(transaction)?;
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
                Self::assign_game(transaction, *target, timestamp, changed, |transaction| {
                    diesel::update(library_entries::table.find(target.id().as_uuid().to_string()))
                        .set(library_entries::favorite.eq(i64::from(*favorite)))
                        .execute(transaction)?;
                    Ok(())
                })?
            }
            GameLibraryOrganizationEnumMutation::MoveToFolder(target, parent) => {
                let game = Self::game(transaction, *target)?;
                let folder = Self::parent(transaction, *parent)?;
                let changed = game.organization().folder() != folder;
                Self::assign_game(transaction, *target, timestamp, changed, |transaction| {
                    diesel::update(library_entries::table.find(target.id().as_uuid().to_string()))
                        .set(library_entries::folder_id.eq(folder.map(|id| id.to_string())))
                        .execute(transaction)?;
                    Ok(())
                })?
            }
            GameLibraryOrganizationEnumMutation::ReplaceTags(target, tags) => {
                let game = Self::game(transaction, *target)?;
                for id in tags.ids() {
                    let found: bool =
                        diesel::select(exists(library_tags::table.find(id.to_string())))
                            .get_result(transaction)?;
                    if !found {
                        return Err(StorageLibraryError::MissingTag(*id));
                    }
                }
                let changed = game.organization().tags() != tags;
                Self::assign_game(transaction, *target, timestamp, changed, |transaction| {
                    let game = target.id().as_uuid().to_string();
                    diesel::delete(
                        library_entry_tags::table.filter(library_entry_tags::game_id.eq(&game)),
                    )
                    .execute(transaction)?;
                    for id in tags.ids() {
                        diesel::insert_into(library_entry_tags::table)
                            .values((
                                library_entry_tags::game_id.eq(&game),
                                library_entry_tags::tag_id.eq(id.to_string()),
                            ))
                            .execute(transaction)?;
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

    fn assign_game(
        transaction: &mut SqliteConnection,
        target: GameLibraryOrganizationGameTarget,
        timestamp: GameLibraryOrganizationTimestamp,
        changed: bool,
        assign: impl FnOnce(&mut SqliteConnection) -> Result<(), StorageLibraryError>,
    ) -> Result<GameLibraryOrganizationReceipt, StorageLibraryError> {
        let revision = if changed {
            assign(transaction)?;
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
