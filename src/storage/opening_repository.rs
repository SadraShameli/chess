use std::collections::HashSet;
use std::num::TryFromIntError;

use diesel::SqliteConnection;
use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel::result::DatabaseErrorKind;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::storage::dataset_package::StorageDatasetPackageSourceIdentity;
use crate::storage::document_store::{StorageDocumentClient, StorageDocumentError};
use crate::storage::schema::{opening_games, opening_occurrences, opening_positions};

mod rows;

use rows::{
    StorageOpeningNewGame, StorageOpeningNewOccurrence, StorageOpeningNewPosition,
    StorageOpeningOccurrenceRow,
};

#[derive(Debug, Error)]
pub enum StorageOpeningError {
    #[error(transparent)]
    Document(#[from] StorageDocumentError),
    #[error(transparent)]
    Diesel(#[from] diesel::result::Error),
    #[error("The opening database reply channel closed: {0}")]
    Channel(#[from] oneshot::error::RecvError),
    #[error("The opening database value exceeds its supported range: {0}")]
    Range(#[from] TryFromIntError),
    #[error("An opening source key needs between 1 and {0} printable bytes")]
    SourceKey(usize),
    #[error("An opening position key needs between 1 and {0} printable ASCII bytes")]
    PositionKey(usize),
    #[error("An opening move needs four or five ASCII bytes")]
    MoveText,
    #[error("An opening game identifier must be between 1 and {0}")]
    GameId(u64),
    #[error("An indexed opening depth must be between {minimum} and {maximum} plies")]
    IndexedPlies { minimum: u16, maximum: u16 },
    #[error("Opening occurrence ply {ply} is beyond the indexed depth {indexed}")]
    OccurrencePly { ply: u16, indexed: u16 },
    #[error("A game repeats an opening position and may index only its first occurrence")]
    RepeatedPosition,
    #[error("The opening game already holds an occurrence for the position")]
    DuplicateOccurrence,
    #[error("The opening game is indexed to {recorded} plies and cannot shrink to {requested}")]
    DepthRegression { recorded: u16, requested: u16 },
    #[error("The opening index write needs {required} bytes but a request may carry {limit}")]
    RequestTooLarge { required: usize, limit: usize },
    #[error("The stored opening rows are inconsistent")]
    Corrupt,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StorageOpeningSource(String);

impl StorageOpeningSource {
    pub const MAX_BYTES: usize = 4096;

    pub fn new(value: String) -> Result<Self, StorageOpeningError> {
        if value.is_empty()
            || value.len() > Self::MAX_BYTES
            || value.chars().any(|character| character == '\u{0}')
        {
            return Err(StorageOpeningError::SourceKey(Self::MAX_BYTES));
        }
        Ok(Self(value))
    }

    pub fn from_identity(
        identity: &StorageDatasetPackageSourceIdentity,
    ) -> Result<Self, StorageOpeningError> {
        Self::new(format!(
            "{}\n{}\n{}",
            identity.source(),
            identity.version(),
            identity.manifest_digest()
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StorageOpeningPositionKey(String);

impl StorageOpeningPositionKey {
    pub const MAX_BYTES: usize = 1024;

    pub fn new(value: String) -> Result<Self, StorageOpeningError> {
        if value.is_empty()
            || value.len() > Self::MAX_BYTES
            || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(StorageOpeningError::PositionKey(Self::MAX_BYTES));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageOpeningOccurrence {
    key: StorageOpeningPositionKey,
    ply: u16,
    next_move: Option<String>,
}

impl StorageOpeningOccurrence {
    pub fn new(
        key: StorageOpeningPositionKey,
        ply: u16,
        next_move: Option<String>,
    ) -> Result<Self, StorageOpeningError> {
        if ply > StorageOpeningGameIndex::MAX_PLIES {
            return Err(StorageOpeningError::OccurrencePly {
                ply,
                indexed: StorageOpeningGameIndex::MAX_PLIES,
            });
        }
        if let Some(movement) = &next_move
            && (!movement.is_ascii() || !(4..=5).contains(&movement.len()))
        {
            return Err(StorageOpeningError::MoveText);
        }
        Ok(Self {
            key,
            ply,
            next_move,
        })
    }

    pub const fn key(&self) -> &StorageOpeningPositionKey {
        &self.key
    }

    pub const fn ply(&self) -> u16 {
        self.ply
    }

    pub fn next_move(&self) -> Option<&str> {
        self.next_move.as_deref()
    }

    pub(crate) fn transport_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.key.0.len()
            + self.next_move.as_ref().map_or(0, String::len)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageOpeningGameIndex {
    source: StorageOpeningSource,
    game: u64,
    indexed_plies: u16,
    occurrences: Vec<StorageOpeningOccurrence>,
}

impl StorageOpeningGameIndex {
    pub const MIN_PLIES: u16 = 40;
    pub const MAX_PLIES: u16 = 120;
    pub const MAX_GAME_ID: u64 = 0x7fff_ffff;

    pub fn new(
        source: StorageOpeningSource,
        game: u64,
        indexed_plies: u16,
        occurrences: Vec<StorageOpeningOccurrence>,
    ) -> Result<Self, StorageOpeningError> {
        if game == 0 || game > Self::MAX_GAME_ID {
            return Err(StorageOpeningError::GameId(Self::MAX_GAME_ID));
        }
        if !(Self::MIN_PLIES..=Self::MAX_PLIES).contains(&indexed_plies) {
            return Err(StorageOpeningError::IndexedPlies {
                minimum: Self::MIN_PLIES,
                maximum: Self::MAX_PLIES,
            });
        }
        let mut keys = HashSet::with_capacity(occurrences.len());
        for occurrence in &occurrences {
            if occurrence.ply > indexed_plies {
                return Err(StorageOpeningError::OccurrencePly {
                    ply: occurrence.ply,
                    indexed: indexed_plies,
                });
            }
            if !keys.insert(occurrence.key.as_str()) {
                return Err(StorageOpeningError::RepeatedPosition);
            }
        }
        Ok(Self {
            source,
            game,
            indexed_plies,
            occurrences,
        })
    }

    pub const fn source(&self) -> &StorageOpeningSource {
        &self.source
    }

    pub const fn game(&self) -> u64 {
        self.game
    }

    pub const fn indexed_plies(&self) -> u16 {
        self.indexed_plies
    }

    pub fn occurrences(&self) -> &[StorageOpeningOccurrence] {
        &self.occurrences
    }

    pub fn transport_bytes(&self) -> usize {
        self.occurrences.iter().fold(
            std::mem::size_of::<Self>() + self.source.0.len(),
            |total, occurrence| total.saturating_add(occurrence.transport_bytes()),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StorageOpeningCursor {
    source: StorageOpeningSource,
    game: u64,
}

impl StorageOpeningCursor {
    pub const fn new(source: StorageOpeningSource, game: u64) -> Self {
        Self { source, game }
    }

    pub const fn source(&self) -> &StorageOpeningSource {
        &self.source
    }

    pub const fn game(&self) -> u64 {
        self.game
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageOpeningHit {
    source: StorageOpeningSource,
    game: u64,
    ply: u16,
    next_move: Option<String>,
}

impl StorageOpeningHit {
    pub const fn source(&self) -> &StorageOpeningSource {
        &self.source
    }

    pub const fn game(&self) -> u64 {
        self.game
    }

    pub const fn ply(&self) -> u16 {
        self.ply
    }

    pub fn next_move(&self) -> Option<&str> {
        self.next_move.as_deref()
    }

    pub fn cursor(&self) -> StorageOpeningCursor {
        StorageOpeningCursor::new(self.source.clone(), self.game)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageOpeningContinuation {
    next_move: Option<String>,
    games: u64,
}

impl StorageOpeningContinuation {
    pub fn next_move(&self) -> Option<&str> {
        self.next_move.as_deref()
    }

    pub const fn games(&self) -> u64 {
        self.games
    }
}

#[derive(Debug)]
pub(crate) enum StorageOpeningEnumRequest {
    Index(Vec<StorageOpeningGameIndex>),
    Depth {
        source: StorageOpeningSource,
        game: u64,
    },
    Hits {
        key: StorageOpeningPositionKey,
        after: Option<StorageOpeningCursor>,
    },
    Continuations(StorageOpeningPositionKey),
    Remove(StorageOpeningSource),
}

#[derive(Debug)]
pub(crate) enum StorageOpeningEnumResponse {
    Indexed(usize),
    Depth(Option<u16>),
    Hits(Vec<StorageOpeningHit>),
    Continuations(Vec<StorageOpeningContinuation>),
    Removed(usize),
}

pub(crate) struct StorageOpeningTransaction;

impl StorageOpeningTransaction {
    pub(crate) const PAGE_SIZE: i64 = 100;

    pub(crate) fn execute(
        connection: &mut SqliteConnection,
        request: StorageOpeningEnumRequest,
    ) -> Result<StorageOpeningEnumResponse, StorageOpeningError> {
        match request {
            StorageOpeningEnumRequest::Depth { source, game } => {
                connection.transaction(|connection| {
                    Self::depth(connection, &source, game).map(StorageOpeningEnumResponse::Depth)
                })
            }
            StorageOpeningEnumRequest::Hits { key, after } => {
                connection.transaction(|connection| {
                    Self::hits(connection, &key, after.as_ref())
                        .map(StorageOpeningEnumResponse::Hits)
                })
            }
            StorageOpeningEnumRequest::Continuations(key) => connection.transaction(|connection| {
                Self::continuations(connection, &key).map(StorageOpeningEnumResponse::Continuations)
            }),
            StorageOpeningEnumRequest::Index(games) => {
                connection.immediate_transaction(|connection| {
                    for game in &games {
                        Self::index_game(connection, game)?;
                    }
                    Ok(StorageOpeningEnumResponse::Indexed(games.len()))
                })
            }
            StorageOpeningEnumRequest::Remove(source) => {
                connection.immediate_transaction(|connection| {
                    Self::remove(connection, &source).map(StorageOpeningEnumResponse::Removed)
                })
            }
        }
    }

    fn depth(
        connection: &mut SqliteConnection,
        source: &StorageOpeningSource,
        game: u64,
    ) -> Result<Option<u16>, StorageOpeningError> {
        let recorded: Option<i32> = opening_games::table
            .filter(opening_games::source.eq(source.as_str()))
            .filter(opening_games::game_id.eq(i32::try_from(game)?))
            .select(opening_games::indexed_plies)
            .first(connection)
            .optional()?;
        recorded
            .map(|plies| u16::try_from(plies).map_err(StorageOpeningError::from))
            .transpose()
    }

    fn position(
        connection: &mut SqliteConnection,
        key: &StorageOpeningPositionKey,
    ) -> Result<Option<i32>, StorageOpeningError> {
        Ok(opening_positions::table
            .filter(opening_positions::identity.eq(key.as_str()))
            .select(opening_positions::position_id)
            .first(connection)
            .optional()?)
    }

    fn hits(
        connection: &mut SqliteConnection,
        key: &StorageOpeningPositionKey,
        after: Option<&StorageOpeningCursor>,
    ) -> Result<Vec<StorageOpeningHit>, StorageOpeningError> {
        let Some(position) = Self::position(connection, key)? else {
            return Ok(Vec::new());
        };
        let mut query = opening_occurrences::table
            .filter(opening_occurrences::position_id.eq(position))
            .order((
                opening_occurrences::source.asc(),
                opening_occurrences::game_id.asc(),
            ))
            .limit(Self::PAGE_SIZE)
            .select(StorageOpeningOccurrenceRow::as_select())
            .into_boxed();
        if let Some(cursor) = after {
            let game = i32::try_from(cursor.game)?;
            query = query.filter(
                opening_occurrences::source
                    .gt(cursor.source.as_str().to_owned())
                    .or(opening_occurrences::source
                        .eq(cursor.source.as_str().to_owned())
                        .and(opening_occurrences::game_id.gt(game))),
            );
        }
        let rows: Vec<StorageOpeningOccurrenceRow> = query.load(connection)?;
        let mut hits = Vec::with_capacity(rows.len());
        for row in rows {
            hits.push(StorageOpeningHit {
                source: StorageOpeningSource::new(row.source)?,
                game: u64::try_from(row.game_id)?,
                ply: u16::try_from(row.ply)?,
                next_move: row.next_move,
            });
        }
        Ok(hits)
    }

    fn continuations(
        connection: &mut SqliteConnection,
        key: &StorageOpeningPositionKey,
    ) -> Result<Vec<StorageOpeningContinuation>, StorageOpeningError> {
        let Some(position) = Self::position(connection, key)? else {
            return Ok(Vec::new());
        };
        let rows: Vec<(Option<String>, i64)> = opening_occurrences::table
            .filter(opening_occurrences::position_id.eq(position))
            .group_by(opening_occurrences::next_move)
            .select((opening_occurrences::next_move, count_star()))
            .order(opening_occurrences::next_move.asc())
            .load(connection)?;
        let mut continuations = Vec::with_capacity(rows.len());
        for (next_move, games) in rows {
            continuations.push(StorageOpeningContinuation {
                next_move,
                games: u64::try_from(games)?,
            });
        }
        Ok(continuations)
    }

    fn index_game(
        connection: &mut SqliteConnection,
        game: &StorageOpeningGameIndex,
    ) -> Result<(), StorageOpeningError> {
        let id = i32::try_from(game.game)?;
        let source = game.source.as_str();
        let recorded: Option<i32> = opening_games::table
            .filter(opening_games::source.eq(source))
            .filter(opening_games::game_id.eq(id))
            .select(opening_games::indexed_plies)
            .first(connection)
            .optional()?;
        let requested = i32::from(game.indexed_plies);
        match recorded {
            Some(recorded) if recorded > requested => {
                return Err(StorageOpeningError::DepthRegression {
                    recorded: u16::try_from(recorded)?,
                    requested: game.indexed_plies,
                });
            }
            Some(_) => {
                diesel::update(
                    opening_games::table
                        .filter(opening_games::source.eq(source))
                        .filter(opening_games::game_id.eq(id)),
                )
                .set(opening_games::indexed_plies.eq(requested))
                .execute(connection)?;
            }
            None => {
                diesel::insert_into(opening_games::table)
                    .values(StorageOpeningNewGame {
                        source,
                        game_id: id,
                        indexed_plies: requested,
                    })
                    .execute(connection)?;
            }
        }
        if game.occurrences.is_empty() {
            return Ok(());
        }
        let identities: Vec<&str> = game
            .occurrences
            .iter()
            .map(|occurrence| occurrence.key.as_str())
            .collect();
        let positions: Vec<StorageOpeningNewPosition<'_>> = identities
            .iter()
            .map(|identity| StorageOpeningNewPosition { identity })
            .collect();
        diesel::insert_or_ignore_into(opening_positions::table)
            .values(&positions)
            .execute(connection)?;
        let stored: Vec<(i32, String)> = opening_positions::table
            .filter(opening_positions::identity.eq_any(&identities))
            .select((opening_positions::position_id, opening_positions::identity))
            .load(connection)?;
        let mut rows = Vec::with_capacity(game.occurrences.len());
        for occurrence in &game.occurrences {
            let position_id = stored
                .iter()
                .find(|(_, identity)| identity == occurrence.key.as_str())
                .map(|(position_id, _)| *position_id)
                .ok_or(StorageOpeningError::Corrupt)?;
            rows.push(StorageOpeningNewOccurrence {
                source,
                game_id: id,
                position_id,
                ply: i32::from(occurrence.ply),
                next_move: occurrence.next_move.as_deref(),
            });
        }
        diesel::insert_into(opening_occurrences::table)
            .values(&rows)
            .execute(connection)
            .map_err(|error| {
                if matches!(
                    error,
                    diesel::result::Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _)
                ) {
                    StorageOpeningError::DuplicateOccurrence
                } else {
                    StorageOpeningError::from(error)
                }
            })?;
        Ok(())
    }

    fn remove(
        connection: &mut SqliteConnection,
        source: &StorageOpeningSource,
    ) -> Result<usize, StorageOpeningError> {
        let removed =
            diesel::delete(opening_games::table.filter(opening_games::source.eq(source.as_str())))
                .execute(connection)?;
        diesel::delete(
            opening_positions::table.filter(
                opening_positions::position_id
                    .ne_all(opening_occurrences::table.select(opening_occurrences::position_id)),
            ),
        )
        .execute(connection)?;
        Ok(removed)
    }
}

pub struct StorageOpeningRepository {
    client: StorageDocumentClient,
}

impl StorageOpeningRepository {
    pub const REQUEST_BYTES: usize = 16 * 1024 * 1024;

    pub const fn new(client: StorageDocumentClient) -> Self {
        Self { client }
    }

    pub async fn index(
        &self,
        games: Vec<StorageOpeningGameIndex>,
    ) -> Result<usize, StorageOpeningError> {
        let mut indexed = 0;
        let mut chunk = Vec::new();
        let mut bytes = 0_usize;
        for game in games {
            let required = game.transport_bytes();
            if required > Self::REQUEST_BYTES {
                return Err(StorageOpeningError::RequestTooLarge {
                    required,
                    limit: Self::REQUEST_BYTES,
                });
            }
            if bytes.saturating_add(required) > Self::REQUEST_BYTES {
                indexed += self.write(std::mem::take(&mut chunk), bytes).await?;
                bytes = 0;
            }
            bytes += required;
            chunk.push(game);
        }
        if !chunk.is_empty() {
            indexed += self.write(chunk, bytes).await?;
        }
        Ok(indexed)
    }

    async fn write(
        &self,
        games: Vec<StorageOpeningGameIndex>,
        bytes: usize,
    ) -> Result<usize, StorageOpeningError> {
        match self
            .client
            .opening(StorageOpeningEnumRequest::Index(games), bytes)
            .await?
        {
            StorageOpeningEnumResponse::Indexed(count) => Ok(count),
            StorageOpeningEnumResponse::Depth(_)
            | StorageOpeningEnumResponse::Hits(_)
            | StorageOpeningEnumResponse::Continuations(_)
            | StorageOpeningEnumResponse::Removed(_) => Err(StorageOpeningError::Corrupt),
        }
    }

    pub async fn indexed_plies(
        &self,
        source: StorageOpeningSource,
        game: u64,
    ) -> Result<Option<u16>, StorageOpeningError> {
        match self
            .client
            .opening(StorageOpeningEnumRequest::Depth { source, game }, 0)
            .await?
        {
            StorageOpeningEnumResponse::Depth(depth) => Ok(depth),
            StorageOpeningEnumResponse::Indexed(_)
            | StorageOpeningEnumResponse::Hits(_)
            | StorageOpeningEnumResponse::Continuations(_)
            | StorageOpeningEnumResponse::Removed(_) => Err(StorageOpeningError::Corrupt),
        }
    }

    pub async fn hits(
        &self,
        key: StorageOpeningPositionKey,
        after: Option<StorageOpeningCursor>,
    ) -> Result<Vec<StorageOpeningHit>, StorageOpeningError> {
        match self
            .client
            .opening(StorageOpeningEnumRequest::Hits { key, after }, 0)
            .await?
        {
            StorageOpeningEnumResponse::Hits(hits) => Ok(hits),
            StorageOpeningEnumResponse::Indexed(_)
            | StorageOpeningEnumResponse::Depth(_)
            | StorageOpeningEnumResponse::Continuations(_)
            | StorageOpeningEnumResponse::Removed(_) => Err(StorageOpeningError::Corrupt),
        }
    }

    pub async fn continuations(
        &self,
        key: StorageOpeningPositionKey,
    ) -> Result<Vec<StorageOpeningContinuation>, StorageOpeningError> {
        match self
            .client
            .opening(StorageOpeningEnumRequest::Continuations(key), 0)
            .await?
        {
            StorageOpeningEnumResponse::Continuations(continuations) => Ok(continuations),
            StorageOpeningEnumResponse::Indexed(_)
            | StorageOpeningEnumResponse::Depth(_)
            | StorageOpeningEnumResponse::Hits(_)
            | StorageOpeningEnumResponse::Removed(_) => Err(StorageOpeningError::Corrupt),
        }
    }

    pub async fn remove_source(
        &self,
        source: StorageOpeningSource,
    ) -> Result<usize, StorageOpeningError> {
        match self
            .client
            .opening(StorageOpeningEnumRequest::Remove(source), 0)
            .await?
        {
            StorageOpeningEnumResponse::Removed(count) => Ok(count),
            StorageOpeningEnumResponse::Indexed(_)
            | StorageOpeningEnumResponse::Depth(_)
            | StorageOpeningEnumResponse::Hits(_)
            | StorageOpeningEnumResponse::Continuations(_) => Err(StorageOpeningError::Corrupt),
        }
    }
}
