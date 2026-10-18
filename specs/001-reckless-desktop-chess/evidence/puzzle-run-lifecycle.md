# Puzzle run lifecycle composition

## Status and scope

T186 pure-domain implementation handoff, based on verified puzzle core
`fb0e873`. T185 selection APIs were read from their writer worktree and
rechecked against the parent's integrated source after `51eb69e`.
This document records authored behavior and static checks. It does not establish
passed compilation, tests, native acceptance, or completion of T186.

Owned paths are [run composition](../../../src/puzzle/run.rs),
[minimal session APIs](../../../src/puzzle/session.rs),
[lifecycle fixtures](../../../tests/puzzle/run.rs), and this evidence file.
Module registration, test-root registration and task markers remain parent-owned.
No dependencies, commits, pushes, additional worktrees or subagents were created
by this lane.

The implementation follows [story 8 and FR-021](../spec.md),
[the puzzle contract](../contracts/study-tools.md#puzzles),
[the lifecycle decision](../decisions.md#puzzle-interchange-and-lifecycle),
[the data model](../data-model.md#reports-openings-and-puzzles), and
[the canonical engineering rules](../../../.specify/rules/README.md).

## Implemented boundary

`PuzzleRun<R>` owns the actual `PuzzleSelection<R>`, `PuzzleSession`, and
`PuzzleTimer`. The session remains the sole owner of current solution traversal,
attempt events, assistance, primary outcome and revision. The timer remains the
sole active-clock and pause/review/readiness owner. Run status is derived from
the timer plus the explicit end reason. No engine, storage, UI, native lifecycle,
wall clock or operating-system dependency enters this composition.

- Every move, hint, practice retry and opponent reply validates its complete
  session/attempt/definition/revision context and calls `PuzzleTimer::check_action`
  before attempt mutation or randomness. Deadline expiry returns an explicit
  `Expired` action result instead of applying the requested action.
- Manual, focus, hidden-window and sleep pauses expose `Hidden` current content.
  Readiness acknowledgement and regained visibility never substitute for Resume.
  Active Resume requires a ready, visible, interactive board. Review stays
  visible until explicitly paused; it waits for Next without charging time.
- Next and Skip arm the selected next board without starting its countdown.
  Skips reveal no skipped solution. Per-puzzle timeout advances once; session
  timeout ends once. Original failed primary outcomes survive skip, timeout and
  later practice completion, with secondary events retained by the real attempt.
- Exhaustion stops the timer and requires explicit Reshuffle and Continue.
  Session balance is carried forward. A timeout on the final item also reaches
  resumable exhaustion, including when expiry and focus loss coincide.
- End Run requires the explicit confirmation value. It finalizes an unfinished
  attempt through the existing skip operation, preserves an earlier failure,
  and records the separate confirmed-end reason in the run event stream.
- Replaced attempts are moved into `previous_attempts`, with exact elapsed active
  time. The current attempt stays in the session and contributes exactly once to
  `PuzzleSummary`. Finalized elapsed metadata preserves the final item's time
  after the timer arms an exhausted collection. This metadata is a frozen result,
  not a second running timer. No rating or points are introduced.

Timed candidates copy only the timer and current session, never the frozen
collection, shuffle order or accumulated random transcript. Listed replies use
T185's `PuzzleSelectionRandom::transaction`: it clones the injected provider and
retains the transcript length, then restores the provider/fault and truncates
new draws on failure. Reply and reshuffle methods require `R: Clone`; ordinary
move/hint/lifecycle methods do not. Advancement preflights revision/attempt
counters and timer transitions before consuming the current selection.

`view` and `summary` are read-only projections. The owner calls `poll` to commit
clock-driven expiry; timed commands also perform that check. `snapshot` polls
before capturing the complete typed in-memory state. `recover` replays the
injected provider through the validated selection snapshot, verifies the current
selected definition against the saved attempt, invalidates the saved revision,
and delegates paused clock restoration to `PuzzleTimer::recover`. Current,
review and exhausted runs require explicit Resume after recovery.

## Verification performed

- The initial lifecycle fixtures were authored before `run.rs` implementation.
  No red-first execution occurred because the parent explicitly reserved Cargo.
- Authored 18 deterministic public-contract tests in `tests/puzzle/run.rs` and
  two private numeric-boundary tests in `src/puzzle/run.rs`. All 20 are unrun
  in this lane. Inputs use real board/definition/session/timer/selection objects,
  explicit nanosecond instants and in-memory random providers, with no engine,
  network, storage writes or sleeps.
- The fixture inventory covers readiness, all interruption reasons, explicit
  Resume, review/Next, active reply/hint/retry time, illegal and foreign input,
  stale revisions, primary-failure preservation, both expiry policies,
  pause/expiry races, exhaustion/reshuffle, confirmed End Run, invalid RNG
  rollback, recovery with archived attempts, replaced-definition rejection,
  and revision/attempt-counter overflow rollback.
- Formatted only the three owned Rust files using
  `/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt --edition 2024`.
  The associated compiler reports `rustc 1.99.0`, matching
  [the toolchain pin](../../../rust-toolchain.toml). Formatter check and
  `git diff --check` passed. Static searches found no authored code comments,
  panic/unwrap/expect placeholders, or prohibited prose punctuation.

## Parent integration and remaining work

The parent must register `puzzle::run` and include `tests/puzzle/run.rs` through
`tests/domain.rs`, then run the consolidated compiler, Clippy and deterministic
suites. The parent already owns the compatible clonable seeded-provider
implementation. This lane makes no Cargo, test-count-pass, full-gate, runtime
performance or product-acceptance claim.

Snapshots currently copy the complete collection/order/random transcript and
archived attempts. They are explicit recovery snapshots, not lightweight durable
checkpoints. T187 still owns serialization and validated persisted reconstruction,
transactional storage, cadence/coalescing, bounded durable lag, Save failure,
Retry Save and storage interruption. It must retain the random seed plus the
parent's named algorithm/version alongside the validated draw transcript and
measure snapshot/cadence cost with realistic data. This lane does not claim a
250 ms checkpoint schedule or one-second durable-lag guarantee.

T190 through T192 still own application commands, cancellation of obsolete work,
native focus/window/sleep events, presentation readiness, actual hidden content,
confirmation dialogs, rendering, persisted-run resume and history interfaces.
History deletion and summaries after deletion remain their existing owners.
No native or disk recovery was exercised here.

## Parent verification

Integrated the real selection/seeded provider and registered the component.
Read-only review found confirmed End at a per-puzzle deadline incorrectly
advanced the collection and left the run open. The new two-puzzle regression
was observed failing with AwaitingReadiness instead of Ended(Confirmed).
The fix records expiry on the current attempt, preserves the timeout primary
outcome and exact elapsed time, then records confirmed End without advancing
or skipping another attempt. Whole-session expiry retains its own end reason.

Formatter and Clippy for the library and every default test target passed.
All 61 library, 289 domain, 213 desktop and 87 presentation tests passed in
the latest affected-component run. This includes the 19 public run cases and
two private arithmetic cases. T186 is accepted for this pure run owner.
The initial library run reported Busy in an existing native fixture event
read; isolated and full selected reruns passed. Its bounded Busy retry belongs
to the ongoing native composition handoff. Native product and durable-storage
acceptance remain open. The existing macOS linker warning remains unsuppressed.
