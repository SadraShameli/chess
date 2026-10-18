# Native library adoption and played recovery

## Scope and evidence status

This is a source-only isolated handoff from parent commit
`647147d9ff95cb745ede46637048ef312e3070af`. It implements the US3 saved-library
and US5 compatible-data recovery slice described in [the specification](../spec.md),
[the game and storage model](../data-model.md), [the native plan](../plan.md),
[the study contract](../contracts/study-tools.md) and
[the storage decisions](../decisions.md#storage-imports-and-backup-recovery).
The [canonical rules](../../../.specify/rules/README.md) were loaded in full.
No task acceptance marker changes in this handoff.

No Cargo, tests, Clippy, build, application runtime, CPU diagnostics, agents,
commits, pushes or additional worktrees were run or created in this lane.
All new contracts remain authored and unexecuted. Formatting and Git patch checks
do not establish compilation, behavior, performance or platform acceptance.

## Parent-observed regression authorization

The supplied tests-only patch was SHA-256 verified and applied before production
edits:

```text
/Users/sadrashameli/Personal/chess/.git/parallel-patches/native-library-recovery-tests.patch
2cad201cb5fbdf90e3c5e2b59db09445952ccef432d523d380151423805bbb78
```

The parent's exact observed command was:

```text
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --test desktop does_not_block_played -- --nocapture
```

It exited 101 with 0 passed, 2 failed and 219 filtered on `5222fe8`. The raw
`/tmp/claude-chess-pgo-diagnosis-20261016/native-library-recovery-red.log` was read
in this lane. Its SHA-256 is
`932889d631de596923453185818c294fe23b52d67bf470fa7dbb441723587d50`.
The failures were `ContentKind { expected: Played, found: Notation }` and a JSON
unknown-field error for `workspace`, each wrapped in Restoration. The native
service blob on that parent-observed baseline is identical to this checkout's
unmodified service blob. These are parent results, not child executions. The
existing macOS linker warning in that log is separate from the assertion failures.

The original two semantic contracts remain in the service suite. Their fixture
now also supports the new readiness assertions and two additional authored
corrupt/future-record cases. A tests-only index tree was frozen before staging
production changes:
`58cc9745df93a089dec076f3ad9bc3f819b888fa`.
That tree records the final test sources over the unmodified production baseline;
it is not a commit or an executed red. Initial inventory/startup contracts were
authored before implementation. Additional edge contracts were authored during
source review. Missing new APIs or module registrations are compilation gaps,
not semantic red evidence.

## Inventory and ownership

`StorageLibraryInventoryRepository::new(client)` dispatches `page(cursor)` through
the existing `begin_library` actor. Its cursor owns an optional `GameId`; entries
carry `GameId` and `GameStorageRevision`. Pages contain at most the canonical
100 entries, their catalog revision and an exclusive stable-ID continuation.
There is no updated-time keyset, OFFSET, runtime, connection or second actor.

The Deferred transaction joins documents to projections, requires matching
content revisions, played content, Library location and no Trash timestamp,
and orders only by canonical ID. Stale projections, notation and Trash are
excluded. The existing adoption-page response is an internal transport so the
other worker's repository response matches need no edit. The public facade
validates game IDs and retains that transport's shared byte reservation. The
reservation covers both bounded candidate and typed-entry vectors until the
public page is dropped. No existing repository private field is accessed.

`DesktopNativeLibraryStartup::new(client, executor, deadline)` owns one page,
one candidate position, either one retained read/preparation future or one
mutation receipt, and any prepared value awaiting enqueue. `advance()` observes
one operation; `run()` drives it to a terminal classification. A cancelled caller
wait leaves the operation, page, current prepared value/receipt, original absolute
deadline and owner intact. No task is spawned. The cursor advances only after
Unrelated, Issue, Changed, observed enqueue refusal or an observed adoption result.

Prepared adoption stays owned until the next advance enqueues it, then the
mutation receipt stays owned until observation. `drain_retained(cleanup_deadline)`
observes already-started operations without starting another candidate or
queuing an unsubmitted prepared value. It preserves the startup deadline.
The game service retains this owner inside its existing restoration future;
shutdown retains an incomplete future or receipt across caller timeout/retry.

The existing canonical decoder classifies every candidate. Unrelated documents
remain generic. Issues retain UUID, observed content revision and typed cause.
At most 100 reports are retained, with a separate saturating total observation
count. Repeated observations across reconciliation sweeps may repeat reports;
the count is not a distinct-document count. Omitted reports never produce Ready.
Changed candidates remain explicitly unresolved even if a subsequent sweep can
adopt their latest revision. No prefix-based skip policy is introduced.

## Reconciliation and readiness

Each page exposes its observed catalog stamp. Each Inserted adoption receipt must
be exactly the next expected stamp. A mismatch detects intervening writes even
when a later own adoption would otherwise hide them. An additional empty page
observation after the last candidate checks the tail and catalog again.

Unexpected stamps restart from the beginning for at most three sweeps. This finds
new candidates inserted behind the cursor without an unbounded ID map. Continued
churn yields explicit unresolved catalog state. Issues persist through sweeps.
The existing generic writer already bumps the catalog in its transaction; future
candidate writers must preserve that invariant. Generic unindexed counts are
never used as readiness evidence, because unrelated documents remain unindexed.

The typed status is `Adopting(progress)`, `Ready(catalog)` or
`RecoveryRequired(Arc<summary>)`. The service's read-only `library_readiness()`
returns an owned snapshot; it exposes no watch lock to callers. Recovery issues
or observed catalog changes invalidate Ready. A Ready value certifies a settled
classification at its stated catalog revision, not future writes or UI readiness.

The service adopts first, then consumes typed played inventory with stable IDs.
Only decoded actual played Setup/Active/Interrupted sessions enter the existing
registry. The canonical codec restores those sessions and clocks paused without
downtime charges. Notation never creates seats, clocks or a runtime session.
Individual corrupt/future bodies or recovery races are reported while valid
played sessions continue recovering. Global page/database/registry failures remain
explicit restoration errors. No blanket success fallback hides corrupt played
content. Original bodies, IDs, content revisions and Unknown modified times remain
owned by the existing adoption transaction, without a rewrite.

## Authored contracts

The two new inventory contracts cover 105 ordered played records over two pages,
insertion order and modified-time independence, typed revisions, repeatable ID
continuation, notation/Trash/stale exclusion and a retained page sharing and
releasing the existing document byte budget.

Nine new startup contracts cover mixed legacy schema-2 played/notation/unrelated/
corrupt/future data over 100 candidates, byte/revision preservation and Unknown
times, Changed before preparation and before adoption, insertion behind the
cursor, retained cancelled preparation/adoption waits and deadline, bounded issue
reports, settled classification with unrelated unindexed documents, observed
cleanup after deadline expiry and bounded sweep exhaustion under continuing churn.
They use real domain objects and storage, controlled capacity and explicit Tokio
time advancement. No wall-clock sleeps were added.

Two new service contracts require corrupt/future reports alongside recovery of
both valid played sessions. The two supplied semantic regressions also continue
to assert paused snapshots, preserved IDs/history/clocks and no non-played factory
admission. No contract in this delivered snapshot has been executed in this lane.

## Parent wiring and acceptance gates

Parent owns these integrations, all excluded from the patch:

- Add `pub mod library_inventory;` to `src/storage/mod.rs` and
  `pub mod library_startup;` to `src/desktop/native/mod.rs`.
- Include `tests/storage/library_inventory.rs` as `storage_library_inventory` and
  `tests/desktop/native/library_startup.rs` as `desktop_native_library_startup`
  from their existing integration roots. The startup tests reuse crate-visible
  helpers from the already registered `desktop_native_game_service` suite.
- The other repository worker supplies
  `StorageLibraryRecovered::into_record(self) -> StorageLibraryRecord`.
- Combine production played-save routing and injected UTC/cause handling before
  validating resumed play, shutdown persistence, organization or full readiness.
  Adoption indexes recovered games; the current generic save path intentionally
  rejects indexed writes. This snapshot alone is unsafe to expose organization.
  No supervisor, registry, bootstrap, document store or save adapter is changed.
- Register modules, compile, execute the two observed regressions and all new
  contracts, then perform required parent-owned checks and real lifecycle review.
  Compilation or missing registration is not a semantic red for another repair.

The service retains its current public constructor and restore/start boundary.
Its startup deadline defaults to 60 seconds from construction. The standalone
owner accepts an injected absolute deadline; large-library timing and any
production deadline-policy adjustment remain parent review and measurement.
No one/max-worker, whole-process memory, large-corpus latency or platform result
is claimed by these source contracts.

## Frozen baseline and export

The exact existing baseline blobs are:

| Path | Baseline blob |
| --- | --- |
| `src/desktop/native/game_service.rs` | `4938374eb8c923b87264e0e94b6f7c2bc6024a23` |
| `src/storage/library_transaction.rs` | `064ef1566f9b373c17ae89a4141b0a998860eaaa` |
| `src/storage/library_transaction/rows.rs` | `c9fae82acb355b61fd1f57528db01937bcdcec2e` |
| `tests/desktop/native/game_service.rs` | `ef615b136ee606083f920f6b238426a8588e5bed` |

The other five owned paths are new at this baseline. The nine-path staged binary
export is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/native-library-adoption-recovery.patch`.
Its SHA-256, frozen staged tree and per-file SHA-256 values are delivered alongside
it in `native-library-adoption-recovery.manifest`. The artifact's own checksum is
kept outside this evidence file to avoid a recursive checksum dependency.

Static checks are standalone Rust 1.99.0 toolchain rustfmt 1.10.0 formatting/check,
Git whitespace, exact owned paths, frozen baseline forward application, local
reverse application and reconstructed-tree equality. These checks do not compile
or execute Rust. The snapshot is frozen for parent execution after export.

## Parent integration progress

The parent registered both production modules and integration suites and supplied
the exact consuming `into_record()` accessor from the played-save owner. The
merged desktop/storage test targets compiled successfully with the shared target
directory and pinned toolchain.

The typed inventory suite passed both cases: zero failures or ignored tests and
159 filtered. The retained startup suite passed all nine cases: zero failures or
ignored tests and 223 filtered. Those executions cover bounded mixed-document
adoption, retained cancellation/receipts, cursor reconciliation, issue limits,
deadline cleanup and stable played inventory. They do not establish the service
regressions or resumed-play persistence, which require the production played-save
adapter and wall-time integration before running.

Actual commands were:

```text
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --test desktop --test storage --no-run
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --test storage storage_library_inventory:: -- --nocapture
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --test desktop desktop_native_library_startup:: -- --nocapture
```

All exited 0. The existing macOS debug linker warning remains unsuppressed.
No canonical, native GPU, other-platform or full readiness acceptance follows
from these focused results. Raw logs remain in the parent integration scratch
pending the combined checkpoint's durable artifact copy.

## Parent canonical integration

The combined source and all relevant regressions were executed by the parent.
[Verified integration](library-runtime-import-integration.md) records the exact
snapshot, 1,666 passing tests, three clean Clippy selections, both optimized
builds, durable log hashes, known linker warning and remaining acceptance gates.
Earlier unrun statements describe this writer's isolated handoff only.
