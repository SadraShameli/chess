# Native analysis prerequisite adapters

This implements the resource and selected-position prerequisites of T089 against
the T083 command ports. It does not accept T089 or compose the analysis UI.

The policy sources are the [constitution](../../../.specify/memory/constitution.md),
[canonical catalog](../../../.specify/rules/README.md),
[US2](../spec.md#user-story-2---use-the-complete-engine-for-analysis-priority-p1),
[capacity decisions](../decisions.md#capacity-and-memory),
[analysis priority](../decisions.md#report-scoring-and-job-scheduling), and
[capability contract](../contracts/reckless-capabilities.md).

## Resource ownership

`DesktopNativeAnalysisResources<M>` receives the same
`Arc<DesktopNativeGameResources<M>>` used by playing engines. It creates no CPU
budget, memory observer or hash allocator. The canonical owner retains the
startup-available-RAM policy and reuses its fresh async memory provider, pinned
NNUE bytes, process allowance and headroom for both kinds of engine child.

`initial(resources)` and `reserve(request)` return a CPU lease and
`DesktopNativeGameResourcesAnalysisReservation`. `admit` wraps those values for
`DesktopEngineCommands`. Initial composition must retain both values across
actual process startup/configuration and call the required async `acknowledge`
only after readiness confirms the requested configuration. Release the startup
CPU lease after that boundary; keep the reservation while the child remains
present, including idle time.

Retry and diagnostic requests must provide exactly the pinned process's actual
default hash. Missing, zero or different startup hints are refused. Configuration
and analysis requests must omit startup hints and have an acknowledged configured
owner. An analysis search must use that owner's exact configured resources.
Diagnostics reserve a separate child and hash peak. Replacement configuration
and Retry reserve the maximum of the retained owner's prior hash, new requested
hash and any startup default. Acknowledgment releases only the new token's
startup excess; the old token still charges its hash until its owner releases it.
An ended search releases its CPU/search token without removing the idle owner's
configured hash reservation.

Pending hash admission changes desired playing-engine shares and wakes canonical
`Notify` waiters. Existing playing-engine hash remains charged until its idle
configuration is acknowledged. Cancellation drops the pending guard, wakes the
same waiters and restores the desired shares. CPU acquisition uses the injected
budget's bounded `try_lease`; there is no blocking lock, lock retry loop or
suspension after successful CPU admission. Once the allocation is returned, the
actor must retain it through failures until the child is observed absent/reaped.
The resource adapter has no process handle and cannot establish that boundary.

Configured analysis hash is preserved when a new live game is requested. Playing
engines divide the remaining hash through their existing quotient/remainder
allocator; admission refuses an impossible startup/minimum rather than shrinking
the configured analysis value. Explicit analysis configuration may wait for
playing engines to acknowledge smaller shares at their idle boundaries.

## Live search priority

`live_demand()` returns a latest-value watch receiver of
`Option<GameEngineContext>`. Every canonical playing-engine `allocate` request
publishes `Some(context)` after marking that game searching and before waiting
for CPU/hash. `send_replace` wakes receivers even for repeated equal contexts.
Intermediate demands may coalesce. This is a demand event, not a current-active
state: the last context remains after the live search ends. Root should subscribe
before admitting lower-priority work and treat every unseen demand as a request
to stop active analysis using its original search receipt, or cancel an isolated
diagnostic. It must retain leases/reservations until completion/reaping.

While any live search is pending or running, new analysis/configuration/Retry/
diagnostic admission refuses. When live searches are absent, human-turn game
registrations retain admission obligations while analysis may borrow available
CPU workers. A new live request waits for the same shared lease; the notification
does not reclaim a lease or pretend the lower-priority child has stopped. Root's
notification-to-Stop/cancel wiring and native wake remain integration work.

## Authoritative selected position

`DesktopGameSnapshot::analysis()` is a typed projection captured directly from
`GameSession`. Eligible completed/analysis phases retain the selected node ID,
original root `BoardPosition` and the selected tree path's original `BoardMove`
values serialized as variant-correct UCI. Replaying that path must equal the
node's authoritative current position. Neither display SAN nor viewed FEN is a
history source. Setup, active and interrupted games expose `LivePlay`
unavailability. Oversized histories expose `OversizedHistory` while ordinary
board/clock/game snapshots remain available; other corrupt-history errors remain
typed capture failures.

`DesktopEngineSelection::new(snapshots, selected)` consumes the authoritative
game-snapshot and selected-game watch receivers. Each authorization reads their
latest values, refuses disconnected/missing/stale/live inputs and compares the
requested original position, full typed history and current position with the
selected node projection. `GameSession::select_view` does not increment the game
revision, so the history comparison also protects same-revision view changes.
The application must publish the new viewed-node snapshot before admitting its
analysis request. After awaiting admission, commands must authorize again before
starting a selection-sensitive effect.

## Checks and remaining integration

Twelve deterministic tests were authored in the owned test files before the
corresponding source implementation. Six cover resource admission, startup/idle
hash, exact process-memory boundaries, replacement acknowledgment/retired tokens,
forged hints, capacity/memory failures, live-demand preemption notification and
pending-admission cancellation. Six cover selected variation history, stale and
same-revision view changes, live-play refusal, disconnected sources,
custom/Chess960/DFRC roots and castling, and oversized engine histories.

Standalone rustfmt from the installed pinned Rust 1.99.0 distribution was used
only on the six owned Rust files. Scoped formatter and whitespace checks passed.
No Cargo command, compile, test execution, Clippy, optimized build, real engine
run, native UI run or performance measurement was executed in this lane under
the parent's explicit gate ownership. Red-first execution is unverified.

The parent owns source/test module declarations, the asynchronous admission and
acknowledgment port changes, command reauthorization after admission, initial
process composition, selected-game/view publication, notification-to-Stop/cancel
wiring, actor retained cleanup and all Cargo gates. The patch includes none of
the source-only T083 dependency snapshot. No task markers, commits, Cargo files,
shared test roots or UI/bootstrap composition are changed here.
