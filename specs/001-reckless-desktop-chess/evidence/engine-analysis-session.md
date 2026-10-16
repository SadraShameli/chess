# Correlated analysis session

Validated 2026-10-16 from pushed HEAD `f74c168`, with Rust 1.99.0 and debug
builds. T079's usable orchestration API lives in
`src/engine/search/session.rs`.

## Ownership and correlation

`EngineSearchSession` owns one existing `EngineSession`, with no second process
owner, actor, worker pool or detached task. A single bounded signal slot retains
a consumed readiness/diagnostic event while publishing a due visual snapshot. Start takes an explicit
caller-admitted `CapacityLease`, a validated authoritative position/history,
typed game/revision context and one tagged search limit. It refuses missing
resource configuration, thread/lease mismatch and terminal roots before I/O.
Hash/resource admission remains with the caller's existing resource controller.

Metadata binds a fresh search ID, session/process generation, game ID, revision,
starting position/full typed history, limit and monotonic acceptance/start instant
before setup I/O. `EngineSearchPosition` now has one shared immutable owner for
initial/current boards, typed moves and rendered command. The session and
orchestrator share it without copying full command/history or parsing metadata
back out of command text.

Expected tags are checked before consuming output or stopping. Replacement cannot
start while a previous lease/barrier or unobserved terminal result remains.
The old final bestmove is consumed before the next search; IDs and revisions are
fresh while a reusable process keeps its generation. Failed processes never
restart automatically. Cancellation retains the original `EngineSearchStop`
receipt/deadlines and distinguishes not-requested, stop and shutdown intent.

The lease stays charged through setup, active search, dropped start/stop waits,
ambiguous stop delivery and unobserved cleanup. Natural bestmove or observed stop
returns capacity while the ready process remains owned. Shutdown releases the
lease only after observed process/reader cleanup; failed cleanup retains both
owners for retry. The engine session retains a pending original failure across
cancelled cleanup waits, and orchestration failures are latched before cleanup.
Typed operation/cleanup causes and bounded diagnostics survive those paths.
The existing public engine stop still uses the same delivery/completion barrier;
the new orchestration also consumes its intermediate parsed analysis messages.

## Snapshot and terminal contract

The first visual publication is due after 50 ms; later publications are spaced
at least 50 ms from actual publication, with no catch-up burst. Every protocol
message is parsed. A bounded rank-ordered map retains complete score/PV messages
as immutable lines, limited by configured MultiPV and legal root moves. Optional
statistics stay attached to their original line. `latest_update` separately
exposes the latest structured message; partial/global statistics never fill an
older candidate's fields. Reported score/bound/perspective and White conversion
remain the existing `EngineAnalysis` values.

Continuously ready input checks publication time after each analysis message and
yields between bounded 64-message batches, preventing timer/task starvation.
Readiness and diagnostics remain explicitly tagged events. Bestmove, stop and
failure are terminal events, independent of the visual cadence. A terminal
snapshot carries final analysis directly; it does not synthesize an extra
throttled telemetry event. Terminal results remain inspectable, and consumers
can inspect retained allocation/process cleanup before retrying shutdown.
Dropping an await retains its owner; callers must explicitly observe cleanup
before disposing of that owner.

## Acceptance

The initial contract run preceded implementation and exited 101 because the
orchestration API and shared typed history did not exist. Fifteen deterministic
cases now cover shared metadata, pre-I/O refusal, rank ordering, independent
optional statistics, raw/White Black-perspective scores, readiness distinction,
512/4,096-message bursts, 20 publications in one injected second, non-starvation,
immediate terminal delivery, empty replacement candidates, stale revision and
foreign-generation controls, old stop barriers, original deadlines, cancelled
setup/delivery/waits, retained cleanup and original failure preservation.
Two final regressions were observed failing before correction: a readiness flood
starved a due snapshot, and a completed stop imposed its expired deadline on a
later idle shutdown. The former now retains one signal while publishing; the
latter applies the stop deadline only while that allocation is still owned.
All fixtures use controlled transcript boundaries and paused Tokio time.

- `cargo fmt --all`: passed.
- `cargo clippy --locked --lib --test engine --test desktop --test native -- -D warnings`:
  passed without warnings.
- `cargo test --locked --lib --test engine --test desktop`: 52 library,
  133 engine and 201 desktop tests passed; zero failures or ignored tests.

- `cargo test --locked --profile dev --test native -- --engine-analysis-session resources/engine`:
  passed against the actual packaged engine after final source edits. Two ranked
  candidates, finite natural completion, same-process replacement, stale-stop
  refusal, infinite-analysis stop/bestmove barrier, full CPU lease recovery and
  observed process/reader cleanup passed. An earlier smoke also passed before
  the two final edge-case regressions; this records the final rerun.
- `cargo test --locked --profile dev --test native -- --game-composition`:
  passed once after final source edits. Both real-engine mate games, two
  post-result rejections, durable restoration, frozen clocks and completed-tab
  exclusion passed. Both bootstraps recovered 14/14 CPU slots, acquired a full
  worker lease, observed zero direct children and reacquired the exclusive profile.
- `cargo fmt --all --check`: passed.

The existing macOS `__eh_frame section too large (max 16MB)` linker warning remains
unsuppressed. No optimized throughput or latency measurement is claimed.

## Scope

Backend orchestration is ready for acceptance. The parent owns task-marker and
shared registration integration; no task markers were changed by this lane.
Completing T079 advances the recorded total to 67/295 with 228 pending. Desktop analysis handlers/UI remain
T083/T084; positive tablebases, full capability coverage, p95/1,000-cycle and
platform/package acceptance remain their own tasks. Live-game orchestration and
the paused-game hash/Retry policy are preserved. The unchanged T077 native
8-option acceptance was not rerun.
No dependencies, owner-profile writes, release/all-feature/all-target builds,
new branches/worktrees/agents, staging, commits or pushes were used.

No extension hooks were registered (`.specify/extensions.yml` is absent).

## Files

Shipping changes: `src/engine/search/{session,position,stop,mod}.rs` and
`src/engine/{session,analysis}.rs`.
Tests: `tests/engine/{search_session,lifecycle}.rs`, `tests/engine.rs`,
`tests/native/engine_analysis_session.rs` and `tests/native.rs`.
Evidence: `specs/001-reckless-desktop-chess/evidence/engine-analysis-session.md`.
Shared module/test registrations include only this lane's T079 additions; the
parent merges registrations from the other isolated lanes.
