# Report FIFO scheduler core

Source-only T164 handoff against commit
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68` plus the revised report foundation:

- Dependency: `/tmp/claude-report-model-scoring-20261017/revision-2/report-model-scoring.patch`.
- Dependency SHA-256: `579e8f540df69d484ab57387811bffe06fb49536fb9a2f98c5c641a0f52f0a21`.
- Export: `/tmp/claude-report-fifo-scheduler-20261017/report-fifo-scheduler.patch`.
- Exact export and file hashes: `manifest.txt` and `SHA256SUMS` in that directory.

No task checkbox changes or runtime, US7, native or persistence acceptance are
claimed. The constitution, catalog and all seven complete
[engineering rules](../../../.specify/rules/README.md) were read, together with
[FR-018 and US7](../spec.md), the
[selected priority policy](../decisions.md#report-scoring-and-job-scheduling),
[study contract](../contracts/study-tools.md#full-game-analysis-reports) and T164.

## Owned changes and dependency compatibility

Only these new paths belong to the incremental export:

- `src/report/scheduler.rs`: bounded FIFO owner, actual job transitions,
  admission, observations and completed-job extraction.
- `src/report/scheduler/active.rs`: one retained attempt and engine binding,
  deferred interruption during dispatch, and actual terminal settlement.
- `src/report/scheduler/dispatch.rs`: one non-cloneable dispatch receipt owning
  the actual lease-bearing search request.
- `src/report/scheduler/policy.rs`: typed root-injected demand and decisions.
- `tests/report/scheduler.rs`: 22 authored contract tests using the existing
  report fixture by `include!`, real jobs and real engine/search session owners.
- This evidence file.

The dependency was SHA-verified and applied unchanged. Shared roots, existing
job/source/configuration/scoring files and the shared fixture are unchanged.
The parent's current `/Users/sadrashameli/Personal/chess/src/report` was inspected
read-only, including its repaired `ReportFindingPositionResult::primary` getter
and the `ReportJob::record_terminal` use of retained primary evaluation. The
parent's `EngineSearchSession` primary retention repair was also inspected
read-only. The scheduler uses public job methods and contains no score retention,
classification or alternative selection implementation. Neither repair is copied
into this patch. The parent's canonical full-FEN search-position repair remains
the foundation's prerequisite.

## Scheduling and ownership

Queue length is an injected `NonZeroUsize`, with no product default. Completed,
cancelled and outdated entries continue to count until explicitly extracted.
Rejected enqueue returns the original job with its typed reason. Duplicate job
identities cannot replace retained work.

The first selected eligible entry becomes the FIFO owner and keeps its position
across priority pauses. Only one attempt can be dispatching or bound. Every
running live game blocks reports, even with no current live search or while a
human is moving. Live search demand also blocks admission. Selected interactive
analysis preempts reports when no live games run. Workbench activity blocks
report admission, and `workbench_admission` returns a typed refusal for conflicting
demand or selected report work. A refusal changes no running report.

`prepare` consumes the root's actual `CapacityLease`, verifies the configured
thread allocation and engine configuration, and asks the actual job for its
first incomplete position. Its receipt transfers the same lease into the
existing `EngineSearchSession` on start. No semaphore, CPU pool, hardware
discovery, default CPU cap, thread, channel, runtime or subprocess is constructed
by production scheduler code. All scheduler transitions are synchronous. The
receipt's explicit async `start` delegates once to the supplied existing search
session; its I/O is that session's existing boundary.

Priority changes never discard a bound search. A `Stop(tag)` decision continues
even if demand clears while stop is pending. Only successful public
`ReportJob::record_terminal` consumption clears a binding. Stale terminals and
repeated binding attempts retain the current owner. Completed positions remain
on the actual job, and the next attempt obtains the entire recorded finite
budget from `ReportJobAttempt::limit`, with no elapsed-time subtraction.

New queued jobs are eligible. Jobs supplied already paused, including pure
relaunch recovery, require `select(id)` before admission. Cancelled and outdated
jobs cannot be selected. Unexpected engine stops, cancellation and failures
require explicit selection; priority interruptions can resume automatically
after their terminal and demand clearance. Engine failure causes remain on the
job through its canonical interruption API.

## Required consuming handshake

1. In the root's serialized owner, feed current demand and obtain the full report
   allocation from the existing shared capacity owner. Call `prepare` with the
   already configured idle search session and that actual lease. Retain the
   returned dispatch receipt and the same session until binding or settlement.
2. Call the receipt's `start` on that session. The request is private, cannot be
   cloned or extracted, and is consumed at most once. A cancelled start future
   leaves the receipt and the actual session available for retained cleanup.
3. After successful start, call `bind` before reading events or sending stop.
   Bind delegates engine, options, context, history, budget and tag validation to
   `ReportJob::bind_engine`. Pauses/cancellation received during dispatch are
   applied after binding. A changed source is retained in one bounded source
   slot until bind or settlement, then passed to `ReportJob::observe_source`.
   Later observations cannot undo the first invalidation. Root presentation must
   not publish a current-source claim while this dispatch observation is pending.
4. For `Stop(tag)`, use the root's actual request receipt/deadline with
   `EngineSearchStop` and retain the same search session through stop and cleanup.
   Dropping a stop future is not a completion acknowledgment. Pass only an actual
   search terminal to `record_terminal`. If recording fails, retain the binding
   and surface the error; do not request another report.
5. If start fails, is cancelled, or finishes before binding, retain its receipt
   and session. `settle_dispatch` withdraws any unconsumed request. It accepts
   only the original engine generation, no outstanding session allocation, and
   either the original observed idle session when no search started or a matching
   actual terminal after the session is idle or shutdown is observed. It refuses
   an active search, a foreign session, an old receipt or a still-bound attempt.
   A returned terminal preserves the actual unbound dispatch failure/cancellation
   cause for the root to retain and surface. No completed result is fabricated.
6. A known terminal game position can use `record_outcome` with the unstarted
   receipt and actual `GameOutcome`. Canonical node/result validation stays in
   `ReportJob`; success withdraws the request and returns its lease without
   starting an impossible engine search. It refuses started or interrupted work.

Dropping the scheduler, a dispatch receipt or an engine session is not a shutdown
protocol. The root must retain these owners through all observed cleanup. A
terminal failure can still require engine/process cleanup owned by the existing
session even after the report job has consumed that terminal.

## Authored checks and executed verification

The first test artifact was written and staged before production files existed.
Additional cases were authored before adding the retained receipt and terminal
outcome APIs. `tests-authored-first.patch` preserves that initial source snapshot;
`01-tests.patch` contains the final tests for this implementation. Neither is an
observed semantic red.

The 22 cases cover literal enqueue order, one-worker shared lease exclusion,
queue backpressure and rejected ownership, live-human-turn gating, interactive
preemption, completed-position retention, full 3000-ms retry budget, cancelled
stop waits, cancellation, stale terminals, irreversible source invalidation,
metadata-only edits, foreign games, explicit relaunch selection, Workbench
refusal, mismatched configuration, dispatch observations, unstarted withdrawal,
foreign sessions, an injected four-worker allocation, cancelled engine setup,
actual engine failure and canonical checkmate completion. The fixture introduces
only an in-memory process-boundary decorator that suspends the `go` write for
controlled cancellation. It does not replace job or session behavior. Time is
paused explicitly; cancellation uses a single controlled future poll without
wall-clock sleeps or timing assertions.

Executed: pinned standalone rustfmt formatting and `--check`, scoped source and
ownership inspection, whitespace checks, dependency reverse-application check,
incremental patch forward application against the dependency in a scratch Git
index, reverse-application check, parent working-tree applicability check and
SHA-256 export verification.

Executed tests: **0**. No Cargo, compiler, test binary, build, Clippy, runtime,
native test, benchmark or CPU diagnostic was run. Formatting and patch
applicability do not establish compilation, behavior, lint cleanliness, process
cleanup, utilization or acceptance. The parent must register and execute the
checks before treating these assertions as evidence of behavior.

## Parent integration obligations

- Register `pub mod scheduler;` in `src/report/mod.rs` and include
  `tests/report/scheduler.rs` from `tests/domain.rs`. Existing foundation root
  registration remains parent-owned. Compile, lint and run the focused contracts
  in the parent's authorized verification lane.
- Supply authoritative demand from every actual running game, including local
  human turns, across start/pause/resume/finish/window/recovery transitions. Do
  not derive this count from active engine searches. Correlate interactive and
  live requests with the owning application revisions and retained sessions.
- Serialize report preparation, engine start/bind, source observations and
  terminal delivery with the root's bounded actor ownership. Keep stop/cleanup
  deadlines tied to actual request receipts and surface delivery/engine failures.
- Allocate all CPU capacity available to this report from the single canonical
  capacity owner, matching its recorded engine configuration. Retain framework
  reservations and phase admission; wait for incompatible leases to finish.
  The one- and four-worker fixtures are injected test budgets, not product caps.
- Before explicit Workbench start, use the refusal check, atomically publish its
  active demand, and obtain the exclusive actual shared lease through the
  existing Workbench job/executor. Retain it through observed cleanup. Interactive
  analysis uses only spare capacity with live games and stops for live demand.
  Indexing uses that same capacity owner in bounded spare chunks, yielding at
  chunk boundaries before report admission; this module supplies no indexing
  executor or independent pool. Puzzles create no engine searches.
- Implement T163 completion/explicit-request triggers and immutable-line
  observations, with no enqueue on reopen/import/metadata-only edits. Supply real
  terminal `GameOutcome` values and preserve replacement preview confirmation.
- Implement T162 durable job/results, FIFO order and admission selection, pause,
  cancellation, outdated state, unbound-dispatch failure records and recovery.
  Restore unfinished jobs paused, retain completed results, and explicitly select
  jobs for scheduler admission. Admit aggregate queue, retained observation,
  receipt, engine and adapter memory at the root; individual job/source charges
  and queue length bounds are not an RSS or global memory guarantee.
- Supply the native service/commands/presentation/persistence integration and
  measure live-game responsiveness, stop latency, CPU utilization, cross-platform
  cleanup and full US7 acceptance separately. No runtime live-game feed, service,
  durable FIFO, native lifecycle or performance acceptance is implemented here.
