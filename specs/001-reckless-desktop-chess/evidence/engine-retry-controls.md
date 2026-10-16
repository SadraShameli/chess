# Engine Retry and bounded controls

Validated 2026-10-16 from pushed HEAD `50de3c4`, using Rust 1.99.0 and debug
builds. This closes the remaining backend acceptance of T076. Existing line,
pipe, diagnostic and shutdown acceptance is linked in
[engine diagnostics](engine-diagnostics.md); diagnostic retention was unchanged.

## Command ownership and saturation

The shipping route is `DesktopApplication` -> `DesktopGameBridge` ->
`DesktopGameRegistryPort::execute` -> `DesktopGameSupervisorPort` -> the owned
`DesktopNativeGameEngine` -> `EngineLiveGame` -> `EngineSession`. The session
serializes operations through exclusive mutable ownership; it has no internal
command mailbox. No unused queue was added.

The application admits at most 256 ordinary requests and one unapplied mutation
per entity. The engine-facing supervisor admits 32 queued ordinary requests,
one executing ordinary request, and six distinct reserved control intents. Each
reserved permit stays with its envelope through deferred processing and response
completion, so those states do not multiply the six-intent bound. At most 39
supervisor requests can be outstanding for one game, below the engine contract's
maximum of 256 pending control messages. `try_submit` returns typed `Busy`
synchronously before creating a waiting task. Engine output is separately
bounded by `EngineProcessLimits::PENDING_LINES`; that is not command capacity.

The new engine-mode acceptance saturates all 32 ordinary slots while a serialized
search waits for controlled output. Each of Stop, Pause and Quit still completes
through its reserved path; repeated held controls refuse with `Busy`. Every
admitted ordinary receipt is observed with its typed stale/closed outcome.
A separate saturated case delays handling by three injected clock seconds: the
original two-second safety deadline has already expired, so the actual session
returns `StopDeadline`, cleans up, and sends no late `stop` command. The supervisor
now converts the original application receipt to runtime time before passing it
through the native adapter's quiescence barrier, preserving upstream queue delay.
This establishes deterministic safety and ownership, not measured p95 latency.

## Explicit recovery

The acceptance uses the real `EngineSession`, `EngineLiveGame`, native factory
and adapter, supervisor, game domain and temporary SQLite storage. Its process
provider is an in-memory controlled transcript boundary. It is not real-process
fault injection, a native renderer test, or owner-profile access.

After two accepted moves, malformed search output produces the exact typed
`EngineSessionError::SearchResponse`, observes cleanup, and interrupts the game.
Board, move tree, view, result and paused clocks survive. CPU leases and hash
registration are released before replacement; no process appears automatically.
Stale Retry revisions refuse before spawning. Unrecovered Resume refuses with
the original typed recovery-required cause before invoking the factory.

Explicit revision-checked Retry starts one fresh validation process/session,
revalidates UCI/readiness and reapplies acknowledged Threads/Hash configuration.
It marks the domain state Recovered, then retires/reaps the idle validation child
before completing the successful response. While still paused there are zero
live children and zero hash registrations, all CPU capacity is returned, clocks
are unchanged, and no `go` is sent. Duplicate Retry refuses with `WrongRecovery`
without spawning. Only separate explicit Resume creates the playing process and
starts its search, as required by the
[paused-memory policy](../decisions.md#capacity-and-memory).

The supervisor acceptance observes separate old-failed, Retry-validation and
Resume processes. A direct native-factory acceptance records three distinct
session generations, taking the validation generation from its idle quiescence
receipt. That validation session never searches. The former and resumed searches
have distinct IDs even for identical game/revision context. A former-generation
stop refuses with `StaleSearch` without touching the resumed search. Output
queued on either retired process cannot reach it; its event carries the resumed
search identity. These generation observations come from the direct native
adapter test, not an exposed supervisor search identity.

Failed startup and cancellation during Retry retain the child, CPU lease and
hash registration while cleanup is blocked. The Retry response remains pending
until cleanup is observed. Afterwards the typed startup failure or cancelled
`Closed` result is returned; the interrupted state and clocks are unchanged,
resources return, no `go` appears, and a later explicit Retry succeeds. Existing
factory-retention and session cancellation/deadline tests continue to cover
cleanup errors and retrying retained shutdown. Shared temporary storage setup was
extracted only because both engine composition and desktop suites now use it.

## Red-first regressions

Before the production fixes, three exact filtered test runs each exited 101:

- `failed_search_retry_readies_one_replacement_and_requires_separate_resume`:
  Retry retired its newly ready engine, attempted another factory creation and
  exhausted the allowed process sequence.
- `resume_before_explicit_retry_never_spawns_a_replacement`: failed at
  `Resume created an engine before explicit Retry`.
- `saturated_search_controls_keep_receipt_deadlines_and_settle_every_admitted_request`:
  the expected typed `StopDeadline` was absent because handling reset the bound.

The fixes reuse domain-owned validation before factory side effects, preserve
only domain recovery state after reaping the validation process, and carry receipt
time through the existing stop barrier. No engine actor, configuration registry or
replacement implementation was introduced. Six new acceptance tests cover the
three regressions, failed/cancelled Retry, generation isolation and successful
reserved controls under saturation.

The paused-memory correction was also observed red-first:
`failed_search_retry_reaps_validation_process_before_explicit_resume_recreates_play`
failed with one paused live child where zero was required. Removing the
Recovered-engine retirement exception fixes that regression while preserving
the three earlier fixes and failed/cancelled Retry ownership checks.

## Validation

Before the paused-memory correction, the full targeted baseline passed: 52
library, 108 engine, 201 desktop, 5 domain pause and 30 presentation tests.
Unchanged saturation/library/domain/presentation checks were not repeated.

After the correction, sequential final checks:

- `cargo fmt --all`: passed.
- `cargo test --locked --test engine retry`: 8 passed, 100 filtered out,
  zero failures or ignored tests. This includes successful Retry/Resume,
  three-generation isolation, unrecovered Resume refusal, failed/cancelled Retry
  and retained cleanup cases.
- `cargo clippy --locked --lib --test engine --test desktop --test native -- -D warnings`:
  passed without warnings.
- `cargo test --locked --profile dev --test native -- --game-composition`:
  passed once after the final retirement change. Two real-engine mate games,
  two post-result rejections, durable restoration and frozen clocks passed.
  Both bootstraps recovered 14/14 CPU slots, acquired the full-worker lease,
  observed zero direct children and reacquired the exclusive profile.
- `cargo fmt --all --check`: passed.

The existing macOS `__eh_frame section too large (max 16MB)` linker warning
remains unsuppressed. No new warning was introduced.

## Scope

T076 is marked complete: 65/295 tasks completed, 230 pending. This is backend
acceptance. T077 remains next for complete eight-option configuration. T071, T091/T092, full UI/platform/package acceptance,
optimized performance and release validation remain assigned to their own tasks.
No extension hooks were registered (`.specify/extensions.yml` is absent).
The paused-memory correction used no Git commands.
No dependencies, release/all-features/all-target builds, staging, commits,
pushes, new branches, worktrees or agents were used.
