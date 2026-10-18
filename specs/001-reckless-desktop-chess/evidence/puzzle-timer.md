# Puzzle countdown boundary

This patch supplies T184's deterministic countdown owner and the focused timer
portion of T177. It follows [FR-021](../spec.md) and the
[puzzle lifecycle decisions](../decisions.md#puzzle-interchange-and-lifecycle).
Task markers and integrated acceptance remain the parent's responsibility.

## Integration and API

`src/puzzle/timer.rs` consumes the real `PuzzleSessionConfig` and
`PuzzleEnumCountdownMode` provided by T182/T183. Their source-ready API was read
from `/Users/sadrashameli/.t3/worktrees/chess/t3-12dc45ac/src/puzzle/`.
No session, attempt, solution, random-provider or configuration source was copied
or changed. The configuration owns mode exclusivity, positive duration validation
and defaults; the timer contains no duplicate default or duration representation.

After integrating T182/T183, the parent must add `pub mod timer;` to
`src/puzzle/mod.rs`, and register `tests/puzzle/timer.rs` from `tests/domain.rs`
with `#[path = "puzzle/timer.rs"] mod puzzle_timer;`. Registrations are proposed
here to avoid competing edits to shared module roots. No placeholder session
modules, Cargo configuration or task-marker changes are included.

`PuzzleTimer::new(config)` arms the first puzzle without starting time.
`update_readiness(now, Ready)` asserts that its board is ready, visible and
interactive and starts its budget. `BoardNotReady` and `InputBlocked` cannot open
that gate. After it opens, those observations leave charging active, including
temporary blocking for listed replies or animations. `Hidden` applies a
`WindowHidden` interruption. Callers supply the truthful readiness observation.

`check_action(now)` returns `ActionAccepted` only while active and strictly before
expiry. Future lifecycle code must obtain that result before accepting a move,
hint, practice retry or other input. An exact-deadline action receives
`Expired(SessionEnded)` or `Expired(PuzzleTimedOut)`. The timer distinguishes these
outcomes without finalizing attempts, changing primary outcomes, advancing the
collection or displaying a summary.

`pause(now, reason)` supports manual, focus-loss, window-hidden and sleep
interruptions. Readiness changes never resume an interrupted timer. Explicit
`resume(now, readiness)` retains its budget; once the initial gate has passed it
may resume charging while input is blocked for a reply. An initially unready
puzzle still needs its full gate. Hidden Resume is rejected. Interruptions survive
review, exhaustion and arming another puzzle. An interruption at or after a
per-puzzle expiry also survives the next arm, preserving explicit Resume.

`enter_review(now)` and `exhaust_collection(now)` stop charging. Only explicit
`arm_next(now)` leaves review/exhaustion or a per-puzzle timeout; readiness then
opens the new puzzle's gate. Session mode carries remaining time forward.
Per-puzzle mode resets its budget only at this arm. Arming an active or already
armed puzzle is rejected, and whole-session expiry remains terminal. The
collection owner remains responsible for requiring the explicit Reshuffle and
Continue action before calling `arm_next` from exhaustion.

## Accounting and snapshots

Every operation receives the existing `ClockInstant`; all durations use
`ClockDuration`. `view(now)` observes precise remaining time, cumulative run
elapsed time, current-puzzle elapsed time, status and the active deadline without
mutating the owner. A deadline is present only while running. Charging is capped
at the remaining budget, so a late poll excludes time after expiry. Run elapsed
time survives every arm; current-puzzle elapsed time resets at the next arm.

Successful mutations retain a monotonic observation watermark. Backward times,
deadline overflow and cumulative elapsed overflow return typed errors. Every
fallible mutation operates on a candidate and commits only after all arithmetic
and deadline checks pass. Rejections preserve counters, phase, interruption and
watermark. Read-only observations and snapshot creation do not move that
watermark or consume a transition.

`snapshot(now)` captures charged balances and typed status. External snapshot
options must pass `PuzzleTimerSnapshot::from_persisted`; validation checks the
mode's balance equation, elapsed ordering, unstarted-puzzle accounting, positive
live remaining time and matching zero-balance expiry. Snapshot fields contain no
process-local monotonic timestamps. `recover(&snapshot)` preserves earned time
and recovers every live phase paused with `Recovery`. It preserves review and
exhaustion as resume destinations. Terminal session expiry stays terminal; a
recovered per-puzzle expiry requires Resume before the next puzzle can charge.
Downtime is excluded, and a new process may supply its own monotonic epoch.

## Verification and limits

The initial tests were authored before the timer implementation. The file now
contains 18 deterministic integration cases. They cover real exclusive defaults,
both countdown modes, initial readiness and visibility, continuing charge across
real session hints, illegal/unlisted moves, practice and listed replies, repeated
interruptions, review and exhausted-collection exclusions, guarded Next, exact
expiry races, late polls, timeout interruptions, backward/overflowing time,
validated snapshot round trips and paused recovery. The session integration uses
real puzzle definitions, boards, session configuration and the existing seeded
reply provider. It uses no engine, sleeps or wall clock.

Only the owned Rust files were formatted with `rustfmt --edition 2024`; the
installed compiler reports the pinned Rust 1.99.0. Their formatter check and the
staged patch whitespace check passed. Cargo was reserved by the parent and was
not invoked. Zero tests have been executed in this worktree; no observed red,
green, Clippy, build or full-gate result is claimed. Parent integration must
register the modules, merge the real session API, and run targeted checks
sequentially with `CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target`.

The implementation has fixed-size state and constant work per operation, with
no I/O, workers, hidden timers, scheduler or allocations in the timer itself.
No throughput, native responsiveness, power or runtime performance measurements
are claimed. The applicable architecture, structure, Rust style, code quality,
testing, performance and verification rules were read and reviewed.

T177's bounded crash-loss acceptance remains incomplete. Checkpoint scheduling,
250 ms cadence, one pending/coalesced write, the one-second durable-lag bound,
save failure/Retry Save, persistence, actual sleep/focus notifications, content
hiding and native UI are explicitly unclaimed. They belong to T178, T186, T187
and later integration. The timer offers deterministic pause and snapshot
primitives for those owners without implementing or claiming their policies.

## Parent verification

The real session/configuration dependency and module/test registrations are
integrated. Formatter and targeted library/domain/engine/presentation Clippy
passed. All 241 domain tests passed, including these 18 deterministic timer
cases; all 55 library tests also passed. Read-only review checked expiry
races, charging, monotonic arithmetic, recovery and practice preservation
without finding a concrete blocker. T184 is accepted as the countdown
owner. T177 remains open for bounded durable checkpoint recovery, and the
full T186 lifecycle integration remains open. No native timing, storage
or full-product acceptance is inferred. The existing linker warning remains.
