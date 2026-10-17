use diesel::connection::DefaultLoadingMode;
use diesel::dsl::sql;
use diesel::sql_types::{BigInt, Bool, Nullable, Text};
use diesel::sqlite::SqliteConnection;
use diesel::{OptionalExtension, RunQueryDsl};

use crate::storage::import_session::StorageImportSessionCommitFootprint;
use crate::storage::library_import::space::StorageLibraryImportSpace;
use crate::storage::library_repository::StorageLibraryError;

pub(super) struct StorageLibraryImportSpaceTrees {
    pub(super) additional_pages: u64,
    pub(super) old_tree_pages: u64,
    pub(super) allocator_operations: u64,
}

struct StorageLibraryImportSpaceTree {
    pages: u64,
    records: u64,
    depth: u64,
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
    const fn is_index(self) -> bool {
        matches!(self, Self::Index)
    }

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

impl StorageLibraryImportSpaceTree {
    const MAX_DEPTH: u64 = 20;

    fn read(
        transaction: &mut SqliteConnection,
        name: &str,
        kind: StorageLibraryImportSpaceEnumTreeKind,
    ) -> Result<Self, StorageLibraryError> {
        let (pages, records, depth): (i64, i64, i64) = sql::<(BigInt, BigInt, BigInt)>("SELECT count(*) FILTER (WHERE pagetype <> 'overflow'), coalesce(sum(CASE WHEN pagetype = 'leaf' OR (")
            .bind::<Bool, _>(kind.is_index())
            .sql(" AND pagetype = 'internal') THEN ncell ELSE 0 END), 0), coalesce(max(CASE WHEN pagetype <> 'overflow' THEN length(path) - length(replace(path, '/', '')) END), 0) FROM dbstat WHERE name = ")
            .bind::<Text, _>(name).get_result(transaction)?;
        let result = Self {
            pages: u64::try_from(pages)?,
            records: u64::try_from(records)?,
            depth: u64::try_from(depth)?,
        };
        if result.pages == 0 || !(1..=Self::MAX_DEPTH).contains(&result.depth) {
            return Err(StorageLibraryError::ImportLayout);
        }
        Ok(result)
    }
}

impl StorageLibraryImportSpaceTrees {
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

    fn include(
        &mut self,
        tree: StorageLibraryImportSpaceTree,
        payload: StorageLibraryImportSpacePayload,
        page: u64,
        count: u64,
    ) -> Result<(), StorageLibraryError> {
        let depth = StorageLibraryImportSpace::add(tree.depth, count.saturating_sub(1))?
            .min(StorageLibraryImportSpaceTree::MAX_DEPTH);
        let balances = StorageLibraryImportSpace::mul(count, depth)?;
        let overflow = payload.overflow(page, count)?;
        let allocations = StorageLibraryImportSpace::add(
            StorageLibraryImportSpace::add(
                StorageLibraryImportSpace::mul(balances, Self::EXTRA_SIBLINGS)?,
                StorageLibraryImportSpace::mul(count, Self::ROOT_CHILD_PAGES)?,
            )?,
            overflow,
        )?;
        let records = StorageLibraryImportSpace::add(tree.records, count)?;
        let structural = payload.kind.structural_pages(records)?;
        let additional = allocations.min(StorageLibraryImportSpace::add(structural, overflow)?);
        self.additional_pages = StorageLibraryImportSpace::add(self.additional_pages, additional)?;
        let touched = StorageLibraryImportSpace::add(
            StorageLibraryImportSpace::mul(balances, Self::OLD_SIBLINGS + 1)?,
            count,
        )?;
        self.old_tree_pages =
            StorageLibraryImportSpace::add(self.old_tree_pages, tree.pages.min(touched))?;
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
        transaction: &mut SqliteConnection,
        name: &str,
        footprint: StorageImportSessionCommitFootprint,
    ) -> Result<StorageLibraryImportSpacePayload, StorageLibraryError> {
        let rows = sql::<(BigInt, Nullable<Text>)>("SELECT cid, name FROM pragma_index_xinfo(")
            .bind::<Text, _>(name)
            .sql(")")
            .load_iter::<(i64, Option<String>), DefaultLoadingMode>(transaction)?;
        let mut columns = 0_u64;
        let mut fixed = 0_u64;
        let mut variable = false;
        for row in rows {
            let (cid, name) = row?;
            let bytes = match name.as_deref() {
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

    pub(super) fn bound(
        transaction: &mut SqliteConnection,
        footprint: StorageImportSessionCommitFootprint,
        page: u64,
    ) -> Result<Self, StorageLibraryError> {
        let mut bound = Self {
            additional_pages: 0,
            old_tree_pages: 0,
            allocator_operations: 0,
        };
        for (name, total, fixed) in [
            ("documents", footprint.body_bytes(), Self::DOCUMENT_FIXED),
            (
                "library_entries",
                footprint.projection_bytes(),
                Self::ENTRY_FIXED,
            ),
        ] {
            bound.include(
                StorageLibraryImportSpaceTree::read(
                    transaction,
                    name,
                    StorageLibraryImportSpaceEnumTreeKind::RowidTable,
                )?,
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
        let mut after: Option<String> = None;
        loop {
            let name = sql::<Text>("SELECT name FROM sqlite_schema WHERE type = 'index' AND tbl_name IN ('documents', 'library_entries') AND (")
                .bind::<Nullable<Text>, _>(after.as_deref())
                .sql(" IS NULL OR name > ")
                .bind::<Nullable<Text>, _>(after.as_deref())
                .sql(") ORDER BY name LIMIT 1")
                .get_result::<String>(transaction).optional()?;
            let Some(name) = name else {
                break;
            };
            bound.include(
                StorageLibraryImportSpaceTree::read(
                    transaction,
                    &name,
                    StorageLibraryImportSpaceEnumTreeKind::Index,
                )?,
                Self::index_payload(transaction, &name, footprint)?,
                page,
                footprint.count(),
            )?;
            after = Some(name);
        }
        Ok(bound)
    }
}
