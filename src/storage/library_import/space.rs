mod layout;
mod trees;

use diesel::SqliteConnection;
use diesel::prelude::*;
use diesel::sql_types::BigInt;

use crate::storage::import_session::{
    StorageImportSessionCancellation, StorageImportSessionCommitFootprint,
};
use crate::storage::library_import::space::layout::StorageLibraryImportSpaceLayout;
use crate::storage::library_import::space::trees::StorageLibraryImportSpaceTrees;
use crate::storage::library_repository::StorageLibraryError;

#[derive(QueryableByName)]
struct StorageLibraryImportSpaceFreelist {
    #[diesel(sql_type = BigInt)]
    value: i64,
}

pub(crate) struct StorageLibraryImportSpace {
    bytes: u64,
    page_ceiling: u64,
}

impl StorageLibraryImportSpace {
    const WAL_HEADER_BYTES: u64 = 32;
    const WAL_FRAME_HEADER_BYTES: u64 = 24;
    const SHM_BLOCK_BYTES: u64 = 32_768;
    const SHM_FIRST_FRAMES: u64 = 4062;
    const SHM_BLOCK_FRAMES: u64 = 4096;
    const MAX_SECTOR_BYTES: u64 = 65_536;
    const SQLITE_MAX_PAGES: u64 = 0xffff_fffe;
    const FREELIST_OPERATION_PAGES: u64 = 3;
    const HEADER_AND_CATALOG_PAGES: u64 = 2;

    pub(super) fn add(left: u64, right: u64) -> Result<u64, StorageLibraryError> {
        left.checked_add(right)
            .ok_or(StorageLibraryError::ImportLayout)
    }

    pub(super) fn mul(left: u64, right: u64) -> Result<u64, StorageLibraryError> {
        left.checked_mul(right)
            .ok_or(StorageLibraryError::ImportLayout)
    }

    pub(super) fn ceil(value: u64, unit: u64) -> Result<u64, StorageLibraryError> {
        Ok(Self::add(value, unit - 1)? / unit)
    }

    fn shm_bytes(frames: u64) -> Result<u64, StorageLibraryError> {
        Self::mul(
            Self::add(
                1,
                Self::ceil(
                    frames.saturating_sub(Self::SHM_FIRST_FRAMES),
                    Self::SHM_BLOCK_FRAMES,
                )?,
            )?,
            Self::SHM_BLOCK_BYTES,
        )
    }

    pub(crate) fn plan(
        connection: &mut SqliteConnection,
        footprint: StorageImportSessionCommitFootprint,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<Self, StorageLibraryError> {
        cancellation.check()?;
        let layout = StorageLibraryImportSpaceLayout::read(connection, cancellation)?;
        let budget = StorageLibraryImportSpaceTrees::plan_budget(
            connection,
            footprint,
            layout.page,
            cancellation,
        )?;
        let page_ceiling =
            Self::add(layout.pages, budget.new_page_budget)?.min(Self::SQLITE_MAX_PAGES);
        let additional_pages = page_ceiling.saturating_sub(layout.pages);
        cancellation.check()?;
        let free = u64::try_from(
            diesel::sql_query("SELECT freelist_count AS value FROM pragma_freelist_count")
                .get_result::<StorageLibraryImportSpaceFreelist>(connection)?
                .value,
        )?;
        let allocator_dirty = free.min(Self::mul(
            budget.allocator_operations,
            Self::FREELIST_OPERATION_PAGES,
        )?);
        let old_dirty = if layout.has_temporary_triggers {
            layout.pages
        } else {
            Self::add(
                Self::add(budget.old_tree_pages, allocator_dirty)?,
                Self::HEADER_AND_CATALOG_PAGES,
            )?
        };
        let dirty = Self::add(old_dirty, additional_pages)?;
        let frame_bytes = Self::add(layout.page, Self::WAL_FRAME_HEADER_BYTES)?;
        let padding = Self::ceil(Self::MAX_SECTOR_BYTES, frame_bytes)?;
        let frames = Self::add(Self::add(dirty, 1)?, padding)?;
        let wal_frames = Self::add(layout.wal_frames, frames)?;
        let wal_high_water =
            Self::add(Self::WAL_HEADER_BYTES, Self::mul(wal_frames, frame_bytes)?)?;
        let wal = wal_high_water.saturating_sub(layout.wal);
        let shm = Self::shm_bytes(wal_frames)?.saturating_sub(layout.shm);
        let main = Self::mul(page_ceiling, layout.page)?.saturating_sub(layout.main);
        let statement_journal = if layout.has_temporary_triggers {
            Self::mul(dirty, Self::add(layout.page, 4)?)?
        } else {
            0
        };
        cancellation.check()?;
        Ok(Self {
            bytes: Self::add(Self::add(main, wal)?, Self::add(shm, statement_journal)?)?,
            page_ceiling,
        })
    }

    pub(crate) const fn bytes(&self) -> u64 {
        self.bytes
    }

    pub(crate) const fn page_ceiling(&self) -> u64 {
        self.page_ceiling
    }
}

#[cfg(test)]
#[path = "../../../tests/storage/library_import_journal.rs"]
mod journal_tests;
