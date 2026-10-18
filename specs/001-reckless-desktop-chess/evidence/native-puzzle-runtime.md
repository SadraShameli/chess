# Retained native puzzle actor source increment

This increment belongs to the authorized implementation of T178, T187 and T190.
The first export was source-only work in the assigned isolated worktree. The
follow-up revision used the parent's explicitly granted exclusive Cargo lane.
All 13 approved user journeys remain intended scope. Full T187 and T190 acceptance stays open until
the parent combines repository, native host, coordinator and presentation wiring
and records the required native evidence.

## Dependency and ownership

The dependency `puzzle-command-owner.patch` was verified with SHA256
`ec95bd1b1b1e7573e403e38819c3aa98648ed7547a969ffb5721ffecde1e775b` and applied
locally before implementation. Its actual command, snapshot, persistence,
checkpoint, run and storage-port APIs were read. Its 11 paths are dependency
inputs and are excluded from this increment's staged export.

The assigning parent reported 19 command-owner tests, 43 selected lower tests
and Clippy passing for that dependency. The assigning parent also reported an
observed failing exact-deadline Pause/Hidden/End regression followed by its fix.
Those are supplied dependency results. They were not rerun or independently
established by this actor revision's targeted checks.

This export owns only:

- `src/desktop/puzzle_bridge.rs`
- `src/desktop/native/puzzle_runtime.rs`
- `src/desktop/native/lifecycle.rs`
- `tests/desktop/native/puzzle_runtime.rs`
- `tests/desktop/native/puzzle_lifecycle.rs`
- This evidence file.

No root module declarations, integration-test roots, manifests, settings,
bootstrap, coordinator, application messages or presentation files are included.

## Consumer boundary and composition

`DesktopPuzzleBridgeRequest` carries the actual coordinator-supplied
`DesktopApplicationReceipt`, the full original `PuzzleSessionContext` and a
closed `DesktopPuzzleBridgeEnumOperation`. Operations are Command, RetrySave,
Snapshot and Summary. User commands are not rebased. The application remains
responsible for its admission, deduplication, commit boundary and reconciliation.
The bridge neither constructs nor completes a private application receipt.

`DesktopPuzzleBridgeProvider` exposes session_id, submit, execute, snapshots and
health. Native Port exposes the immutable initialized run's session_id even
before the first snapshot. Root must bind a retained UI intent to this session
before forwarding a current-context interruption. An old session's UI intent
must not be redirected through a global current-port lookup to a replacement run.

Synchronous submit acquires bounded admission before returning a cloneable
`DesktopPuzzleBridgeReceipt`. Its wait method borrows a retained watch receiver;
cancelling a wait does not consume the outcome. Execute performs admission before
returning its boxed future, so dropping that future does not retract admitted
work. The actor owns accepted state and writes independently of response waiters.
Authoritative watch snapshots permit reconciliation if a caller discards every
copy of its bridge receipt.

Results are Snapshot with an `Arc<DesktopPuzzleSnapshot>`, or Summary with that
snapshot and `PuzzleSummary`. Summary is computed only for an explicit Summary
operation. Cadence snapshots do not scan archived attempts. The canonical
snapshot owner reuses its immutable attempt projection where its context permits.

`DesktopNativePuzzleRuntime::new(owner, clock, wake, scheduler)` accepts an already
validated `DesktopPuzzleCommands<R>`, the same `Arc<C: ClockTimeProvider>` and
initialization epoch used for that owner, an injected application wake and an
injected scheduler. It returns the retained actor and its Port. The production
scheduler receives the root's existing Tokio Handle and a paired ClockInstant
and Tokio Instant epoch. It translates owner deadlines without creating a runtime,
thread, worker pool, standalone clock origin or checkpoint cadence.

Root spawns run on its existing runtime and retains the actor returned alongside
the run result. `run(&mut self)`, `shutdown_retained(original_deadline)` and
`is_shutdown_complete()` preserve that ownership contract. SQLite must remain
open until the actor proves shutdown complete.

## Ordering, checkpoints and shutdown

The bounded ordinary queue has 256 slots. A separate two-slot user-control queue
handles Pause, Hidden readiness and confirmed End. Native interruptions have a
separate reserved slot, and shutdown has its own reserved slot with an atomic
reservation that remains owned during cleanup. Saturation returns typed Busy;
it allocates no unbounded waiting tasks.

The loop selects ready shutdown, native interruptions and urgent user commands
before a ready write completion or timer. It does not snapshot or Poll before
dispatching those commands. This preserves the dependency's exact-deadline
interruption behavior. The command owner still applies its canonical guard
before every accepted input. A native interruption chooses the owner's current
context inside the actor; the narrow retry handles a context revision created
by that same canonical guard.

The actor schedules `owner.next_deadline()`. It copies neither the checkpoint
cadence nor durable-lag arithmetic. Each timer observation samples the injected
clock. Write completion first awaits retained `owner.wait()`, then calls
`owner.settle(clock.now()?)` with freshly sampled time. Dropping the outer run
future leaves the storage handle or acknowledged receipt in the owner. Ordinary
pending writes do not independently block active play; the canonical owner
decides when lag, failure or explicit-resume gating requires a freeze.

The initial inactive write is established before readiness can start countdown.
There is no generated OpponentReply command, UI RNG or animation pause. The root
submits one typed OpponentReply after its acknowledgement boundary; actual reply
and animation waiting remain charged by the canonical timer.

Shutdown enters Closing, pauses and hides the run through the canonical owner,
captures that accepted transition, and drains the newest durable state. It checks
that the snapshot is paused or ended, hidden, free of retained or unsaved writes,
and has matching durable context and elapsed time before completion. Save failure
and deadline exhaustion return typed refusal and retain the owner and newest
state. RetrySave and read operations remain available after refusal; Resume is
refused while Closing. Save recovery never implicitly resumes.

The pending shutdown envelope and its original absolute deadline are retained on
the actor. Cancelling an outer cleanup future does not release its reservation or
extend that deadline. Re-entering the same attempt can shorten but cannot extend
it. A completed refusal releases the reservation before notifying its observer;
a subsequent explicitly admitted shutdown is a new attempt. Root can retry save
and request that new attempt while keeping SQLite and the profile owner alive.

The follow-up revision adopts an already admitted shutdown envelope from its
reserved queue before selecting a direct cleanup deadline. This includes an
envelope queued before the actor's first poll. A later direct cleanup deadline
cannot renew it. Any queued shutdown encountered during final notification is
also checked against its own original deadline before acknowledgment.

The actor owns a private Idle/Waiting/SettlementBlocked storage phase. It awaits
only a Waiting write and settles each observed completion once. A rejected old
or mismatched receipt remains retained and blocks further settlement attempts.
The actor pauses through the canonical command owner, retains the typed failure
in health, and continues servicing Snapshot, RetrySave and native interruptions.
An unsafe RetrySave against that retained receipt returns the owner's typed
refusal. Cleanup also refuses rather than spinning or dropping the receipt.

A repository admission refusal during snapshot capture is recovered narrowly:
the original typed storage failure is retained, and a second authoritative
snapshot publishes the owner's frozen state and SaveBlocked health. The current
request receives that refusal, while the actor remains available for explicit
RetrySave. Ordinary in-flight writes remain permitted; neither this phase
tracking nor the recovery path inspects or duplicates storage-driver internals.
No lower owner API or stable public bridge port changed in this revision.

## Lifecycle mapper

`DesktopNativeLifecycle` binds one typed puzzle-window identity. Its typed signal
mapper handles that window's focus, visibility, minimization and occlusion, plus
an injected suspend signal. Hiding the puzzle window pauses it even when another
app-owned window is focused. Events for other windows do not control the puzzle.
Focus, visibility and resume restoration produce no Resume command. The adapter
contains no game port or game mutation; live-game lifecycle remains parent-owned.

Port interrupt takes a typed pause reason and requires no application receipt.
Actual OS/window/power adapters remain parent integration work. An injected
Suspend signal is deterministic mapper coverage, not evidence of actual OS sleep
notification delivery. Winit suspension alone is not desktop sleep proof.

## Private checks and controlled fixtures

There are 20 actor tests and 2 lifecycle mapper tests in the refreshed export.
All 22 passed in the exclusive lane after the observed regressions were fixed.
The original export had 15 actor tests and 2 mapper tests, authored but unrun.
Actor fixtures use actual domain definitions, runs, command
owners, persistence replay and publicly admitted application receipts. The only
storage fake implements the existing PuzzleStorageRepository/PuzzleStorageWrite
port and returns a receipt for its exact controlled immutable request.

Tests manually poll the retained actor using a controlled scheduler and explicit
write completion, with the fixture's poll unconstrained by Tokio's cooperative
budget so a saturated finite queue can be inspected synchronously. Production
run keeps Tokio's normal cooperative budget. Virtual Tokio time is used for
deadline and production scheduler checks. No wall-clock sleeps are authored.
The old-receipt regression additionally limits native wake publications to eight
and observes health at that boundary. The original biased loop deterministically
exhausted this budget, so its semantic failure could not hang the test harness.

Authored coverage includes:

- Inactive initial save, denied early readiness and fresh readiness after commit.
- Owner-provided 250 ms deadlines, no early write, one in-flight write and newest
  countdown coalescing while play remains active.
- Stalled wake beyond one second, historical freeze and explicit Resume.
- A committed write observed late, with fresh settlement that preserves freeze.
- Exact-deadline Pause, Hidden and confirmed End before ready timer/storage work.
- Exact-deadline native Hidden and focus restoration without automatic Resume.
- Current-context native interruption after an accepted ordinary move.
- Cancelled receipt/run waits, dropped execute futures and actual application
  cancellation before acceptance.
- Successful urgent Pause, Hidden and native pause under ordinary saturation,
  plus independently admitted shutdown and typed refusal of queued ordinary work.
- Paused durable recovery, explicit reply commands, charged reply waiting and
  explicit summary retrieval.
- Save-failed shutdown retaining newest state, explicit RetrySave, no implicit
  Resume and eventual newest-state drain.
- Cancelled shutdown waits retaining the original absolute deadline, even when
  cleanup is re-entered with a later deadline.
- Production scheduler translation using the injected runtime and epoch pair.
- Per-window focus, visibility, occlusion and suspend mapping without any
  restoration-triggered Resume or game lifecycle dependency.
- Initial and snapshot-triggered repository admission refusal, authoritative
  hidden pause publication, typed SaveBlocked failure and explicit save recovery.
- Coalesced follow-up admission refusal preserving the newest paused context and
  elapsed state until an explicit retry drains it.
- Rejected duplicate/old committed receipt with blocked health, bounded progress,
  ordinary snapshot delivery, typed RetrySave refusal and native pause delivery.
- Queued original shutdown deadline before the first actor poll, cancelled direct
  cleanup, and refusal of both the later cleanup deadline and a late success reply.

Both test files are source-included under `#[cfg(test)]` by their corresponding
owned source module. This deliberately gives the narrow storage fake access to
the real crate-private `PuzzlePersistenceReceipt::committed` constructor. It does
not expose a new production constructor or require an integration-test root edit.
Parent must register the three new source modules. No extra registration of
these two test files belongs in `tests/desktop.rs` or `tests/native.rs`.
The targeted checks used local declarations in `src/desktop/mod.rs` and
`src/desktop/native/mod.rs`, including the dependency's existing source modules.
Those local declarations are excluded from the export and remain parent-owned.

## Observed red and green sequence

The three implementation files stayed at the delivered baseline while the
regression fixtures and local module declarations were prepared. That baseline
was the six-path patch with SHA256
`5385a3f2d4ba7004e1ba2327986558f873515c8e64d28cbd18edb29377171eba`.

The first compile found a fixture-only deprecation: Rust 1.99 renamed the atomic
fetch_update method to try_update. The fixture was corrected without a checker
suppression or implementation change. The next compile passed.

The frozen implementation then ran 20 actor tests: 16 passed and 4 failed in
0.01 seconds. Observed semantic failures were:

- Initial repository admission refusal stopped the actor with
  `Err(Commands(Storage(Repository(Admission))))`.
- Snapshot-triggered save refusal stopped the actor with that same typed error.
- The old-receipt probe returned `Err(Wake(Unavailable))`, observed
  `published_ready: true`, and had zero remaining publication budget. This
  established both failed progress and incorrect Ready health within a finite
  controlled run.
- Direct cleanup with a later deadline renewed the queued original shutdown.

The coalesced-follow-up refusal test already passed the frozen baseline. It
remains a passing boundary check; no failing result is claimed for that case.

Only after those semantic failures were observed were the runtime phase,
admission recovery and queued-deadline fixes implemented. The first follow-up
run passed 19 of 20 actor cases. The remaining queued-deadline case had passed
its semantic assertions but returned Empty during fixture cleanup because its
write was already acknowledged. The fixture drain now polls ready actor work
before expecting an unanswered storage request. No regression assertion was
removed or weakened. The next actor run passed all 20 cases in 0.02 seconds,
followed by both mapper cases passing in 0.00 seconds.

The exact scoped commands were run sequentially, using Cargo's default detected
parallelism without a job or test-thread cap. Every Cargo invocation set:

```sh
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib --no-run
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib desktop::native::puzzle_runtime::tests -- --nocapture
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib desktop::native::lifecycle::tests -- --nocapture
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo clippy --locked --all-features --lib -- -D warnings
```

The all-feature library Clippy check passed with warnings denied. It checks the
library target; it is not a claim of all-target, integration-test or native-harness
Clippy acceptance. No Rust source changed after these final passing checks.
The exclusive lane was explicitly released with zero active tool sessions.

Raw stdout/stderr logs are preserved in
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/native-puzzle-runtime-r2-logs/`:

- `01-compile-frozen.log`: `c4c79e10dee2815adc368d02b6c575f97f70b8dbbea38261ab294b484da364b9`
- `02-compile-frozen.log`: `78ee67f7760e64596f81406cdaa46ace17d6bcc0e145e7aa6b5b5d3c467e0793`
- `03-frozen-semantic-reds.log`: `d9ccc4b08a7e6d7cb768255f86bf57a6c3c500c81bbaf6de6264971044973c7d`
- `04-runtime-green-attempt.log`: `2bdebd2733f60fb15407ad6d34c0c2999f09094f0a340794c1bd6c2a82b06ee4`
- `05-runtime-green.log`: `ea352d1d1ac01ce1af4e5665738e6370b38ffafc9a12f59b654329c20aa10899`
- `06-lifecycle-green.log`: `b3088c0eaadf4dbbba66a916f3ee7eb2e4287780515df14b0e32d89ea2f386fe`
- `07-lib-clippy.log`: `0373baf83e6c4f7f63261ddb1b80c4d2cd479c047889d977142877fb878a44e8`

## Verification and remaining acceptance

Executed here: dependency SHA256 verification and clean application, instruction
and API source review, standalone pinned-toolchain rustfmt, formatting checks,
and staged diff/content checks. The installed Tokio 1.53.3 source was inspected
for Handle entry, deadline translation, timeout and watch/channel behavior.

The targeted compilation, tests and library Clippy above were executed only
after the explicit exclusive-lane grant. No broad release/native/GPU build,
CPU diagnostics, production renderer, native window/power delivery, OS
sleep/relaunch, full application quit or native acceptance was executed here.
No dependencies, suppressions, commits, pushes, delegated agents or additional
worktrees were created. The parent again owns execution and shared registrations.

Production collection discovery and Start factory wiring remain parent/US13
work. Run-history queries and deletion remain T189 work. The full native
presentation, storage/bootstrap shutdown ordering and real OS evidence remain
open; this increment does not mark T187, T190 or any complete user journey done.
