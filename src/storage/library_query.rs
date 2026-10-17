pub(crate) mod folders;

use diesel::dsl::{count_star, exists, sql};
use diesel::sql_types::{BigInt, Bool, Nullable, Text};
use diesel::sqlite::Sqlite;
use diesel::{
    BoolExpressionMethods, BoxableExpression, ExpressionMethods, NullableExpressionMethods,
    QueryDsl,
};

use crate::storage::library_query::folders::StorageLibraryQueryFolders;
use crate::storage::schema::{library_entries, library_entry_tags};

type StorageLibraryQueryPredicate =
    Box<dyn BoxableExpression<library_entries::table, Sqlite, SqlType = Bool>>;
type StorageLibraryQueryDatePredicate =
    Box<dyn BoxableExpression<library_entries::table, Sqlite, SqlType = Nullable<Bool>>>;

diesel::define_sql_function! {
    #[sql_name = "instr"]
    fn storage_library_query_instr(value: Nullable<Text>, pattern: Text) -> Nullable<BigInt>;
}

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
use crate::storage::trash::StorageTrashEnumLocation;

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
    fn expressions(self) -> &'static [&'static str] {
        match self {
            Self::UpdatedDescending | Self::UpdatedAscending => {
                &["COALESCE(library_entries.modified_ms, 0)"]
            }
            Self::DateDescending | Self::DateAscending => &[
                "COALESCE(library_entries.date_year, 0)",
                "COALESCE(library_entries.date_month, 0)",
                "COALESCE(library_entries.date_day, 0)",
            ],
            Self::PliesDescending | Self::PliesAscending => &["library_entries.plies"],
        }
    }
    fn null_rank(self) -> &'static str {
        match self {
            Self::UpdatedDescending | Self::UpdatedAscending => {
                "(library_entries.modified_ms IS NULL)"
            }
            Self::DateDescending | Self::DateAscending => {
                "(library_entries.date_year IS NULL OR library_entries.date_month IS NULL OR library_entries.date_day IS NULL)"
            }
            Self::PliesDescending | Self::PliesAscending => "(library_entries.plies IS NULL)",
        }
    }
    fn key_prefix(
        self,
        key: &StorageLibraryQueryKey,
        count: usize,
    ) -> Result<StorageLibraryQueryPredicate, StorageLibraryError> {
        let mut predicate: StorageLibraryQueryPredicate =
            Box::new(sql::<BigInt>(self.null_rank()).eq(key.rank));
        for (index, expression) in self.expressions().iter().take(count).enumerate() {
            let value = *key
                .values
                .get(index)
                .ok_or(StorageLibraryError::CursorQuery)?;
            predicate = Box::new(predicate.and(sql::<BigInt>(expression).eq(value)));
        }
        Ok(predicate)
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

#[derive(Debug, Clone, Default)]
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
    pub(crate) fn selection(
        &self,
        catalog: StorageLibraryCatalogRevision,
    ) -> Result<library_entries::BoxedQuery<'_, Sqlite>, StorageLibraryError> {
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
            .filter(library_entries::location.eq(c.location.sql()))
            .into_boxed::<Sqlite>();
        if let Some(key) = &c.white {
            query = query.filter(
                storage_library_query_instr(library_entries::white_key, key.as_str()).gt(0_i64),
            );
        }
        if let Some(key) = &c.black {
            query = query.filter(
                storage_library_query_instr(library_entries::black_key, key.as_str()).gt(0_i64),
            );
        }
        if let Some(key) = &c.either {
            query = query.filter(
                storage_library_query_instr(library_entries::white_key, key.as_str())
                    .gt(0_i64)
                    .or(
                        storage_library_query_instr(library_entries::black_key, key.as_str())
                            .gt(0_i64),
                    ),
            );
        }
        if let StorageLibraryEnumFilter::Value(_) = c.date {
            let windows = self
                .windows
                .as_ref()
                .ok_or(StorageLibraryError::UnpreparedQuery)?;
            let mut alternatives: StorageLibraryQueryDatePredicate =
                Box::new(sql::<Bool>("0").nullable());
            for window in windows {
                let mut clause: StorageLibraryQueryDatePredicate = Box::new(
                    library_entries::date_month
                        .is_null()
                        .or(library_entries::date_month.eq(i64::from(window.month)))
                        .and(
                            library_entries::date_day
                                .is_null()
                                .or(library_entries::date_day.eq(i64::from(window.day))),
                        )
                        .and(
                            library_entries::date_year
                                .is_null()
                                .or(library_entries::date_year
                                    .between(i64::from(window.first), i64::from(window.last))),
                        ),
                );
                if window.month == 2 && window.day == 29 {
                    clause = Box::new(clause.and(library_entries::date_year.is_null().or(
                        sql::<Bool>("(library_entries.date_year % 400 = 0 OR (library_entries.date_year % 4 = 0 AND library_entries.date_year % 100 <> 0))").nullable(),
                    )));
                }
                alternatives = Box::new(alternatives.or(clause));
            }
            query = query.filter(
                library_entries::date_year
                    .is_not_null()
                    .or(library_entries::date_month.is_not_null())
                    .or(library_entries::date_day.is_not_null())
                    .and(alternatives),
            );
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
            query = query.filter(library_entries::favorite.eq(i64::from(favorite)));
        }
        if let Some(opening) = &c.opening {
            query = query.filter(
                storage_library_query_instr(library_entries::opening_key, opening.as_str())
                    .gt(0_i64),
            );
        }
        if let StorageLibraryEnumFilter::Value(code) = &c.code {
            query = query.filter(library_entries::opening_code.eq(code.as_str()));
        }
        if let StorageLibraryEnumFilter::Value(plies) = c.plies {
            query = query.filter(
                library_entries::plies.between(i64::from(plies.minimum), i64::from(plies.maximum)),
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
                query = query.filter(StorageLibraryQueryFolders::predicate(*id));
            }
        }
        match &c.tags {
            StorageLibraryEnumTagFilter::Any => {}
            StorageLibraryEnumTagFilter::AnyOf(tags) | StorageLibraryEnumTagFilter::AllOf(tags) => {
                let keys: Vec<String> = tags.ids().iter().map(ToString::to_string).collect();
                let matched = library_entry_tags::table
                    .filter(library_entry_tags::game_id.eq(library_entries::game_id))
                    .filter(library_entry_tags::tag_id.eq_any(keys));
                match &c.tags {
                    StorageLibraryEnumTagFilter::AnyOf(_) => query = query.filter(exists(matched)),
                    StorageLibraryEnumTagFilter::AllOf(tags) => {
                        query = query.filter(
                            matched
                                .select(count_star())
                                .single_value()
                                .eq(Some(i64::try_from(tags.ids().len())?)),
                        );
                    }
                    StorageLibraryEnumTagFilter::Any => return Err(StorageLibraryError::Protocol),
                }
            }
        }
        let sort = c.sort;
        let rank = sort.null_rank();
        let expressions = sort.expressions();
        query = query.order(sql::<BigInt>(rank).asc());
        for expression in expressions {
            query = if sort.descending() {
                query.then_order_by(sql::<BigInt>(expression).desc())
            } else {
                query.then_order_by(sql::<BigInt>(expression).asc())
            };
        }
        query = query.then_order_by(library_entries::game_id.asc());
        if let Some(cursor) = &self.cursor {
            let mut alternatives: StorageLibraryQueryPredicate =
                Box::new(sql::<BigInt>(rank).gt(cursor.key.rank));
            for (index, expression) in expressions.iter().enumerate() {
                let value = *cursor
                    .key
                    .values
                    .get(index)
                    .ok_or(StorageLibraryError::CursorQuery)?;
                let comparison: StorageLibraryQueryPredicate = if sort.descending() {
                    Box::new(sql::<BigInt>(expression).lt(value))
                } else {
                    Box::new(sql::<BigInt>(expression).gt(value))
                };
                alternatives =
                    Box::new(alternatives.or(sort.key_prefix(&cursor.key, index)?.and(comparison)));
            }
            let equal = sort.key_prefix(&cursor.key, expressions.len())?;
            alternatives =
                Box::new(alternatives.or(
                    equal.and(library_entries::game_id.gt(cursor.key.id.as_uuid().to_string())),
                ));
            query = query.filter(alternatives);
        }
        Ok(query.limit(i64::try_from(Self::PAGE_SIZE + 1)?))
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
