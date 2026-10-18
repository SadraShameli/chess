# Saved puzzle run inventory

## Scope and test plan

This initially source-only slice continues the authorized implementation without reducing
any of the 13 journeys. It owns only the puzzle repository, the necessary typed
storage actor command/client additions, their existing integration test file and
this evidence. The parent owns registration, migration compatibility, task status,
composition and the exclusive execution lane. No migration or schema change is
part of this read-only query.

Before implementation, author deterministic checks using the existing real puzzle
owner, transactional repository and isolated SQLite fixtures:

- Empty/single inventories, current revisions, close/reopen durability and mutual
  isolation from the game document inventory.
- Multiple bounded pages, numeric identity ordering, repeatable exclusive
  cursors, an absent cursor identity and an insertion below a retained cursor.
- Decimal identities across `i64::MAX` through `u64::MAX`, including exclusive
  exhaustion at the maximum and the maximum storage revision.
- Noncanonical, zero, overflowing, oversized and malformed identity/revision
  metadata, with whole-page refusal rather than partial results.
- Stored event-count bounds, including the schema's rejection of invalid counts.
- Missing referenced collection and checkpoint rows, which must remain visible
  to the query and produce typed corruption.
- Semantically invalid collection, checkpoint and journal bodies: inventory
  remains metadata-only while the existing load path refuses recovery.
- Shared CPU waiting, bounded queue admission, cancellation of accepted callers
  and shutdown while a borrowed response wait remains live.

The API is absent in the starting source. These are new-contract tests authored
before production implementation. Missing-API compile failures would not be
semantic red evidence. Cargo, builds, tests and CPU diagnostics were withheld
until the parent explicitly granted the exclusive execution lane.

## Verification status

The ten new inventory tests and all 52 affected storage tests passed. The
all-feature library/storage Clippy selection passed with `-D warnings` and zero
warnings. The test builds emitted the existing macOS linker warning described
below. The parent's reported 1,029-test baseline predates this slice; these new
results establish the inventory checks independently of that baseline.
T187's full/native acceptance, T190 and T189 remain open. No installed dataset
catalog, validated recoverable-run list, latest/recency selection, history deletion
or new-run factory is implemented here. Only the existing `load` operation proves
recovery and the recovered run's phase.

## Stable API and implemented source

```rust
StoragePuzzleRepository::INVENTORY_PAGE_SIZE: usize

StoragePuzzleRepository::inventory(
    &self,
    after: Option<PuzzleSessionId>,
) -> Result<StoragePuzzleRunPage, StoragePuzzleError>

StoragePuzzleRunPage::entries(&self) -> &[StoragePuzzleRunMetadata]
StoragePuzzleRunPage::next_cursor(&self) -> Option<PuzzleSessionId>

StoragePuzzleRunMetadata::id(&self) -> PuzzleSessionId
StoragePuzzleRunMetadata::storage_revision(&self) -> PuzzlePersistenceStorageRevision
StoragePuzzleRunMetadata::persisted_event_count(&self) -> usize
```

`inventory` is asynchronous. The page size reuses
`StorageDocumentStore::INVENTORY_PAGE_SIZE`, currently 256. A full page supplies
its final identity as the next exclusive cursor. A short page supplies `None`.
An exactly full final page therefore requires one final empty query. Callers
must stop at `None`; passing it again starts a new enumeration.

Each query uses one SQLite statement on the existing storage actor's connection.
It selects only run identity, storage revision, event count and indexed related-row
existence checks. It never selects collection/checkpoint bodies or reads event or
attempt rows. `EXISTS` retains a run with missing related rows so validation returns
`StoragePuzzleError::Corrupt`. Every selected row is validated before the page can
be returned, with no partial-success result.

The cursor predicate and ordering compare `(length(run_id), run_id COLLATE BINARY)`
explicitly. No signed numeric cast is used. Identity and revision transport fields
are each capped at 20 bytes before Rust allocation and validated as nonzero `u64`
values whose decimal rendering exactly equals the stored text. Counts use checked
integer conversion and the codec's existing event bound. Inventory does not verify
that the declared event count matches journal rows or that bodies/proofs recover.

The typed actor command reuses canonical admission and memory reservations before
enqueueing. It retains admission while queued/executing, even when its caller is
canceled. The response page retains its fixed-capacity entry-buffer reservation
until dropped. The actor obtains and releases its existing shared CPU lease around
the statement. The client retains no strong owner-channel sender across the
borrowed response wait. Closed storage refuses new inventory requests.

Pagination is an identity traversal with current metadata per statement. It is
stable for an unchanged collection and excludes identities at/below the supplied
cursor; it is not a cross-page database snapshot. Inserting a lower identity does
not cause repetition on a later page. IDs provide no recency ordering. The current
schema has no expression index for numeric identity ordering, so SQLite may scan
and sort run metadata. Returned page and application entry-buffer sizes are bounded;
no production-scale query latency or SQLite scratch-memory measurement is claimed.

## Authored checks and handoff

Ten new integration cases were written before the inventory implementation in
`tests/storage/puzzle_repository.rs`. Existing fixture ownership now accepts a
typed test identity, while the original fixture entry point retains its behavior.
The new cases use real transactional saves and SQLite corruption fixtures. Body
damage is restored independently after each refusal so each collection, checkpoint
and journal check starts from a recoverable saved run. SQLite CHECK constraints
remain enabled; invalid event counts are rejected by the existing schema, and
the upper valid metadata bound is listed even though the absent journal prevents
recovery. Malformed identity rows retain consistent foreign-key references. The
missing-collection fixture briefly disables foreign-key enforcement only on its
test injection connection to create a dangling reference, then restores and
asserts enforcement before calling inventory. Production actor enforcement is
unchanged.

Queue and shutdown cases hold an explicit shared CPU lease and poll borrowed
futures. They check capacity refusal after canceling an accepted caller, eventual
admission release, closed admission and successful draining while a response wait
remains live. No sleep or production data path is used.

No existing persistence defect was changed. The first execution found two
fixture setup failures, detailed below; they are not semantic reds. The new API
compiled without correction, and its implementation required no change after
execution. Consolidated parent composition and native gates remain with the parent.

Executed in this lane: direct `rustfmt --edition 2024` on the three owned Rust
files, its explicit-file `--check`, scoped Git whitespace checks and a scan of
added Rust lines for comments, suppression/panic helpers and forbidden dash
punctuation. These static checks passed. The initial attempt to invoke the
formatter through `rustup run 1.99.0` failed because `rustup` is absent; the installed
formatter reports `rustfmt 1.10.0` and was used directly. No Cargo/compiler,
build, test or CPU-diagnostic command ran before the exclusive-lane grant.

## Exclusive-lane execution

The owner granted this existing worktree the exclusive Cargo lane after the
parent's canonical `make verify` exited 0 and checkpoint `d703763` was committed.
This worktree stays on `10169bd9aa10431aea370ed7f1dbda5bd0f8a1e3`; no parent source,
root registration, dependency or migration was copied. The compiler and Cargo
both report 1.99.0, matching `rust-toolchain.toml`. All build/test/lint commands
used `CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target`. No job or test-thread
cap was set. Commands ran sequentially.

One initial version-only `cargo --version` invocation omitted that environment
prefix. It built nothing and was repeated with the required prefix; every
subsequent Cargo invocation, including metadata, used it. The slip was reported
to the parent when discovered.

Exact Cargo check commands:

```sh
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --test storage saved_run_inventory_ -- --nocapture
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --test storage
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo clippy --locked --all-features --lib --test storage -- -D warnings
```

Execution results:

- `01-inventory.log`: compiled, then 8 passed and 2 failed, with 42 filtered out;
  exit 101. Both failed before inventory when fixture mutations returned SQLite
  `ConstraintViolation`, extended code 787, `FOREIGN KEY constraint failed`.
  The original identity fixture rewrote a referenced run primary key; the
  missing-collection fixture attempted an orphan with enforcement active.
- Only `tests/storage/puzzle_repository.rs` changed after that result. The identity
  fixture now inserts/removes malformed rows with their own checkpoints, keeping
  references consistent. The orphan fixture explicitly injects corruption as
  described above. No production fix or observed semantic defect is claimed.
- `02-inventory.log`: 10 passed, zero failed/ignored, 42 filtered out; exit 0.
  This repeats the first command against the corrected fixtures.
- `03-storage.log`: 52 passed, zero failed/ignored/filtered out; exit 0.
  These are 42 pre-existing cases and the ten inventory cases, including the
  existing persistence, queue, capacity and retained-shutdown checks.
- `04-clippy.log`: all-feature library/storage checks passed; exit 0, zero warnings.

All test invocations emitted the pre-existing debug application linker warning:
`ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected`.
Rust reports that `linker_messages` ignores `-D warnings`. It remains unsuppressed;
the Clippy invocation itself emitted no warning. No out-of-scope checker, dependency,
build-profile or linker change was made.

Exact command records, raw combined stdout/stderr, exit codes, toolchain/base
versions and source hashes are retained in
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-run-inventory-logs/`.
`01-source.sha256` identifies the initial staged Rust source; `02-source.sha256`
identifies the corrected source used by all successful checks. The final four
owned files are recorded in `source-final.sha256`; `patch.sha256` identifies the
refreshed full patch, and `SHA256SUMS` covers the log artifacts.

All three tool execution sessions were observed to exit. The final process
snapshot in `active-build-processes.log` contains zero Cargo, rustc, Clippy or
rustfmt processes. The exclusive lane was explicitly released before the final
static evidence/patch export. No further Cargo or CPU-diagnostic work ran afterward.
This lane did not rerun full `make verify`, execute native acceptance or benchmark
production-scale query latency. Those results must be established after parent
composition, rather than inferred from the isolated storage checks.

The handoff is an explicit-path staged binary diff at
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-run-inventory.patch`.
It contains only the four assigned paths. No commit, push, additional agent,
additional worktree, parent source edit or migration/schema edit was made.

Applicable policy was read from the [canonical catalog](../../../.specify/rules/README.md)
and all seven referenced rules. This evidence records concrete scope and outcomes;
the policy remains in its canonical owners.
