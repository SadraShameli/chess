pub mod board;
pub mod capacity;
pub mod clock;
pub mod desktop;
pub mod engine;
pub mod game;
pub mod notation;
pub mod package;
pub mod presentation;
pub mod puzzle;
pub mod scene;
pub mod storage;
pub mod workbench;

#[cfg(test)]
#[path = "../tests/game/storage_wall_time_fixture.rs"]
mod game_storage_wall_time_fixture;

#[cfg(test)]
#[path = "../tests/storage/document_write_fixture.rs"]
mod storage_document_write_fixture;
