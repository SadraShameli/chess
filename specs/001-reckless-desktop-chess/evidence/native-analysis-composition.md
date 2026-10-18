# Native analysis and workbench composition

This is a source-only T089 implementation handoff based on `51eb69e`, with the
parent's concrete T083 actor/handler and native resource/selection snapshots
applied locally as dependencies. It is not a native acceptance report. The
parent owns shared module/test registration, Cargo execution and task markers.

## Application path

`src/desktop/native/bootstrap.rs` constructs one canonical
`Arc<DesktopNativeGameResources<M>>` and injects that same owner into the game
factories and the separate analysis runtime. The existing package-validation
child is still reaped; analysis starts a separately admitted child using the
selected package and real `EngineSessionStartup`.

`src/desktop/native/engine_runtime.rs` subscribes to live-demand changes before
initial admission. It stores the CPU lease, hash reservation, startup and
initialized session on its owner before awaiting later stages. Actual
configuration and readiness precede resource acknowledgment. The initialization
CPU lease then drops while the configured hash reservation transfers into the
retained command owner. Its initial hash is the canonical advertised startup
hash; its CPU setting uses the canonical workload policy, rather than an
independent budget.

Live demand is an event stream. Startup consumes changes, including repeated
contexts, and does not interpret a retained historical `Some` as current demand.
The actor receives the same watch through `with_live_demand`; canonical admission
still decides whether live work currently has priority. Runtime UI availability
uses authoritative live turns/seats and actual shared CPU availability, capped
by the workload policy. It does not derive availability from retained demand.

The service task returns its owner with its outcome. A failed cleanup keeps the
owner or running task retained. The root keeps a bounded ordered-event consumer
alive through shutdown, switches it to draining when presentation closes, and
observes both service and delivery tasks. It releases owners only after observed
completion. An abnormal lost worker owner blocks completion rather than polling
an already-consumed task or claiming its child was reaped.

Graphics preparation/recovery uses the reserved native `quiesce` port after
checking the event generation and before the workload drain. It keeps the event
consumer alive and does not resume analysis automatically. Quit cleanup includes
the analysis owner before releasing database/executor/profile ownership, under
the supplied shared shutdown deadline.

## Mounted presentation

`DesktopNativeHostServices` transfers a typed panel connection into
`PresentationApp::mount_engine`. The application retains the combined panel
owner across frames and supplies its actual selected game. The owner keeps the
existing T084/T085 views and controllers mounted, rather than reconstructing
them on each egui pass.

Selection comes exclusively from `DesktopGameSnapshot.analysis()` and its
matching viewed node. The original position and complete authoritative UCI path
are retained. Setup, active and interrupted live-game views cannot enable
analysis. Switching games, revisions or same-revision history invalidates the
old presentation and stops/cancels its actual work. A late Start response for a
replaced selection receives its own matching stop/cancel.

Controller input IDs and native request IDs remain separate. Each native request
is retained through admission failure and explicit Retry. Ordinary pending work
has a separate reserved urgent-control slot. A pending diagnostic Start exposes
Cancel Diagnostic Setup using the job ID derived from its actual native Start
request ID, even before any job snapshot exists. No queued/running job state is
fabricated to enable that control. Active Stop and Cancel remain reachable while
an ordinary request is pending.

The mounted controls include search modes and Start/Stop, discovered option
configuration and Clear Hash, directory selection followed by tablebase
preparation/loading, isolated workbench operations, progress, cancellation,
terminal results and export. Telemetry and bounded progress use latest-value
watches; ordered terminal events are unthrottled and delivered through bounded
channels with native wakes. Presentation defers terminal consumption while its
originating Start receipt is unresolved.

Export first requests the retained T086 artifact and displays its exact bytes,
digest, length and diagnostic omissions. Explicit Save selects an opaque native
WriteFile destination and applies that same preview identity and digest. The
parent-owned export handler performs `StorageAtomicFileStage` publication and
uses actual isolated-child observation. Presentation never regenerates an export
or infers child provenance from the selected package.

The narrow addition to `src/presentation/workbench/view.rs` accepts a typed
backend tablebase-selection failure. Standalone defaults remain available. The
root supplies the actual platform policy result; Windows setup is explicitly
disabled with its backend reason until an owner-approved alias-parent flow is
implemented in the still-open T080 work. A temporary directory is not represented
as Windows owner approval.

## Authored checks and integration requirements

`tests/presentation/engine_panels.rs` contains ten hermetic cases using real game
snapshots, the retained native runtime/actor, canonical selection/resources,
application receipts and native file-handle validation. The process and available-memory observation boundaries use controlled local
fixtures; the active-game setup check reuses the existing adjudication fixture.

The cases cover authoritative full history; unavailable live assistance;
mounted Start/Stop with repeated egui passes; same-revision history invalidation;
actual canonical live-demand preemption and CPU reassignment; terminal draining
after panel drop; cancellation of a stalled diagnostic setup before a snapshot;
dropped initial-handshake await ownership; idle Clear Hash plus an isolated
compiler job and byte-identical preview/file export; and configuration-failure
ownership. These cases are authored, not claimed passing.

`tests/desktop/native/bootstrap_games.rs` now retries only transient application
`Busy` at both request submission and `try_event`, using one bounded async helper
and cooperative yielding. This addresses the observed fixture boundary; it does
not assert a production game-runtime fault or add sleeps/test-thread caps.

`tests/native/engine_composition.rs` supplies a separately selected packaged-child
composition harness through the actual bootstrap/coordinator/game and engine
ports. It checks a completed game's analysis, correlated Start/observed Stop and
CPU release. It substitutes a headless host callback and is not GPU, rendered
widget, screen-reader or performance acceptance.

Parent registrations are required:

- Export `desktop::native::engine_runtime` and `presentation::engine_panels`.
- Include `tests/presentation/engine_panels.rs` from `tests/presentation.rs`.
- Include `tests/native/engine_composition.rs` only from `tests/native.rs`; route
  the optional `--engine-composition` harness argument to
  `NativeEngineCompositionHarness::run()`.

Lower changes remain parent-owned: fallible tablebase policy; bridge
`selection`; retained exact-preview export dispatch; live-demand pulse admission
and `quiesce`; post-admission selection reauthorization. Those dependency files
are excluded from this handoff patch.

## Verification status

The feature prerequisite script resolved the existing feature with
`SPECIFY_FEATURE_DIRECTORY=specs/001-reckless-desktop-chess`. Requirements and
native-desktop checklists contain 16/16 and 40/40 checked review items; the review
companion has no checkbox items. No extension hook file is present.

The installed compiler reports Rust `1.99.0 (b940084d7 2026-09-28)`, matching the
project pin. Explicit owned Rust files were formatted using the formatter from
that installation, `rustfmt 1.10.0`, with edition 2024 and `skip_children=true`.
Formatter parsing is not a Rust type check.

No Cargo command, compilation, test run, Clippy run, optimized build, native
engine execution or GPU run was performed in this lane because the parent
explicitly retained those checks. No red-test execution is claimed. The parent
must run the registered cases and required sequential gates after integrating
its lower-layer dispatch/policy changes. Actual native graphics, accessibility,
Windows paths/link privileges, positive tablebase cardinality, performance and
capability acceptance remain unverified.

## Parent integration and immediate Stop boundary

The parent registered the source and test modules, integrated export destination
consumption, and retained early Quit quiescence with the original absolute
deadline. All-target/all-feature Clippy and the separately selected native
target Clippy passed after fixing typed error propagation and bounding the
largest owned test futures with heap allocation.

The first packaged-child composition execution submitted Stop immediately after
Start's pipe-delivery acknowledgment. It returned a correctly correlated
`Failed(Engine(StopDeadline))` and reaped the child. This is a pinned upstream
ordering limit, rather than evidence of a normal active-search Stop success.
The exact upstream commit was fetched read-only again during integration:

- [UCI listener](https://github.com/codedeliveryservice/Reckless/blob/0e92358f5acd66e5ac77b1bf558202e47c515435/src/uci.rs#L111)
  handles Stop immediately while queuing Go for the main loop.
- [Search startup](https://github.com/codedeliveryservice/Reckless/blob/0e92358f5acd66e5ac77b1bf558202e47c515435/src/threadpool.rs#L78)
  sets the status to RUNNING, so a queued search can overwrite that earlier Stop.
- The listener answers `isready` directly, so it cannot fence search consumption.
  Minimal mode provides no search telemetry until completion.

The verified source SHA-256 values are
`09bd7a855739ffd1a539577447ff02df241ec6cbe5463cfe067758fe990d0e1d`
for `uci.rs` and
`3065c396f6895095b8657e2f33ee8710023a4ecd082520a4cb29c3ffcee1d2ff`
for `threadpool.rs`. Vendor code and the two-second safety deadline are unchanged.
The ordinary native Stop harness now waits for correlated search telemetry,
then requires the actual Stopped terminal. This establishes an active-search
test boundary; it does not resolve the upstream immediate-Stop race. T091 and
T092 performance and lifecycle acceptance remain open.

The optimized all-target build and separately named native build passed. Direct
execution of the release harness from Cargo's `target/release/deps` correctly
refused that executable's non-package resource location. The unchanged optimized
harness was copied into a temporary macOS `.app/Contents/MacOS` fixture with
the verified engine and piece resources under `Contents/Resources/resources`.
Its normal analysis Start, correlated telemetry, Stop and complete root cleanup
then passed. This checks the production resource locator in a staged layout;
it is not installer, graphical or accessibility acceptance.

The optimized standalone workbench harness passed all five isolated operations,
the 45-position depth-one benchmark, upstream defaults 16/1/12, perft 400,
depth-twelve cancellation, live readiness and exact lease recovery. The existing
game-composition harness also passed in development and optimized builds: two
real-engine mates, post-result input
rejection, durable restoration, frozen clocks, completed-tab relaunch exclusion
and both full 14/14 CPU recoveries with zero direct children and exclusive
profile reacquisition. Its event polling now retries only transient application
`Busy` inside the existing five-second bound, sharing the submission retry path.
No production lock behavior or deadline was changed for that fixture correction.
