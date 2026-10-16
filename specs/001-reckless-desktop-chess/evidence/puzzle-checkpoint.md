# Puzzle checkpoint ownership

This slice implements the deterministic checkpoint state machine for the focused
parts of T178/T187. It follows [FR-021 and Story 8](../spec.md),
[the selected durability policy](../decisions.md#puzzle-interchange-and-lifecycle)
and [the study-tools contract](../contracts/study-tools.md#puzzles).
It does not complete either task. Storage, native lifecycle notifications and
cadence/capture-cost acceptance remain outstanding.

## Integration contract

`PuzzleCheckpoint<S>` owns at most one queued, writing or failed immutable
snapshot and one latest coalesced replacement. `S` has no `Clone` bound. The
owner never clones, serializes or inspects the payload. A replacement must
contain the cumulative accepted state and events needed to supersede prior
replacements. The parent must register `pub mod checkpoint;` in
`src/puzzle/mod.rs` and include `tests/puzzle/checkpoint.rs` from
`tests/domain.rs`. This patch changes neither shared root.

Observations use the actual `PuzzleSessionContext`, checked session revisions,
`ClockInstant`, `ClockDuration`, `PuzzleRunEnumStatus` and cumulative active
elapsed counter. `PuzzleCheckpointObservation::from_run` reads the real run.
The parent added the `session_id()` and `revision()` context accessors in its
checkout. T186 source was read directly from
`/Users/sadrashameli/Personal/chess/src/puzzle/run.rs` and was not copied or
changed. Its confirmed-End-at-per-puzzle-expiry correction remains parent-owned.

The driver must follow this order:

1. Construct an initial inactive or recovered-paused snapshot. `new` queues it
   without asserting durability. Submit it and observe its actual committed
   result before starting countdown. Calling the advancement guard earlier
   freezes with `InitialSaveRequired` and requires a real paused snapshot and
   explicit Resume after saving.
2. Call `before_advance(now)` before any run polling, accepted input, transition
   or snapshot operation that can charge time. `PuzzleRun::snapshot` itself
   polls, so it belongs after this guard too.
3. On `FreezeRequired`, pause the actual run at `freeze.pause_at()` before
   accepting advancement. This instant may precede `now` after a delayed
   callback. The run must not already have been advanced to `now`. Capture the
   resulting paused state at `now` and record that accepted transition. An
   already paused run stays paused; do not replay an old pause instant into it.
4. After applying a permitted run operation, poll/snapshot consistently before
   constructing its observation if expiry can change context. Record every
   accepted transition with `AcceptedTransition`. Unchanged or rejected input
   is not a transition. For ordinary active observations, `needs_snapshot` with
   `Countdown` checks cadence before constructing the recovery payload. A
   revision/status change requires transition capture instead.
5. `begin_write(now)` produces a token for `pending()` and retains that exact
   snapshot. The writer must retain/observe its operation and echo the token to
   `complete`. Tokens bind a checked write identity, original full session and
   attempt/source context, original revision and original active elapsed.
   Only a real committed transaction maps to `Committed`. Failure and observed
   cancellation map to `Failed`; dropping a future or requesting cancellation
   does not prove observed cancellation or transaction quiescence.
6. Handle the advancement result from `complete` before further timer work.
   It evaluates the old durable boundary before accepting a receipt, so a late
   success cannot erase a missed freeze. Stale, duplicate, foreign, not-started,
   old-retry and altered-original-revision/counter completions cannot resolve a
   current request. Only the serialized currently writing token can succeed.
7. `retry_save(now)` is explicit and starts a fresh write identity for the same
   failed payload. A newer coalesced snapshot does not replace it. Successful
   retry promotes the newer snapshot for its own observed save. Both failure
   and observed cancellation retain unsaved memory and expose Retry Save.
8. Once the newest actual paused snapshot is durable and no write remains,
   the owner exposes `AwaitingResume`. Only explicit `resume(now)` opens its
   gate. The caller then performs actual `PuzzleRun::resume` and records that
   transition. No readiness/focus event or save acknowledgment resumes either
   owner. Ordinary focus/manual pauses continue to use the run's own explicit
   Resume semantics even when the checkpoint gate never froze.

There is no replacement countdown. Cadence uses the real cumulative active
elapsed difference, including across per-puzzle resets. The guard derives only
the remaining durability allowance from the last actual observation. It freezes
at the exact one-second boundary while a save remains unresolved. Review and
other inactive wall time consume no allowance. Deadline, elapsed and write-ID
arithmetic is checked; arithmetic errors must interrupt the caller, not permit
timer advancement. A recovered run uses the actual recovery revision and a new
process-local clock epoch; this owner persists no monotonic timestamp.

The existing `GameSave` receipt/gate logic and `GameSession` acknowledgment and
save-failure interruption were inspected. Their invariant that an old receipt
cannot clear a newer required save, and that success cannot resume a paused
session, is preserved here using puzzle context and full request correlation.
Puzzle code imports no game-owned save behavior. Multiple countdown requests
may share a domain revision, so write identity and active counters are also
necessary; comparing only against the current run revision is insufficient.

## Verification status

The initial tests were written before implementation. There are 14 integration
tests and three private boundary tests. They use controlled nanosecond time,
real validated puzzle definitions and the real run where applicable. Coverage
includes cadence, immediate transition capture, stalled/coalesced writes, exact
boundary and delayed completion races, committed-counter rebasing, immutable
retry, original-revision and counter correlation, stale/out-of-order/duplicate
completion, cancellation, explicit Resume, initial unsaved state, backwards
time/counters, deadline/write/retry-ID overflow and a non-Clone payload.
The focus/sleep/hide cases exercise domain signals, not OS notifications.

The standalone formatter is pinned by the installed `rustc 1.99.0` and reports
`rustfmt 1.10.0`. Only the two owned Rust files are formatted with edition 2024.
Their formatter check and the staged patch whitespace check are the local
verification scope. Cargo is reserved by the parent and has not been invoked.
Zero tests have executed here. No observed failing/passing test, Clippy, build,
full gate or native acceptance is claimed. Parent integration must register the
files, consume the authoritative updated context/run source and run checks.

## Remaining acceptance

The migration and transactional puzzle repository are not included. No concrete
durable receipt adapter, I/O actor, database transaction, cancellation protocol,
native focus/visibility/sleep subscription, storage-error display or production
pause reason is implemented. The integration test uses the existing Manual
pause reason solely to exercise actual run freezing; the parent owns the
saving/interruption reason and production driver ordering.

The state machine creates no tasks, runtime, wall clock, randomness, logger,
storage dependency or hidden timer. Its write sequencing is serial because
later durability depends on the observed prior request. Its snapshot count is
bounded, but payload size depends on retained history and selection data.
`needs_snapshot` avoids constructing a full recovery payload on non-due ticks;
the existing full `PuzzleRun::snapshot` still clones recovery collections when
the caller chooses that payload. A writer may use immutable shared ownership
or encode at its effect boundary without changing this generic owner. The
actual 250 ms scheduler, full-capture cost, memory budget, transactional event
retention and shutdown/quiescence behavior require integration measurements.
No runtime or performance result is inferred from this design.

The constitution/catalog and all seven canonical rule files were loaded.
Architecture, structure, naming, checked arithmetic, no-comment scope and
dependency direction were reviewed. Checklists remain read-only, task markers
remain parent-owned, and no dependency/shared source/configuration was changed.

## Parent verification

Integrated with the real run and context accessors, and registered both source
and domain tests. Formatter and Clippy for the library and every default test
target passed. All 61 library, 289 domain, 213 desktop and 87 presentation tests
passed in the latest affected-component run, including the three private and
14 public checkpoint cases. Read-only review found no concrete blocker in the
lag guard, cadence, coalescing, immutable retries or explicit Resume gate.

This accepts the deterministic checkpoint boundary only. T178/T187 remain
open for the actual persisted reconstruction, transactional writer, production
run-before-advance integration, native focus/window behavior and measured
checkpoint cost. A caller must honor FreezeRequired before advancing the run;
there is no background durability claim. The existing linker warning remains.
