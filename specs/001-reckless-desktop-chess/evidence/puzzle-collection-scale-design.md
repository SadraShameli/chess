# Production puzzle collection scale design

Status: concrete engineering proposal for parent composition/design approval.
This document implements no runtime behavior and accepts no native or production
scale gate. All 13 journeys and the approved 200,000-puzzle starter target remain.
The existing preparation policy still owns actual source availability and installed
byte accounting; the run codec's size limit is not a reason to reduce that target.

Only this new evidence file belongs to this design lane. The parent owns the
separate saved-run inventory change, shared module/test registrations, schema
number allocation, task reconciliation and final composition. No task IDs or
migration filenames are reserved here. Only migrations `0001_core.sql` and
`0002_puzzles.sql` currently exist; planned future numbers are not installed schema.

## Inputs and observed implementation

Read the global owner guidance, project instruction bridge,
[constitution](../../../.specify/memory/constitution.md), all seven rules in the
[canonical catalog](../../../.specify/rules/README.md), the authorized
`speckit-implement` procedure, [specification](../spec.md), [plan](../plan.md),
[tasks](../tasks.md), [data model](../data-model.md),
[study contract](../contracts/study-tools.md#puzzles),
[US13 preparation research](../research.md#deterministic-starter-subset-preparation),
[starter decisions](../decisions.md#starter-subset-selection-v1), and
[persistence evidence](puzzle-persistence.md). Language-scoped global rules do not
match the Rust/Markdown files used here. No additional subtree guidance exists.

The isolated checkout starts at `10169bd9aa10431aea370ed7f1dbda5bd0f8a1e3`.
Source inspection also used the actual `/Users/sadrashameli/Personal/chess` root,
initially including its uncommitted puzzle owner/runtime/panel and checkpoint
corrections. At this repair, root is `d703763b6f729737cb6d375a5f5055ce40e1e63b`;
the parent reports that commit pushed and `make verify` passed with 1,091 tests.
The next inventory/scene full gate is running and has no result claimed here.
These are parent-reported integration results, not measurements or checks
performed by this lane and not production dataset acceptance. Independent native
read-only review identified the recovery-bundle backup and unavailable-run End
gaps repaired below; both were accepted by the parent.

Concrete source facts:

- `StoragePuzzleTransaction::save` writes `puzzle_collections.collection_id` equal
  to the run ID. Its immutable JSON body is a frozen run envelope containing
  session/configuration/filter/seed, every supplied definition, initial order and
  initial draws. It is not an installed catalog or a discovery API.
- `PuzzlePersistenceRun::new` captures and validates all supplied definitions
  before `PuzzleSelection::new` applies filters. `PuzzleSelection` owns the
  filtered `Vec<PuzzleDefinition>`, permutation and all recorded random draws.
  `PuzzleSelectionSnapshot::from_persisted` validates membership, permutation,
  visited-prefix and shuffle decisions. `restore` requires all frozen definitions.
- `PuzzleRun::new`, `advance`, and `reshuffle_and_continue` obtain the next full
  definition synchronously from selection. `PuzzleRun` retains full previous
  attempts and run events. `PuzzleSolution` owns its definition, current tree node
  and position. The final exhausted attempt stays current, so counting archived
  attempts alone omits it. These are APIs that must change, not lazy-loading hooks
  that already exist.
- `PuzzlePersistenceCodec::restore` reconstructs definitions and replays every
  accepted operation through the real run owner, compares projections/archives,
  and appends `Recover`. Its checkpoint DTO is a projection, not a restorable
  domain snapshot. `PuzzleRunSnapshot` contains live private domain objects, not
  a validated external interchange format.
- `PuzzlePersistenceSnapshot::attempt_rows` stores outcome/count/last-event
  projections. Those rows alone cannot reconstruct the accepted path, all hints,
  reply choices or full review. Their correctness currently depends on full
  journal replay and comparison.
- The repository checks storage revision, collection digest and the exact journal
  prefix proof. Receipts are created only after commit. `PuzzleStorageDriver`
  retains writes, retry payloads and unsettled receipts, rebases coalesced
  snapshots, and separates `wait().await` from fresh-time `settle(now)`.
- `DesktopPuzzleCommands::guard` precedes inputs/time advancement. Capture uses
  `execute_transition`, `needs_snapshot`, and the existing checkpoint owner.
  `DesktopNativePuzzleRuntime` selects over commands, urgent interruptions,
  deadlines, retained storage completion and shutdown. It does not yet acquire
  catalog definitions. Snapshot publication contains a bounded current board,
  save state and archived count, with shared attempt snapshots across timer ticks.
- `PresentationPuzzleSetupCollection` currently supplies source/version, label,
  count and theme list. Its intent contains dataset/filter/config/request ID.
  It does not prove a provider, manifest or installed source exists.
- `StorageDatasetPackage` presently provides format/checksum/location/integrity
  verification. It has no actual manifest, definition lookup, provenance registry
  or installed-version lease. US13 still owns those features and acquisition.
- `StoragePuzzleRecovered::_memory_reservation` charges encoded transport bytes
  to the document store's 64 MiB budget. `into_owner` drops that permit. It does
  not reserve the decoded live graph, retained definitions or a catalog cache.

### Established blocker and unmeasured costs

The v1 document bound is 16,777,216 bytes. At 200,000 definitions it permits only
83.88608 bytes per definition, before order, draws, filters, events or checkpoints.
Even this intentionally invalid, empty-content lower bound for the actual DTO is
124 ASCII bytes:

```json
{"identity":{"source":"","version":"","puzzle":""},"variant":"standard","fen":"","lines":[],"difficulty":null,"themes":null}
```

Its 200,000 copies alone occupy 24,800,000 bytes, excluding array separators.
Nonempty identity, legal FEN and accepted lines increase that bound. A production
starter therefore cannot fit the v1 envelope regardless of hardware. Filters do
not rescue construction because capture precedes filtering. This is a structural
size proof, not a benchmark. The 100,000-event bound and retained full replay also
impose a finite run lifetime independent of installed collection size.

Construction/revalidation cost, peak decoded tree memory, recovery latency,
catalog I/O, cache hit rate and actual installed bytes remain unmeasured. The
design below specifies work and gates; it claims no speedup, native acceptance or
measured memory reduction. Increasing constants, truncating membership, substituting
a fixture starter or changing countdown defaults does not resolve this blocker.

## Storage and domain ownership

Use immutable versioned SQLite catalog files beside, and separate from, the
mutable user database. Reuse bundled rusqlite, Serde, SHA-256, Tokio and the shared
`CapacityExecutor`. No new storage framework, runtime or independent worker pool
is needed. Read-only catalog queries may use admitted executor jobs and bounded
read connections; all user-data writes remain on `StorageDocumentActor`.

Introduce these lower contracts in `src/puzzle/catalog.rs`:

- `PuzzleCatalogVersionRef`: validated source ID, source version, manifest SHA-256
  and definition-format version. The manifest binds catalog file/index digests,
  record count and canonical record ordering. A path is adapter location metadata,
  never identity. A replacement with the same source/version but different digest
  cannot satisfy an existing run reference.
- `PuzzleCatalogOrdinal`: checked nonnegative `u64` record ordinal, represented
  with checked SQLite `i64` conversion at the boundary. `PuzzleCatalogRecordRef`
  combines version and ordinal for a not-yet-loaded member.
  `PuzzleCatalogDefinitionRef` adds existing `PuzzleDefinitionIdentity` and
  definition digest after lookup. Selection does not pretend its compact ordinal
  array already contains every identity/digest. Recovery of an attempted
  definition requires the full saved reference, not merely an ordinal.
- `PuzzleCatalogDefinition`: a validated reference plus a shared definition
  allocation with the memory token described below.
  Its constructor verifies body digest, identity/version, variant, metadata and
  the complete solution through existing domain constructors. No raw FEN or
  serialized tree enters a live attempt without this validation.
- `PuzzleCatalogProvider`: an actual effect port consumed by desktop orchestration,
  with associated typed error/lease types and async `pin(version)`,
  `filter_page(lease, frozen_filter, cursor)`, and
  `definition(lease, definition_request)` operations. Each request carries
  cancellation/correlation and explicit byte/item bounds. The returned lease
  guards the referenced generation. The pure run never calls this port.

`src/storage/puzzle_catalog.rs` implements this port as `StoragePuzzleCatalog`
with `StoragePuzzleCatalogError`. It consumes US13 registry locations/integrity
results; it is not a second discovery registry. A typed definition request
distinguishes a new selected record reference from an exact saved definition
reference. `src/puzzle/definition_codec.rs` owns `PuzzleDefinitionCodec`, shared
by catalog bodies and attempted snapshots; the legacy codec retains its exact
v1 wire decoder. Extend the existing dataset-format enum for a prepared puzzle
catalog when this adapter lands, rather than labelling a SQLite file `PuzzleCsv`.

Catalog disk schema, version 1, has one typed manifest and these relations:

- `definitions(ordinal INTEGER PRIMARY KEY, puzzle_id TEXT UNIQUE, variant,
  rating NULLABLE INTEGER, body BLOB, body_digest BLOB)`, with digest lengths,
  checked ranges and a contiguous-ordinal validation at preparation/publication.
- `themes(ordinal, theme, PRIMARY KEY(ordinal, theme))` and indexes on
  `(rating, ordinal)` and `(theme, ordinal)`. Exact case-sensitive tokens and
  full original theme sets are retained, including unavailable metadata.
- Source metadata/provenance relations retain the approved preparation identity,
  source row information, full metadata and preparation-only stratum. Their
  source/version/license/selection manifests extend `StorageDatasetPackage` under
  US13, rather than forming another provenance registry in the puzzle component.

Definition bodies encode starting position plus ordered accepted UCI lines and
metadata using a versioned typed codec. Ordinals are assigned by exact PuzzleId
UTF-8 byte order after validation/deduplication. Preparation selection order and
its canonical sequence digest remain separately retained provenance. A catalog
format/version pins tree construction and line/child ordering: changing branch
ordering must not silently reinterpret saved opponent choices or hint order.

Build and validate in staging, close/checkpoint the database so no WAL sidecar is
part of publication, hash every installed artifact, then atomically publish a
manifest-addressed immutable directory. Do not rewrite published files in place.
Catalog filter queries use the same `PuzzleSelectionFilter` semantics as the
domain: inclusive rating bounds; missing rating excluded only by an active rating
filter; any selected full theme matches; no theme filter includes unavailable
themes. Return ordered ordinals with keyset pagination and an exact count. Never
return a successful partial filtered result after integrity failure.

## Frozen selection and deterministic randomness

Refactor `src/puzzle/selection.rs` so the run selection owns compact membership
and permutation ordinals, not all definition trees. Keep `PuzzleSelectionFilter`
as the canonical predicate owner; expose typed metadata matching for indexed
adapter contract comparisons. Freeze membership from one pinned catalog version,
one filter snapshot and one filter-semantics version. New-run canonical membership
is in catalog ordinal order, independent of query/worker completion order. A
converted v1 run has an explicitly versioned legacy base-order encoding described
below; it is never silently sorted and shuffled again.

`PuzzleSelectionPrepared` is a validated owned value containing version/filter,
member ordinals, permutation, count, digests, RNG checkpoint and generation.
`PuzzleSelection::from_prepared` validates a complete permutation and all bounds
before exposing `current_ref`/`next_ref`. It cannot manufacture a definition.
For 200,000 members, two `u64` arrays have 3,200,000 bytes of payload, excluding
containers and validation scratch. This is arithmetic, not total heap usage.
Validation scratch is reused and separately admitted. There is no visited-ID
vector: the frozen order prefix plus cursor determines exactly the visited set.

Persist the complete membership and actual shuffled order as immutable binary
pages in the user database, outside the checkpoint envelope. Pages contain
big-endian `u64` ordinals with count/length/digest checks, at most 64 KiB per page.
A manifest hashes the ordered page digests with framed counts and lengths.
`PuzzleSelectionSnapshot` v2 contains that manifest reference, epoch and cursor.
All matching identities remain selected, including definitions not yet loaded.
No keyset offset, cache eviction or missing row can silently consume a member.

Continue the existing descending Fisher-Yates algorithm and the actual seeded
`Xoshiro256PlusPlus` sequence for the first implementation. Persist actual reply
draws with alternatives, choice, attempt and draw ordinal. A failed operation
rolls back RNG and selection, as the current transaction wrapper does. At explicit
Reshuffle and Continue, prepare and persist the next complete order using a clone
of the current RNG; install it and its advanced RNG state only after its manifest
and accepted transition commit. Exhaustion itself consumes no draw or duration.

The current provider exposes seed/freshness, not state. Do not invent a public
`get_state`, set its private fields, or infer state from draw count: range sampling
can consume additional generator words. Local inspection of the pinned
`rand-0.10.3/src/rngs/xoshiro256plusplus.rs` confirms supported Serde derives behind
its existing `serde` feature and a four-word `s` state; its `uniform_int.rs`
confirms range-dependent sampling and the `unbiased` feature distinction.

Add the existing rand dependency's `serde` feature, subject to parent lock/feature
verification. `src/puzzle/random_provider.rs` owns
`PuzzleRandomProviderCheckpoint`: algorithm ID/version, exact sampler contract,
RNG encoding version, seed, logical draw count, and validated four-word state.
A private typed wire DTO rejects all-zero state, extra fields and invalid lengths;
the adapter uses rand's supported Serde implementation to capture/restore it.
There is no unsafe access, dynamic JSON field lookup or raw run-field restore.
Checkpoint encoding is tied explicitly to rand 0.10.3 and the selected sampler
features, not advertised as any future rand release's format. All supported
targets are 64-bit; alternatives/choices are fixed-width on disk, converted
checked at the boundary. Golden seed/range/shuffle/reply/state vectors must pin
the actual enabled feature graph before this becomes a persistence contract.

`PuzzleSelectionRandom` then retains a bounded uncommitted draw suffix and one
constant-size provider checkpoint instead of every lifetime draw. Recovery
regenerates and compares persisted epoch order/digest on the shared executor,
restores the checked generator checkpoint and replays only the bounded tail.
Each immutable epoch manifest stores RNG checkpoints immediately before and after
its shuffle. Order verification starts from that epoch's before-state, never
from the run's later state after opponent replies. Prepared buffers and their
before/after states are installed once under their generation/request identity.
Unknown RNG/sampler versions refuse recovery. An upgrade must ship the previous
decoder/sampler or an explicit tested transition, never reinterpret old state.

## Supplying definitions without charging preparation time

Replace `PuzzleRun`'s synchronous access to `selection.current()` with a pure
two-step transition. New values belong in `src/puzzle/run.rs`:

- `PuzzleRunDefinitionRequest` holds expected session context, selection manifest,
  epoch/cursor, target `PuzzleCatalogRecordRef` and a monotonically increasing
  load generation. A known saved current definition also pins its full digest.
- `PuzzleRunEnumProgress` distinguishes `Current`, `AwaitingDefinition(request)`,
  `Exhausted`, and terminal states. Waiting is not an active countdown phase.
- `PuzzleRun::prepare_start(prepared, first_definition, config)` validates exact
  selected identity before constructing the first session.
- `PuzzleRun::prepare_advance(expected, now, reason)` finalizes/archive-emits the
  old attempt once, arms the next timer, and returns the required definition
  request. Existing `next`, `skip`, per-puzzle expiry and explicit reshuffle use
  this owner. Exhaustion retains the final current attempt once, as today.
- `PuzzleRun::supply_definition(request, definition, now)` validates generation,
  context, ordinal/digest and phase before replacing the attempt. It emits the
  existing revisioned transition and leaves the board awaiting readiness. A stale
  result neither changes cursor/RNG nor revives an ended or hidden run.

Re-evaluate loaded metadata against the frozen canonical filter before installing
a selected record; mismatch is catalog corruption, not a skipped member.
`PuzzleSession::{new, replace_attempt}`, `PuzzleAttempt::new` and
`PuzzleSolution::new` consume the validated shared definition handle. Replace
`PuzzleSolution`'s owned tree clone with that handle while retaining borrowed
`definition()`/`position()` access for existing board/attempt behavior. The
archived record owns a snapshot reference, not another clone of the live tree.
V2 persistence records reference-only `PrepareAdvance`/`SupplyDefinition`
operations and their load generation; it does not serialize an I/O task/future.

The cursor reservation, archived outcome and pending definition reference become
one durable accepted operation. Save failure retains that operation and request;
Retry Save does not repeat Skip, timeout, cursor advance or RNG draws. The first
display/input of a replacement attempt requires its state to be queued under the
existing save guard; initial run activation still waits for its initial receipt.
Definition completion is not readiness and is never Resume.

Desktop owns the asynchronous lookup. Extend `DesktopPuzzleCommands` with pure
`definition_request`/`supply_definition` operations plus a typed load state;
`DesktopNativePuzzleRuntime` owns and selects a retained acquisition job alongside
its existing queues, deadlines, `owner.wait`, interruption and shutdown. A single
`await definition()` must not hold the actor loop, borrow, or lock. Completion
uses a newly sampled `ClockTimeProvider` instant. Queued commands and late
results retain expected revisions/request IDs. End/close cancels prefetch, joins
its observed completion and preserves a durable pending reference if appropriate.

Readiness remains `Ready` only after board resources are ready, visible and
interactive for the specific context. Loading occurs between attempts with zero
new active time. Actual replies, animations, hints and practice within an active
attempt remain charged. Solved review waits for explicit Next; exhaustion waits
for explicit Reshuffle and Continue. Focus loss/hide/sleep while loading records
a pause; completing lookup or restoring focus cannot dismiss it.

Preserve `DesktopPuzzleCommands::guard` before every ordinary input, acquisition
installation and timer advancement. Preserve 250 ms capture cadence, one retained
write plus one latest coalesced snapshot, and the original 1 s durable-lag freeze
boundary. `wait().await` retains completion; fresh-time `settle(now)` evaluates
that original boundary before acknowledging durability. Neither commit nor
successful retry automatically resumes. Loading/cancellation/compaction must
remain selectable while urgent focus/sleep/quit and durability deadlines run.

## V2 user schema, accepted operations and bounded lifetime

A new consecutive migration is required. Do not modify the existing migration
2 constraints or call v2 content schema 1. The parent allocates its filename and
serializes this change with its inventory/repository work and planned US13 schema.

Required logical relations and keys, with final names parent-coordinated:

- `puzzle_run_manifests(run_id PRIMARY KEY, format_version=2, manifest_digest,
  catalog_ref, frozen_filter, filter_version, config, seed, selection_manifest)`.
  JSON fields are bounded typed DTOs; digests have checked 32-byte lengths.
- `puzzle_selection_pages(run_id, epoch, kind, page_ordinal, item_count, digest,
  body, PRIMARY KEY(run_id, epoch, kind, page_ordinal))`, immutable after publish.
  Membership and order manifests prove total counts, ordered page boundaries,
  uniqueness and permutation equality. Pending preparation is not a visible run.
- `puzzle_run_states(run_id PRIMARY KEY, storage_revision, checkpoint_generation,
  checkpoint_body, checkpoint_digest, committed_operation_ordinal,
  committed_operation_proof, compacted_through, selection_epoch, selection_cursor,
  current_attempt_id, status)`. Use checked counters and the existing revision
  representation policy; no lifetime `<=100000` constraint applies to v2.
- `puzzle_operation_segments(run_id, segment_ordinal, first_operation,
  last_operation, base_proof, end_proof, body_digest, body, state_digest,
  PRIMARY KEY(run_id, segment_ordinal))` and a bounded append tail. Accepted
  operation ordinals are globally monotonic, never renumbered after compaction.
  Immutable archived segments remain on disk until explicit history deletion.
- `puzzle_attempt_records(run_id, attempt_id, definition_snapshot_digest,
  attempt_start_proof, final_proof, outcome_projection, review_manifest,
  PRIMARY KEY(run_id, attempt_id))`, with separate bounded pages for the complete
  accepted-attempt transcript. Run-lifecycle segments reference attempts rather
  than duplicate their move/definition bodies in unrelated records.
- `puzzle_definition_snapshots(digest PRIMARY KEY, format_version, body)` stores
  exact definitions actually attempted, including the active attempt, as user
  history. This content-addressed store does not copy every unattempted definition.
  References are removed/collected only by confirmed history deletion.
- User-origin `puzzle_recovery_bundles` and bounded `puzzle_recovery_bundle_pages`
  retain the complete unique v1 recovery material described below, with manifest,
  origin collection digest, ordered page hashes/counts and run-reference keys.
  These are included user data, not excluded installed starter files.
- Durable catalog-version reference rows link every nonterminal run to its exact
  version. US13's `dataset_registry` owns physical version leases and deletion;
  puzzle persistence must participate in that one registry, not duplicate it.

The current repository's storage revision, exact base proof and immutable manifest
comparison remain required on each transaction. Define a version-2 journal proof
with its own domain tag, framed lengths, previous proof, accepted operation
ordinal, operation body and resulting state digest. Initial proof binds the run
manifest and selection manifest. Identical counts/revisions do not authorize a
different prefix. `PuzzlePersistenceSaveRequest` v2 carries its original base
ordinal/proof, compact-anchor generation, bounded operation suffix, state and
affected attempt/snapshot changes. A transaction updates all of those plus
durable version references atomically. Receipts bind exactly that request/token.

`PuzzlePersistenceCodec` becomes a version dispatcher, with real v1 and v2 typed
documents and version-specific replay. New `src/puzzle/persistence/checkpoint.rs`
owns `PuzzlePersistenceCheckpoint` and its validator; an organization-only module
split must preserve component-prefixed type names. V2's checkpoint contains:
run/config IDs, manifest refs, epoch/cursor, session revision and attempt ID,
validated current definition reference, attempt progress/witness, pending load,
remaining/active durations and timer phase, finalized elapsed duration, RNG state,
summary accumulator, archive counts and exact operation/compaction proofs. It
contains no collection-wide definitions, visited list or lifetime event array.

Proposed initial bounds are 256 KiB per checkpoint, 64 KiB per operation/page,
and a replay segment sealed before either 4 MiB or 4,096 operations. Admission
reserves room for the sealing/recovery transition before accepting another
operation. These are new independently bounded v2 objects, not raised v1 limits.
Definition bodies retain explicit existing line/ply validation budgets and have
separate encoded/decoded memory admission. A large definition is not packed into
the checkpoint or used to enlarge every periodic save.

### Compaction and validation, including a long current attempt

Compaction is mandatory for lifetime support, not automatic history deletion.
After a committed prefix, the owner exports a compact checkpoint at the exact
ordinal/proof. Seal its transcript segment and checkpoint in one transaction;
retain the original segment for audit/review. Drop the in-memory prefix only
after its receipt settles and every retained/coalesced snapshot can rebase to the
same certified anchor. Pending writes retain their original immutable request.
An old-generation receipt cannot acknowledge a newer checkpoint. If compaction
cannot make progress, apply save backpressure before admitting beyond the bound.
Do not accept an operation and only then discover that its journal cannot fit.

`PuzzleRun::previous_attempts: Vec<PuzzleRunAttempt>` and `events: Vec<_>` become
bounded uncommitted transitions plus archive counters. Introduce a typed emitted
`PuzzleRunAttemptRecord` and incremental `PuzzleSummaryAccumulator` on the summary
owner. A finalized record folds once; the current attempt contributes once even
when exhausted/ended. Keep primary outcome, later assistance/practice and active
elapsed values distinct. Summary reads and history pages do not reconstruct all
previous definitions. Pure reference tests compare this fold with today's
`PuzzleSummary::from_attempts` on the complete same attempts.

Compacting only between puzzles is insufficient: repeated unlisted moves and
practice retries can make one current attempt's events unbounded. Add
`PuzzleAttemptCheckpoint` on `attempt.rs` and `PuzzleSolutionCheckpoint` on
`solution.rs`. Their private interchange forms restore only through fallible
domain constructors, not derives into mutable fields. The solution constructor
starts from the verified definition and replays the bounded accepted path to
derive cursor/position. Validate the attempted failure move and listed-node
witness, hint stage at the current solver node, assistance, primary outcome,
practice phase and event counters against the compact certificate and tail.
Timer balance/phase uses `PuzzleTimerSnapshot::from_persisted` and recovery uses
`PuzzleTimer::recover`; no persisted process-clock instant is restored.

The compact certificate binds the canonical fold output, definition snapshot,
operation prefix proof and aggregate counters. Only the domain fold can create
it during execution or verified replay. At restart validate its checksum,
relations and domain invariants, then canonically replay/compare the bounded tail.
This changes the normal recovery boundary: a previously committed validated
checkpoint is trusted for its archived prefix. A hash is not proof that arbitrary
deserialized phase/outcome fields represent legal execution. Independent tests
must compare each compact state with full canonical replay; a streamed audit can
verify historical segments without making every launch replay the full lifetime.
Reject a broken anchor/tail/attempt projection, including a digest-valid but
domain-invalid checkpoint. Coordinated malicious replacement of database and all
proofs is not prevented by an unauthenticated hash chain, in either version.
The parent intends to adopt this recovery model only after real full-replay
equivalence and domain-invariant gates pass before production. This is an
engineering acceptance gate, not a new user-approval flow or a testless change.
Retaining mandatory lifetime replay would leave the lifetime blocker unresolved.
Replay across several attempts streams their verified definition snapshots and
operation pages with bounded lookahead, folds completed attempts and releases
their graphs. A bounded byte tail must not cause eager loading of thousands of
large definition trees. Domain replay receives validated values from that
pipeline; file/SQLite access stays in the executor-backed adapter.

## Review, history deletion and definition retention

On first activation of an attempt, commit its exact validated definition snapshot
with the attempt's initial state. At every accepted move/hint/reply/practice/
skip/timeout transition, append its complete typed transcript and update its
projection under the same transaction/proof. A review API loads one attempt's
snapshot and bounded transcript pages, replays through the actual attempt owner
and checks its final projection. A requested large history review is paginated
and cancelable; it does not inflate the active run graph. Current review can use
the live verified definition; previous review uses the attempt's own snapshot,
not the catalog's current version or merely its last event/outcome row.

Review after replacement/removal must preserve starting FEN, accepted branches,
actual solver moves, reply choices, progressive hints, assistance, failed primary
outcome through practice solve/skip/timeout and elapsed active duration. Retain
the final attempt at exhaustion exactly once. Never rebuild a historical attempt
using a PuzzleId match from a newer source version.

History is kept indefinitely until explicit deletion. Treat paused, recovered,
loading, save-blocked and exhausted-but-continuable runs as nonterminal: refuse
their attempt/session/all-history deletion under the existing active-run rule.
Deletion of terminal-run attempts removes their projections, transcript payloads,
definition-snapshot references and summary contribution in one transaction;
recompute affected summary totals from retained typed records with bounded scans.
Deleting the terminal current attempt produces an explicit deleted-current state,
not a dangling current ID or a resumable blank board.

Deletion needs versioned redaction certificates for affected operation ranges:
retain ordinal/proof boundaries and an explicit deleted-history marker, erase
attempt payloads and embedded copies, and mark those ranges unavailable to review
or full audit. Never call missing payloads successful canonical replay. Because
run events reference attempt bodies separately, unrelated attempts survive.
Terminal checkpoints/aggregate caches must be replaced with a validated redacted
checkpoint anchored to the deletion transaction. Dataset definitions remain
installed; remove a deduplicated user snapshot only after its last history/current
reference disappears. Session/all-history deletion clears all corresponding
segments, checkpoints and snapshots. Old external backups follow the existing
backup retention policy; this does not promise to erase already exported backups.

## Every valid v1 saved run remains usable

Keep the genuine bounded v1 decoder and its canonical replay path as an explicit
legacy format reader. Do not feed reference-only content to `decode_parts`,
substitute a current `PuzzleRunSnapshot`, or deserialize projected fields into
`PuzzleAttempt`, `PuzzleSession` or `PuzzleRun`. Read original bytes/proofs and
validate the complete v1 document, attempt rows and RNG transcript before conversion.
Its existing bounds limit this one-time work. Corrupt/unsupported rows produce a
recoverable per-run refusal and preserve original data, not an empty success.

At load, convert each valid v1 run transactionally and lazily, regardless of
whether awaiting readiness, active-at-save, manually/focus/save-paused, review,
exhausted, per-puzzle timeout boundary, session-expired or confirmed ended.
Inventory may list a legacy descriptor without converting or replaying it.
The parent-owned inventory API must expose format/readiness metadata; extend its
implementation later in the serialized migration change rather than editing
that slice here.

The converter consumes the fully validated live v1 owner to export private typed
checkpoint/attempt records through their owning methods. Preserve session and
attempt IDs, revision progression, config, remaining durations, pending phases,
all attempted snapshots/transcripts, failure/assistance/practice, exact filtered
membership, saved permutation, visited prefix, seed, and actual RNG state after
all recorded shuffle/reply draws. Capture generator state through the supported
rand encoding above. Future choices therefore continue the same stream, including
after the next shuffle; no new seed, RNG version switch or discarded draw prefix
is needed. Recover once into a paused phase, appending one correlated recovery
transition; never apply legacy Recover and v2 Recover twice by accident.

Set `PuzzleSelectionEnumBaseOrder::LegacyV1SourceOrder` on converted runs and
retain their exact filtered `PuzzleSelectionSnapshot.persisted().collection`
sequence as ordinal pages. Fresh v2 runs use `CanonicalOrdinalV1`. The legacy
base order persists across all later reshuffles: sorting that sequence would
change future permutations even with identical RNG state. For the current epoch,
replay the bounded v1 random transcript from its actual seed up to
`shuffle_start` to capture the before-shuffle RNG state, then through its
`length - 1` shuffle draws for the after-state; retain the independently captured
current provider state after all later replies. The verified v1 saved order must
equal Fisher-Yates of those legacy base-order pages. This is an explicit
versioned representation, not an assumption that all old orders were canonical.

A v1 body may be the only surviving copy of its definitions. Convert its full
embedded collection into a staged immutable local catalog without requiring a
network provider, original CSV or real installed manifest. Preserve its original
source/version identities and source order as legacy-order provenance; map all
saved membership/order identities to new canonical ordinals. Catalog origin is
explicitly `LegacyEmbeddedRun(collection_digest)` with provenance unavailable
where v1 never recorded it. This is recovered local user material, not proof of
US13 acquisition/license/preparation or permission to redistribute a starter.
Its manifest digest prevents collision with a subsequently installed real catalog
having the same source/version strings. Attempted definitions are also retained
as user snapshots, independently of that local catalog.

The complete `LegacyEmbeddedRun` recovery bundle is unique user-origin data,
including unattempted definitions needed for exact remaining selection. Retain
its manifest and exact catalog/index bytes in bounded content-addressed pages in
the user database. A materialized immutable catalog file is only its derived
runtime representation. Backup includes the authoritative bundle pages; restore
can rebuild the exact digest-pinned file without any original installation,
source CSV or network. An apparent match by source/version name never makes the
bundle replaceable. Do not discard it in favor of a non-backed-up local file.

Retain the bundle while any nonterminal run needs it, or a backup/restore/job
lease references it. Collection is permitted only after all such references
release and all retained attempted definitions have independent user snapshots;
if promoted to an independently retained personal collection, its normal
confirmed dataset-deletion policy also applies. No time-based eviction or
starter-exclusion rule may collect this unique user material while needed.

Publish/fsync the recovered catalog before the cutover transaction, hold its
runtime lease, then atomically install the complete recovery-bundle manifest/pages,
v2 run state/history and durable version reference while checking the original
v1 storage revision/end proof. Validate bundle completeness/hashes before cutover.
A crash before commit leaves v1 authoritative and a collectable staging artifact;
a crash after commit loads only v2 with its durable user bundle and exact catalog.
Never make a row authoritative while its file is unpublished. An explicit format
cutover marker prevents duplicate inventory entries. Keep original v1 data until
conversion and the complete user-bundle pages verify; then remove superseded
embedded/event/attempt rows in the same completed migration/cutover policy so
later history deletion has no hidden
copy. A failed conversion leaves the complete v1 run available for retry.

Golden v1 documents must be produced from the actual schema-1 serializer and
stored as fixtures, not recreated with a hand-written approximation of its JSON.
Legacy continuity is gated on before/after equivalence, including later saves,
subsequent replies, exhaustion/reshuffle and a second relaunch. No dataset fixture
or fresh seed is a substitute for this conversion.

## Version leases, memory and recovery

US13's registry must distinguish durable references from runtime leases. A
nonterminal run pins the exact catalog version even while the app is closed;
opening it additionally acquires an in-memory lease before exposing Resume.
Replacement atomically changes the default version only. Deletion refuses any
version with durable nonterminal-run references or active query/runtime leases.
Terminal runs release their catalog pin only with the terminal-transition commit
and durable attempted snapshots. Closing a runtime releases its live lease, not
the saved run's durable reference. Recovered runs retain the lease through their
owner lifetime, queued
definition jobs, retries and retained writes until quiescent shutdown/cutover.

Missing/corrupt catalogs produce `MissingVersion`, `DigestMismatch`,
`UnsupportedCatalogFormat` or `InvalidDefinition` outcomes with exact source/
version/digest dependency information. Keep the run paused/hidden and preserve
order, cursor, remaining duration and journal. Offer manual locate/reinstall of
the exact version and Retry Load; never silently adopt latest, skip a corrupt
member, refilter, download, reset time or release the saved pin before a confirmed
terminal commit. Exact current and previous attempt snapshots still support
history inspection when available,
but missing future membership data prevents continuation. Corrupt user snapshots
are their own history error, not a reason to query a different catalog version.
Unrelated library, live play and other datasets remain available.

### End an unavailable run without acquiring future definitions

`DesktopPuzzleService::open` validates the persisted user checkpoint/anchor,
bounded tail and attempted snapshots before requiring a catalog runtime lease.
Return `DesktopPuzzleServiceEnumOpen::Unavailable(DesktopPuzzleServiceUnavailable)`
when the future catalog cannot be pinned. Split its owned state by dependency:

- `PuzzleRunUnavailableState` in `src/puzzle/run.rs` contains only validated domain
  context/session revision, config/timer, current-or-pending attempt state and
  canonical summary state. Its fallible constructor consumes validated domain
  values; it owns no storage revision, persistence checkpoint/generation, journal
  ordinal/proof, dependency failure, request identity or write handle.
- `PuzzlePersistenceUnavailableRecovery` in `src/puzzle/persistence.rs` wraps that
  pure state and the actual validated persistence checkpoint, storage revision,
  checkpoint generation, exact operation ordinal/proof and pinned catalog
  reference. It validates persistence expectations before preparing an End write.
  It depends on `run` and the checkpoint/proof owners; `run` imports neither
  persistence, storage nor desktop. No lower type imports this outer envelope.
- `DesktopPuzzleServiceUnavailable` in `src/desktop/puzzle_service.rs` retains the
  persistence envelope, typed dependency failure, request identity and write/
  receipt/retry lifecycle. It supplies the unavailable view and Retry Load/End
  actions without constructing a fake live run or a competing write owner.

The descriptor is not an inventory summary or an invented fresh `PuzzleRun`.
No runtime file lease is fabricated for an absent file; the durable version
reference remains owned.
Corrupt user checkpoint/history that cannot validate remains a distinct error;
catalog failure does not authorize trusting unchecked projections.

The descriptor supports explicit confirmed End as well as Retry Load. A typed
`DesktopPuzzleEndUnavailableRequest` carries request ID, expected context/storage
revision, checkpoint generation/prefix proof and confirmed End intent. The pure
`PuzzleRunUnavailableState::end_confirmed(expected_context, confirmation)` checks
the domain context and returns `PuzzleRunUnavailableEndTransition`, containing
the canonical terminal domain state and attempt/summary changes. This pure
transition has no persistence expectations or I/O. It requires no provider pin,
future-definition lookup, RNG draw, new attempt allocation or elapsed-time
advance. Outcomes are phase-specific:

- For an activated unfinished current attempt, use its verified user snapshot
  and attempt checkpoint. Preserve any primary failure, assistance and practice;
  apply the existing End skip event, assigning primary Skipped only if unresolved.
  Preserve its durable elapsed time. Update that current attempt's contribution
  exactly once through the canonical summary fold. An already solved/review or
  finalized current attempt retains its outcome and receives no invented skip.
- If the previous attempt is already archived and the next definition/attempt
  has never activated, cancel the pending request and end between attempts.
  Preserve the archived outcome/elapsed time and existing summaries exactly.
  Create no skipped/timed-out next attempt, no attempt ID or additional time.
  Reject a checkpoint claiming both that archived contribution and the same
  attempt as an unarchived current contribution.

Desktop first checks request identity/cancellation and uses the persistence
envelope to validate storage revision, checkpoint generation and prefix proof.
The envelope invokes the pure End transition, whose owner advances the domain
revision once, then prepares the existing persistence save request. The repository
transaction rechecks expected storage revision/generation/proof against durable
rows before committing that transition. It appends the confirmed End operation,
updates affected attempt/summary/checkpoint rows and releases the durable run
version reference atomically. Only commit constructs its terminal receipt.
The terminal checkpoint retains the End request identity and resulting proof for
exact duplicate-result recovery after an uncertain completion.
Read-only opening, confirmation display, canceled confirmation, failed admission,
conflict or failed transaction release no pin and leave history unchanged.
Retain an accepted write/request across canceled waits and uncertain completion;
settle its actual receipt before publishing End or permitting history deletion.
Retry uses the same request identity/payload; a committed duplicate returns its
original result without another skip/fold. A late lookup cannot revive the ended
generation. After successful terminal commit/settlement, normal separately
confirmed attempt/session/all-history deletion may proceed. Failed or canceled
uncommitted End remains unavailable and retryable, with its durable pin intact.

New `src/desktop/puzzle_definitions.rs` owns `DesktopPuzzleDefinitions` and its
cache/load lifecycle. Its cache key includes version manifest, ordinal, definition
digest and definition-codec version. Count encoded bytes, decoded tree/node/line
allocations and retained `Arc` allocations. Transfer an owned live-memory
reservation with every loaded definition/run buffer; cache eviction releases it
only when the final current/history/job reference drops. Move the reservation
into the run owner wrapper on recovery. Do not reuse the transport permit as
proof of this ownership or let `into_owner` drop a new live reservation.
Concretely, implement the shared handle as an `Arc` allocation containing the
definition and an owned `PuzzleCatalogDefinitionReservation` port token. That
lower port exposes its reserved byte count; the desktop memory-policy adapter
implements release on drop. Cache/current/history/job clones share the allocation
and token, so the last handle releases both together. The domain neither acquires
permits nor imports the desktop adapter or Tokio semaphore. Run/selection buffer
reservations remain owned by the outer retained runtime/recovery wrapper.

Initial tuning proposals: 32 MiB of cached decoded definitions, 16 MiB selection/
run working buffers, and 64 MiB aggregate preparation buffers. They are ceilings
within a composition-root memory policy, not three new uncoordinated allocators.
A definition too large for idle cache may be loaded as a separately admitted live
object with cache bypass; unavailable memory gives a typed refusal, not silent
eviction of a live attempt. Per-definition graph admission needs checked node/
line/string counts or conservative preallocation before construction. Actual
layout overhead must be measured, including transient validation copies.

Prefetch at most the next definition for each admitted run; current attempts and
visible history pages have priority. Cache capacity is byte based, not a claim
that a fixed number of arbitrary puzzle trees fits. Preparation streams bounded
pages/chunks, applies backpressure before creating jobs and retains only admitted
in-flight buffers. Use `CapacityExecutor::submit`/`submit_waiting` jobs, observe
`join`, cancellation and shutdown, and reuse the root's `CapacityBudget` leases.
Do not launch one future/thread per puzzle, nested Rayon pools or an independent
Tokio blocking budget. Parallel independent validation/hash chunks may use all
admitted shared capacity; ordered selection/shuffle/fold dependency stays explicit.
Heavy catalog verification/recovery/preparation respects the existing acknowledged
preparation phase and engine leases. Read-only connection/page caches also count
toward memory, including SQLite page cache, not just Rust allocation estimates.

## Native service/panel and backup contracts

`src/desktop/puzzle_service.rs` should own `DesktopPuzzleService`, wired once in
bootstrap using the parent inventory port, catalog provider, shared executor,
user repository, clock and native wake provider. Concrete operations:

- `collections(page)` returns installed version reference, label, exact count,
  supported variant, metadata choices, integrity and availability. This is backed
  by US13 discovery. It never derives collections by treating saved run envelopes
  as an installed catalog.
- `start(DesktopPuzzleStartRequest)` takes request ID, pinned version, frozen
  filter, config and injected seed. It asynchronously prepares membership/order,
  obtains the first definition, commits initial metadata/pin/snapshot, constructs
  the existing command/runtime owner and returns its provider plus first snapshot.
- `open(saved_run_id, request_id)` uses versioned repository read/conversion,
  validates the user checkpoint/tail off the storage actor's control loop, then
  returns a paused runtime after exact-version pinning or the unavailable recovery
  descriptor above. Confirmed `end_unavailable(request)` needs no catalog access.
- `history_page(run_id, cursor)` and `review_attempt(run_id, attempt_id)` return
  typed bounded pages/review jobs; confirmed delete uses the history owner above.
  No API returns all definitions or every historical attempt as one UI payload.

Extend `PresentationPuzzleSetupCollection`/`PresentationPuzzleSetupIntent` from
source/version-only `PuzzleSelectionDataset` to the validated digest-pinned
version reference. Keep the existing config/filter/request semantics and default
countdowns. Expose preparing/validating/loading/missing/corrupt states with
cancel/retry and truthful counts. `DesktopPuzzleSnapshot` adds current definition
reference, selection progress and typed acquisition state while retaining its
existing context, timer, save status and bounded board snapshot. It uses the
archive count owner rather than `previous_attempts().len()`.
`PresentationPuzzleController` retains per-context readiness deduplication and
stale-result rejection. Review uses a separate bounded result; paused training
never exposes its board/solution through an accidental review snapshot.

`StoragePuzzleRepository` needs versioned bounded read results distinct from
fully decoded `StoragePuzzleRecovered`. Its SQL transaction reads a consistent
snapshot/segment manifest and bounded payload; CPU-heavy graph validation/replay
runs on the injected shared executor. Pagination across mutable reads uses a
pinned revision and immutable segment keys; reject/retry an inconsistent snapshot.
Keep the present `PuzzleStorageRepository` save port and driver semantics, adding
v2 ordinal/anchor receipts rather than weakening proof checks. Coordinate changes
to `src/storage/puzzle_repository.rs`, `document_store.rs` and their tests with
the parent's saved-run inventory lane after that slice lands.

US11 backup includes user run manifests, full frozen selection pages, compact
anchors, history segments/snapshots, deletion certificates and source/version/
digest dependency metadata. Exclude replaceable installed starter/catalog files.
Include complete unique `LegacyEmbeddedRun` user recovery bundles and their
manifests/pages, as well as actual attempted-definition snapshots. Bundle pages
participate in the same revision-consistent user-data backup; any temporary
materialization remains excluded. File location or a catalog-shaped format does
not change user-origin recovery data into an excluded dataset.

Restore does not claim excluded datasets were restored. Preview exact missing
version dependencies and restore all runs paused. Reconstruct an included legacy
bundle's exact catalog from verified user pages, preserving its remaining
membership/order/RNG and ability to continue without original files. A missing
excluded replaceable catalog blocks continuation until located; its validated
unavailable descriptor still permits confirmed End. Membership/order and
durable pins remain, even when a referenced file is absent. Do not automatically
download, bind to a same-named version with another digest, or include excluded
replaceable installed catalogs. Legacy backups are read with the actual v1
decoder and converted under the same recovery path; incompatible/newer schemas leave current
data unchanged. Restore's existing safety backup/profile lease/atomic replacement
protocol still owns cross-file replacement and crash recovery.

## Phases and minimal first implementation slice

These are dependency-ordered engineering slices for parent task planning, not
claimed completed tasks or reservations of shared files.

1. Freeze real v1 fixture bytes and establish the meaningful failing 200,000-row
   start/save/reopen contract. Implement the catalog reference/definition codec,
   read-only indexed adapter and filter-equivalence checks using a generated local
   catalog. Add the consecutive v2 schema after the parent inventory merge.
2. Implement compact frozen membership/order and supported RNG checkpoints, pure
   definition-request transitions, v2 save/proof/tail recovery, and desktop
   asynchronous supply. Start, save, close and reopen a 200,000-member fixture
   without constructing all definition graphs. This is the minimal vertical slice:
   one real SQLite fixture catalog, one run, one current definition, exact next
   selection and paused recovery through the production command/runtime APIs.
   It does not need provider acquisition, a new UI framework or a broad app rewrite.
3. Complete bounded active-attempt/previous-attempt compaction, exact review and
   summary folds, all-v1 transactional conversion, durable/runtime leases,
   history deletion/redaction and backup-reference fixtures. These are required
   before calling v2 a production run-lifetime replacement. Do not leave the v1
   lifetime vector hidden behind the new catalog API.
4. US13 supplies actual acquisition/license/provenance manifests, CSV preparation,
   version registry/discovery and final starter indexes. Connect the service to
   the existing setup/controller/runtime and perform native/package/scale gates.
   Fixed fixtures keep earlier slices testable; they never replace this phase.

Required existing owners to change are `puzzle::{selection, random_provider, run,
session, attempt, solution, summary, persistence, storage_repository}`,
`storage::{puzzle_repository, document_store, migration, dataset_package}`,
`desktop::{puzzle_commands, puzzle_snapshot, puzzle_bridge,
native::puzzle_runtime, bootstrap}`, and `presentation::puzzle::{setup, controller,
view, summary}`. Add working catalog/checkpoint/definition-service components only
with their implementing slice. US13's planned `dataset_registry`,
`dataset_integrity`, `dataset_commands` and preparation tools retain their owners;
do not make empty puzzle substitutes. Parent extends the study/data model contracts
and task plan in the same approved serialized implementation change.

## Failing fixtures, regressions and measurement gates

No Cargo/build/test/CPU diagnostic, dataset download, installation, commit, push,
subagent or extra worktree was run by this design lane. Document/patch checks are
the only local verification. Every result below is required future evidence.

Meaningful red first: in source-mirrored `tests/puzzle/collection_scale.rs` and
`tests/storage/puzzle_catalog.rs`, deterministically generate 200,000 distinct
valid identities/FEN/accepted-line records locally, write the real catalog,
request all matching members and assert successful start/save/reopen. Against
the old constructor, first demonstrate `PuzzlePersistenceError::Budget` at the
actual defect boundary; the new positive acceptance assertion must fail there.
Do not call an absent API, missing fixture or unavailable tool a red regression.
Also supply a selective filter whose one match is among 200,000 supplied rows;
the old capture-before-filter path must expose its same budget failure.

Additional fixtures, registered later through existing Cargo-discovered roots:

- Indexed filtering equals the canonical metadata predicate for inclusive
  boundaries, unavailable ratings/themes, multiple exact tokens, deduplication,
  different source order/page sizes/worker completion and UTF-8 identity order.
  Persist every member, complete the epoch without repeats, relaunch at several
  cursors and require the exact next IDs. New filters and replacement versions
  must not change a saved run.
- Golden RNG/state vectors include seed zero/max, alternatives one and boundary
  ranges, branched replies, failed-operation rollback, interleaved reply/shuffle,
  initial and later reshuffles, and checkpoint restore across supported targets.
  Use independently fixed expected choices plus comparison with the currently
  pinned provider, not a duplicate sampler implementation in the fixture.
- Golden v1 serializer documents cover every run phase, both timer modes, failed
  then hinted/practice-solved/skip/timeout paths, promotions, multiple branches,
  missing source, empty history and final exhausted current attempts. Compare
  converted state, summary, full review, next reply/shuffle, save/reload and second
  recovery; inject failures before/after file publication and SQL cutover.
- Convert a genuine v1 saved run, back up the converted user data, remove every
  original profile/catalog/source/staging file, then restore into an empty profile
  offline. Rebuild solely from included recovery-bundle pages, require explicit
  Resume, and compare every remaining selected identity, listed reply, duration,
  outcome and later reshuffle against uninterrupted canonical replay. Reject
  missing/corrupt bundle pages without claiming compatible recovery; prevent
  bundle collection while run/backup/restore leases need it.
- Controlled definition acquisition covers late/wrong generation/digest, missing
  file/row, corrupt index/body, cancellation, capacity exhaustion, end during load,
  focus/hide/sleep during load and retained shutdown. No cursor/RNG/time advance
  is caused by failed lookup or repeated completions.
- Keep existing 250 ms/1 s deadline tests and add delayed acquisition/compaction/
  catalog reads while save completion and urgent focus/sleep/quit race. Freeze at
  the original lag boundary; preserve the split wait/settle API, same retry
  payload, coalesced rebase, initial-save gate and explicit Resume.
- Force multiple compactions with more than 100,000 accepted operations, many
  epochs and a single long practice attempt. Compare every compact checkpoint
  with full reference replay/summary; assert bounded retained operation buffers,
  no full-prefix decode on ordinary reopen and no automatic history loss.
- Tamper contiguous ordinals, equal-count divergent prefixes, manifests/pages,
  checkpoint anchors, RNG words/version, compact state invariants, attempt records
  and archive proofs. Reject atomically; do not accept a valid count with the wrong
  content. Inject failed/late receipts during anchor-generation change.
- Review old attempted snapshots after catalog replacement/removal; delete one
  terminal attempt, terminal current attempt, session and all history. Assert
  exact remaining summaries, no orphan snapshots/embedded payload copies, truthful
  redacted audit ranges, unaffected catalog and refusal for every nonterminal run.
- Replacement/deletion races and reopen prove durable and runtime pins separately.
  Drop/transfer of recovery transport permits must leave actual live graph/cache
  reservations held until the last owner/job/Arc releases them.
- With a missing/corrupt future catalog, open an unavailable descriptor from the
  real validated checkpoint, then confirm End for an unresolved current attempt,
  failed-practice current attempt, solved review and archived-plus-never-activated
  next attempt. Assert exact outcomes/counts/durations and zero future lookups,
  RNG draws, synthetic attempts or duplicate archived contributions. Verify
  subsequent confirmed history deletion and released durable version pin.
  Exercise the pure unavailable-state End against canonical `PuzzleRun` End
  outcomes separately from outer envelope revision/proof and desktop lifecycle
  fixtures. Verify the lower state imports no persistence/storage/desktop owners.
- Exercise unavailable End cancellation before acceptance, failed admission/commit,
  revision/prefix/generation conflict, canceled pending wait, delayed/uncertain
  completion, duplicate retry and late acquisition. Uncommitted failure retains
  the pin/checkpoint/history; committed End is recovered exactly once after
  relaunch, and no Saved/terminal publication or deletion precedes its receipt.
  Invalid user checkpoints must refuse mutation without fabricated descriptors.
- Backup/restore excluding replaceable catalogs preserves attempted review and
  exact dependency refs/order; included legacy bundles preserve continuation.
  Missing excluded data permits confirmed End but no continuation. Include v1
  backups, digest collision by name, canceled restore and interrupted cutover.

Measure optimized builds only, using the existing reference-machine and sampling
protocol in [decisions](../decisions.md#acceptance-measurement-protocol). Record
compiler/lock/features, OS/CPU/GPU/storage, fixture/catalog/manifest hashes,
worker budget, cold/warm cache state, repetitions and actual percentiles. Do not
run these measurements concurrently with release compilation or other diagnostics.

Required quantitative/structural gates:

- Actual approved starter preparation reaches the available selected target up
  to 200,000 puzzles under the unchanged policy, with genuine source/license/
  selection hashes and measured installed component totals. The puzzle component
  remains within its approved 300,000,000-byte ceiling and all starters within
  2,000,000,000 bytes. Run-envelope limits cause zero target reduction.
- For 2,000, 20,000 and 200,000 valid fixture members, each periodic checkpoint is
  at most 256 KiB and contains zero collection definitions; per-tick storage work
  depends on bounded suffix/current state, not collection or lifetime size.
  Recover no more than one 4 MiB/4,096-operation tail after a verified compact
  anchor. Separately measure selection-page validation/preparation costs.
- Proposed engineering target: p95 Start Run and paused reopen at 200,000 members
  within 3 s on the four-thread/8 GiB baseline; p95 ready-definition transition
  within 100 ms warm and a visible loading state for cold misses. These are design
  targets requiring measurement/parent adoption, not existing measured acceptance
  or substitutions for the specification's game-start/input limits.
- Preserve the specified native p95 feedback <=100 ms and frame intervals
  <=16.7 ms under the recorded full-capacity workload, plus durable-lag <=1 s in
  every controlled persistence failure case. Record checkpoint commit and
  acquisition latency distributions separately; a freeze is correctness behavior,
  not evidence that the steady-state latency target passed.
- Report peak RSS, live decoded graph bytes, cache/SQLite page-cache bytes,
  selection buffers and transient preparation allocations separately. Enforce the
  admitted live/cache/buffer ceilings with deterministic counters. Show memory
  stays bounded over >100,000 operations and repeated relaunches; encoded transport
  reservations are not the memory result.
- Compare one admitted worker with the root's detected maximum on independent
  definition validation/preparation workloads. Report throughput, memory and
  cancellation/foreground latency, plus whole-process idle CPU under the existing
  <1%-of-one-logical-core target. Do not claim linear scaling or create separate
  budgets to make a benchmark look parallel.
- Run canonical gates after the authorized source implementation, then execute
  actual native service/panel, focus/sleep/recovery and offline installed-package
  scenarios on all supported targets. Source-only fixtures and the parent's
  reported hermetic suite cannot satisfy these native/US13 release gates.

## Decisions still requiring implementation resolution

The architecture above selects indexed immutable SQLite definitions, persisted
compact membership/order, existing seeded RNG continuity, asynchronous desktop
supply, bounded validated checkpoint recovery and retained paginated history.
Remaining decisions are concrete integration/measurement work:

- Compact-anchor adoption requires passing real full-replay equivalence and
  domain-invariant gates before production; no testless change or additional user
  approval flow is proposed. Final checkpoint/segment/cache tuning remains based
  on measurements. Full archived audit remains available except explicit deleted
  ranges; automatic full lifetime replay is not retained.
- Verify the resolved rand feature graph and Serde state wire vectors with the
  pinned dependency. Any future sampler/library upgrade must retain a real
  previous-version implementation. No speculative state API is assumed.
- Parent chooses final relation names/consecutive migration number and aligns
  its saved-run inventory read contract with v1/v2 descriptors and cutover markers.
  US13 integrates the durable version-reference keys into its one registry.
- Validate decoded-definition admission estimates and actual SQLite cache/read
  connection reuse under the shared executor before fixing the live memory limits.
  Space checks cover retained versions, staging and user history, not just the
  installed starter cap. Full disk and compaction failure must preserve the last
  valid run and report a recoverable refusal.
- US13 must produce the real source manifest/provider/catalog and measured bytes;
  none exists merely because the lower interfaces are implemented. Native
  composition, packages and design approval remain parent-owned.
