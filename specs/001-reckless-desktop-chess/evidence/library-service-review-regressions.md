# Library service independent-review regression handoff

Five deterministic tests are authored: four blocker regressions and one
valid-size publication-pressure control. Tests executed in this lane: **0**.
Compilation, semantic red observations, formatting, Cargo, Clippy, builds,
application execution and final acceptance remain parent-owned. No production
repair, source registration, existing test-root edit, fixture edit, import-space
edit, checkpoint-regression edit, agent, extra worktree, commit or push is part
of this incremental artifact.

The global guidance, project bridge, constitution, catalog and all seven
canonical rules were read. The feature remains US3; this handoff does not alter
task markers. The governing contracts remain in the
[rule catalog](../../../.specify/rules/README.md).

## Frozen dependency

- Parent commit: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
- Dependency patch: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-review-dependency.patch`.
- Dependency sidecar: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-review-dependency.json`.
- Verified dependency SHA-256: `45318933010b2e27219d0cd7184f4229eb1647038d5993817387d021510b3ad5`.
- Verified dependency tree: `b4c3f2da385a5aac364a0ee4cedfe0bf1ed416b2`.
- Export: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-review-regressions.patch`.
- Exact manifest: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-review-regressions.manifest.json`.

The dependency was applied unchanged and its staged tree was verified before
authoring. The export is incremental from that tree. The manifest records the
patch hash, final staged tree, exact paths, per-file blobs and SHA-256 values,
test names and authored/executed counts. Parent-reported runs and repairs after
this freeze are coordination context, not execution evidence for these tests.

## Parent-owned registration

The export contains exactly these four new paths:

- `tests/desktop/library_service/played_recovery_review.rs`
- `tests/desktop/library_service/admission_memory_review.rs`
- `tests/desktop/game_registry/saved_open_review.rs`
- `specs/001-reckless-desktop-chess/evidence/library-service-review-regressions.md`

Parent must append the following private inclusions to the existing
`tests/desktop/library_service.rs`:

```rust
#[path = "library_service/played_recovery_review.rs"]
mod played_recovery_review;
#[path = "library_service/admission_memory_review.rs"]
mod admission_memory_review;
```

Parent must append this private inclusion to the existing
`tests/desktop/game_registry/saved.rs`:

```rust
#[path = "saved_open_review.rs"]
mod saved_open_review;
```

Both existing roots remain byte-for-byte identical to the frozen dependency in
this lane. No additional source-root inclusion is exported. The admission
control uses the shared played-review observation owner, so both library
modules must be registered together.

## Authored tests and intended observations

1. `desktop::library_service::tests::played_recovery_review::fresh_retry_save_recovers_original_played_publication_and_reconciles_both_service_receipts`

   A real local played session explicitly enters Analysis before canonical
   SQLite seeding. The actual saved-open path installs its owner. With all
   remaining receiving memory reserved, a small notes edit must be accepted
   exactly once and report typed `EditorPublicationPending` from real memory
   saturation. The retained supervisor receipt must report Publishing, no
   storage acknowledgement, and the original application outcome. The actual
   registry snapshot must expose the accepted revision before recovery.

   After memory release, a newly admitted application receipt submits
   `RetrySave` with the last published context. Its outcome and the original
   service outcome are captured before any cleanup continuation. Both must
   reconcile to the same real played save acknowledgement, original epoch,
   accepted generation and source. The stored source must contain exactly one
   revision increment and one content-revision increment, with one UTC read and
   the original timestamp. Frozen source inspection predicts stale-context or
   Busy refusal because fresh work does not recover the older publication.

2. `desktop::library_service::tests::played_recovery_review::cancelled_original_service_receipt_recovers_already_accepted_played_publication`

   The same real acceptance observations are collected before requesting
   cancellation. After memory release, the original application receipt and
   byte-equivalent operation are resubmitted. Cancellation must not erase the
   already accepted played edit or cause another UTC read. The test captures
   the service result before cleanup and requires the real original save
   acknowledgement. Frozen source inspection predicts service `Cancelled`
   before it can observe the accepted played publication. This is distinct from
   the parent's notation Checkpoint regression.

3. `desktop::game_registry::saved::tests::saved_open_review::failed_saved_supervisor_initialization_releases_each_source_reservation_without_registry_shutdown`

   Two distinct real saved played IDs are opened sequentially without shutting
   down the review registry between attempts. Application receipts use the
   existing working application clock. The review factory boundary arms a
   separate injected clock only after real loading has reached factory
   construction. Supervisor Initialize then receives a single typed clock
   failure. Its factory is an explicit resource-free unit value, so constructor
   failure does not imply an unobserved factory resource or require weakening
   the existing fixture's factory/shutdown assertions.

   Each attempt records the exact initialization error, original application
   correlation, absence of an editor receipt, empty snapshots, UnknownGame from
   the real registry port and receiving-memory availability. Read-only SQLite
   observations compare every seeded document body/revision, library content
   revision, modified time, location, digest and catalog revision after each
   failure. All reservations must be released after each attempt while the
   registry remains alive. Frozen source inspection predicts retained source
   reservations accumulating across the two failed constructions.

4. `desktop::library_service::tests::admission_memory_review::over_total_budget_notes_are_refused_before_admission_without_retained_payload_growth`

   Two sequential attempts each contain exactly
   `StorageDocumentStore::MEMORY_BUDGET + 1` ASCII note bytes. Each must receive a
   typed size refusal directly from `try_submit`, before any service future is
   polled. Accepted refusal shapes reuse existing canonical storage
   DocumentTooLarge or codec TooLarge/TextTooLarge errors through the service's
   Library error. Generic Busy, cancellation and unrelated refusals do not
   satisfy the test.

   For baseline cleanup, any unexpectedly admitted command is cancelled before
   polling service/CPU work, then driven to actual service Cancelled. No huge
   annotation mutation is executed. The source context and notes, exact stored
   body/revision, library revision, original UTC and UTC-read count must remain
   unchanged. Actual retained service request count, retained note lengths,
   admission slots and shared receiving permits are observed immediately after
   admission and after completion for both attempts. Refusal must retain zero
   additional note bytes or requests and consume no admission slot. Frozen
   source inspection predicts two admitted payloads retained by the transport
   despite their cancellation, with no matching storage-budget charge.

   The summed note lengths are an observable lower bound on logical retained
   payload bytes at the actual owner. They do not measure String capacity,
   allocator overhead, process RSS, all vector payloads or a general aggregate
   byte ceiling for many individually valid commands. Two oversized allocations
   are used; no volume or wall-time benchmark is claimed.

5. `desktop::library_service::tests::admission_memory_review::valid_sized_played_notes_under_temporary_memory_pressure_remain_recoverable`

   This positive control uses small valid notes with the same full receiving
   pool, then releases memory and retries the original uncancelled played
   receipt. It requires the same real save acknowledgement, one mutation, one
   UTC read, original timestamp and complete cleanup. It protects the accepted
   transient-pressure behavior while the over-total-budget regression requires
   pre-admission refusal. Source inspection predicts this control can pass on
   the frozen baseline; it has not been run here.

## Cleanup and verification boundary

For the two blocker recovery tests, the original and retry service outcomes are
copied into handle-free observations before fallback. If publication remains
pending, cleanup uses the actual supervisor receipt retained by the real
registry with the original application outcome. The canonical continuation
obtains and checks an actual SQLite save acknowledgement; it does not replace a
captured failing service result with success. The service then observes its
real receipt's completed pending state during cleanup.

Each scenario closes its service/registry, observes storage and executor
shutdown, preserves existing factory cleanup assertions, drops receipts and
publications, and checks full shared-memory and CPU-capacity release before its
final blocker assertions. There are no sleeps or timing thresholds. Three-second
timeouts bound missing ownership observations or cleanup; elapsed time is not a
passing condition. No private production state is mutated by the tests.

Only source inspection and Git artifact integrity checks were performed here.
No compilation or observed red/green result is claimed. Parent must register the
new modules, establish the four semantic reds on the frozen source, and only
then begin a separately authorized production repair round. The existing
Checkpoint regression and the parent's Setup/Analysis and unchanged-note fixture
corrections remain outside this patch.
