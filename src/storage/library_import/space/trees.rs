use rusqlite::Transaction;

use crate::storage::import_session::{
    StorageImportSessionCancellation, StorageImportSessionCommitFootprint,
    StorageImportSessionError,
};
use crate::storage::library_import::space::StorageLibraryImportSpace;
use crate::storage::library_repository::StorageLibraryError;

pub(super) struct StorageLibraryImportSpaceTrees {
    pub(super) new_page_budget: u64,
    pub(super) old_tree_pages: u64,
    pub(super) allocator_operations: u64,
    #[cfg(test)]
    inventory_vm_steps: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorageLibraryImportSpaceEnumCardinality {
    Exact(u64),
    AtLeast(u64),
}

struct StorageLibraryImportSpaceProbe {
    cardinality: StorageLibraryImportSpaceEnumCardinality,
    #[cfg(test)]
    inventory_vm_steps: u64,
}

#[derive(Clone, Copy)]
enum StorageLibraryImportSpaceEnumTable {
    Documents,
    Entries,
}

impl StorageLibraryImportSpaceEnumTable {
    #[cfg(test)]
    const ALL: [Self; 2] = [Self::Documents, Self::Entries];

    const fn probe_sql(self) -> &'static str {
        match self {
            Self::Documents => "SELECT id FROM documents ORDER BY id LIMIT ?1",
            Self::Entries => "SELECT game_id FROM library_entries ORDER BY game_id LIMIT ?1",
        }
    }

    fn from_name(name: &str) -> Result<Self, StorageLibraryError> {
        match name {
            "documents" => Ok(Self::Documents),
            "library_entries" => Ok(Self::Entries),
            _ => Err(StorageLibraryError::ImportLayout),
        }
    }

    fn probe(
        self,
        transaction: &Transaction<'_>,
        limit: u64,
        check: &mut impl FnMut() -> Result<(), StorageImportSessionError>,
    ) -> Result<StorageLibraryImportSpaceProbe, StorageLibraryError> {
        check()?;
        if limit == 0 {
            return Err(StorageLibraryError::ImportLayout);
        }
        let mut statement = transaction.prepare(self.probe_sql())?;
        check()?;
        let mut rows = statement.query([i64::try_from(limit)?])?;
        let mut records = 0_u64;
        let cardinality = loop {
            check()?;
            let Some(row) = rows.next()? else {
                break StorageLibraryImportSpaceEnumCardinality::Exact(records);
            };
            let _id = row.get_ref(0)?.as_str()?;
            records = StorageLibraryImportSpace::add(records, 1)?;
            check()?;
            if records == limit {
                break StorageLibraryImportSpaceEnumCardinality::AtLeast(limit);
            }
        };
        drop(rows);
        Ok(StorageLibraryImportSpaceProbe {
            cardinality,
            #[cfg(test)]
            inventory_vm_steps: u64::try_from(
                statement.get_status(rusqlite::StatementStatus::VmStep),
            )?,
        })
    }
}

struct StorageLibraryImportSpacePayload {
    total: u64,
    maximum: u64,
    kind: StorageLibraryImportSpaceEnumTreeKind,
}

#[derive(Clone, Copy)]
enum StorageLibraryImportSpaceEnumTreeKind {
    RowidTable,
    Index,
}

impl StorageLibraryImportSpaceEnumTreeKind {
    const fn max_local(self, page: u64) -> u64 {
        match self {
            Self::RowidTable => page - 35,
            Self::Index => (page - 12) * 64 / 255 - 23,
        }
    }

    fn structural_pages(self, records: u64) -> Result<u64, StorageLibraryError> {
        let nonempty = match self {
            Self::RowidTable => StorageLibraryImportSpace::mul(records, 2)?,
            Self::Index => records,
        };
        StorageLibraryImportSpace::add(nonempty, 1)
    }
}

impl StorageLibraryImportSpacePayload {
    fn new(
        variable_total: u64,
        variable_maximum: u64,
        fixed: u64,
        count: u64,
        kind: StorageLibraryImportSpaceEnumTreeKind,
    ) -> Result<Self, StorageLibraryError> {
        Ok(Self {
            total: StorageLibraryImportSpace::add(
                variable_total,
                StorageLibraryImportSpace::mul(fixed, count)?,
            )?,
            maximum: StorageLibraryImportSpace::add(variable_maximum, fixed)?,
            kind,
        })
    }

    fn overflow(&self, page: u64, count: u64) -> Result<u64, StorageLibraryError> {
        let max_local = self.kind.max_local(page);
        if self.maximum <= max_local {
            return Ok(0);
        }
        StorageLibraryImportSpace::add(
            self.total / (page - 4),
            count.min(self.total / (max_local + 1)),
        )
    }
}

impl StorageLibraryImportSpaceTrees {
    const MAX_DEPTH: u64 = 20;
    const SERIAL_BYTES: u64 = 9;
    const UUID_BYTES: u64 = 36;
    const LOCATION_BYTES: u64 = 7;
    const EXTRA_SIBLINGS: u64 = 2;
    const OLD_SIBLINGS: u64 = 3;
    const ROOT_CHILD_PAGES: u64 = 1;
    const DOCUMENT_FIXED: u64 = 4 * Self::SERIAL_BYTES + Self::UUID_BYTES + 1;
    const ENTRY_FIXED: u64 = 28 * Self::SERIAL_BYTES
        + Self::UUID_BYTES
        + 2 * 20
        + 32
        + 8
        + Self::UUID_BYTES
        + 5 * 8
        + Self::LOCATION_BYTES;

    fn touched_pages(count: u64) -> Result<u64, StorageLibraryError> {
        StorageLibraryImportSpace::add(
            StorageLibraryImportSpace::mul(
                StorageLibraryImportSpace::mul(count, Self::MAX_DEPTH)?,
                Self::OLD_SIBLINGS + 1,
            )?,
            count,
        )
    }

    fn probe_limit(count: u64) -> Result<u64, StorageLibraryError> {
        if count == 0 {
            return Err(StorageLibraryError::ImportLayout);
        }
        Self::touched_pages(count)
    }

    fn include(
        &mut self,
        cardinality: StorageLibraryImportSpaceEnumCardinality,
        payload: StorageLibraryImportSpacePayload,
        page: u64,
        count: u64,
    ) -> Result<(), StorageLibraryError> {
        let balances = StorageLibraryImportSpace::mul(count, Self::MAX_DEPTH)?;
        let overflow = payload.overflow(page, count)?;
        let allocations = StorageLibraryImportSpace::add(
            StorageLibraryImportSpace::add(
                StorageLibraryImportSpace::mul(balances, Self::EXTRA_SIBLINGS)?,
                StorageLibraryImportSpace::mul(count, Self::ROOT_CHILD_PAGES)?,
            )?,
            overflow,
        )?;
        let touched = Self::touched_pages(count)?;
        let old = match cardinality {
            StorageLibraryImportSpaceEnumCardinality::Exact(records) => {
                payload.kind.structural_pages(records)?.min(touched)
            }
            StorageLibraryImportSpaceEnumCardinality::AtLeast(minimum) => {
                if minimum < touched {
                    return Err(StorageLibraryError::ImportLayout);
                }
                touched
            }
        };
        let incoming =
            StorageLibraryImportSpace::add(payload.kind.structural_pages(count)?, overflow)?;
        self.new_page_budget =
            StorageLibraryImportSpace::add(self.new_page_budget, allocations.min(incoming))?;
        self.old_tree_pages = StorageLibraryImportSpace::add(self.old_tree_pages, old)?;
        self.allocator_operations = StorageLibraryImportSpace::add(
            self.allocator_operations,
            StorageLibraryImportSpace::add(
                allocations,
                StorageLibraryImportSpace::mul(balances, Self::OLD_SIBLINGS)?,
            )?,
        )?;
        Ok(())
    }

    fn index_payload(
        transaction: &Transaction<'_>,
        name: &str,
        footprint: StorageImportSessionCommitFootprint,
        check: &mut impl FnMut() -> Result<(), StorageImportSessionError>,
    ) -> Result<StorageLibraryImportSpacePayload, StorageLibraryError> {
        check()?;
        let mut statement = transaction.prepare("SELECT cid, name FROM pragma_index_xinfo(?1)")?;
        check()?;
        let mut rows = statement.query([name])?;
        let mut columns = 0_u64;
        let mut fixed = 0_u64;
        let mut variable = false;
        loop {
            check()?;
            let Some(row) = rows.next()? else {
                break;
            };
            let cid: i64 = row.get(0)?;
            let name = row.get_ref(1)?.as_str_or_null()?;
            let bytes = match name {
                None if cid == -1 => 8,
                Some("id" | "game_id" | "folder_id") => Self::UUID_BYTES,
                Some("location") => Self::LOCATION_BYTES,
                Some("rules_variant") => 8,
                Some("start_kind") => 8,
                Some("mode") => 19,
                Some("result") => 7,
                Some("opening_code") => 3,
                Some("identity_initial_fen") => 512,
                Some(
                    "modified_ms" | "date_year" | "date_month" | "date_day" | "plies" | "favorite",
                ) => 8,
                Some("white_key" | "black_key") if !variable => {
                    variable = true;
                    0
                }
                _ => return Err(StorageLibraryError::ImportLayout),
            };
            columns = StorageLibraryImportSpace::add(columns, 1)?;
            fixed = StorageLibraryImportSpace::add(fixed, bytes)?;
        }
        if columns == 0 {
            return Err(StorageLibraryError::ImportLayout);
        }
        fixed = StorageLibraryImportSpace::add(
            fixed,
            StorageLibraryImportSpace::mul(
                StorageLibraryImportSpace::add(columns, 1)?,
                Self::SERIAL_BYTES,
            )?,
        )?;
        StorageLibraryImportSpacePayload::new(
            if variable {
                footprint.projection_bytes()
            } else {
                0
            },
            if variable {
                footprint.largest_frame_bytes()
            } else {
                0
            },
            fixed,
            footprint.count(),
            StorageLibraryImportSpaceEnumTreeKind::Index,
        )
    }

    pub(super) fn plan_budget(
        transaction: &Transaction<'_>,
        footprint: StorageImportSessionCommitFootprint,
        page: u64,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<Self, StorageLibraryError> {
        Self::plan_budget_checked(transaction, footprint, page, &mut || cancellation.check())
    }

    fn plan_budget_checked(
        transaction: &Transaction<'_>,
        footprint: StorageImportSessionCommitFootprint,
        page: u64,
        check: &mut impl FnMut() -> Result<(), StorageImportSessionError>,
    ) -> Result<Self, StorageLibraryError> {
        check()?;
        let limit = Self::probe_limit(footprint.count())?;
        let documents =
            StorageLibraryImportSpaceEnumTable::Documents.probe(transaction, limit, check)?;
        let entries =
            StorageLibraryImportSpaceEnumTable::Entries.probe(transaction, limit, check)?;
        let mut budget = Self {
            new_page_budget: 0,
            old_tree_pages: 0,
            allocator_operations: 0,
            #[cfg(test)]
            inventory_vm_steps: StorageLibraryImportSpace::add(
                documents.inventory_vm_steps,
                entries.inventory_vm_steps,
            )?,
        };
        for (probe, total, fixed) in [
            (&documents, footprint.body_bytes(), Self::DOCUMENT_FIXED),
            (&entries, footprint.projection_bytes(), Self::ENTRY_FIXED),
        ] {
            check()?;
            budget.include(
                probe.cardinality,
                StorageLibraryImportSpacePayload::new(
                    total,
                    footprint.largest_frame_bytes(),
                    fixed,
                    footprint.count(),
                    StorageLibraryImportSpaceEnumTreeKind::RowidTable,
                )?,
                page,
                footprint.count(),
            )?;
        }
        check()?;
        let mut statement = transaction.prepare("SELECT name, tbl_name FROM sqlite_schema WHERE type = 'index' AND tbl_name IN ('documents', 'library_entries')")?;
        check()?;
        let mut rows = statement.query([])?;
        loop {
            check()?;
            let Some(row) = rows.next()? else {
                break;
            };
            let name = row.get_ref(0)?.as_str()?;
            let table = StorageLibraryImportSpaceEnumTable::from_name(row.get_ref(1)?.as_str()?)?;
            let cardinality = match table {
                StorageLibraryImportSpaceEnumTable::Documents => documents.cardinality,
                StorageLibraryImportSpaceEnumTable::Entries => entries.cardinality,
            };
            budget.include(
                cardinality,
                Self::index_payload(transaction, name, footprint, check)?,
                page,
                footprint.count(),
            )?;
        }
        check()?;
        Ok(budget)
    }
}

#[cfg(test)]
#[path = "../../../../tests/storage/library_import_planning_work.rs"]
mod planning_work_tests;
