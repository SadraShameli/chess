use diesel::helper_types::InnerJoinQuerySource;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Nullable, Text};
use diesel::sqlite::Sqlite;
use diesel::{BoxableExpression, SqliteConnection};

use crate::board::enums::BoardEnumVariant;
use crate::capacity::executor::CapacityExecutorCancellation;
use crate::game::enums::{GameEnumMode, GameId};
use crate::game::library_organization::{
    GameLibraryOrganizationFolderId, GameLibraryOrganizationState, GameLibraryOrganizationTagSet,
    GameLibraryOrganizationTimestamp,
};
use crate::game::session::{GameMetadataDate, GameMetadataOpeningCode, GameMetadataText};
use crate::game::storage_repository::GameStorageRevision;
use crate::notation::game_identity::{NotationGameIdentity, NotationGameIdentityEnumResult};
use crate::storage::library_prepared::{StorageLibraryEnumStartKind, StorageLibraryProjection};
use crate::storage::library_repository::{
    StorageLibraryCatalogRevision, StorageLibraryError, StorageLibraryMemory,
};
use crate::storage::schema::{documents, library_entries, library_entry_tags, library_folders};
use crate::storage::trash::StorageTrashEnumLocation;

diesel::define_sql_function! {
    #[sql_name = "instr"]
    fn library_instr(haystack: Nullable<Text>, needle: Text) -> diesel::sql_types::Integer;
}

diesel::define_sql_function! {
    #[sql_name = "coalesce"]
    fn library_coalesce(
        value: Nullable<diesel::sql_types::Integer>,
        fallback: BigInt,
    ) -> BigInt;
}

type StorageLibraryQuerySource = InnerJoinQuerySource<library_entries::table, documents::table>;
type StorageLibraryQueryBool =
    Box<dyn BoxableExpression<StorageLibraryQuerySource, Sqlite, SqlType = Bool>>;
type StorageLibraryQueryNullableBool =
    Box<dyn BoxableExpression<StorageLibraryQuerySource, Sqlite, SqlType = Nullable<Bool>>>;
type StorageLibraryQueryInteger =
    Box<dyn BoxableExpression<StorageLibraryQuerySource, Sqlite, SqlType = BigInt>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, diesel::deserialize::FromSqlRow)]
#[diesel(sql_type = diesel::sql_types::Integer)]
pub(crate) struct StorageLibraryMillis(i64);

impl StorageLibraryMillis {
    pub(crate) const fn new(value: i64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> i64 {
        self.0
    }

    pub(crate) fn optional(
        value: Option<i64>,
    ) -> library_nullable_millis<
        diesel::dsl::IntoSql<Option<i64>, Nullable<BigInt>>,
        diesel::dsl::IntoSql<Option<i64>, Nullable<BigInt>>,
    > {
        library_nullable_millis(
            value.into_sql::<Nullable<BigInt>>(),
            None::<i64>.into_sql::<Nullable<BigInt>>(),
        )
    }

    pub(crate) fn expression(
        self,
    ) -> diesel::dsl::Cast<diesel::dsl::IntoSql<i64, BigInt>, diesel::sql_types::Integer> {
        self.0
            .into_sql::<BigInt>()
            .fallible_cast::<diesel::sql_types::Integer>()
    }
}

impl diesel::deserialize::FromSql<diesel::sql_types::Integer, Sqlite> for StorageLibraryMillis {
    fn from_sql(
        mut value: diesel::sqlite::SqliteValue<'_, '_, '_>,
    ) -> diesel::deserialize::Result<Self> {
        Ok(Self(value.read_long()))
    }
}

diesel::define_sql_function! {
    #[sql_name = "ifnull"]
    fn library_nullable_millis(
        value: Nullable<BigInt>,
        fallback: Nullable<BigInt>,
    ) -> Nullable<diesel::sql_types::Integer>;
}

diesel::define_sql_function! {
    #[sql_name = "coalesce"]
    fn library_coalesce_millis(
        value: Nullable<diesel::sql_types::Integer>,
        fallback: diesel::sql_types::Integer,
    ) -> diesel::sql_types::Integer;
}

diesel::define_sql_function! {
    #[sql_name = "max"]
    fn library_max_millis(
        left: diesel::sql_types::Integer,
        right: diesel::sql_types::Integer,
    ) -> diesel::sql_types::Integer;
}

#[derive(Debug, Queryable, Selectable)]
#[diesel(table_name = library_entries)]
pub(crate) struct StorageLibraryQueryRow {
    pub(crate) game_id: String,
    pub(crate) content_revision: String,
    pub(crate) organization_revision: String,
    pub(crate) content_kind: String,
    pub(crate) rules_variant: String,
    pub(crate) start_kind: String,
    pub(crate) mode: Option<String>,
    pub(crate) result: String,
    pub(crate) white_display: Option<String>,
    pub(crate) white_key: Option<String>,
    pub(crate) black_display: Option<String>,
    pub(crate) black_key: Option<String>,
    pub(crate) date_year: Option<i32>,
    pub(crate) date_month: Option<i32>,
    pub(crate) date_day: Option<i32>,
    pub(crate) opening_display: Option<String>,
    pub(crate) opening_key: Option<String>,
    pub(crate) opening_code: Option<String>,
    pub(crate) plies: i32,
    pub(crate) modified_ms: Option<StorageLibraryMillis>,
    pub(crate) favorite: i32,
    pub(crate) folder_id: Option<String>,
    pub(crate) location: String,
    pub(crate) trashed_ms: Option<StorageLibraryMillis>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StorageLibraryEnumFilter<T> {
    #[default]
    Any,
    Value(T),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageLibraryDateInterval {
    lower: (u16, u8, u8),
    upper: (u16, u8, u8),
}

impl StorageLibraryDateInterval {
    pub fn new(
        lower: GameMetadataDate,
        upper: GameMetadataDate,
    ) -> Result<Self, StorageLibraryError> {
        let (Some(ly), Some(lm), Some(ld), Some(uy), Some(um), Some(ud)) = (
            lower.year(),
            lower.month(),
            lower.day(),
            upper.year(),
            upper.month(),
            upper.day(),
        ) else {
            return Err(StorageLibraryError::DateInterval);
        };
        if (ly, lm, ld) > (uy, um, ud) {
            return Err(StorageLibraryError::DateInterval);
        }
        Ok(Self {
            lower: (ly, lm, ld),
            upper: (uy, um, ud),
        })
    }

    fn windows(
        self,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Vec<StorageLibraryDateWindow>, StorageLibraryError> {
        let mut windows = Vec::with_capacity(366);
        for month in 1..=12 {
            StorageLibraryError::check_cancellation(cancellation)?;
            for day in 1..=31 {
                if GameMetadataDate::new(None, Some(month), Some(day)).is_err() {
                    continue;
                }
                let first = self.lower.0 + u16::from((month, day) < (self.lower.1, self.lower.2));
                let last = self.upper.0 - u16::from((month, day) > (self.upper.1, self.upper.2));
                if first > last {
                    continue;
                }
                let witness = (first..=last.min(first.saturating_add(8)))
                    .any(|year| GameMetadataDate::new(Some(year), Some(month), Some(day)).is_ok());
                if witness {
                    windows.push(StorageLibraryDateWindow {
                        month,
                        day,
                        first,
                        last,
                    });
                }
            }
        }
        Ok(windows)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageLibraryDateWindow {
    month: u8,
    day: u8,
    first: u16,
    last: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StorageLibraryEnumFolderFilter {
    #[default]
    Any,
    Unfiled,
    Exact(GameLibraryOrganizationFolderId),
    Subtree(GameLibraryOrganizationFolderId),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StorageLibraryEnumTagFilter {
    #[default]
    Any,
    AnyOf(GameLibraryOrganizationTagSet),
    AllOf(GameLibraryOrganizationTagSet),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StorageLibraryEnumSort {
    #[default]
    UpdatedDescending,
    UpdatedAscending,
    DateDescending,
    DateAscending,
    PliesDescending,
    PliesAscending,
}

impl StorageLibraryEnumSort {
    fn descending(self) -> bool {
        match self {
            Self::UpdatedDescending | Self::DateDescending | Self::PliesDescending => true,
            Self::UpdatedAscending | Self::DateAscending | Self::PliesAscending => false,
        }
    }
    fn value_count(self) -> usize {
        match self {
            Self::UpdatedDescending | Self::UpdatedAscending => 1,
            Self::DateDescending | Self::DateAscending => 3,
            Self::PliesDescending | Self::PliesAscending => 1,
        }
    }
    fn value(self, index: usize) -> StorageLibraryQueryInteger {
        match (self, index) {
            (Self::UpdatedDescending | Self::UpdatedAscending, _) => {
                Box::new(library_coalesce(library_entries::modified_ms, 0))
            }
            (Self::DateDescending | Self::DateAscending, 0) => {
                Box::new(library_coalesce(library_entries::date_year, 0))
            }
            (Self::DateDescending | Self::DateAscending, 1) => {
                Box::new(library_coalesce(library_entries::date_month, 0))
            }
            (Self::DateDescending | Self::DateAscending, _) => {
                Box::new(library_coalesce(library_entries::date_day, 0))
            }
            (Self::PliesDescending | Self::PliesAscending, _) => {
                Box::new(library_entries::plies.cast::<BigInt>())
            }
        }
    }
    fn null_rank(self) -> StorageLibraryQueryBool {
        match self {
            Self::UpdatedDescending | Self::UpdatedAscending => {
                Box::new(library_entries::modified_ms.is_null())
            }
            Self::DateDescending | Self::DateAscending => Box::new(
                library_entries::date_year
                    .is_null()
                    .or(library_entries::date_month.is_null())
                    .or(library_entries::date_day.is_null()),
            ),
            Self::PliesDescending | Self::PliesAscending => Box::new(false.into_sql::<Bool>()),
        }
    }
    pub(crate) fn key(self, entry: &StorageLibraryQueryEntry) -> StorageLibraryQueryKey {
        let mut values = [0; 3];
        let rank = match self {
            Self::UpdatedDescending | Self::UpdatedAscending => {
                values[0] = entry
                    .modified
                    .map_or(0, GameLibraryOrganizationTimestamp::value);
                i64::from(entry.modified.is_none())
            }
            Self::DateDescending | Self::DateAscending => {
                let date = entry.projection.date();
                values = [
                    date.and_then(GameMetadataDate::year).map_or(0, i64::from),
                    date.and_then(GameMetadataDate::month).map_or(0, i64::from),
                    date.and_then(GameMetadataDate::day).map_or(0, i64::from),
                ];
                i64::from(date.is_none_or(|date| {
                    date.year().is_none() || date.month().is_none() || date.day().is_none()
                }))
            }
            Self::PliesDescending | Self::PliesAscending => {
                values[0] = i64::from(entry.projection.plies());
                0
            }
        };
        StorageLibraryQueryKey {
            rank,
            values,
            id: entry.id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageLibraryPlyInterval {
    minimum: u32,
    maximum: u32,
}

impl StorageLibraryPlyInterval {
    pub fn new(minimum: u32, maximum: u32) -> Result<Self, StorageLibraryError> {
        if minimum > maximum || maximum > StorageLibraryProjection::MAX_PLIES {
            return Err(StorageLibraryError::PlyInterval);
        }
        Ok(Self { minimum, maximum })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StorageLibraryQueryOptions {
    pub location: StorageTrashEnumLocation,
    pub white: StorageLibraryEnumFilter<GameMetadataText>,
    pub black: StorageLibraryEnumFilter<GameMetadataText>,
    pub either_player: StorageLibraryEnumFilter<GameMetadataText>,
    pub date: StorageLibraryEnumFilter<StorageLibraryDateInterval>,
    pub result: StorageLibraryEnumFilter<NotationGameIdentityEnumResult>,
    pub rules_variant: StorageLibraryEnumFilter<BoardEnumVariant>,
    pub start_kind: StorageLibraryEnumFilter<StorageLibraryEnumStartKind>,
    pub mode: StorageLibraryEnumFilter<GameEnumMode>,
    pub favorite: StorageLibraryEnumFilter<bool>,
    pub tags: StorageLibraryEnumTagFilter,
    pub folder: StorageLibraryEnumFolderFilter,
    pub opening_name: StorageLibraryEnumFilter<GameMetadataText>,
    pub opening_code: StorageLibraryEnumFilter<GameMetadataOpeningCode>,
    pub plies: StorageLibraryEnumFilter<StorageLibraryPlyInterval>,
    pub sort: StorageLibraryEnumSort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageLibraryQueryCriteria {
    location: StorageTrashEnumLocation,
    white: Option<String>,
    black: Option<String>,
    either: Option<String>,
    date: StorageLibraryEnumFilter<StorageLibraryDateInterval>,
    result: StorageLibraryEnumFilter<NotationGameIdentityEnumResult>,
    variant: StorageLibraryEnumFilter<BoardEnumVariant>,
    start: StorageLibraryEnumFilter<StorageLibraryEnumStartKind>,
    mode: StorageLibraryEnumFilter<GameEnumMode>,
    favorite: StorageLibraryEnumFilter<bool>,
    tags: StorageLibraryEnumTagFilter,
    folder: StorageLibraryEnumFolderFilter,
    opening: Option<String>,
    code: StorageLibraryEnumFilter<GameMetadataOpeningCode>,
    plies: StorageLibraryEnumFilter<StorageLibraryPlyInterval>,
    sort: StorageLibraryEnumSort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLibraryQueryCursor {
    criteria: Box<StorageLibraryQueryCriteria>,
    catalog: StorageLibraryCatalogRevision,
    key: StorageLibraryQueryKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageLibraryQueryKey {
    rank: i64,
    values: [i64; 3],
    id: GameId,
}

#[derive(Debug, Clone)]
pub struct StorageLibraryQuery {
    criteria: Box<StorageLibraryQueryCriteria>,
    cursor: Option<StorageLibraryQueryCursor>,
    windows: Option<Vec<StorageLibraryDateWindow>>,
}

impl Default for StorageLibraryQuery {
    fn default() -> Self {
        Self {
            criteria: Box::new(StorageLibraryQueryCriteria {
                location: StorageTrashEnumLocation::Library,
                white: None,
                black: None,
                either: None,
                date: StorageLibraryEnumFilter::Any,
                result: StorageLibraryEnumFilter::Any,
                variant: StorageLibraryEnumFilter::Any,
                start: StorageLibraryEnumFilter::Any,
                mode: StorageLibraryEnumFilter::Any,
                favorite: StorageLibraryEnumFilter::Any,
                tags: StorageLibraryEnumTagFilter::Any,
                folder: StorageLibraryEnumFolderFilter::Any,
                opening: None,
                code: StorageLibraryEnumFilter::Any,
                plies: StorageLibraryEnumFilter::Any,
                sort: StorageLibraryEnumSort::default(),
            }),
            cursor: None,
            windows: None,
        }
    }
}

impl StorageLibraryQuery {
    pub const PAGE_SIZE: usize = 100;
    pub fn new(options: StorageLibraryQueryOptions) -> Result<Self, StorageLibraryError> {
        match &options.tags {
            StorageLibraryEnumTagFilter::Any => {}
            StorageLibraryEnumTagFilter::AnyOf(tags) | StorageLibraryEnumTagFilter::AllOf(tags)
                if tags.ids().is_empty() =>
            {
                return Err(StorageLibraryError::EmptyFilter);
            }
            StorageLibraryEnumTagFilter::AnyOf(_) | StorageLibraryEnumTagFilter::AllOf(_) => {}
        }
        Ok(Self {
            criteria: Box::new(StorageLibraryQueryCriteria {
                location: options.location,
                white: Self::player_key(options.white)?,
                black: Self::player_key(options.black)?,
                either: Self::player_key(options.either_player)?,
                date: options.date,
                result: options.result,
                variant: options.rules_variant,
                start: options.start_kind,
                mode: options.mode,
                favorite: options.favorite,
                tags: options.tags,
                folder: options.folder,
                opening: match options.opening_name {
                    StorageLibraryEnumFilter::Any => None,
                    StorageLibraryEnumFilter::Value(text) => {
                        Self::check_text(&text)?;
                        let key = NotationGameIdentity::canonical_text(text.as_str());
                        if key.is_empty() {
                            return Err(StorageLibraryError::EmptyFilter);
                        }
                        Some(key)
                    }
                },
                code: options.opening_code,
                plies: options.plies,
                sort: options.sort,
            }),
            cursor: None,
            windows: None,
        })
    }
    fn check_text(text: &GameMetadataText) -> Result<(), StorageLibraryError> {
        if text.as_str().len() > StorageLibraryProjection::MAX_TEXT_BYTES {
            Err(StorageLibraryError::TextTooLarge)
        } else {
            Ok(())
        }
    }
    fn player_key(
        filter: StorageLibraryEnumFilter<GameMetadataText>,
    ) -> Result<Option<String>, StorageLibraryError> {
        match filter {
            StorageLibraryEnumFilter::Any => Ok(None),
            StorageLibraryEnumFilter::Value(text) => {
                Self::check_text(&text)?;
                NotationGameIdentity::canonical_player(Some(&text))
                    .map(Some)
                    .ok_or(StorageLibraryError::EmptyFilter)
            }
        }
    }
    pub fn with_cursor(
        mut self,
        cursor: StorageLibraryQueryCursor,
    ) -> Result<Self, StorageLibraryError> {
        if cursor.criteria != self.criteria {
            return Err(StorageLibraryError::CursorQuery);
        }
        self.cursor = Some(cursor);
        Ok(self)
    }
    pub(crate) fn prepare(
        mut self,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self, StorageLibraryError> {
        StorageLibraryError::check_cancellation(cancellation)?;
        self.windows = match self.criteria.date {
            StorageLibraryEnumFilter::Any => None,
            StorageLibraryEnumFilter::Value(interval) => Some(interval.windows(cancellation)?),
        };
        Ok(self)
    }
    pub(crate) fn transport_bytes(&self) -> Result<usize, StorageLibraryError> {
        let criteria = &self.criteria;
        let strings: usize = [
            &criteria.white,
            &criteria.black,
            &criteria.either,
            &criteria.opening,
        ]
        .into_iter()
        .flatten()
        .map(String::len)
        .sum();
        let tags = match &criteria.tags {
            StorageLibraryEnumTagFilter::Any => 0,
            StorageLibraryEnumTagFilter::AnyOf(tags) | StorageLibraryEnumTagFilter::AllOf(tags) => {
                std::mem::size_of_val(tags.ids())
            }
        };
        let bytes = std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<StorageLibraryQueryCriteria>()
            + strings * 2
            + tags * 2
            + self.windows.as_ref().map_or(0, |windows| {
                windows.capacity() * std::mem::size_of::<StorageLibraryDateWindow>()
            });
        Ok(bytes)
    }
    pub(crate) fn cursor(
        &self,
        catalog: StorageLibraryCatalogRevision,
        entry: &StorageLibraryQueryEntry,
    ) -> StorageLibraryQueryCursor {
        StorageLibraryQueryCursor {
            criteria: self.criteria.clone(),
            catalog,
            key: self.criteria.sort.key(entry),
        }
    }
    pub(crate) fn is_date_filtered(&self) -> bool {
        matches!(self.criteria.date, StorageLibraryEnumFilter::Value(_))
    }
    pub(crate) fn load(
        &self,
        connection: &mut SqliteConnection,
        catalog: StorageLibraryCatalogRevision,
    ) -> Result<Vec<StorageLibraryQueryRow>, StorageLibraryError> {
        if let Some(cursor) = &self.cursor {
            if cursor.catalog != catalog {
                return Err(StorageLibraryError::StaleCursor);
            }
            if cursor.criteria != self.criteria {
                return Err(StorageLibraryError::CursorQuery);
            }
        }
        let c = &self.criteria;
        let mut query = library_entries::table
            .inner_join(documents::table)
            .filter(library_entries::content_revision.eq(documents::revision))
            .filter(library_entries::location.eq(c.location.sql()))
            .select(StorageLibraryQueryRow::as_select())
            .into_boxed();
        if let Some(key) = &c.white {
            query = query.filter(library_instr(library_entries::white_key, key).gt(0));
        }
        if let Some(key) = &c.black {
            query = query.filter(library_instr(library_entries::black_key, key).gt(0));
        }
        if let Some(key) = &c.either {
            query = query.filter(
                library_instr(library_entries::white_key, key)
                    .gt(0)
                    .or(library_instr(library_entries::black_key, key).gt(0)),
            );
        }
        if let StorageLibraryEnumFilter::Value(_) = c.date {
            let windows = self
                .windows
                .as_ref()
                .ok_or(StorageLibraryError::UnpreparedQuery)?;
            query = query.filter(Self::date_predicate(windows)?);
        }
        if let StorageLibraryEnumFilter::Value(result) = c.result {
            query = query.filter(library_entries::result.eq(result.pgn_token()));
        }
        if let StorageLibraryEnumFilter::Value(variant) = c.variant {
            query = query.filter(
                library_entries::rules_variant.eq(StorageLibraryProjection::variant_sql(variant)),
            );
        }
        if let StorageLibraryEnumFilter::Value(start) = c.start {
            query = query.filter(library_entries::start_kind.eq(start.sql()));
        }
        if let StorageLibraryEnumFilter::Value(mode) = c.mode {
            query =
                query.filter(library_entries::mode.eq(StorageLibraryProjection::mode_sql(mode)));
        }
        if let StorageLibraryEnumFilter::Value(favorite) = c.favorite {
            query = query.filter(library_entries::favorite.eq(i32::from(favorite)));
        }
        if let Some(opening) = &c.opening {
            query = query.filter(library_instr(library_entries::opening_key, opening).gt(0));
        }
        if let StorageLibraryEnumFilter::Value(code) = &c.code {
            query = query.filter(library_entries::opening_code.eq(code.as_str()));
        }
        if let StorageLibraryEnumFilter::Value(plies) = c.plies {
            query = query.filter(
                library_entries::plies
                    .between(i32::try_from(plies.minimum)?, i32::try_from(plies.maximum)?),
            );
        }
        match &c.folder {
            StorageLibraryEnumFolderFilter::Any => {}
            StorageLibraryEnumFolderFilter::Unfiled => {
                query = query.filter(library_entries::folder_id.is_null());
            }
            StorageLibraryEnumFolderFilter::Exact(id) => {
                query = query.filter(library_entries::folder_id.eq(id.to_string()));
            }
            StorageLibraryEnumFolderFilter::Subtree(id) => {
                let subtree = Self::folder_subtree(connection, id.to_string())?;
                query = query.filter(library_entries::folder_id.eq_any(subtree));
            }
        }
        match &c.tags {
            StorageLibraryEnumTagFilter::Any => {}
            StorageLibraryEnumTagFilter::AnyOf(tags) => {
                let ids: Vec<String> = tags.ids().iter().map(ToString::to_string).collect();
                query = query.filter(
                    library_entries::game_id.eq_any(
                        library_entry_tags::table
                            .select(library_entry_tags::game_id)
                            .filter(library_entry_tags::tag_id.eq_any(ids)),
                    ),
                );
            }
            StorageLibraryEnumTagFilter::AllOf(tags) => {
                let ids: Vec<String> = tags.ids().iter().map(ToString::to_string).collect();
                let count = i64::try_from(ids.len())?;
                query = query.filter(
                    library_entries::game_id.eq_any(
                        library_entry_tags::table
                            .select(library_entry_tags::game_id)
                            .filter(library_entry_tags::tag_id.eq_any(ids))
                            .group_by(library_entry_tags::game_id)
                            .having(diesel::dsl::count_star().eq(count)),
                    ),
                );
            }
        }
        let sort = c.sort;
        if let Some(cursor) = &self.cursor {
            query = query.filter(Self::cursor_predicate(sort, &cursor.key));
        }
        query = query.order_by(sort.null_rank().asc());
        for index in 0..sort.value_count() {
            query = if sort.descending() {
                query.then_order_by(sort.value(index).desc())
            } else {
                query.then_order_by(sort.value(index).asc())
            };
        }
        let rows = query
            .then_order_by(library_entries::game_id.asc())
            .limit(i64::try_from(Self::PAGE_SIZE + 1)?)
            .load(connection)?;
        Ok(rows)
    }

    pub(crate) fn folder_subtree(
        connection: &mut SqliteConnection,
        root: String,
    ) -> Result<Vec<String>, StorageLibraryError> {
        let folders: Vec<(String, Option<String>)> = library_folders::table
            .select((library_folders::folder_id, library_folders::parent_id))
            .load(connection)?;
        let mut subtree = Vec::new();
        if folders.iter().any(|(id, _)| *id == root) {
            subtree.push(root);
        }
        let mut cursor = 0;
        while cursor < subtree.len() {
            let parent = subtree[cursor].clone();
            for (id, folder_parent) in &folders {
                if folder_parent.as_deref() == Some(parent.as_str()) && !subtree.contains(id) {
                    subtree.push(id.clone());
                }
            }
            cursor += 1;
        }
        Ok(subtree)
    }

    fn date_predicate(
        windows: &[StorageLibraryDateWindow],
    ) -> Result<StorageLibraryQueryNullableBool, StorageLibraryError> {
        let year = library_entries::date_year;
        let month = library_entries::date_month;
        let day = library_entries::date_day;
        let mut alternatives: StorageLibraryQueryNullableBool =
            Box::new(false.into_sql::<Bool>().nullable());
        for window in windows {
            let mut clause: StorageLibraryQueryNullableBool = Box::new(
                month
                    .is_null()
                    .nullable()
                    .or(month.eq(i32::from(window.month)))
                    .and(day.is_null().nullable().or(day.eq(i32::from(window.day))))
                    .and(
                        year.is_null()
                            .nullable()
                            .or(year.between(i32::from(window.first), i32::from(window.last))),
                    ),
            );
            if window.month == 2 && window.day == 29 {
                let divisible = |divisor: i32| (year / divisor * divisor).eq(year);
                clause =
                    Box::new(clause.and(year.is_null().nullable().or(
                        divisible(400).or(divisible(4).and(diesel::dsl::not(divisible(100)))),
                    )));
            }
            alternatives = Box::new(alternatives.or(clause));
        }
        Ok(Box::new(
            year.is_not_null()
                .or(month.is_not_null())
                .or(day.is_not_null())
                .nullable()
                .and(alternatives),
        ))
    }

    fn cursor_predicate(
        sort: StorageLibraryEnumSort,
        key: &StorageLibraryQueryKey,
    ) -> StorageLibraryQueryBool {
        let rank_bound = key.rank != 0;
        let mut alternatives: StorageLibraryQueryBool = Box::new(sort.null_rank().gt(rank_bound));
        for index in 0..=sort.value_count() {
            let mut prefix: StorageLibraryQueryBool = Box::new(sort.null_rank().eq(rank_bound));
            for earlier in 0..index {
                prefix = Box::new(prefix.and(sort.value(earlier).eq(key.values[earlier])));
            }
            let step: StorageLibraryQueryBool = if index == sort.value_count() {
                Box::new(library_entries::game_id.gt(key.id.as_uuid().to_string()))
            } else if sort.descending() {
                Box::new(sort.value(index).lt(key.values[index]))
            } else {
                Box::new(sort.value(index).gt(key.values[index]))
            };
            alternatives = Box::new(alternatives.or(prefix.and(step)));
        }
        alternatives
    }
}

pub struct StorageLibraryQueryEntry {
    id: GameId,
    content_revision: GameStorageRevision,
    projection: StorageLibraryProjection,
    organization: GameLibraryOrganizationState,
    modified: Option<GameLibraryOrganizationTimestamp>,
    location: StorageTrashEnumLocation,
    partial_date_match: bool,
}

pub(crate) struct StorageLibraryQueryEntryOptions {
    pub(crate) id: GameId,
    pub(crate) content_revision: GameStorageRevision,
    pub(crate) projection: StorageLibraryProjection,
    pub(crate) organization: GameLibraryOrganizationState,
    pub(crate) modified: Option<GameLibraryOrganizationTimestamp>,
    pub(crate) location: StorageTrashEnumLocation,
    pub(crate) date_filtered: bool,
}

impl StorageLibraryQueryEntry {
    pub(crate) fn new(options: StorageLibraryQueryEntryOptions) -> Self {
        let partial_date_match = options.date_filtered
            && options.projection.date().is_some_and(|date| {
                date.year().is_none() || date.month().is_none() || date.day().is_none()
            });
        Self {
            id: options.id,
            content_revision: options.content_revision,
            projection: options.projection,
            organization: options.organization,
            modified: options.modified,
            location: options.location,
            partial_date_match,
        }
    }
    pub const fn id(&self) -> GameId {
        self.id
    }
    pub const fn content_revision(&self) -> GameStorageRevision {
        self.content_revision
    }
    pub fn organization(&self) -> &GameLibraryOrganizationState {
        &self.organization
    }
    pub const fn modified(&self) -> Option<GameLibraryOrganizationTimestamp> {
        self.modified
    }
    pub const fn location(&self) -> StorageTrashEnumLocation {
        self.location
    }
    pub const fn is_partial_date_match(&self) -> bool {
        self.partial_date_match
    }
    pub fn white(&self) -> Option<&GameMetadataText> {
        self.projection.white()
    }
    pub fn black(&self) -> Option<&GameMetadataText> {
        self.projection.black()
    }
    pub fn date(&self) -> Option<GameMetadataDate> {
        self.projection.date()
    }
    pub fn opening(&self) -> Option<&GameMetadataText> {
        self.projection.opening()
    }
    pub fn opening_code(&self) -> Option<&GameMetadataOpeningCode> {
        self.projection.code()
    }
    pub fn result(&self) -> NotationGameIdentityEnumResult {
        self.projection.result()
    }
    pub fn rules_variant(&self) -> BoardEnumVariant {
        self.projection.variant()
    }
    pub fn start_kind(&self) -> StorageLibraryEnumStartKind {
        self.projection.start()
    }
    pub fn mode(&self) -> Option<GameEnumMode> {
        self.projection.mode()
    }
    pub fn plies(&self) -> u32 {
        self.projection.plies()
    }
}

pub struct StorageLibraryQueryPage {
    entries: Vec<StorageLibraryQueryEntry>,
    next: Option<StorageLibraryQueryCursor>,
    catalog: StorageLibraryCatalogRevision,
    unindexed_documents: u64,
    _memory: StorageLibraryMemory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageLibraryEnumIndexCoverage {
    EveryDocumentIndexed,
    UnindexedDocuments(u64),
}

impl StorageLibraryQueryPage {
    pub(crate) fn new(
        entries: Vec<StorageLibraryQueryEntry>,
        next: Option<StorageLibraryQueryCursor>,
        catalog: StorageLibraryCatalogRevision,
        unindexed_documents: u64,
        memory: StorageLibraryMemory,
    ) -> Self {
        Self {
            entries,
            next,
            catalog,
            unindexed_documents,
            _memory: memory,
        }
    }
    pub fn entries(&self) -> &[StorageLibraryQueryEntry] {
        &self.entries
    }
    pub fn next_cursor(&self) -> Option<&StorageLibraryQueryCursor> {
        self.next.as_ref()
    }
    pub const fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.catalog
    }
    pub const fn unindexed_document_count(&self) -> u64 {
        self.unindexed_documents
    }
    pub const fn index_coverage(&self) -> StorageLibraryEnumIndexCoverage {
        if self.unindexed_documents == 0 {
            StorageLibraryEnumIndexCoverage::EveryDocumentIndexed
        } else {
            StorageLibraryEnumIndexCoverage::UnindexedDocuments(self.unindexed_documents)
        }
    }
}
