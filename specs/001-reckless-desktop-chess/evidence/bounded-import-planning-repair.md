# Bounded import planning source handoff

This is a source-only continuation of the all-13-journeys T097/T100 import
work. No task marker or acceptance status changes. The parent retains exclusive
Cargo, shared-target, default-parallelism verification ownership.

## Frozen prerequisite and observed regression

The checkout started at `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68` and applied
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-import-planning-red.patch`.
Its SHA-256 is
`6b34fee121c603b3372f2375cddb01f7e6b2daf8a790426055e519b28303d3f3`.
The applied tree `3ed38cff48b50cb6018eb9d7a5d74399c8370824` is the incremental
export baseline, including the parent's three SQLite-integer fixture compile
corrections. Those compile failures were not semantic-red evidence.

[The original regression evidence](import-planning-work-tests.md) records the
actual semantic red: zero passed, one failed, 551 filtered. Both Deferred and
Immediate inventory counts increased from 658 to 2578 VM steps when retained
document overflow increased from zero to 128 pages. Additional-page, old-tree
and allocator bounds remained 44, 14 and 86. Preservation, checkpoint and cleanup
controls passed before the final assertion. The supplied log SHA-256 is
`ea12d76984e97e23b407dd6199e51e8624c8ae0d688151d5ec4c4c819f730ad9`.

The dependency patch, sidecar, source bundle and log hashes were checked during
this round. That verifies the supplied evidence's identity, not a new test run.

## Planner and cancellation contract

Production planning has no `dbstat` query. It streams only `documents.id` and
`library_entries.game_id`, ordered by their compact primary-key indexes, with a
SQL LIMIT. Query-plan tests compile these exact owning-enum queries and require
the migrated covering autoindexes without a temporary sort. No old body or
whole identifier vector is materialized.

For actual selected count C, the canonical depth ceiling D=20 gives the per-tree
old-touch bound V=C*(4*D+1). K=V is the probe limit. Zero C is rejected. Fewer
than K rows establishes `Exact(R)`; reaching K establishes only `AtLeast(K)`,
including the case where the installed count happens to equal K. Each parent
table is probed once. Its cardinality also bounds all its indexes, including
partial indexes, whose population cannot exceed their parent table.

All balance/allocation bounds use D=20. Structural caps are available only for
`Exact(R)`, and are applied when R<C. If R>=C, operation bounds are used even
when R is known exactly. This keeps tiny-import allowance stable when a small
unrelated document is added to a nonempty installed library. It also retains
useful structural caps when a bulk incoming batch dominates the prior catalog.
A capped probe never invents an exact record or tree-page count. Rowid structural
pages are bounded by 2R+1, index pages by R+1. Existing payload-local/overflow
rules remain schema-specific and unchanged.

The public repository method is now:

```rust
fn estimate_commit_space(
    &self,
    snapshot: StorageImportSessionSnapshot,
    footprint: StorageImportSessionCommitFootprint,
    cancellation: StorageImportSessionCancellation,
) -> StorageImportSessionRepositoryFuture<'_, u64>;
```

Freeze clones the session's real token into `ImportEstimate`; the transaction
passes it by reference to layout and tree planning. Commit uses its existing
request token for its own planning. Checks precede planning, layout queries,
primary-key and index-metadata queries, each streamed row and subsequent probes,
and page-ceiling installation. There is no production replacement token,
SQLite hook, interrupt or global configuration mutation. The private checked
implementation allows deterministic between-row cancellation with the actual
cancellation type; production supplies its token's `check` operation.

The normal old-dirty allowance is not clamped to whole-database N. Its terms are
the import's old-tree touch bound, the existing freelist-operation allowance and
header/catalog pages. The pre-existing conservative TEMP-trigger branch stays.
Main growth still includes N*S minus actual main-file length, so uncheckpointed
backlog remains chargeable. WAL/SHM high-water terms and the conservative logical
statement-journal allowance remain. The page ceiling still flows through the
original scoped `min(previous, N+E)` install/verify/restore implementation.
Restoration follows observed commit or rollback, preserving compound causes and
connection retirement. Metadata arms and adoption certification remain intact.

## Canonical SQL journal proof conditions

Only the three production write strings were extracted onto their owning types:
`StorageLibraryTransactionContent::INSERT_DOCUMENT_SQL`, its `PROJECT_SQL`, and
`StorageLibraryTransaction::BUMP_CATALOG_SQL`. Their SQL text is unchanged and
production calls these constants. The new EXPLAIN tests consume these same
constants under all real migrations, WAL, FK ON, defer-FK OFF and an Immediate
transaction. The actor opens WAL/FK ON/synchronous FULL connections in
`src/storage/document_store.rs`. Current relevant main triggers are absent;
existing migrations' unrelated triggers remain present.

The source reviewed is the Cargo-locked `libsqlite3-sys 0.38.2` amalgamation,
SQLite 3.53.2, source ID
`2026-06-03 19:12:13 d6e03d8c777cfa2d35e3b60d8ec3e0187f3e9f99d8e2ee9cac695fd6fcdf1a24`.
Its SHA-256 is
`0a409f1633283fa31a9126b11fbfd64a1991c5d30defad07e5745d4667f5e23d`.
The source establishes these conditional compiler facts:

- `sqlite3VdbeMakeReady`, line 90443: `usesStmtJournal` requires both
  `isMultiWrite` and `mayAbort`.
- `sqlite3Insert`, line 139781: the canonical single VALUES insert starts with
  no multiwrite request when there is no SELECT or relevant trigger.
- Constraint/UPSERT generation precedes FK generation at 140333-140338.
  `sqlite3FkRequired`, 138417-138465, skips UPDATE FK work when the SET list
  changes neither child nor parent keys. The canonical UPSERT changes neither
  game_id nor folder_id. Its UPDATE is ONEPASS_SINGLE at 160308-160316.
- `fkLookupParent`, 137711-137728, emits a direct foreign-key constraint Halt
  for an immediate single-row insert with no multiwrite/top-level trigger
  context. Other paths use FkCounter. Single-row insertion into a parent skips
  immediate child-FK scans at 138300-138307.
- The catalog singleton UPDATE has no FK, key change, trigger or subquery;
  the one-pass branch at 160327-160355 requests multiwrite only when the
  selected strategy is not ONEPASS_SINGLE.

The parent must run the compiled counterchecks before accepting the conclusion
that these exact production writes have no statement journal. The tests print
SQLite version/source ID and actual numeric opcode fields, require the entry
UPSERT's direct FK Halt paths with no Program/FkCounter, and require the catalog
SeekRowid path without rowsets, ephemeral scans, Program or FK counters. They do
not interpret Transaction.p5 or absent opcode comments as a journal flag.

A real TEMP BEFORE INSERT fault trigger on library_entries is a positive
control: its compiled canonical UPSERT must contain Program and FkCounter. A
separate real actor import faults on its second entry, after a prior row and the
current document were inserted; it requires rollback of both games, catalog
preservation and page-limit restoration. Existing TEMP fault fixtures are kept.

This is a conditional pinned-source argument with authored compiled-SQL checks,
not observed compiled proof or physical VFS evidence. TEMP-trigger plans retain
the prior conservative logical journal allowance. Its actual temporary-volume
placement and physical allocation remain unproved. There is no temp_store
change, OR ROLLBACK substitution, unsafe code, vendor edit or new dependency.

## New checks and remaining parent work

Twelve new tests are authored: seven planner checks, three canonical SQL compiler
checks, and two actor checks. The existing estimate-wait cancellation regression
also requires that the captured estimator token becomes cancelled when the real
session is cancelled. The original old-note VM assertion and all its preservation
controls remain. Its counter now sums actual covering-probe statement VM steps,
which must be nonzero. It does not omit actual queries or substitute a synthetic
counter.

The tests-before-source boundaries are recorded in the adjacent export manifest. The
initial counterchecks were staged and snapshotted before production edits; the
additional ballast-bound countercheck was written before its corresponding
bound adjustment. No added check has been executed in this source-only lane.
The existing forty import cases, checkpointed-ballast admission, uncheckpointed
backlog refusal, useful 1000-record/256-MiB and tiny-import/16-MiB limits,
SQLITE_FULL, cancellation, rollback faults, restoration causes and retirement
controls remain for parent execution.

Only direct pinned-rustfmt formatting, static source review, diff checks,
SQL-text identity checks, preservation checks, path ownership and patch/tree/hash
checks are authorized here. The first formatter invocation incorrectly passed a
rustup-style selector to a standalone binary; the explicit installed 1.99.0
formatter was then used successfully. No Cargo, tests, builds, Clippy, runtime
SQL diagnostics, agents, worktrees, commits or pushes ran. Parent execution,
compile proof, full native/platform acceptance and millions-scale memory/work
measurements are pending. The parent owns the independent library-service
integration and any prerequisite edits to library_query, metadata,
annotationcommands or library_repository.
