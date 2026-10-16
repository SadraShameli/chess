# Native engine handlers: T083 source handoff

Source is staged in the isolated handler worktree. No Cargo command, commit,
push, dependency change, presentation edit, executor edit, release operation or
additional worktree/agent was used. Parent owns integration and verification.

## Implemented surface

`DesktopEnumCommand::Engine(DesktopEngineCommandsOperation)` dispatches through
`DesktopCoordinator::with_engine` and the existing application admission and
receipt cache. Engine Stop and workbench Cancel have separate reserved control
keys. Snapshot/result queries are ordinary reads. Mutations check the envelope's
configuration revision. Directory handles are consumed by their actual owner,
purpose and current expiry before leaving the coordinator.

`DesktopEngineCommands` uses the actual `EngineSearchSession`. Start validates the
authoritative selection and complete initial position/history through the
injected selection port. Configure validates the setting against the discovered
capabilities before resource admission, refuses an active search/diagnostic and
rejects direct variant/path changes. Clear Hash uses the validated button setting.
Explicit Retry requires a failed session, reaps retained startup/session owners,
restores acknowledged options on a separately admitted replacement and publishes
a fresh generation without starting a search or resuming a game/clock.

`DesktopEngineService` is an owned actor with bounded ordinary/urgent queues and
a reserved lifecycle channel. It creates no runtime or CPU pool and spawns no
per-request waiter. Analysis telemetry uses a latest-value watch channel. Terminal
analysis and diagnostic results use a bounded ordered channel with retained
pending delivery; saturated terminal delivery still admits urgent controls.
The concrete bridge port exposes `shutdown(deadline)` for lifecycle composition.
Cleanup errors and undelivered terminal results keep their owners available.

The first application receipt now retains both application-clock and Tokio-clock
instants. Engine stop and diagnostic cancellation use that original runtime
receipt, including queue delay. The separate cancellation handle contains only
the shared atomic flag, so filesystem jobs do not capture receipt/result graphs.

Tablebase preparation and alias cleanup run on the injected `CapacityExecutor`.
Their futures remain in their owner across interrupted awaits. A replacement
selection retains old aliases until the actual engine has acknowledged the new
path. Loaded/Unavailable come from `EngineTablebaseStatus::record_configuration`,
not readiness alone. Failed alias cleanup remains owned for an explicit retry.

`DesktopWorkbenchCommands` consumes the actual `WorkbenchExecutor`. Its result
holds the reaped executor and exposes the original `WorkbenchJob`, canonical
final diagnostics and `WorkbenchExecutorObservation`, without a second result
schema. `export_context()` uses the actual isolated observation, or the explicit
unobserved context when setup never reached that boundary. The required
`DesktopWorkbenchCommandsObservationProvider` receives borrowed jobs, canonical
active diagnostics and execution observations for initial, sampled active and
terminal UI projection. Parent's export/result modules remain untouched.

## Narrow shared changes

- `EngineSearchSession::idle_engine()` grants mutable engine access only when
  no CPU lease or unobserved terminal remains and the engine is Idle.
- `EngineSearchPosition` and its validated immutable state derive Eq/PartialEq.
- `WorkbenchEnumParameters` and `WorkbenchPerftParameters` derive Clone/Eq for
  semantic native command payload comparison. No raw-data payload shim was added.
- `DesktopFailure` implements Display/Error for typed service/test propagation.
- Existing game control completion now shares a typed control-family completion
  implementation with engine controls; live-game commands retain their behavior.

## Verification status

Standalone pinned rustfmt completed for all owned Rust files. `git diff --check`
passed. Eight deterministic tests were authored under `tests/desktop/` and
registered in `tests/engine.rs`: native start/stop/history/stale selection; idle
configuration/capacity/revision; real actor dispatch with a full ordinary queue;
selected-directory scope/load/retained cleanup; cancelled directory preparation;
isolated diagnostic result/provenance; cancel/cleanup resource ownership; and
explicit failed-generation Retry without Resume. Tests have not been compiled
or run because Cargo remains reserved by the parent. No red-run claim is made.

The patch is based on 8300a5e. It consumes the parent's T082 executor and the
confirmed observation/export APIs now integrated over 81e1566. Those dependency
files are deliberately excluded from the patch. Cargo verification must happen
in the parent's consolidated source tree, using its reserved shared target.

## Exact T089 composition boundary

1. Configure the initial dedicated analysis session through existing startup,
   capacity and hash admission, then pass its retained memory reservation into
   `DesktopEngineCommandsServices`. Implement `DesktopEngineCommandsAdmissionProvider`
   against the same canonical CPU/hash owners used by live games. Requests expose
   workload, desired resources and transient startup default-hash requirements.
   No independent budget or success-returning default adapter is provided.
2. Implement `DesktopEngineCommandsSelectionProvider` from authoritative viewed
   game/revision/history and workspace state. Its required `authorize_context`
   and `authorize` methods must refuse live-play/stale selections. This service
   has no mutable game/clock/registry reference and never invokes Resume.
3. Construct `DesktopEngineService::new(commands, clock, tablebase_policy,
   workbench_observer)` at the native root. Retain its owner on run failure and
   cleanup failure. Install a concrete port clone through
   `DesktopCoordinator::with_engine(Arc::new(port.clone()))`; keep a concrete clone
   for lifecycle `port.shutdown(deadline)`. Continue consuming terminal delivery
   during shutdown and retry cleanup while ownership is incomplete.
4. Mount `DesktopEngineBridgeStreams`: map actual metadata/terminal results into
   the T084 controller, use its domain snapshots for telemetry, and consume ordered
   terminal results with request/context correlation and native wake notifications.
   The workbench observer projects borrowed jobs into T085's existing snapshots;
   its separate diagnostics reference is the canonical active tail. Final jobs
   already own that tail. Supply bounded publication/wake behavior in this adapter.
5. Preserve the controller action's request identity when creating/submitting the
   native request. Translate its generation/configuration/selection context and
   validated options/parameters; do not mint new requests during repeated frames.
   Directory selection uses the existing native FileSelect Directory command,
   followed by PrepareTablebases with that opaque handle and LoadTablebases.
6. Add the two required exhaustive match arms for the new Engine command/value
   variants in `src/presentation/app.rs` while mounting those controllers. They
   are intentionally untouched by this handler lane. As staged, the isolated
   old-base checkout is not a compilable standalone final application; dependency
   integration and these T089-owned presentation arms precede Cargo verification.
7. Use `DesktopWorkbenchCommandsResult::export_context()` with the parent's
   canonical export/result preview APIs. Destination staging/commit belongs to
   the export composition lane and is outside this patch.

## Retained actor correction after parent integration

The original inline dispatch serialized urgent polling behind pending admission,
setup and configuration. `engine-actor-retained.patch` replaces that dispatch
with one retained, owned operation future. The future owns the actual commands
object; request identity/receipt/response and an interruption sender remain in
the actor. Dropping `run()` preserves that future. A matching control or lifecycle
request interrupts its await and returns the lower owner, whose explicit fields
retain child, lease, transient reservation and filesystem operation ownership.
Interrupted original responses stay pending until the corresponding search/job
or complete shutdown has actually been reconciled. Async admission and its
acknowledgment are covered by the same ownership boundary; their public signatures
are supplied by the parent's commands/API lane.

Urgent controls and the separate lifecycle receiver are polled while an ordinary
operation is pending. No per-request task or waiter is spawned. A refused second
Workbench Start does not clear the first job's running state. Pending diagnostic
job identity is deterministically bound to the existing native request UUID, so
a matching Cancel can address admission/setup before Start finishes. Cancelling
before admission completion drops only that operation's owned reservation future;
the original request is settled after the owner confirms no job was created.

Abandoned execute/shutdown responses are retained as bounded typed outcomes and
never abort dispatch or queue draining. Shutdown settles every queued request
before cleanup. Active analysis terminals are captured from the retained session,
matched by tag, queued once, and cleared from active search state. Delivery
backpressure retains the terminal and keeps shutdown incomplete; retry cannot
create another terminal for the same tag. Workbench terminal capture uses the
same job identity guard across cleanup retries.

Apply the tests separately to the parent's integrated handler tree for red-first
execution. `engine-actor-tests-strengthened.patch` follows the initial four-test
patch and keeps the same pinned run future alive across stalled setup and shutdown.
`engine-actor-tests-retention.patch` follows that strengthening and adds retained
run, failed cleanup and full-terminal-delivery regressions. The tests remain
source-only in this lane; the parent owns all Cargo runs and reports actual results.

The source correction is an UNSTAGED binary diff of only
`src/desktop/engine_service.rs` and this evidence file against this worktree's
preserved staged handler baseline. Its old blobs remain in the common Git object
store. Apply with `git apply --3way` over the parent's 92f4d07 tree plus its staged
handler integration/API corrections. It preserves the parent's local terminal
sender clone and uses `DesktopEngineCommandsWorkbenchRequest` at the existing
async Start call boundary. Do not reapply the original full native-handler patch.
The saved original index patch is `native-handlers-before-actor.patch`.

## Live priority and graphics quiescence correction

`engine-live-priority.patch` is a new binary delta against exact parent main
copies captured before this round. It preserves the parent's actor Clippy fixes,
typed request UUID access, and async resource admission/acknowledgment contract.
It includes only the actor, bridge, focused tests and this amendment. No Cargo,
command/coordinator/tablebase policy implementation, commit or dependency edit
was performed in this lane.

The real terminal-backpressure hang was an accepted lifecycle request stranded
by early exit: after `pump()` flushed the retained terminal, shutdown looked
complete before the reserved shutdown retry was read. Completion and closed-port
exit now account for the lifecycle inbox. The existing regression retains its
success expectations and adds virtual-time deadlines with stage-specific failure
messages. Each newly started shutdown also clears the preceding attempt's cached
outcome, fixing the cancelled D1 then retried D2 public shutdown race.

The stable composition APIs are:

- `DesktopEngineService::with_live_demand(watch::Receiver<Option<GameEngineContext>>) -> Self`
- `DesktopEngineBridgePort::quiesce(tokio::time::Instant) -> Pin<Box<dyn Future<Output = Result<(), DesktopFailure>> + Send>>`

Every observed watch change captures Tokio time before further work, including
same-context values and coalesced changes. One retained internal preemption
operation stops the actual search tag or cancels the actual isolated job. It
never fabricates an application receipt, takes ordinary admission, spawns a task,
or acquires a second resource budget. Pending ordinary futures are interrupted
before polling their newly ready admission continuation. Stop uses the original
receipt, and later demand changes do not extend an active stop's deadline.

Quiesce and confirmed Shutdown share a bounded typed lifecycle channel. Clean
Stop retains the idle analysis child and persistent hash. Pending ambiguous
configuration/startup uses bounded retained cleanup; the real failed engine state
requires explicit Retry. Quiescence acknowledges only observed resource release,
keeps the actor open, and never starts analysis or resumes a game. Dropped run or
control waiters leave their owned operation and cleanup intact. Terminal/error
publication stays bounded and independent of urgent/lifecycle progress.

The bridge's opaque effect payload is now named `selection`, with typed
`ExportPreview` and `Exported` result variants for the parent's export controller.
The fallible tablebase policy lookup completes its request with the policy refusal
before preparation; it does not fail the actor. The parent owns the policy trait
and its platform implementation, including the explicit Windows refusal.

Four additional controlled tests cover demand during admission, duplicate and
coalesced demand with the original Stop receipt and no Resume, dropped quiesce/run
waiters during diagnostic cleanup pressure, and ambiguous configuration quiescence
followed by explicit Retry. They reuse real sessions/executors and the multi-spawn
provider, with exact CPU/hash/child ownership checks. They are registered through
`desktop_engine_commands::diagnostics` in the existing `engine` integration target.
The new tests and the amended backpressure test are source-only until the parent
runs Cargo. Standalone rustfmt and whitespace validation passed.

The exact quiesce/builder signatures were sent to the running
`parallel-20261016-native-analysis-composition-r1` worker through its existing T3
thread. No new agent or worktree was created. Apply this round's patch instead of
reapplying older actor replacement patches. Its old file blobs are retained in
the common Git object store for `git apply --3way`.
