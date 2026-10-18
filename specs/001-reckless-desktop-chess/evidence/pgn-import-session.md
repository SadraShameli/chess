# PGN import session source handoff

Current repair base: `647147d9ff95cb745ede46637048ef312e3070af`, plus the exact
supplied lifecycle tree `d688d1ade0970a6ccd3e94d5229815d4d1f46197` and r1
tree `018a80805d1323d015c96dcb0342d2aff492c491`.
This remains a source-only T097/T100 handoff from this writer. T097 and T100 remain
open. Root execution logs, their hashes, observed failures and the incremental
repair are recorded in [the review repair evidence](pgn-import-session-review-tests.md).
The supplied dependency already contains the root's three lifecycle repairs.
No Cargo, build, test, Clippy or runtime diagnostic was executed by this writer.

The parent subsequently executed r1: 66 passed, 6 failed. Five behavioral reds
identify a commit-file ownership regression; the sixth is an initial-position
fixture error. This round transfers the typed Commit spool instead of the last
spool and corrects that fixture without changing its assertions. It adds one
unrun selected-frame/refreeze countercheck. The prepared-frame proof verifier
and public query ports are unchanged. The review evidence records the exact raw
log, narrow measured identical-bucket results and remaining acceptance limits.

The implementation consumes the selected `std::fs::File`, retains that original
handle until observed finalization, and injects the existing `Arc<CapacityExecutor>`.
It creates no runtime, pool, native thread, database connection or library writer.
Native selection, module registration, the real repository adapter and the atomic
SQLite import transaction remain root-owned integration work.

## Foundation paths and repair scope

- `src/storage/import_session.rs`
- `src/storage/import_session/difference.rs`
- `src/storage/import_session/disk.rs`
- `src/storage/import_session/filesystem.rs`
- `src/storage/import_session/index.rs`
- `src/storage/import_session/model.rs`
- `src/storage/import_session/prepared.rs`
- `src/storage/import_session/stage.rs`
- `tests/storage/import_session.rs`
- `specs/001-reckless-desktop-chess/evidence/pgn-import-session.md`

The r1 dependency adds the focused `import_session/classes.rs` component. This
round changes only `disk.rs`, `stage.rs`, the foundation test file, the review
difference fixture and the two evidence notes. Filesystem ownership and parent
lifecycle repairs remain unchanged.
No root module declaration, Cargo configuration, test root, schema, actor, library
owner, native owner or task marker is changed. The mirrored test file is included
privately from `import_session/stage.rs`; root registration of
`storage::import_session` makes those tests discoverable without editing test roots.

## Frozen consumer ports

The declarations in `src/storage/import_session/model.rs` are authoritative.
All payloads below are exported through `storage::import_session`.

```rust
pub type StorageImportSessionRepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, StorageImportSessionRepositoryError>> + Send + 'a>>;

pub trait StorageImportSessionRepository: Send + Sync + RefUnwindSafe + 'static {
    type PreparedProjection: AsRef<[u8]>;

    fn snapshot(&self)
        -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionSnapshot>;

    fn candidates(&self, query: StorageImportSessionCandidateQuery)
        -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidatePage>;

    fn read_candidate(&self, candidate: StorageImportSessionCandidate)
        -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidateBody>;

    fn prepare_projection(
        &self,
        record: &StorageLibraryRecord,
        body: StorageImportSessionBodyProof<'_>,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self::PreparedProjection, StorageImportSessionRepositoryError>;

    fn estimate_commit_space(
        &self,
        snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
    ) -> StorageImportSessionRepositoryFuture<'_, u64>;

    fn commit_import(
        &self,
        prepared: StorageImportSessionPreparedCommit,
        cancellation: StorageImportSessionCancellation,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCommitReceipt>;
}

pub trait StorageImportSessionFilesystemProvider: Send + Sync + RefUnwindSafe + 'static {
    fn create_spool(&self) -> Result<File, StorageImportSessionError>;
    fn duplicate(&self, file: &File) -> std::io::Result<File>;
    fn observe_space(&self, volume: StorageImportSessionEnumVolume)
        -> Result<StorageImportSessionSpace, StorageImportSessionError>;
    fn read_at(&self, file: &mut File, offset: u64, bytes: &mut [u8])
        -> std::io::Result<usize>;
    fn write_at(&self, file: &mut File, offset: u64, bytes: &[u8])
        -> std::io::Result<()>;
    fn length(&self, file: &File) -> std::io::Result<u64>;
    fn flush(&self, file: &mut File) -> std::io::Result<()>;
    fn release(&self, file: &mut File) -> std::io::Result<()>;
    fn cleanup_retained(&self) -> Result<(), StorageImportSessionError>;
}

pub trait StorageImportSessionIdentityProvider: Send + Sync + RefUnwindSafe + 'static {
    fn next_game(&self) -> Result<GameId, GameIdentityError>;
    fn next_move(&self) -> Result<GameMoveId, GameIdentityError>;
}
```

`StorageImportSessionResources<R, F, I>` contains `executor`, `repository`,
`filesystem` and `identities`, each as an injected `Arc`. Public operations are
`prepare`, `page`, `detail`, `choose`, `freeze`, `commit` and `cancel_retained`.
They retain submission, worker result, join, repository future and matched result
inside the session when a caller drops only its wait. A different request cannot
replace a pending operation. Explicit cancellation reaches the executor job and
observes its completion before cleanup. The native composition must retain the
session owner through finalization; dropping the entire owner is not a shutdown
completion protocol.

`prepare_projection` is a synchronous, cancellable computation on the existing
executor. It must use the owning backend's validated projection builder and owning
encoding. It must not synchronously wait for the actor or create a connection.
The body proof has no public constructor or deserializer and exposes the exact
schema-3 JSON, SHA-256 and byte length produced by the single canonical codec pass.
The associated projection is appended to disk immediately and released with the
current game. No backend projection DTO, projection codec, identity normalizer or
private backend import is duplicated here.

Candidate queries carry the fixed catalog stamp, a cursor and a 100-reference
limit. Queries share the current validated `NotationGameIdentity` through `Arc`
and expose its borrowed `NotationGameIdentityStructuralKey`. The public
`StorageImportSessionCandidateQuery::structural_digest_for` owns SHA-256 of the
compact JSON tuple `(structural.variant(), structural.initial_fen(), structural.mainline_uci())`.
Both stage and query construction consume it; the backend can use the same owner
and borrowed canonical FEN/UCI fields without duplicating this policy.
The adapter must index this representation consistently. Candidates are strictly
ordered by UUID bytes, including across pages. Each body must match its requested
game and content revision. Every page must report the original catalog stamp.
Adoption must be complete before preparation. A final snapshot comparison and the
commit stamp prevent a newly inserted matching game from bypassing preview.

The destination contains Unfiled or a non-nil folder UUID and its organization
revision. Root must map the backend's catalog and organization revisions into the
snapshot and validate both under the transaction. The SQLite/WAL estimate belongs
to `estimate_commit_space`; the session supplies selected counts, exact body and
projection byte totals and the largest framed payload. No schema growth formula
is invented in this owner.

## Retained physical stage and preview

The source is copied in checked chunks into an anonymous spool with the exact
observed byte count and SHA-256. The parser reads that snapshot through the
filesystem port. It uses the pinned `NotationPgnImport::next_game`, including its
20 GiB, million-game, total/per-game ply, depth, text and decoded-pending limits.
Recoverable errors produce disk-backed whole-game exclusions. Fatal termination,
limits, cancellation and I/O failure cannot publish a committable preview.

Each valid game receives final injected game/move IDs. The stage derives identity
through the existing library record and captures/encodes schema 3 once, appending
the body and the backend projection before releasing the current game. The
264-byte directory holds source ordinal, status, choices, body/projection/metadata
ranges, digests, duplicate counts and a candidate proof digest. Variable metadata
and exclusions are on disk. The structural and new-ID indexes use 40-byte entries.
The external sorter accounts for both decoded run entries and encoded output
under its typed 128 MiB ceiling and merges with fan-in two. Sort scratch growth is
space-checked before each write. Duplicate game IDs refuse preparation.

Matching first narrows by structural digest, then invokes full canonical identity
comparison. It includes all prior staged records independently of their choices.
Unknown-player bridges are not collapsed into representatives or equivalence
classes. A first unmatched valid record defaults to Import; later exact or
potential matches default to Skip. Import is refused for a duplicate unless the
caller chooses explicit Keep Copy. There is no overwrite or merge operation.
The count is duplicate records, rather than candidate-pair count. Relationship
pages have a separate cursor and show exact/potential relations and differences.

Differences use canonical node preorder, parent ordinal, child ordinal and
variant-correct UCI. They compare the selected mainline, root/node notes, NAGs,
markings, variations, typed metadata, warnings and source clock values. UUIDs and
whole-body byte inequality do not determine annotation differences. Mainline
paths are reconstructed only for requested details. Summary pages and detail
pages have both a 100-item ceiling and a byte cap.

Space is observed before copying, before every growing source/body/projection/
directory/metadata/index/sort/commit write, at freeze and immediately before
repository commit. Existing staged bytes include the source snapshot, bodies,
projections and indexes; freeze reserves the additional selected framed copies
plus the backend's SQLite/WAL estimate, summing additional requirements when both
anchors identify the same volume. Actual disk-full errors propagate. Root must
also invoke the capability's `recheck_space` inside `BEGIN IMMEDIATE`.

Unix anonymous creation uses the held stage directory descriptor with rustix
1.1.5 `openat(CREATE | EXCL | NOFOLLOW | CLOEXEC)` and checks `unlinkat` before
returning the file. Failed unlink keeps the file/name/anchor in the filesystem
owner for explicit cleanup retry. Free space uses `fstatvfs` and the held volume
descriptor. The Windows branch calls pinned tempfile 3.27.0 anonymous creation;
its inspected implementation uses exclusive sharing and `DELETE_ON_CLOSE`.
Windows requires an injected anchored space provider and native Windows evidence
remains pending. No vendor source is edited or represented as repaired.

## Frozen proof and actor capability

Freeze verifies the original immutable directory chain, original source digest,
both index roots, current preview/decision directory, metadata and each original
body/projection before writing the selected stream. The confirmation binds the
proof format, session identity, choice revision, source digest/length, ordered
original frames, body/projection proofs, frozen IDs, explicit choices, candidate
revisions/body digests, catalog and destination revisions, counts and space totals.
Changing a choice invalidates the old confirmation before mutating its disk row.
There is no rebase operation and no public confirmation deserializer.

The actor consumes a privately constructed `StorageImportSessionPreparedCommit`:

```rust
fn next_frame(
    &mut self,
    cancellation: &StorageImportSessionCancellation,
) -> Result<Option<StorageImportSessionPreparedFrame<'_>>, StorageImportSessionError>;

fn finish(
    &self,
    cancellation: &StorageImportSessionCancellation,
) -> Result<(), StorageImportSessionError>;

fn recheck_space(&self) -> Result<(), StorageImportSessionError>;
```

Frames borrow one body's and one projection's verified byte slices from the
capability. The next read requires releasing the previous borrow. Header lengths
cannot exceed the originally reserved largest frame or remaining stream bytes.
Local SHA checks, strictly increasing source ordinals, exact count/length/EOF and
the independently retained frozen aggregate digest are all required. Reading to
`None` performs the final check as well. Changing a body or projection together
with its local checksum still changes the frozen aggregate and refuses completion.
The capability cannot be reconstructed from a pathname, unchecked deserializer or
caller-supplied hash.

Each frame also exposes the hashes computed from its actual body and projection
bytes. The backend's lower projection bridge can compare its content SHA with
`frame.body_digest()` without a second body serialization or notation replay.

Root's actor must begin an insert-only `BEGIN IMMEDIATE`, check catalog stamp,
destination revisions, new IDs, cancellation, memory reservation and anchored
space, then consume one verified frame at a time. Its lower backend bridge must
recover only the already validated projection encoding from the sealed frame,
without replaying/decoding a notation tree or waiting for the executor on the
actor. Any mismatch, trailing/truncated frame, cancellation, stale revision, ID
conflict, actual disk-full or projection refusal must roll back all inserts.
Only verified EOF/count/aggregate completion permits one catalog bump and commit.
The receipt carries a commit ID and bounded count, rather than per-game receipts.

The session stores a successful receipt before attempting cleanup. Cleanup failure
returns `CommittedCleanupFailed` with the receipt and typed cleanup cause. Retry
cannot call the insertion port again; `cancel_retained` retries cleanup and reports
the retained committed receipt. Primary and cleanup failures retain typed causes.

## Authored tests and verification limits

The current parent r1 run and this source-only commit-file repair are recorded
at the start of the review evidence. The historical test/measurement status
below describes the preceding r1 handoff; the parent has since observed the
original 11 annotation regressions pass and the new identical-complete operation
budget pass. This incremental round is uncompiled and unrun by this writer.

The parent reports that the original 41 deterministic foundation tests passed.
Its 22-case review expansion produced 20 actual reds, then 11 remaining annotation
reds after its lifecycle repairs. The raw execution evidence and hashes are linked
above. This incremental repair addresses those observed annotation failures and
the measured identical-complete reconstruction bottleneck. Nine additional cases
are authored and unrun. No new repair result is reported green; root owns all
compilation, semantic testing and performance acceptance.

Coverage includes whole-game invalid-branch exclusion, fatal recovery refusal,
zero valid selection, text and source bounds, page/index budgets, overflow,
partial/conflicting metadata, exact/potential/default choices, nontransitive
bridges, staged/library candidates, synthetic structural digest collisions,
UUID-independent differences, selected-line/variation changes, stale catalog and
candidate revisions, incomplete adoption, changed choices and forged confirmation,
body/projection plus local-checksum corruption, later-frame truncation, trailing
bytes, frame order, source rename, space refusal, injected disk-full, interrupted
copy/sort, cancelled encoding entry, dropped prepare/commit waits, one-worker scheduling, explicit
cancel, and retained committed cleanup failure/retry.

The test repository is a schema-free effect implementation over real canonical
records. It verifies the sealed stream before acknowledging a commit and does
its fixture decoding on the injected executor. These tests do not establish actual
SQLite rollback, actor dispatch, schema-sensitive space accuracy, catalog adoption,
native file-selection lifetime, or release/platform acceptance. Root must add and
execute the real backend/native integration tests, including late-frame failure,
disk-full and cancellation rollback under its single-connection transaction.
Existing parser/document boundary suites remain required for the full advertised
parser limits.

The existing codec's per-body 64 MiB cap and the parser's decoded accounting are
not a whole-process RSS proof. Detail comparison can hold two bounded games and
freeze/commit can hold a bounded body and projection together. Projection decoding
and backend reservations need their owner's evidence. The repair uses bounded
disk-backed complete identity classes with full collision checks and weighted
record counts, while retaining individual partial identities and lazy annotation
relationships. The observed identical bucket now has a separate authored operation
budget, with no child measurement of the repaired source. Distinct compatible
partial identities can still require quadratic comparisons. Sort/merge throughput,
cancellation latency, full scale and one/max-worker acceptance remain unmeasured.
No performance, whole-RSS, Windows-native or complete T097/T100 acceptance claim
is made.

Permitted checks for this handoff are the standalone formatter from the installed
Rust 1.99.0 toolchain, formatter check, static source/scope/whitespace inspection,
and binary patch forward/reverse applicability checks. Cargo, tests, Clippy,
builds, release gates, CPU/runtime diagnostics, commits, pushes, agents and extra
worktrees were not run or created by this writer. Source and staged snapshot are
frozen after the final permitted checks and patch export.

## Parent canonical integration

The combined source and all relevant regressions were executed by the parent.
[Verified integration](library-runtime-import-integration.md) records the exact
snapshot, 1,666 passing tests, three clean Clippy selections, both optimized
builds, durable log hashes, known linker warning and remaining acceptance gates.
Earlier unrun statements describe this writer's isolated handoff only.
