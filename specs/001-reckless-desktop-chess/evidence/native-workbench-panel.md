# T085 standalone native workbench panel

Implementation lane baseline: `b8c5dae`, in the existing isolated T3 worktree.
The parent owns task markers, integration, commits, pushes and cleanup.

## Implemented surface

`src/presentation/workbench/` contains the standalone egui component, its
controller, input configuration and immutable backend projections. It does not
construct engine sessions, jobs, children, CPU leases, filesystem handles or
executors. The only shared registrations are `src/presentation/mod.rs` and
`tests/presentation.rs`.

The advanced section iterates `EngineEnumOption::ALL` and uses the actual
`EngineCapabilities` descriptors. Numeric inputs display upstream defaults,
units and discovered bounds, including dynamic Threads bounds. Validation uses
`EngineOptionsSetting` and `EngineOptionsEnumValue`; resource admission remains
the backend's responsibility. Applied values come from `EngineOptionsApplied`.
Missing applied resources are labeled unavailable rather than replaced with
upstream defaults. Drafts require an explicit per-option Save and survive
backend refusal or a configuration acknowledgment for another field.

Clear Hash is an explicit idle-only button with no value. The selected game's
variant is read-only and cannot change the board. SyzygyPath is represented by
native directory selection and backend configuration intents, without text
command entry, path quoting or filesystem work in egui.

Tablebase projections consume `EngineTablebaseStatus` and typed applied
tablebase outcomes. Prepared directories remain configured until a real backend
load outcome arrives. Loaded cardinality, unavailable tables and errors remain
distinct. The panel explains readable tables, traversal permission, private Unix
aliases, writable alias parents and Windows link privileges. A readiness barrier
alone does not become a loaded result.

All `WorkbenchEnumOperation::ALL` operations are selectable: position inspector,
raw White-perspective static NNUE integer, compiler diagnostics, perft with root
divide, and the built-in benchmark. The draft reuses `WorkbenchPerftDepth`,
`WorkbenchBenchParameters` and `WorkbenchJobDeadline` for defaults and limits.
An absent selected position prevents position-dependent operations while
compiler and benchmark remain available. Inputs, current position and available
CPU capacity are checked before emitting Start. The backend must still perform
authoritative exclusive admission.

`PresentationWorkbenchJobSnapshot::capture` projects a real `WorkbenchJob`.
It preserves its package/profile, identity, parameters, state, progress,
deadline, cancellation and outcome. Results are formatted from the existing
typed result family, without implementing another executor or result schema.
Integer values are rendered directly as decimal text. The view retains at most
128 result rows and the last 32 bounded diagnostic lines, with explicit omitted
counts. Full exports must read the backend-owned job rather than these display
rows. Cancellation can retain a late payload while keeping its actual
cancellation outcome visible.

Unsupported controls are explanatory text with links to the pinned upstream
parser and build definitions. The panel does not offer ponder, strength/Elo,
mate/searchmoves, WDL probabilities, EvalFile, engine books or unsupported
Syzygy controls as usable options. NUMA and SPSA limitations remain explicit.

## Public API and wiring

1. Build `PresentationWorkbenchControllerContext` from the current
   `EngineSearchGeneration`, configuration `DesktopRevision` and selected
   `EngineSearchContext`. Construct `PresentationWorkbenchController::new` with
   this context and an optional validated `EngineSearchPosition`.
2. Capture `PresentationWorkbenchSnapshot` from the selected package, discovered
   capabilities, optional applied options and optional managed tablebase status.
   Pass it to `receive_engine`. Set typed admission using `set_admission`.
   Report setup failure through `engine_unavailable`.
3. Retain one `PresentationWorkbenchView` per panel and call
   `show(ui, &mut controller)`. Its optional intent contains a typed action,
   request identity and originating context. Repeated egui passes cannot submit
   another action while one is pending, and a frame can emit at most one intent.
4. Route Configure to T083's idle option barrier. Call
   `configuration_applied` with the original context/request, exactly the next
   configuration revision and a freshly captured actual backend snapshot.
   This updates the controller's context. Use the resulting context for later
   operations and events. Route failures through `reject`.
5. Route SelectTablebaseDirectories to the native picker and T080 preparation.
   Retain the real `EngineTablebaseLocation` in the backend. Return prepared
   status through `tablebases_selected`, or close an abandoned picker with
   `selection_cancelled`. ConfigureTablebases asks the backend to configure that
   retained location and then uses `configuration_applied` with its actual
   load/unavailable/error result. The panel never owns or cleans aliases.
6. Route Start to T081/T082 with its existing `WorkbenchEnumParameters` and
   `WorkbenchJobDeadline`. `accept_job` binds the original request/context to a
   captured backend job. `receive_job` accepts later snapshots for the same job.
   Generation, selected position revision, configuration revision, request,
   package, job identity, immutable job metadata, progress and state order guard
   against stale replacement. Use `replace_context` when selection or engine
   generation changes. The backend retains and cancels old jobs independently.
7. Route Cancel to the exact backend job in the intent. Keep delivering snapshots
   while the child is cancelling and being reaped. No UI action declares a job
   cancelled before the backend reports it.
8. Route Export to T086 using its exact job identity. Export remains unavailable
   for unfinished jobs, is pending until acknowledged and shows completion only
   after `exported(context, request, job)`. Failed and cancelled jobs may export
   their actual diagnostics/outcome. T086 owns reproducible metadata, export
   format, native destination selection and writing.

Configuration and job updates with stale contexts leave the current projection
unchanged. Events from an old selection must not be relabeled with a newer
context. Native handler/app wiring belongs to T083/T089 and is not implemented
by this lane.

## Authored verification

Eleven tests under `tests/presentation/workbench.rs` and its fixture subtree
were authored against public typed engine/job contracts. The initial mounted
tests preceded implementation. The fixture starts the real `EngineSession`
handshake against an in-memory `EngineProcess`, obtains real applied options and
process generation, and selects a checksum-validated package fixture. It does
not invent parallel engine or job payloads. Mounted tests use the repository's
matching `egui_kittest` and font definitions.

Coverage includes all eight options, dynamic Threads limits, malformed and
out-of-range values, explicit Save, one-shot Clear Hash during discarded egui
passes, busy/capacity refusal, retained drafts after typed backend refusal,
canonical diagnostic defaults, all five real result types, progress, exact
large integers, bounded output, wrong-job cancellation, export admission and
acknowledgment, loading/setup failure, missing-position prerequisites,
cancelled/failed/timed-out outcomes, tablebase configured/loaded/unavailable/
failed distinctions, and stale request/generation/configuration contexts.

Standalone rustfmt completed for owned source and test files. No Cargo command
has run: the owner explicitly requires the parent's grant after integrating
T084. Consequently compilation, Clippy and all eleven authored tests remain
unverified at this source-ready handoff. No red test run is claimed while this
hold is active. After grant, use the shared
`CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target` for sequential
targeted Clippy and presentation tests. No dependencies or Cargo files changed.

This component has no actual window, GPU, production app wiring, real diagnostic
child execution or native platform acceptance claim. Genuine positive Syzygy
table loading, Windows permission behavior, accessibility/keyboard review in a
real app, T086 export writing and complete product gates remain in their owning
tasks.

## Parent verification

Integration at `ebed5c0` first found an invalid egui ID salt; the view now uses
the canonical option name. A read-only review also found export completion
feedback nested inside the engine-unavailable branch. The existing mounted
`job_defaults_progress_results_export_and_wrong_identity_are_typed` case was
observed failing because its success label was absent. Moving that feedback
outside the branch fixed the regression without changing the expected result.

`cargo fmt --all`, `cargo clippy --locked --lib --test presentation -- -D warnings`
and `cargo test --locked --profile dev --test presentation -- --quiet` passed.
All 87 presentation tests passed, including these 11 new cases, with no failures
or ignored tests. T085 is accepted as a standalone component; T083/T089 still
own desktop/application wiring, and T086 owns export encoding/writing. The
existing macOS linker warning remains unsuppressed.
