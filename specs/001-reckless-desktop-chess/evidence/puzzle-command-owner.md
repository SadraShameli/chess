# Puzzle command owner handoff

This slice starts from local commit `5e9f55fd94056050e3ba00d7480c7c87f78ac00c`.
It authors the retained desktop puzzle command owner and its compact immutable
projection for the focused parts of T178/T187/T190. T187 and T190 remain open
for native scheduling, lifecycle wiring and full acceptance. All 13 approved
journeys remain in scope. No task or checklist marker is changed.

The constitution, catalog and all seven canonical rules were read. The
`speckit-implement` workflow was loaded, and prerequisites resolved with the
explicit canonical feature directory. Requirements and native requirements
checklists contain respectively 16/16 and 40/40 checked criteria; the separate
native review contains no checkbox items. These are requirements review counts.
No extension hook file is present.

## Ownership and integration

[DesktopPuzzleCommands](../../../src/desktop/puzzle_commands.rs) is generic over
the lower-owned `PuzzleStorageRepository`. It retains the actual
`PuzzlePersistenceRun`, `PuzzleStorageDriver` and latest immutable read model.
Caller construction supplies the validated collection, configuration, filter,
session identity and exact declared seeded provider through the existing
`PuzzlePersistenceRun::new`, or supplies its validated recovered run. This slice
adds no collection discovery, engine search, second repository or chess rules.
Production collection preparation remains US13. History deletion remains T189;
deleting an individual projection row cannot change immutable journal recovery.
No deletion operation is attempted.

The parent registers `pub mod puzzle_commands;` and `pub mod puzzle_snapshot;`
in `src/desktop/mod.rs`. The owner includes its mirrored test file as a
crate-local test module so the typed storage fake can construct the existing
crate-private committed receipt. Do not also register that file in
`tests/desktop.rs`. Shared source/test roots, native/bootstrap/presentation,
messages, coordinator, selection, session and attempt files remain parent-owned.

The public API is:

- `new(run, repository, now, storage_revision, persisted_event_count)` queues the
  initial capture without starting a fallible write that could discard ownership.
  Call `begin_write(now)` or `snapshot(now)` to submit it before waiting.
- `context()` returns the full current `PuzzleSessionContext`.
- `execute(expected_context, command, now)` checks the full identity and revision
  before applying a real persistence command. An automatic pause or expiry can
  invalidate the supplied context; obtain a fresh snapshot before another input.
- `snapshot(now)` guards, polls, captures due state and submits a queued write,
  then returns `DesktopPuzzleSnapshot`.
- `summary(now)` follows the same guard and obtains the canonical summary on
  demand. Countdown snapshots do not rebuild a summary over archived attempts.
- `retry_save(expected_context, now)` explicitly retries the retained failed
  request with its original payload and expected storage revision.
- `has_retained_write()` reports a live lower handle or an unsettled completion.
  After a failure is settled it is false, while the failed request remains
  retained for Retry Save. `wait()` must only be selected when it is true.
- `wait().await` observes completion without reading a stale clock value.
  Call `settle(fresh_now)` afterward. Dropping a pending wait leaves the same
  lower handle and receiver retained. Settling may immediately start the next
  coalesced write; recheck `has_retained_write()` after settlement.
- `next_deadline()` selects the earlier canonical checkpoint deadline and actual
  timer expiry. It introduces no second cadence or durability arithmetic.

The patch excludes shared root registrations. Both source modules were registered
temporarily for the verification below, then those local registrations were
removed. The public API and parent registration contract are unchanged.

## Advancement, durability and recovery

Every command, explicit polling command, snapshot, summary, write submission,
retry and settlement uses the checkpoint guard before any run operation that
can advance time. The initial constructor rejects an active run before its
first capture. A delayed callback pauses exactly at `freeze.pause_at()` before
polling or capturing at the callback's current time. It never first advances
the actual run to the delayed instant. An already paused run is not fed an old
pause instant again. Paused snapshots hide both board and hint content.

A fresh inactive initial save blocks Ready and countdown advancement. Once the
initial inactive state is actually committed, the narrow checkpoint
`release_initial` gate permits a new explicit Ready to start the run. Readiness
is never queued for automatic replay. While the fresh initial save is blocked,
inactive read-only projection does not poll the actual timer forward, preserving
the historical initial freeze instant if the eventual write fails. A recovered
run is already paused, and an initial save failure creates a real save pause;
both require explicit Resume.

`SaveBackpressure` is a distinct `PuzzleTimerEnumPauseReason`, with exhaustive
capture/domain conversion in the existing persistence codec. It does not reuse
Manual or FocusLost. The lower checkpoint retains one original queued/writing/
failed snapshot and one newest coalesced replacement. Accepted transitions are
captured immediately. Countdown captures use its canonical 250 ms cadence.
The one-second durability boundary and scheduler deadlines share the same
checked calculation in the checkpoint owner.

Settlement guards against the old durable boundary before acknowledging a
receipt. A late success cannot erase a missed freeze or replace a newer attempt.
Repository failures preserve their typed source, freeze the actual run and
capture the newest paused state before propagating the error. Retry Save changes
only the write token for the original failed request. The newer paused state
must then receive its own committed receipt before explicit Resume is enabled.
Resume readiness is validated by the real run before the checkpoint gate opens;
an invalid Resume leaves the gate closed. Save success never calls Resume.

`PuzzlePersistenceRun::poll` extracts existing capture-time polling, preserving
expiry journaling without constructing a payload on every non-due tick.
`execute_transition` uses the same command dispatch and canonical run operations,
but does not append an unchanged readiness/pause operation. Existing `execute`
and replay retain their previous recording behavior, including old no-op
journals. No presentation copy of readiness or puzzle-transition rules is added.

## Projection and cost boundary

[DesktopPuzzleSnapshot](../../../src/desktop/puzzle_snapshot.rs) contains full
context, configuration, timer/status, current attempt projection when visible,
archive count and checkpoint/durable/write state. The visible projection carries
the existing native board snapshot, phase, primary outcome, assistance, practice,
hint and latest event. It contains no solution-tree, collection or archive clone.
An immutable `Arc` retains the projection keyed by full context and run status;
unchanged countdown ticks reuse it. A transition invalidates the projection.
Only timing and save metadata are refreshed on ordinary ticks.

Persistence captures continue to share the pre-encoded collection and unsaved
journal entries through the existing immutable representation. No
`PuzzleRun::snapshot` is called by this owner. There is no blocking I/O, runtime,
thread, detached task, sleep, unbounded queue or independent wall clock in the
production component. Runtime scheduling and the production storage adapter
remain injected. These are source properties; capture latency, peak memory,
native frame behavior and storage throughput remain unmeasured.

## Initial authored baseline

Sixteen focused owner fixtures use real validated domain objects, fixed seeds,
explicit clocks and bounded channels only at the actual storage port. They cover:

- Initial commit before countdown, fresh Ready and recovered explicit Resume.
- Exact delayed snapshot/input/completion freezes and newest paused durability.
- Cancelled waits retaining the same request, typed admission/commit failure,
  exact failed-payload retry and initial-save failure.
- Full session/attempt/definition/version/revision correlation, older receipt
  acknowledgement without replacing a newer attempt, and duplicate rejection.
- Focus/sleep/window-hide/manual signals, immutable projection reuse, illegal
  rejection with running time, hints, failed practice, listed replies, review,
  exhaustion, reshuffle, confirmed End and both expiry modes.

Four mirrored fixtures add checkpoint deadline/initial-release boundaries,
save-pause timer reconstruction and typed codec/no-op polling coverage. Tests
contain no wall-clock sleeps. Lifecycle fixtures exercise typed signals, not OS
notifications. Invalid-input, stale-input and storage-error outcomes remain typed.

The initial delivered patch, SHA-256
`bea376816ff23d0c30cfbb5d0fbb7a5cc591b09f496ee3a2cd8a7bc596733632`,
contained these 20 authored, unrun fixtures. Its handoff claimed no formatter,
Cargo, Clippy, test, build, diagnostic, native or red/green execution. The parent's
previous 388/975 results describe its earlier revision and do not validate this
patch. The following executed results supersede only that unrun status.

## Exact-deadline lifecycle correction and executed evidence

Independent review identified a P1 in `execute`: it polled before dispatch and
then checked the original context again. At a 100 ms per-puzzle deadline, polling
advanced attempt 1/revision 1 to attempt 2/revision 3, so FocusLost, Hidden and
confirmed End returned `StaleContext`. The next board remained visible or the
run remained open, bypassing the lifecycle arbitration already owned by the run.

Under the parent's exclusive Cargo lane, three deterministic owner regressions
were added before the production correction. Scoped Rust formatting and a
compile-only selection passed without any compile fixes. The unchanged lifecycle
implementation then produced all three semantic failures:

- `exact_deadline_focus_loss_pauses_and_hides_the_next_attempt`.
- `exact_deadline_hidden_readiness_pauses_and_hides_the_next_attempt`.
- `exact_deadline_confirmed_end_finalizes_without_advancing_to_an_open_attempt`.

The red filter selected four tests: the three new failures and one existing
game-premove test that passed. It exited 101 with 1 passed, 3 failed, 0 ignored
and 99 filtered out. Each new failure reported the old attempt 1/revision 1
against the prematurely advanced attempt 2/revision 3.

The owning fix keeps the durability guard and full-context validation first.
Actual commands then dispatch directly through the canonical persistence/run
operation, which arbitrates expiry as part of that operation. Only the explicit
Poll command pre-polls. The FocusLost and Hidden cases now retain interruption
across timeout advancement, hiding the next board until explicit Resume. Confirmed
End finalizes the timed-out current attempt and ends the run without opening the
next attempt. No domain rule or public API was replaced.

Every Cargo invocation used
`CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target`, with detected default
parallelism and no job/thread cap. Commands ran sequentially. Scoped formatting
and the final standalone formatter check both passed. The exact Cargo selections
and observed results were:

```text
cargo test --locked --lib exact_deadline_ --no-run
exit 0, compilation passed
cargo test --locked --lib exact_deadline_ -- --nocapture
exit 101, 1 passed, 3 failed, 0 ignored, 99 filtered out
cargo test --locked --lib desktop::puzzle_commands::tests:: -- --nocapture
exit 0, 19 passed, 0 failed, 0 ignored, 84 filtered out
cargo test --locked --lib puzzle::checkpoint::tests:: -- --nocapture
exit 0, 3 passed, 0 failed, 0 ignored, 100 filtered out
cargo test --locked --test domain puzzle_checkpoint:: -- --nocapture
exit 0, 16 passed, 0 failed, 0 ignored, 281 filtered out
cargo test --locked --test domain puzzle_timer:: -- --nocapture
exit 0, 19 passed, 0 failed, 0 ignored, 278 filtered out
cargo test --locked --test domain puzzle_persistence:: -- --nocapture
exit 0, 5 passed, 0 failed, 0 ignored, 292 filtered out
cargo clippy --locked --lib --test domain -- -D warnings
exit 0, no Clippy warnings or errors
```

The five green selections total 62 passing tests, including all 23 newly authored
fixtures: 19 owner cases and four mirrored cases. They are targeted checks,
not a broad test or native acceptance count. The domain test commands also
emitted the existing macOS linker warning:

```text
ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected
```

The toolchain states that `linker_messages` ignores `-D warnings`. No suppression
or linker configuration change was made. Successful test exits therefore do not
constitute a zero-warning build claim. The requested Clippy selection emitted no
warning. Raw compile/red/green/Clippy/format logs and the exact command manifest
are delivered under the repository root's absolute `.git/parallel-patches/`
`puzzle-command-owner-logs/` directory.

All Cargo sessions finished before the lane was explicitly released to the
parent. No broad build, native/GPU execution, dependency change, suppression,
commit, agent or extra worktree was introduced. Full parent gates remain pending.

T187/T190 remain open for the actual native 250 ms cadence, fresh-time completion
selection, focus/window/sleep integration, save-error controls and complete
end-to-end persistence acceptance. Native egui wiring, production collection
discovery, US13 resources and T189 history/deletion are outside this owned patch.
All other journeys and their acceptance obligations remain open with their
respective owners.
