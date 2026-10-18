# T187 puzzle persistence verification handoff

Status: journal-integrity and completion-time corrections are source-ready.
The parent observed both regressions failing before their corresponding source
fixes. This correction lane runs no Cargo commands; updated Rust compilation,
Clippy and passing tests remain parent-owned and unverified here. Earlier passing
counts below describe only the dependency baseline. T187 remains open for native
cadence, freeze integration and full acceptance.

This lane consumes the verified puzzle core at `7ce747f`. It follows the
[canonical rules](../../../.specify/rules/README.md),
[Story 8 and FR-021](../spec.md#user-story-8---solve-chess-puzzles-priority-p1),
[puzzle decisions](../decisions.md#puzzle-interchange-and-lifecycle),
[study contract](../contracts/study-tools.md), and
[puzzle model](../data-model.md#reports-openings-and-puzzles).

## Schema and transaction

The actual next migration is
`src/storage/migrations/0002_puzzles.sql`, registered as version 2 in
`StorageMigration::core()`. Only migration 1 existed. The parent accepted this
consecutive sequence and owns changing T187's planned `0005_puzzles.sql` path.
No empty migrations are introduced.

Five dedicated tables hold immutable collection/configuration metadata, runs,
append-only ordered events, compact current checkpoints and attempt projections.
No puzzle envelope enters `documents`, so generic game inventory and native game
recovery keep their existing contents. The same `StorageDocumentActor`, SQLite
connection, bounded admission/memory policies, capacity lease and runtime execute
all puzzle commands. No second writer, connection-owning actor or runtime is added.

A write uses an immediate SQLite transaction, checks the original expected storage
revision and exact journal prefix, verifies the pinned collection digest, inserts
only unsaved events, upserts affected attempts/checkpoint and advances the storage
revision. Only successful `transaction.commit()` constructs a committed receipt.
Existing immutable metadata is compared by digest rather than reread in full for
countdown writes. Read transactions validate versions, digest, contiguous event
ordinals, counts, canonical checkpoint reconstruction and all attempt projections.
Missing or inconsistent rows are errors.

## Validated recovery and randomness

Private, versioned interchange DTOs derive serialization. Domain state does not
derive deserialization. FEN, moves, source identities, metadata, filters and config
are reconstructed through their canonical constructors. The codec reconstructs
attempts and run state by replaying ordered typed operations through `PuzzleRun`.
Each operation carries accumulated active duration, expected resulting context,
attempt state, event counts and any finalized prior attempt. Legal listed moves,
opponent reply choices, progressive hints, practice completion and lifecycle
transitions are regenerated and compared with the saved projection. Saved primary
outcomes, phase, assistance, elapsed balances and counts cannot directly populate
mutable domain fields. A failed primary therefore remains failed after a practice
solve. Listed solution branches and explicit promotion pieces remain source data.

The collection records the actual `Xoshiro256PlusPlus` implementation, algorithm
contract version 1 and the exact supplied `u64` seed. An already-consumed provider
is refused. Initial shuffle order/draws and actual reply choices are saved;
subsequent ordering is reconstructed through the seeded canonical operations.
Recovery initializes that declared provider and consumes its actual transcript.
It never infers a seed or substitutes another generator. Unknown versions and
algorithm identities are refused.

Only active durations are serialized. No process-monotonic instant, deadline or
clock epoch appears in the payload. Replay uses a private virtual clock built from
those durations. Canonical `PuzzleRun::recover` then restores the timer paused and
records a recovery operation, permitting later saves and further relaunches without
an arbitrary random reset. Visibility and save success never resume the run.

## Checkpoint ownership

`PuzzlePersistenceRun::capture(now, persisted_event_count)` polls expiry, captures
compact current state and references only unsaved journal entries. Collection bytes
are encoded once and shared immutably. Captures do not call the expensive recovery
snapshot or clone archived attempts/solution definitions. Full recovery snapshots
are used only at the explicit recovery boundary.

`PuzzleStorageDriver` consumes injected `PuzzleStorageRepository` and
`PuzzleStorageWrite` ports. It retains one write handle and uses the existing
`PuzzleCheckpoint` owner for one latest coalesced state, cadence and durable-lag
validation. A newer snapshot is rebased onto the actually committed event count
before its next transaction. Cancelling an awaited `wait(&mut self)` leaves the
handle and original receiver owned by the driver. A failed request retains its
original immutable payload and expected storage revision; Retry Save changes only
the checkpoint write token. An actual committed receipt is retained until
checkpoint acknowledgement succeeds. A typed repository error and request are
retained if failure acknowledgement rejects an invalid time. Neither save nor
Retry Save calls run Resume.

The driver exposes the existing guard/freeze result. The eventual native owner
must call it before every input/time advancement, pause at the returned boundary,
record that paused state, drive due captures every 250 ms and require explicit
Resume after the newest paused snapshot is committed. This lane does not claim
that a running native window is already wired to those guards.

## Journal integrity and completion-time correction

The previous save guard compared the collection digest and event count without
proving the contents of the already-persisted prefix. Two owners with the same
session, seed and collection could have equal journal lengths but different
accepted operations. A current storage revision then admitted an incompatible
checkpoint. The corrected snapshot carries its original base proof and cached
proofs beside immutable journal entries. Each newly appended event extends the
SHA-256 chain once. Capturing a countdown checkpoint reads the cached prefix in
constant time and references only the unsaved suffix; it never hashes or clones
the retained journal body. This is a source property, not measured latency.

The version-1 initial proof hashes the bytes `chess.puzzle.journal.initial`, a
four-byte big-endian proof version and the collection SHA-256. Each extension
hashes `chess.puzzle.journal.event`, that version, the previous 32-byte proof,
an eight-byte big-endian UTF-8 event length and the exact canonical event bytes.
The SQL run row stores the proof version and terminal 32-byte digest. An immediate
write transaction validates metadata/version/initial proof, compares the exact
base proof as well as count/revision/collection, and advances the proof atomically
with event/checkpoint rows. Loads recompute the persisted chain and compare it
again with canonical replay before appending recovery. Empty-prefix metadata,
unsupported versions and malformed digests are refused. Slice/conversion causes
remain typed source errors. Migration 2 is still this unshipped slice's owning
migration; a database already created from the earlier provisional migration 2
requires the parent's compatibility decision before use. No user data was migrated
or rewritten by this lane.

Committed receipts retain the terminal proof. The driver validates its initial
persisted count/revision combination and records the proof at that boundary.
Snapshot recording and coalesced rebasing compare the original prefix against
that proof, including when an earlier in-flight receipt changes the base.

The previous asynchronous `PuzzleStorageDriver::wait(now)` acknowledged with an
instant captured before awaiting the database. A delayed completion could move
durability past the old one-second deadline without freezing. The API now requires
`wait().await` to retain the receipt or typed repository failure, followed by
synchronous `settle(now)` with a freshly sampled application-clock instant.
Waiting never advances checkpoint durability or the storage revision. Settlement
evaluates the original boundary before installing the new durable state. Dropping
a pending wait retains its handle; repeated waits on a retained completion are
idempotent. Rejected settlement retains its receipt/error and immutable request.
No public instant is reinterpreted and no hidden clock or sleep is introduced.

Tests were exported before either source fix in
`puzzle-persistence-integrity-red.patch`. The prefix fixture initially failed setup
with `ActiveInitialState`, then `InvalidElapsed`; these were not defect evidence.
The tests-only `red-fixture` and `red-timeline` correction patches established an
inactive initial commit and separately acknowledged readiness before Hint. The
parent then reported meaningful red:

- `/tmp/chess-persistence-prefix-red-20261016.log`: the divergence rejection
  assertion failed because the baseline accepted the incompatible journal;
  one selected test failed and zero passed.
- `/tmp/chess-persistence-time-red-20261016.log`: completion advanced revision to
  3 when the test required revision 2 until current-time settlement.

These red results were reported by the parent, not executed in this lane. The
timing regression was then updated to the split API and asserts the returned
freeze retains the original one-second boundary after controlled delayed
completion. Existing cancellation, failed-save retry and rejected-acknowledgement
tests also use the split API. New cases cover actual event-bearing coalesced
rebasing with a later recovered-owner save, SQLite constraint rejection of
unsupported proof versions and malformed lengths, and refusal of wrong
empty/nonempty prefix proofs on reads and writes. No constraint is disabled for
these fixtures.

Parent test filters are
`current_revision_and_equal_event_count_do_not_authorize_a_divergent_prefix`,
`delayed_completion_requires_current_time_settlement_before_durability_advances`,
`coalesced_events_rebase_to_the_receipts_original_journal_prefix` and
`invalid_journal_proof_metadata_is_rejected_on_read_and_write`, all in the
`storage` target. The correction suite has not been run here. Required consolidated
gates, native execution and performance measurements remain outstanding.

Executed in this correction lane: explicit-file Rust 2024 formatting and its
matching check, plus an independent in-memory Python SQLite 3.53.1 smoke of both
real migrations, proof-version/digest-length constraints and a valid initial-proof
round trip. The first SQL-smoke invocation stopped before execution because it
named `0001_documents.sql`; it was rerun against the actual `0001_core.sql`.
This smoke does not exercise the Rust adapter or prove its tests pass.

## Dependency baseline verification

The following results were recorded in the supplied dependency patch before the
two integrity defects were found. They are not passing evidence for this delta.

Executed:

- Explicit-file `rustfmt --edition 2024` and its matching `--check`, followed by
  pinned Cargo formatting with local verification registrations.
- `cargo clippy --locked --lib --test domain --test storage -- -D warnings`: passed.
- Sequential `cargo test --locked --lib`, `cargo test --locked --test domain`, and
  `cargo test --locked --test storage`: 60, 293 and 35 passed respectively, totaling
  388 passed with zero failed, ignored or filtered-out tests. The new codec and
  repository suites account for 4 and 7 of those cases.
- Every Cargo command used
  `CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target`. The active compiler
  reported Rust 1.99.0, matching the pin. Compiler and test parallelism retained
  their detected defaults.
- Scoped whitespace checks, including newly added paths through an isolated
  temporary Git index used to build the owned-path binary patch.
- Authored-source scan for comments, suppression/panic helpers and forbidden dash
  punctuation.
- In-memory SQL smoke using uv-managed Python 3.14 and SQLite 3.53.1: both real
  migrations, five puzzle tables, separate game inventory, immutable collection
  metadata and full rollback after an injected final checkpoint failure.

Executed successfully: 11 deterministic Rust tests across
`tests/puzzle/persistence.rs` and `tests/storage/puzzle_repository.rs`. They cover
branched solutions, failed then assisted practice completion, all listed promotion
choices retained in the source tree, repeated relaunch with active-duration
accounting, malformed payload/schema/algorithm/balances/counts, stale storage
revision, real SQLite close/reopen and hidden paused recovery, transaction rollback,
cancelled waits, one write plus latest coalescing, original failed-snapshot retry,
retained typed failure/committed receipt when acknowledgement rejects time, and
corrupt checkpoint/attempt rows. No sleeps, network or user-data paths are used.
The Rust tests exercise the actual bundled rusqlite adapter. The earlier SQL smoke
uses Python's SQLite independently.

The first targeted Clippy invocation failed with four `FromSql` errors because
`usize` reads require rusqlite's disabled optional `fallible_uint` feature. The
repository now reads SQLite integer columns as `i64` and converts them with checked
conversions. Its new rollback test uses an `i64` SQL count for the same reason.
No dependencies, features, existing regression tests or expectations were changed.
The next Clippy pass and all requested suites passed. No runtime test failure was
observed before these corrections.

The domain and storage test invocations also linked/reused the debug application
binary and emitted this macOS linker warning:
`ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected`.
Rust reports that `linker_messages` ignores `-D warnings`. The targeted Clippy run
had zero warnings; the test builds had this linker warning. It was reported to the
parent without a suppression, thread cap or out-of-scope build change. Full
optimized/native builds and end-to-end native acceptance were not run in this lane.

## Parent integration and limits

The verification worktree locally registers
`puzzle::{persistence, storage_repository}`, `storage::puzzle_repository` and both
new nested suites. The parent must register them in its integration checkout and
run the remaining canonical gates after applying the patch. The owned patch
contains no Cargo files, module/test roots, tasks, history deletion or native/UI
changes.

The codec is explicitly bounded to 16 MiB per encoded run, 100,000 journal entries,
200,000 candidate definitions, 1,024 lines per definition and 4,096 plies per line;
identity tokens are limited to 1,024 bytes. Journal budget admission reserves room
for a checkpoint and recovery. These bounds do not establish support for a full
production dataset or arbitrarily long runs. Recovery replays the run's complete
journal and validates collection solutions on the existing capacity-budgeted
storage actor. Dataset references, production-scale recovery latency, memory
measurements, native scheduling/freeze/error presentation and end-to-end Story 8
acceptance remain open. History retention has no automatic deletion path.

## Parent consolidated validation

The parent observed both integrity regressions fail against the previous source,
then integrated the versioned prefix chain and completion/settlement split.
The complete all-feature hermetic selection passed 975 tests, including all
42 storage tests, 293 domain tests and 84 private library tests. Both canonical
Clippy selections passed without warnings. The optimized all-target build and
the separately named native harness build passed. The pre-existing debug macOS
linker `__eh_frame` warning remains unsuppressed.

Read-only revalidation found no remaining concrete blocker in prefix validation,
coalesced rebasing, committed-receipt retention, fresh-time settlement or exact
Retry Save payload/revision ownership. These results do not accept T187's native
250 ms scheduling, one-second freeze presentation or complete Story 8 acceptance.
