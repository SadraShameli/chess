# Atomic library import source handoff

The original handoff history below is retained. The later authorized repair is
documented in [Space repair after the parent semantic red](#space-repair-after-the-parent-semantic-red)
and [the current source/test handoff](import-space-review-tests.md). Its current
incremental export and evidence limits supersede the original space policy.

This is the source-only US3 T097/T100 adapter and transaction handoff from
`t3-307c0e78`. It is uncompiled and unrun. T097/T100 remain pending, including
space-bound and scale acceptance. Source formatting, static arithmetic and patch
checks establish neither semantic reds nor successful SQLite behavior.

The mandatory global/project instructions, constitution, all seven canonical
rules, feature plan, relevant specification/data-model sections, storage and
identity decisions, study/message contracts and current owner evidence were
read. No Cargo command, test, build, Clippy, application/runtime check, CPU
measurement, agent, commit, push or extra worktree was used. No task markers or
checker settings changed. No additional existing defect was repaired.

## Exact baseline and exclusions

The full supplied dependency is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-runtime-import-current.patch`.
Its SHA-256 is
`cee393a24f2a999675a030bdc3199c39d8daf7eda431c5e65954c3a7af40f7a1`.
The JSON manifest, base commit
`647147d9ff95cb745ede46637048ef312e3070af`, all 67 staged paths and staged tree
`e9219be3c2255064d9611ac5ae5ac37c8a9fc773` were verified both immediately after
application and again before staging this slice. The final patch compares against
that supplied tree. It excludes dependency-only hunks, local/shared module and
test roots, codecs, schemas/migrations, dependencies/Cargo, selection/domain,
supervisor, presentation and mounted UI.

The parent's reported dependency component results were 74 import-framework,
22 lifecycle, 26 supervision, 18 played plus two writer, 232 desktop, 451 domain
and 15 play-panel executions. The parent subsequently reported canonical
`make verify` exit zero with 1666 tests, three Clippy selections and both optimized
builds, published as `e618533aa6f2f74da54b3d745f2b7b4ac19ecbc3` with matching remote
SHA. Those are parent-supplied dependency results, not executions or acceptance
of this new source. The dependency source blobs remain this handoff's baseline;
parent evidence additions do not change it.

The adjacent `library-atomic-import.json` records every exported path's baseline
and final Git blob/SHA-256, the staged tree and patch SHA-256. The adjacent
`library-atomic-import-baseline-307c0e78/` retains the supplied dependency metadata,
initial tests-first source, static cost calculation and verification evidence.

## Parent integration

Register `pub mod library_import;` in the existing storage module root. Re-export
`StorageImportSessionCandidateCursor` with the other model types in the existing
import-session root. Change that root's sole initial
`.query(ordinal, 0, token)` to
`.query(ordinal, StorageImportSessionCandidateCursor::start(), token)`.
These three parent-owned root changes are deliberately absent from the export.
The new adapter includes `tests/storage/library_import.rs` privately; do not also
register that file as an integration suite. Existing native startup tests retain
their existing root registration.

The public adapter is:

```rust
StorageLibraryImportRepository<W: GameStorageWallTimeProvider + ?Sized + 'static>
StorageLibraryImportRepository::new(
    client: StorageDocumentClient,
    destination: StorageImportSessionDestination,
    wall_time: Arc<W>,
) -> Self
```

It implements every existing `StorageImportSessionRepository` port. Its associated
projection type is opaque `StorageLibraryImportPreparedProjection`, with only
`AsRef<[u8]>` exposed. The candidate cursor owns `Option<GameId>` and provides
`start()`, `after(GameId)` and `boundary()`. Candidate query construction/cursor
access and candidate-page construction/continuation now take/return this cursor;
metadata-page and staged-game ordinal cursors retain their separate contracts.

The crate-private retained certification bridge is:

```rust
StorageLibraryRepository::begin_certify_adoption(
    &self,
    expected: StorageLibraryCatalogRevision,
) -> Result<StorageLibraryAdoptionCertificationPending, StorageLibraryError>
StorageLibraryRepository::begin_revoke_adoption(
    &self,
) -> Result<StorageLibraryAdoptionCertificationPending, StorageLibraryError>
StorageLibraryAdoptionCertificationPending::wait(
    &mut self,
) -> Result<StorageLibraryAdoptionCertification, StorageLibraryError>
```

The returned certification exposes crate-private `catalog()` and
`is_certified()`. The one transaction dispatcher now takes
`(&mut Connection, request, &Arc<Semaphore>, &mut bool, Uuid)`. Its actor caller and
narrow private fixtures are updated here. No second writer, connection, runtime,
executor, actor, memory pool or identity/projection registry is introduced.

## Ownership and atomic behavior

Actor certification initializes false on every open/reopen. Only exact-current
`CertifyAdoption(expected)` can set it true. Typed content, organization, Trash,
restore and import operations retain coverage as the catalog advances. Every
successful generic Save invalidates it after commit; failures preserve it.
Snapshots read certification, current catalog and exact destination revision in
one actor transaction. Unrelated generic documents may remain unindexed.
Neither an unindexed count nor cached native Ready(C) certifies import readiness.

Native startup completes its settled issue-free sweep before retaining and
observing certification. A raced handshake reconciles within the existing sweep
budget; an exhausted race observes revocation before publishing recovery.
Recovery findings enqueue retained revocation, and the service observes it before
finishing restore/start admission. Deadline cancellation retains command handles
for cleanup. The existing unsettled-sweep test now drains the new final retained
handshake before asserting its unchanged RecoveryRequired result.

Candidates use the actual persisted rules variant, canonical initial FEN, ply
count and canonical JSON UCI sequence. Their keyset is
`game_id > after ORDER BY game_id LIMIT 101`; at most 100 candidates escape.
Document/projection revisions must match. Trash candidates participate without
mutation. Full identity comparison remains in the framework. Candidate reads
check requested ID/revision, projection freshness and body SHA in one actor
snapshot. Typed stale-candidate failures remain distinct from storage failures.

Real candidate bodies move one String and its producer permit into shared
reserved backing. Every clone retains that same guard. No String-to-Arc<str>
payload copy escapes the producer budget. Bounded owned Arc<str> fixtures remain
available separately. Candidate pages also retain their output charge.

BodyProof now retains the exact originating record reference, checked by the
real adapter. Its constructor remains sealed. Preparation reuses
`StorageLibraryProjection::from_record`, the existing prepared identity builder
and canonical normalization. The private versioned wire binds origin, game ID,
body length/SHA, notation/no-mode, every projected value/key and FEN/UCI.
Projection/string retention and serialization capacity are charged before their
allocations through existing prepared-memory growth. A counting writer determines
JSON capacity before the reserved writer allocates; there is no temporary JSON
payload. Actor recovery validates typed constructors, canonical keys/FEN/UCI,
actual body/frame hashes, ID and preparation origin without decoding a chess tree.

CommitImport owns the sealed PreparedCommit, cancellation token, once-sampled
validated UTC and budget-origin permit. Existing preparation ownership policy
checks the request, and the private frame origin rejects foreign actor
preparation. Receiving admission reserves the largest frame plus the existing
checked prepared decode allowance. The actor retains admission, byte charge and
capacity lease through actual commit/rollback and result publication. The
framework's existing retained commit future owns the admitted receipt even when
one caller wait is cancelled.

One BEGIN IMMEDIATE rechecks coverage, catalog and exact folder, calls
`prepared.recheck_space()` inside the transaction and recomputes the current
layout estimate. Any increased allowance refuses. It streams one verified frame
at a time through the content owner's factored insert-only primitive. New rows
have revision 1, Library location, favorite false, empty tags, default organization
revision and exactly the frozen folder. Existing IDs cannot be overwritten.
Count/body/projection totals, EOF and `prepared.finish()` must all agree before
one catalog bump and the final cancellation checks. Only successful COMMIT
returns the actual receipt. Primary and rollback failures remain available;
an unresolved transaction stops the actor instead of admitting another command.

## Space repair after the parent semantic red

The later source-only repair in `t3-b9b15da1` supersedes the original estimator
and its 43.89 MiB/tiny-game, 42.14 GiB/1000-game and 41.15 TiB/million-game
reference costs. The parent observed the valid checkpointed-overflow regression
before this production change. Exact inputs, command, count, log hash and the
11 new unrun counterchecks are recorded in
[the space review evidence](import-space-review-tests.md). This source does not
establish a green regression or complete T097/T100 acceptance.

The repair starts at commit `465837a95c0de9129460171ea57a5edb2d054aac` plus the
41-path `library-annotation-review-current.patch`, SHA-256
`4cad29d61a745483cf92cff20a08b7e1365c5f378e16201de555c779243c5d27`, producing
exact baseline tree `d6aa39759f9be323023c0f4c4e6e404542c2c72d`. Its incremental
export excludes unchanged dependencies, roots, metadata, annotation/editor,
codecs, domains and migrations. The snapshot-memory regression stays untouched.
The JSON manifest beside `library-import-space-repair.patch` in the parent's
`.git/parallel-patches/` records the final source tree and all 13 exported paths.

The pinned `libsqlite3-sys 0.38.2` amalgamation has SHA-256
`0a409f1633283fa31a9126b11fbfd64a1991c5d30defad07e5745d4667f5e23d`, checked
again during this repair. Its relevant source locations are:

- Cursor depth cap 20 at 72600; payload limits at 76577-76580; local/overflow
  selection at 80255-80277 and overflow capacity S-4 at 80345-80386.
- Allocation with `BTALLOC_ANY` at 79647-79980 and freeing at 79983 onward.
  With auto-vacuum disabled, allocation visits one freelist trunk, not an
  unbounded exact-page search. Its header/trunk/selected-page writes are bounded.
- Balance outputs at 81670-81715, old-page reuse and allocation at 81805-81831,
  one child for root deepening at 82195-82200, and at most one deepening per
  insertion at 82276. These do not allocate every output page anew.
- WAL overwrite of a page already spilled by this transaction at 71582-71617;
  only the final commit-marker frame can append a second copy. FULL sector
  padding follows at 71619-71648. The sector sanitizer caps at 65536 bytes at
  62299-62307. Prior committed frames are never counted as overwritable.
- WAL index capacities at 68057-68071: first block 4062 frames, later blocks
  4096, each block 32768 bytes. Checkpoint extends the main file only when
  needed at 69718-69733. WAL undo restores its index/header without truncating
  spilled file bytes at 71210-71248.
- Statement journal spill default 64 KiB at 24042, anonymous delete-on-close
  open at 64125-64137, and records of S+4 bytes at 64151-64176. Savepoint bitmaps
  journal a page once per savepoint at 60663-60684. Canonical import statements
  have no enclosing explicit savepoint; statement release resets the logical
  record index at 66638-66648, while commit/rollback closes/reset journals at
  61395-61406. `libsqlite3-sys/build.rs:156` enables dbstat.
- `max_page_count` takes effect through `sqlite3PagerMaxPageCount` at
  63446-63462; the pager rejects a page beyond it with SQLITE_FULL at
  65194-65204. Its returned value can differ from a request below current size.

### Payload, allocation and dirty-page accounting

Let S be page size, N current logical page count, D0/W0/H0 actual main/WAL/SHM
lengths, and C/B/P the sealed count/body/projection totals. Zero reserved bytes,
auto_vacuum=0, UTF-8 and WAL remain required. Unsupported persistent triggers on
modified tables refuse. Index column metadata must match the supported canonical
field family; expression/unknown or multiply variable-key layouts refuse.
Temporary trigger instrumentation conservatively charges every existing main
page as potentially dirty. It grants no general custom-trigger/temp-database
extension contract. No global connection-layout checker or schema was changed.

`dbstat` inventories only the two changed table trees and their actual indexes.
It streams non-overflow page counts, row/entry counts and actual depths. Existing
unrelated overflow pages are read by the inventory but are not charged as dirty
because insert-only canonical statements neither rewrite nor delete their cells.
The catalog's single small revision row and page 1 receive separate dirty slots.

Documents alone receive B, plus a per-record bound of 4*9+36+1 for serial header,
UUID and revision. Library rows receive P, plus
28*9+36+2*20+32+8+36+5*8+7 bytes for header, UUID, revisions, digest, timestamp,
folder, integers and the location literal. These fixed components intentionally
overlap some wire content. Index records receive their own column/header bounds;
only player-key indexes receive P, and no index receives document bodies. FEN
keys use the validated 512-byte ceiling. A serialized projection contains at
least the UTF-8 bytes of its retained variable values. The largest sealed frame
bounds each record's variable payload before insertion, and recovery verifies it.

For total payload T, per-record maximum M and record count C, overflow is zero
when M <= maxLocal. Otherwise an upper bound is
`floor(T/(S-4)) + min(C, floor(T/(maxLocal+1)))`: each overflowing record has at
least maxLocal+1 payload bytes, and its overflow chain needs at most
`1 + floor(payload/(S-4))` pages. maxLocal is S-35 for rowid table leaves, or
`floor((S-12)*64/255)-23` for indexes. This retains local payload credit and
per-record rounding without multiplying B across unrelated indexes.

For each tree, let d be its current level count and
`h = min(20, d + max(C-1, 0))`, the maximum pre-insertion level count in the
batch. Each insertion can deepen once. A per-insertion allocation bound is
`A = C*(2*h+1) + overflow`: up to two additional siblings per balancing level
and one root child. A global structural bound is `R+C+1` for an index, or
`2*(R+C)+1` for a rowid table, plus new overflow. Non-root index pages have an
entry; rowid table leaves have records and interior pages branch. Each tree's
chosen extension is the minimum of those bounds. The global bound is used
without subtracting already allocated tree pages, deliberately retaining slack
rather than treating a final page count as proof of transient allocation.
For fresh 2-table/12-index data its structural term grows at about 16 pages per
record, before schema-specific overflow. It does not grow at 1400 pages/record.

E is the summed chosen extension, capped with N at SQLite's 0xfffffffe page
limit. A scoped pager ceiling enforces N+E during the entire write transaction;
if SQLite's transient shape needs more, it fails SQLITE_FULL and rolls back.
The structural argument by itself does not assert transient success.

An existing changed tree contributes at most
`min(old_nonoverflow_pages, C*(4*h+1))` dirty page IDs: three old siblings and a
parent at each level, plus the directly changed leaf/root. Allocator operation
count is bounded by `sum(A + 3*C*h)`, including potentially freed old siblings.
Existing freelist dirty pages are capped by both its real count and three pages
per such operation. These page-one/trunk/selected-page slots overlap other
counts conservatively. Add two header/catalog slots and cap old IDs by N.
Then `U = bounded_old_dirty_IDs + E`. Distinct-ID accounting, not accumulated
write calls, bounds WAL growth even with repeated spills. This is a proposed
source proof requiring parent review, not an observed allocation trace.

### Additional file growth and scoped cleanup

With `F0 = ceil(max(W0-32,0)/(S+24))`, existing WAL length includes retained or
partial high-water frames, so treating F0 as append-protected is conservative.
A nonempty WAL must have a valid recognized magic and matching page size.
Let `Ppad = ceil(65536/(S+24))` and `F = U+1+Ppad`. The file-length allowance is:

- Main: `max(0, (N+E)*S-D0)`. Uncheckpointed N*S-D0 backlog remains included.
- WAL: `max(0, 32+(F0+F)*(S+24)-W0)`. It reserves past existing file high-water
  rather than assuming stale WAL frames can be reused.
- SHM: `max(0, blocks(F0+F)*32768-H0)`, using the first/later capacities above.
- Statement subjournal: `U*(S+4)` additional bytes. Single statement savepoints
  can revisit dirty IDs across statements, but their logical subjournal starts
  reset; the disk file's retained high-water is the maximum, not the sum.

The public repository port is unchanged. Internally `StorageLibraryImportSpace`
now owns `plan`, `bytes` and `page_ceiling`; `StorageLibraryImportPageLimit`
owns `install` and `finish`. Commit installs
`min(previous_max_page_count, planned_N_plus_E)` and verifies the actual returned
limit before the first insert. The dispatcher owns the scope through observed
COMMIT or rollback and checks the returned restored previous limit. Installation
failure retains scope ownership for cleanup. Successful automatic SQLITE_FULL
rollback still requires limit restoration. The existing stricter limit is never
raised during import. `Rollback` retains primary plus rollback error;
`ImportScopeCleanup` retains that primary plus restore cause. A committed cleanup
failure is explicitly labeled committed. An unresolved transaction or limit
retires the actor; it cannot silently process the next request under that scope.
Metadata response arms and actor certification remain unchanged.

### Remaining acceptance limits

No production import or new test was executed in this lane. `dbstat` work is
linear in affected tree pages, including traversal of old overflow chains; its
latency and I/O at large installed scales need measurement. The ceiling makes
the chosen allocation budget real but may safely refuse an unusual transient
shape that could eventually fit. Parent scale checks must assess usefulness.

Anonymous statement journals may live on a filesystem different from the
SQLite database. Their byte bound is included in `sqlite_bytes`, but the existing
port separately observes only staging and database volumes, not SQLite's temp
volume. Free space there, actual anonymous-file high-water, and target VFS
allocation/sector behavior remain exact physical-space acceptance blockers.
Length bounds do not establish filesystem blocks, physical copy-on-write demand,
concurrent external disk consumption, RSS, full million-game throughput or
cross-platform evidence. No global PRAGMA relaxation, memory journal conversion,
new production runtime/connection, unsafe code or vendor/dependency change was
introduced to hide those limits. T097/T100 remain open.

## Original adapter tests and remaining gates

The initial seven-contract tests-first file was frozen before production edits:
Git blob `5b3634d7253955f5a6fbda1db4080d610b9c9d92`, SHA-256
`040ad1592053c9dc905154f519d286971486d277b63dc8c2f1f9a0e53d506c39`.
Missing new APIs or root registration are not semantic reds. Subsequent source
contracts and narrow fixture adaptations remain unrun.

The original adapter supplied 28 private import tests and two public
native-startup tests. The current space review adds 11 cases beside its original
regression, for 40 declared private import tests, all unrun in this repair lane.
Coverage includes multi-record/no-mode/one-catalog-bump success, exact folder and
default organization, Library/Trash body preservation, keyset bounds and full
UUID cursors, stale candidate/freshness errors, body/projection/local/aggregate/
trailing-EOF corruption, late truncation, late existing ID and last-insert unique
failure, actor-observed late cancellation, after-COMMIT retained receipt,
foreign preparation, shared-clone and batch-budget release, generic coverage
revocation, typed checkpoint/old preview refusal, reopened coverage reset,
folder rename/reparent/delete, raced certification/reconciliation and recovery
revocation, inside-transaction free-space/increased-layout refusal, real
SQLITE_FULL via actor-controlled max_page_count and final catalog failure.

The growth countercheck populates indexes, uses Unicode/large fields, a long PGN
line and 2048-ply mainline, lowers the actual SQLite cache to four pages, enables
spilling and compares isolated database/WAL/SHM file lengths against the confirmed
allowance before and after checkpoint. This repair lane did not execute it.
Even a pass would establish only that fixture's observed logical file growth;
the new bounded-admission cases and physical-space limits remain separate.

Private actor SQL fixtures use the existing connection for pragmas, abort/unique
triggers and targeted state corruption. They do not fabricate SQLite receipts.
The test repository forwards actual preparation/SQLite commit results; its gates
only control cancellation, foreign receiving actor, observed receipt publication
and the free-space response. Synchronization uses Notify/Condvar and actor
inventory barriers, with no wall-clock sleeps. Existing assertions are retained.

The supplied dependency already includes the three root integrations. Parent
must compile and resolve genuine API compatibility issues, then execute focused
import/framework/native and transaction contracts. Parent owns
semantic/canonical/native gates and raw logs.
A source-derived bound review and actual per-component SQLite growth measurements
are separate acceptance work. Any additional existing defect still requires a
tests-only handoff and parent-observed semantic red before repair.

## Static export and source freeze

Only the installed standalone rustfmt 1.10.0, Git whitespace/path/blob/applicability
checks and source review were used by this repair lane.
The final binary full-index incremental patch is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-import-space-repair.patch`.
Its external manifest records the exact frozen source tree and checksums.
Source freezes after export for parent review/execution. No new green result is
claimed by this writer.
