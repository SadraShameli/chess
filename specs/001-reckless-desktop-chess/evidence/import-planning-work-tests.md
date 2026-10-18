# Tiny-import inventory work regression handoff

This authorized tests-first lane authored one private structural workload test
and narrow private query telemetry. Tests executed: **0**. No Cargo, tests,
builds, Clippy, formatter, runtime diagnostics, agents, additional worktrees,
commits or pushes were run. Compilation and semantic red remain parent-owned.
No production algorithm repair is included or authorized by this handoff.

The global and root guidance, constitution, catalog and all seven canonical
rules were read. The applicable behavior is US3 annotation preservation and
T097/T100 import planning. This adds evidence alongside
[the existing space repair](import-space-review-tests.md) without changing its
acceptance status or the feature's task markers.

## Frozen dependency and incremental export

- Baseline commit: `465837a95c0de9129460171ea57a5edb2d054aac`.
- Dependency: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-actor-dependency.patch`.
- Verified dependency SHA-256: `3a8fd30ab4d987148dad1678c08afaf4b40cc0e368ded496cba408b448a96d67`.
- Verified dependency tree: `3af7406383b36908adae237eba518ef801d92c13`.
- Incremental patch: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-import-planning-work-tests.patch`.
- Exact manifest: the adjacent `library-import-planning-work-tests.manifest.json`.

The manifest records the final staged tree, patch SHA-256, baseline and final
blobs and SHA-256 for each of the three authored paths, and authored/executed
counts. The frozen dependency is staged as the baseline. The exported diff is
relative to its verified tree and excludes its unchanged hunks. Shared roots,
dependencies, schema, bounds, page ceilings, rollback, cancellation and actor
retirement behavior were not edited.

## Canonical operation and observation

`StorageLibraryImportSpaceTree::read` prepares its unchanged literal query and
uses the same bindings, row conversions and layout validation. After its real
`query_row`, a private `cfg(test)` field reads
`Statement::get_status(StatementStatus::VmStep)`. A private `cfg(test)` field on
`StorageLibraryImportSpaceTrees` sums those actual statement counters during
the existing `include` calls. No query is copied into a test implementation,
and no telemetry is global or visible through a public API.

The test invokes the actual `StorageLibraryImportSpaceTrees::bound` owner used
by `StorageLibraryImportSpace::plan`, after the actual layout read, inside real
deferred and immediate transactions. This observes all canonical document,
entry and affected-index inventory reads. It does not invoke the actor-facing
freeze/commit workflow or claim to count every statement in the full space
plan. Each transaction is explicitly rolled back after observation.

Both fixtures receive the very same copied `StorageImportSessionCommitFootprint`:
one pending insertion, 1024 body bytes, 1024 projection bytes, and a largest
frame including the canonical prepared-frame header. This is a fixed typed
estimator input, not a newly generated import spool or a throughput benchmark.
The footprint is constructed once and passed unchanged to all four bound calls.

Pinned `rusqlite` 0.40.2 exposes the status API without an additional feature
(`src/statement.rs:715-726`, `877-885`). Pinned `libsqlite3-sys` 0.38.2's
`sqlite3/sqlite3.c:232008-232030` follows old overflow chains in
`statDecodePage`. Its `statNext` at `232147-232172` returns one overflow row per
page when aggregate mode is off. The unchanged production query filters
overflow pages inside its aggregates; the virtual-table rows still reach VM
execution. Source inspection therefore predicts a larger measured step count.
That prediction has not been executed here.

## Fixtures and controls before the workload assertion

Each fixture owns a new real SQLite database initialized through the canonical
migrations, using 4096-byte pages, WAL, foreign keys and FULL synchronous mode.
Actual `NotationGame`, `GameTree` and annotation objects produce documents with
one legal `e2e4` ply. The production prepared-write owner and transaction
dispatcher save both old records and their actual library projections.

Both fixtures use exactly the same two old game IDs, move IDs, rows, metadata,
indexed keys, mainline and ply count. Each small note has 16 ASCII bytes. Each
large note adds `64 * (4096 - 4)` bytes, producing 261904-byte notes within the
existing text limit. Controls require zero small-document overflow pages, at
least 128 large-document overflow pages, and actual main-file growth. They also
require equal non-overflow page counts by tree and page type, identical schema,
identical ordered metadata including every indexed column and plies, and equal
canonical allocation/touch bounds. Metadata comparison excludes the unindexed
content digest, which necessarily changes with the old body.

For each fixture, five snapshots cover the checkpointed baseline, a repeated
baseline before planning, completion of each transaction mode, and a final
checkpoint. Both truncating checkpoints must report `(0, 0, 0)`. Snapshots must
match exactly, including full main/WAL file SHA-256 and lengths, all document
IDs/revisions/body digests/lengths, metadata, schema, catalog, tree shape,
overflow count and SQLite total-change count. WAL must already be empty before
planning. Saved document digests must also equal the original encoded domain
documents. These controls distinguish old payload work from backlog, changed
index shape, pending writes or content mutation.

Fixture construction, SQLite work and file hashing run inside the existing CPU
executor. The test closes both SQLite connections, explicitly removes and
verifies both temporary directories, and awaits executor shutdown before any
assertion. It then checks all controls and emits the real counters before its
final workload assertion. The final requirement is that growing only old note
overflow cannot increase either inventory step count when the structural work,
indexed inputs and pending footprint are equal. There is no timing threshold,
sleep, cache-flush trick, new dependency, progress hook or trace callback.

## Parent red gate

Exact authored test name:

```text
storage::library_import::space::trees::planning_work_tests::tiny_import_inventory_work_is_independent_of_checkpointed_old_note_bytes
```

Suggested parent invocation, not executed in this lane:

```text
cargo test --locked --all-features --lib storage::library_import::space::trees::planning_work_tests::tiny_import_inventory_work_is_independent_of_checkpointed_old_note_bytes -- --exact --nocapture
```

Preserve the actual exit status and complete output, including overflow counts
and both transaction-mode counters. A compilation, fixture, preservation or
cleanup failure is not the requested semantic red. If the counters do not grow,
preserve that result and investigate the observer before authorizing repair;
do not add an artificial failing threshold. VM instructions count virtual-table
row processing, not every pager read or byte inside a virtual-table callback.
The source's per-overflow-row return is why this counter is expected to expose
this particular workload. This handoff makes no measured claim about that
expectation, latency, cancellation responsiveness or writer occupancy.

Only after the parent observes and preserves the final structural workload red
should it delegate a new production-repair round. The proposed covering capped
key probes and typed exact/at-least cardinality remain future design work.

## Parent execution

After checkpoint `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`, the parent
formatted the authored source and corrected three fixture SQLite integer-boundary
compile errors. These compile errors are not semantic-red evidence.

The exact test then executed with the pinned toolchain, default parallelism and
the shared target directory. It exited 101: zero passed, one failed and 551
filtered. All fixture-shape, checkpoint, preservation and cleanup controls passed
before the final workload assertion.

| Observation | Small old notes | Large old notes |
| --- | --- | --- |
| Document overflow pages | 0 | 128 |
| Deferred inventory VM steps | 658 | 2,578 |
| Immediate inventory VM steps | 658 | 2,578 |
| Additional-page bound | 44 | 44 |
| Old-tree-page bound | 14 | 14 |
| Allocator-operation bound | 86 | 86 |

Log: `library-import-planning-work-semantic-red-2.log` in the session scratch
directory. SHA-256:
`ea12d76984e97e23b407dd6199e51e8624c8ae0d688151d5ec4c4c819f730ad9`.
Frozen production/observer blob: `1e8ae8e5d9cd07f602cc3d918de1d16eff83817d`.
Frozen test blob: `cb19a39f15e3dfb7ad516bd405edc0160b36773e`.
The source, complete log and manifest are preserved in
`.git/parallel-patches/import-planning-work-baseline-20261017/`.

The parent authorized a new source-only production-repair round from this
observed regression. No repaired result or canonical gate is claimed here.


The subsequent [bounded planner source handoff](bounded-import-planning-repair.md)
preserves this semantic-red baseline and records the incremental source changes
and unexecuted parent counterchecks.
