# Persistence and library migration

## T296 dependency resolution, 2026-10-17

`Cargo.toml` adds exact pins: diesel 2.3.14 (`sqlite`,
`returning_clauses_for_sqlite_3_35`, `32-column-tables`, `with-deprecated`,
default features off), diesel_migrations 2.3.2 (`sqlite`), diesel-derive-enum
2.1.0 (`sqlite`), csv-core 0.1.13, strum 0.28.0 (`derive`) and a direct
libsqlite3-sys 0.38.2 `bundled` pin. `Cargo.lock` resolves one libsqlite3-sys
0.38.2 shared by rusqlite, diesel and the package. rusqlite remains until T309.

Licenses: diesel family and diesel-derive-enum MIT OR Apache-2.0, csv-core
Unlicense/MIT, strum MIT.

Probe: a scratch crate with this package's exact `[lints]` tables, a `table!`,
`Queryable`/`Selectable`/`Insertable`/`AsChangeset` row, `DbEnum` with
`db_rename`, strum derives, `immediate_transaction`, `sql_query`, boxed load.

- Without `with-deprecated`, `DbEnum` fails: diesel-derive-enum 2.1.0 expands
  to `diesel::backend::RawValue`, which Diesel 2.3.14 gates behind
  `with-deprecated`. Enabling it compiles with zero warnings under
  `unsafe_code = "forbid"` and every denied Clippy lint, so it is selected.
  Revisit if a later diesel-derive-enum release drops the alias.
- Focused checks run: probe `cargo clippy --all-targets` (pass, zero
  warnings); package `cargo check --locked --lib` (pass). Not run: package
  Clippy, tests and builds.

Toolchain: the pinned 1.99.0 wrapper sets `DYLD_LIBRARY_PATH`; macOS strips it
through `/usr/bin/env`, so custom target directories use a scratch wrapper
variant reading `CHESS_TARGET_DIR`.
## T297 rusqlite baseline

Measured the current rusqlite storage implementation before any Diesel
conversion, using `benches/storage.rs` (`harness = false`, public repository
APIs only, so the same target runs unchanged against the Diesel implementation).

Command (from the worktree root, optimized `bench` profile, thin LTO):

```
CHESS_TARGET_DIR=$PWD/target <pinned-rust-1.99.0 wrapper> cargo bench --bench storage
```

Environment: Apple M4 Pro, 14 hardware threads (`available_parallelism` = 14),
24 GiB RAM, macOS 27.0.1 ARM64, AC power (`powermode 2`), rustc 1.99.0 pinned,
bundled SQLite via `libsqlite3-sys`. Cargo.lock sha256 prefix `bba4714661691d21`.
Store on a per-run `tempfile` directory on the local APFS volume; OS disk cache
warm, no cache purge between runs. Commit was the working tree on top of
HEAD 8c8a794 with the T296 dependency changes.

Protocol: std `Instant` only. Per run a fresh store and executor, then warm-up
samples (discarded: 3 for import and keyset walk, 20 otherwise), then measured
samples (20 per import phase, 50 keyset walks, 200 for every other workload).
Five independent runs per workload and per CapacityBudget (1 worker, then the
detected maximum of 14). Per-run p95 is nearest rank, index ceil(0.95 * n)
counting from one. Runs are reported individually and never averaged. Cell
format: p95 ms (p50, max).

Workloads and fixtures (deterministic, no random data):

- Import: 500 existing library games (pawn-move openings, unique headers and
  annotations), adoption certified through `DesktopNativeLibraryStartup`, then
  per sample a new import session of 100 incoming games (4-ply pawn openings,
  structurally unique across samples)
  through `StorageImportSession` with the production
  `StorageImportSessionFilesystem` and `StorageLibraryImportRepository`; the
  store grows by 100 games per sample (500 to 2,500 across warm-up and
  samples). `prepare`, `freeze` and `commit` are timed separately plus their
  sum; every commit is checked to land 100 games.
- Library query: 1,500 games (20 white groups, 7 opponents, 5 years, 4
  results, 1 to 6 plies) written through the library repository; first page
  of 100 (default sort), a white filter with date sort, and a full keyset walk
  of 15 pages in plies order that must visit all 1,500 records.
- Game document: one Opera-game document alternating between two bodies,
  saved and reopened through the document client (`save`/`read`) and through
  the library repository (`prepare_write`+`begin_save`+`wait`, then `load`);
  revisions are checked after each open.
- Puzzle event append: `PuzzleStorageDriver` over `StoragePuzzleRepository`,
  one Pause/Resume event appended per sample (journal grows by one event per
  checkpoint); timed region is `begin_write` plus `wait`.
- Editor draft save: `StorageEditorRepository::save` of a draft whose counters
  change each sample, with the receipt revision as the next expected revision.

Results (p95 ms, with p50 and max, per run):

| workload | workers | samples/run | run 1 | run 2 | run 3 | run 4 | run 5 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| import prepare (100 games, 500 existing) | 1 | 20 | 39.953 (p50 31.556, max 48.216) | 40.778 (p50 33.016, max 54.817) | 37.842 (p50 30.377, max 39.410) | 38.876 (p50 33.738, max 45.972) | 38.237 (p50 30.152, max 39.022) |
| import freeze (100 games, 500 existing) | 1 | 20 | 155.831 (p50 55.673, max 363.193) | 76.188 (p50 57.284, max 107.082) | 57.057 (p50 54.480, max 60.553) | 66.218 (p50 55.964, max 68.171) | 51.836 (p50 47.107, max 55.404) |
| import commit (100 games, 500 existing) | 1 | 20 | 13.605 (p50 8.294, max 17.487) | 10.774 (p50 7.937, max 18.506) | 10.856 (p50 7.954, max 11.138) | 11.030 (p50 8.234, max 11.859) | 10.435 (p50 7.853, max 10.738) |
| import prepare+freeze+commit | 1 | 20 | 212.797 (p50 99.053, max 396.813) | 130.088 (p50 98.464, max 155.801) | 103.090 (p50 91.094, max 106.230) | 110.789 (p50 99.302, max 114.937) | 94.447 (p50 85.664, max 96.643) |
| library first page (100 of 1500) | 1 | 200 | 3.048 (p50 2.648, max 3.540) | 2.856 (p50 2.561, max 4.293) | 3.115 (p50 2.598, max 12.918) | 2.829 (p50 2.461, max 3.255) | 2.650 (p50 2.419, max 4.138) |
| library filtered page (white filter, date sort) | 1 | 200 | 1.607 (p50 1.426, max 2.444) | 1.558 (p50 1.367, max 2.669) | 1.692 (p50 1.405, max 4.804) | 1.565 (p50 1.312, max 2.014) | 1.536 (p50 1.284, max 1.743) |
| library keyset walk (15 pages, plies sort) | 1 | 50 | 93.870 (p50 37.547, max 205.880) | 31.751 (p50 29.399, max 32.542) | 35.059 (p50 29.858, max 35.700) | 29.267 (p50 26.753, max 29.650) | 53.273 (p50 28.029, max 89.061) |
| game document save (document client) | 1 | 200 | 0.136 (p50 0.064, max 0.459) | 0.173 (p50 0.077, max 1.154) | 0.198 (p50 0.101, max 1.049) | 0.115 (p50 0.070, max 0.706) | 0.131 (p50 0.073, max 0.721) |
| game document open (document client) | 1 | 200 | 0.011 (p50 0.010, max 0.018) | 0.013 (p50 0.010, max 0.051) | 0.024 (p50 0.011, max 0.031) | 0.010 (p50 0.009, max 0.014) | 0.011 (p50 0.010, max 0.033) |
| game document save (library write) | 1 | 200 | 0.288 (p50 0.207, max 1.191) | 0.399 (p50 0.257, max 1.090) | 0.496 (p50 0.329, max 0.845) | 0.320 (p50 0.234, max 0.410) | 0.338 (p50 0.242, max 0.479) |
| game document open (library load) | 1 | 200 | 0.072 (p50 0.060, max 0.109) | 0.072 (p50 0.061, max 0.131) | 0.092 (p50 0.076, max 0.104) | 0.061 (p50 0.058, max 0.067) | 0.064 (p50 0.060, max 0.180) |
| puzzle event append | 1 | 200 | 0.082 (p50 0.065, max 0.253) | 0.113 (p50 0.081, max 0.379) | 0.143 (p50 0.100, max 0.416) | 0.091 (p50 0.076, max 0.331) | 0.094 (p50 0.075, max 0.292) |
| editor draft save | 1 | 200 | 0.064 (p50 0.051, max 0.358) | 0.080 (p50 0.060, max 0.503) | 0.105 (p50 0.068, max 0.579) | 0.063 (p50 0.057, max 0.426) | 0.072 (p50 0.059, max 0.472) |
| import prepare (100 games, 500 existing) | 14 | 20 | 38.879 (p50 32.935, max 40.074) | 39.587 (p50 32.535, max 41.198) | 39.201 (p50 32.523, max 40.294) | 37.049 (p50 31.571, max 40.324) | 38.116 (p50 33.361, max 40.739) |
| import freeze (100 games, 500 existing) | 14 | 20 | 59.066 (p50 51.381, max 62.605) | 55.627 (p50 51.226, max 55.836) | 81.529 (p50 51.770, max 102.544) | 57.784 (p50 51.444, max 58.965) | 59.392 (p50 54.036, max 62.083) |
| import commit (100 games, 500 existing) | 14 | 20 | 10.469 (p50 7.871, max 10.974) | 10.830 (p50 7.873, max 10.952) | 10.117 (p50 7.970, max 10.473) | 10.408 (p50 7.845, max 10.808) | 10.519 (p50 8.052, max 11.932) |
| import prepare+freeze+commit | 14 | 20 | 102.308 (p50 92.469, max 102.926) | 102.774 (p50 89.944, max 102.849) | 119.797 (p50 94.601, max 150.906) | 95.889 (p50 92.684, max 96.548) | 104.663 (p50 94.626, max 106.156) |
| library first page (100 of 1500) | 14 | 200 | 2.823 (p50 2.498, max 3.316) | 2.736 (p50 2.473, max 3.281) | 2.670 (p50 2.431, max 3.101) | 2.665 (p50 2.449, max 2.842) | 2.868 (p50 2.555, max 3.125) |
| library filtered page (white filter, date sort) | 14 | 200 | 1.619 (p50 1.367, max 1.746) | 1.550 (p50 1.353, max 3.208) | 1.497 (p50 1.325, max 1.657) | 1.549 (p50 1.324, max 1.673) | 1.665 (p50 1.389, max 3.271) |
| library keyset walk (15 pages, plies sort) | 14 | 50 | 29.898 (p50 29.007, max 30.540) | 30.554 (p50 29.345, max 30.894) | 30.508 (p50 28.940, max 30.582) | 32.131 (p50 29.758, max 32.929) | 30.479 (p50 28.888, max 30.970) |
| game document save (document client) | 14 | 200 | 0.157 (p50 0.073, max 0.933) | 0.162 (p50 0.075, max 0.768) | 0.147 (p50 0.075, max 0.736) | 0.205 (p50 0.092, max 0.861) | 0.153 (p50 0.076, max 3.944) |
| game document open (document client) | 14 | 200 | 0.012 (p50 0.010, max 0.039) | 0.012 (p50 0.010, max 0.018) | 0.011 (p50 0.010, max 0.015) | 0.020 (p50 0.011, max 0.022) | 0.012 (p50 0.010, max 0.030) |
| game document save (library write) | 14 | 200 | 0.359 (p50 0.244, max 0.522) | 0.356 (p50 0.248, max 0.497) | 0.325 (p50 0.246, max 0.520) | 0.441 (p50 0.299, max 1.834) | 0.339 (p50 0.249, max 1.152) |
| game document open (library load) | 14 | 200 | 0.071 (p50 0.061, max 0.089) | 0.072 (p50 0.062, max 0.086) | 0.069 (p50 0.062, max 0.107) | 0.093 (p50 0.067, max 0.110) | 0.075 (p50 0.062, max 0.119) |
| puzzle event append | 14 | 200 | 0.092 (p50 0.079, max 0.368) | 0.102 (p50 0.081, max 0.328) | 0.093 (p50 0.080, max 0.337) | 0.130 (p50 0.083, max 0.351) | 0.094 (p50 0.070, max 0.341) |
| editor draft save | 14 | 200 | 0.066 (p50 0.058, max 0.481) | 0.064 (p50 0.057, max 0.472) | 0.067 (p50 0.056, max 0.538) | 0.072 (p50 0.059, max 0.510) | 0.069 (p50 0.056, max 0.891) |

Observations and limits:

- Worker count has no visible effect on any storage workload: the document
  store is a single writer actor and library preparation here is small. The
  one-versus-14 comparison therefore shows no scaling; it records that the
  budget does not change storage latency, not that parallelism is exercised.
- Import prepare p50 is about 31 ms and freeze about 55 ms per 100 games; commit
  is the cheapest phase at about 8 ms p50. One run-1 freeze and keyset-walk
  outlier (max 363 ms and 206 ms) occurred at the first measured run of the
  process and is kept, not omitted.
- Sub-millisecond writes (document save 0.06 ms p50, editor 0.05 ms) reflect
  the SQLite durability settings of the current implementation on macOS, where
  `fsync` does not flush the drive cache; these are not comparable to Linux or
  Windows results and the baseline is recorded for macOS ARM64 only.
- Measured on macOS ARM64 only. Windows and Linux (Intel UHD 620-class) were
  not measured. The import path uses the Unix filesystem provider, so the bench
  is unix-only; `StorageImportSessionUnixSpace` is not exported for Windows.
- Not measured: folder and tag metadata pages (`StorageLibraryMetadata*`),
  trash, dataset package and selection repositories, concurrent mixed
  read/write contention, peak memory, and cold-OS-cache timing.
- The bench source was reformatted with rustfmt after the measured run;
  behavior is unchanged. Per-run results above are from that identical logic.

## T298 to T301 storage core, 2026-10-17

`StorageConnection` (`src/storage/connection.rs`) applies busy_timeout 5000,
WAL, foreign_keys ON and synchronous FULL to every Diesel and remaining
rusqlite connection. `StorageMigrationRunner` (`src/storage/migration.rs`)
embeds `src/storage/migrations/000N_name/up.sql` (moved byte-for-byte) and runs
inside one `immediate_transaction`: legacy `user_version` 1 to 4 is adopted
into `__diesel_schema_migrations`, a higher value returns `FutureSchema`, an
applied version absent from the embedded set returns `FutureMigration`, and
pending migrations run as savepoints. `user_version` is no longer written. The
hand-written runner and `open_with_migrations` were deleted with their callers.

Fixtures `tests/fixtures/migrations/legacy_v1..v4.sqlite` were generated by
applying the original SQL files with representative rows. Red first: the
adoption, fresh-database and unknown-version cases failed against the old
runner; the above-4 and failing-later-migration cases already held and remain
regression guards.

`src/storage/schema.rs` is generated by diesel_cli 2.3.14 `sqlite-bundled`.
`make schema-check`, now part of `make verify`, rebuilds a database from the
embedded migrations, regenerates, formats with the project rustfmt settings
and diffs. `DIESEL` and `RUSTFMT` are overridable; on this machine the pinned
toolchain's rustfmt runs through the wrapper because `/bin/sh` strips
`DYLD_LIBRARY_PATH`.

Focused checks after coordinator merge: `cargo check --lib --tests` clean;
`cargo test --test storage` 204 passed; `cargo test --lib library_transaction`
27 passed; `cargo check --bench storage` clean; `make schema-check` passed.
Not run: Clippy, other suites, `make verify`.

## Wave 2 conversions, 2026-10-17

- T314 squash (lane-a): `0001_initial/up.sql` replaces 0001..0004 with an
  identical `sqlite_master`; pre-Diesel databases return
  `UnsupportedLegacyDatabase` unchanged; legacy fixtures deleted.
- T302 partial (lane-a): actor holds `StorageDocumentStoreConnections`
  (`diesel()`, `legacy()`); document read and inventory on Diesel. Save waits
  for a Diesel `bump_catalog` from the library conversion.
- T303 (lane-b): puzzle repository on Diesel with private rows; events insert
  in chunks of 1,000. Puzzle append p95 0.105 to 0.186 ms versus the 0.082 to
  0.143 ms baseline, about 0.03 ms slower; recheck in T312.
- T304 (lane-c): editor repository on Diesel. Transactions use explicit
  `BEGIN`/`COMMIT`/`ROLLBACK` through `batch_execute` because a
  `RAISE(ROLLBACK)` trigger would otherwise mark Diesel's transaction manager
  broken; an execution value carries the unresolved state.

Coordinator focused checks after merge: `cargo test --test storage --
puzzle_repository editor_repository migration_continuity` 39 passed;
`cargo test --test desktop editor` 23 passed.

## T310 csv-core, 2026-10-17

`src/puzzle/csv_import.rs` decodes records with `csv_core::Reader::read_record`
into a fixed `MAX_ROW_BYTES + 1` output buffer and a 4096-entry field-end
buffer; `OutputFull`/`OutputEndsFull` map to `RowTooLarge`. csv-core never
reports malformed framing, so a validation-only phase machine remains for
`UnexpectedQuote`, `TrailingQuotedData`, `UnclosedQuote` and
`BareCarriageReturn`, plus explicit empty-line, CRLF and BOM handling. Net
size grew (+303/-137). Merged by owner decision. New behavior: rows above 4096
fields are rejected as `RowTooLarge`. Focused check: `cargo test --test domain
csv_import` 22 passed.

## T311 strum, domain modules (lane-c), written without a build

Scope: `src/board`, `src/game`, `src/clock`, `src/engine` (not `configuration.rs`),
`src/notation`, `src/puzzle`, `src/opening`, `src/report`. `src/desktop`,
`src/storage`, `src/presentation` and `src/update` are other lanes. Already on
strum before this pass: `BoardEnumVariant` and `GameEnumMode`. Spellings are kept
identical through explicit `#[strum(serialize = ...)]`. Nothing here has been
compiled or run yet.

Converted tables:

| Enum | Table replaced | Derives added |
| --- | --- | --- |
| `EngineEnumOption` (`src/engine/enums.rs`) | `name()` match; the `ALL` plus `find` option lookup in `EngineCapabilities::discover` | `EnumString`, `IntoStaticStr`, `Display`, `EnumIter` |
| `GameConfigurationEnumPreset` (`src/game/configuration.rs`) | `label()` match | same |
| `GameSaveCoordinatorEnumBarrier` (`src/game/save_coordinator.rs`) | `label()` match | same |
| `EnginePackageEnumTarget` (`src/engine/package/manifest.rs`) | `triple()` match | same |
| `EnginePackageEnumProfile` (`src/engine/package/manifest.rs`) | `label()` match | same |
| `EnginePackagePlatformEnumFeature` (`src/engine/package/platform.rs`) | `label()` match | same |
| `EnginePackagePlatformEnumOsState` (`src/engine/package/platform.rs`) | `label()` match | same |
| `PuzzleCsvImportEnumField` (`src/puzzle/csv_import.rs`) | `from_name()` match; unknown headers stay `None` through `parse().ok()` | `EnumString` |
| `NotationPgnExportEnumTag` (`src/notation/pgn_export.rs`) | `name()` match | `AsRefStr` |
| `NotationPgnExportEnumDirective` (`src/notation/pgn_export.rs`) | `opening()` match and the hand-kept `ALL` | `IntoStaticStr`, `EnumIter` |

Left as they are, with the reason:

- `ALL` constants on `EngineEnumOption`, `GameConfigurationEnumPreset` and
  `EnginePackageEnumProfile` stay because `src/presentation`, `src/desktop`,
  `tools/` and tests index them (`ALL.map`, array equality, `.len()`). Each now
  has an `EnumIter` order test against `ALL`; remove `ALL` once the
  presentation and desktop lanes move to `iter()`.
- `NotationGameIdentityEnumResult::pgn_token` and
  `EngineDiagnosticEnumCommand::command` match on enums with data or format
  arguments, which strum cannot derive.
- `GameSession`, `PuzzleDefinition*`, `Opening*` `as_str`/`name`/`code` are
  string newtypes, not enum tables.
- UCI info keyword and PGN tag parsing in `src/engine/analysis.rs` and
  `src/notation/pgn_import.rs` assign into structures, so they are parsers, not
  enum tables.

Contract coverage added: `tests/engine/options.rs`, `tests/game/configuration.rs`,
`tests/engine/package.rs`, `tests/desktop/game_recovery_policy.rs` and
`tests/puzzle/csv_import.rs` check each spelling, its round trip and the
`EnumIter` order.

## T311 strum, `src/storage/**` increment (lane-e)

Written, not compiled or run. Every persisted spelling is kept with an explicit
`#[strum(serialize = ...)]`.

| Owner | Table replaced | Now |
| --- | --- | --- |
| `dataset_registry.rs` `StorageDatasetRegistryEnumKind` | `text`/`parse` (`corpus`, `book`, `puzzles`) | `EnumString`, `AsRefStr`, `Display`, `EnumIter`, Diesel `AsExpression`/`FromSqlRow` |
| `dataset_registry.rs` `StorageDatasetRegistryEnumOrigin` | `text`/`parse` (`starter`, `personal`) | same derives |
| `dataset_registry.rs` `StorageDatasetRegistryEnumHolder` | `text` (`run`, `query`, `report`, `history`) | same derives |
| `dataset_package.rs` `StorageDatasetPackageEnumFormat` | `format_text`/`parse_format` (`pgn`, `polyglot`, `puzzle_csv`, `prepared_puzzle_catalog_v1`) | same derives; serde names unchanged |
| `dataset_package.rs` `StorageDatasetPackageEnumValidation` | `validation_text`/`parse_validation` (`unchecked`, `integrity_verified`) | same derives |
| `text_column.rs` | none for the five enums above | five `storage_text_column!` entries; the registry now binds and loads the enums directly instead of strings |
| `backup/manifest.rs` `BackupEnumCategory` | `slug`, `from_slug`, hand-kept `ALL` (`manual`, `automatic`, `pre-restore`, `pre-update`) | `EnumString`, `IntoStaticStr`, `EnumIter`; `slug()` returns the static name; serde snake_case unchanged; retention iterates `iter()` |
| `dataset_package/provenance.rs` `StorageDatasetPackageEnumRawField` | `name()` | `AsRefStr` |
| `library_import/space/trees.rs` `StorageLibraryImportSpaceEnumTable` | `from_name` and test-only `ALL` | `EnumString` (`documents`, `library_entries`) mapped to `StorageLibraryError::ImportLayout`, and `EnumIter` |
| `import_session/disk.rs` `StorageImportSessionDiskEnumFile` | hand-kept `ALL` (13) | `EnumIter` |
| `dataset_selection/sequence.rs` `StorageDatasetSelectionEnumSubset` | hand-kept `ORDER` | `EnumIter` (declaration order is the policy order) |
| `dataset_selection/caps.rs` `StorageDatasetSelectionEnumComponent` | hand-kept `ORDER` and `ORDER.len()` array sizes | `EnumIter`, `EnumCount` (`COUNT`) |

Consumers updated for the removed `ORDER`: `dataset_package/{manifest,measurement}.rs`,
`tools/data/build.rs`, `tests/storage/{dataset_selection,dataset_package}.rs`,
`tests/opening/starter_data.rs`, and the private-table test in
`tests/storage/library_import_planning_work.rs`.

Left alone: enums that already used strum (`StorageReportEnum*`,
`StorageGameDocumentEnumContentKind`, `StorageLibraryEnum*`, `StorageTrashEnumLocation`);
the SQL probe text in `library_import/space/layout.rs` (`query`), which is query
text and not an enum spelling table; `disk.rs` `index()`, which is a positional
index and not a spelling.
