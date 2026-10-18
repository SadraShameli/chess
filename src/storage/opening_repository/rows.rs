use diesel::prelude::*;

use crate::storage::schema::{opening_games, opening_occurrences, opening_positions};

#[derive(Insertable)]
#[diesel(table_name = opening_positions)]
pub(super) struct StorageOpeningNewPosition<'a> {
    pub(super) identity: &'a str,
}

#[derive(Insertable)]
#[diesel(table_name = opening_games)]
pub(super) struct StorageOpeningNewGame<'a> {
    pub(super) source: &'a str,
    pub(super) game_id: i32,
    pub(super) indexed_plies: i32,
}

#[derive(Insertable)]
#[diesel(table_name = opening_occurrences)]
pub(super) struct StorageOpeningNewOccurrence<'a> {
    pub(super) source: &'a str,
    pub(super) game_id: i32,
    pub(super) position_id: i32,
    pub(super) ply: i32,
    pub(super) next_move: Option<&'a str>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = opening_occurrences)]
pub(super) struct StorageOpeningOccurrenceRow {
    pub(super) source: String,
    pub(super) game_id: i32,
    pub(super) ply: i32,
    pub(super) next_move: Option<String>,
}
