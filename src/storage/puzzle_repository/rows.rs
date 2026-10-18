use diesel::prelude::*;
use diesel::sql_types::{Binary, Bool, Integer, Nullable, Text};

use crate::storage::schema::{
    puzzle_attempts, puzzle_checkpoints, puzzle_collections, puzzle_events, puzzle_runs,
};

#[derive(Insertable)]
#[diesel(table_name = puzzle_collections)]
pub(super) struct StoragePuzzleNewCollection<'a> {
    pub(super) collection_id: &'a str,
    pub(super) schema_version: i32,
    pub(super) digest: &'a [u8],
    pub(super) body: &'a str,
}

#[derive(Insertable)]
#[diesel(table_name = puzzle_runs)]
pub(super) struct StoragePuzzleNewRun<'a> {
    pub(super) run_id: &'a str,
    pub(super) collection_id: &'a str,
    pub(super) storage_revision: &'a str,
    pub(super) event_count: i32,
    pub(super) journal_version: i32,
    pub(super) journal_digest: &'a [u8],
}

#[derive(Insertable)]
#[diesel(table_name = puzzle_events)]
pub(super) struct StoragePuzzleNewEvent<'a> {
    pub(super) run_id: &'a str,
    pub(super) ordinal: i32,
    pub(super) body: &'a str,
}

#[derive(Insertable)]
#[diesel(table_name = puzzle_attempts)]
pub(super) struct StoragePuzzleNewAttempt<'a> {
    pub(super) run_id: &'a str,
    pub(super) attempt_id: &'a str,
    pub(super) body: &'a str,
}

#[derive(Insertable)]
#[diesel(table_name = puzzle_checkpoints)]
pub(super) struct StoragePuzzleNewCheckpoint<'a> {
    pub(super) run_id: &'a str,
    pub(super) body: &'a str,
}

#[derive(AsChangeset)]
#[diesel(table_name = puzzle_runs)]
pub(super) struct StoragePuzzleRunAdvance<'a> {
    pub(super) storage_revision: &'a str,
    pub(super) event_count: i32,
    pub(super) journal_digest: &'a [u8],
}

#[derive(Queryable)]
pub(super) struct StoragePuzzlePrefixRow {
    pub(super) revision: String,
    pub(super) count: i32,
    pub(super) digest: Vec<u8>,
    pub(super) schema: i32,
    pub(super) journal_version: i32,
    pub(super) journal_digest: Vec<u8>,
}

#[derive(QueryableByName)]
pub(super) struct StoragePuzzleGuardedRunRow {
    #[diesel(sql_type = Text)]
    pub(super) revision: String,
    #[diesel(sql_type = Integer)]
    pub(super) count: i32,
    #[diesel(sql_type = Binary)]
    pub(super) digest: Vec<u8>,
    #[diesel(sql_type = Integer)]
    pub(super) schema: i32,
    #[diesel(sql_type = Integer)]
    pub(super) journal_version: i32,
    #[diesel(sql_type = Binary)]
    pub(super) journal_digest: Vec<u8>,
    #[diesel(sql_type = Nullable<Text>)]
    pub(super) collection: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub(super) checkpoint: Option<String>,
}

#[derive(QueryableByName)]
pub(super) struct StoragePuzzleInventoryRow {
    #[diesel(sql_type = Nullable<Text>)]
    pub(super) run_id: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub(super) storage_revision: Option<String>,
    #[diesel(sql_type = Integer)]
    pub(super) event_count: i32,
    #[diesel(sql_type = Bool)]
    pub(super) has_collection: bool,
    #[diesel(sql_type = Bool)]
    pub(super) has_checkpoint: bool,
}

#[derive(QueryableByName)]
pub(super) struct StoragePuzzleGuardedAttemptRow {
    #[diesel(sql_type = Text)]
    pub(super) attempt_id: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub(super) body: Option<String>,
}
