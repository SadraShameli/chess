# Research: Reckless Desktop Chess

**Date**: 2026-10-16

**Status**: Native-egui architecture revision on 2026-10-16. Source/API research
resolves the owner's latest framework choice. Existing engine, data, behavior and
exactness findings remain authoritative alongside [resolved decisions](decisions.md).

**Scope**: Read-only upstream source/manifest/license checks and documentation
changes. No native-egui dependencies were installed, compiled or benchmarked by
this revision. Existing partial implementation/evidence covers the previous
shell; it is neither erased nor represented as native acceptance.

## 1. Desktop framework and presentation

**Decision**: Bevy 0.20.0 + bevy_egui 0.43.1, using the plugin-compatible egui
0.36 family. Bevy owns the native application/window/render host. It replaces
the proposed eframe shell; egui remains the Rust widget toolkit.

**Authority**: The owner requested native Rust, then an engine handling native
Metal/Vulkan-class APIs, and could not select among the researched options.
Choose Bevy under the existing instruction to pick the best recommendation.
This is a delegated assistant decision, not an explicit quoted Bevy answer.
The owner removed the interim Vulkan-only requirement. No forced API or
Vulkan-to-Metal packaging path is selected.

**Rationale**: The real-time sculpted board, materials, lighting and shadows are
central to the approved product. Bevy supplies established scene facilities and
wgpu backend management, while egui suits the substantial desktop panels.
Existing Rust domain/backend components remain independent of ECS. Keeping one
host avoids independent windows/devices and duplicate authoritative state.
Frequent Bevy breaking releases and custom premium-widget/accessibility work
are acknowledged maintenance costs, not reasons to reduce product scope.

**Alternatives considered**:

| Alternative | Why not selected |
| --- | --- |
| eframe 0.37 + egui-wgpu 0.37 + wgpu 30.0.1 | Native API abstraction works, but leaves more scene/lighting/shadow implementation than Bevy |
| eframe + sgl-3d 0.4.2 | Compatible with wgpu 30 and supplies scene facilities, but young; no verified native-egui/macOS integration |
| Glow + three-d | Established OpenGL rendering, but does not provide desired native Metal/Vulkan-class selection |
| Godot + Rust bindings | Complete engine/editor, but C++ runtime, different UI and required authored unsafe extension entrypoint conflict with current Rust policy |
| Former browser app | Superseded by owner native-Rust choice |

No performance superiority is asserted. Backend and scene-source inspection is
not a native execution or accessibility result. Exact compatibility is in
[renderer research](#native-egui-renderer-and-integration).

Sources: [Bevy 0.20](https://github.com/bevyengine/bevy/tree/v0.20.0),
[bevy_egui 0.43.1](https://github.com/vladbat00/bevy_egui/tree/v0.43.1),
[plugin manifest](https://github.com/vladbat00/bevy_egui/blob/v0.43.1/Cargo.toml),
[Bevy render settings](https://github.com/bevyengine/bevy/blob/v0.20.0/crates/bevy_render/src/settings.rs).

## 2. Engine selection and complete coverage

**Decision**: Pin Reckless v0.9.0, released 2026-03-01, commit
`0e92358f5acd66e5ac77b1bf558202e47c515435`. Integrate through one owned UCI
subprocess, retaining the exact tagged upstream source separately from app code.

**Rationale**: Its Cargo package is binary-only, non-published, and does not expose
a supported Rust library API. An in-process fork would couple the application to
unsafe internal representations, vendor changes, and search implementation details.

**Alternatives considered**: Embedding private modules, swapping in another
engine, or exposing only `bestmove` would fail compatibility or owner scope.
The [capability matrix](contracts/reckless-capabilities.md) is the sole detailed
coverage inventory; it includes all options, commands, statistics, build features,
unsupported popular options, and suspected broken SPSA scaffolding.

Source inspection overrides inaccurate summaries:

- Threads' advertised upper bound is dynamic, not the README's fixed 512.
- The official macOS asset is thin ARM64, despite release prose calling it universal.
- NUMA is optional Linux/libnuma build support, not enabled in official packages.
- `readyok` during search does not acknowledge completion; most commands are
  discarded until the search is stopped.
- Syzygy path quoting cannot repair its whitespace-splitting parser.
- Terminal positions and malformed inputs must be intercepted before search.
- Static `eval` and search centipawns have different normalization/perspectives.

Sources:

- [Stable release and assets](https://github.com/codedeliveryservice/Reckless/releases/tag/v0.9.0)
- [Cargo metadata](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/Cargo.toml)
- [UCI implementation](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/uci.rs)
- [Search initialization](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/search.rs)
- [FEN parser](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/board/parser.rs)
- [Reporting](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/thread.rs)
- [Score normalization](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/types/score.rs)
- [Time control](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/time.rs)
- [NUMA implementation](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/numa.rs)

## 3. Chess rules, notation, and persistence

**Decision**: Wrap shakmaty 0.30.2 in owning BoardPosition/GameSession types; use
pgn-reader 0.29.1 for bounded streaming PGN import, and an owned exporter for the
supported move-tree/annotation contract. The owner selected SQLite for indexed
data, with PGN/FEN interchange. Select bundled rusqlite and the storage actor/
schema ownership in resolved decisions.

**Rationale**: Legality, FEN/SAN/UCI conversions, castling, and standard/Chess960
support are concrete library facilities. pgn-reader moved into the shakmaty
repository; its old separate repository is not the current release source.
Its 0.29.1 manifest depends on shakmaty 0.30. Both declare Rust 1.97 minimum,
compatible with the project's 1.99 pin.

**Alternatives considered**: The engine's FEN parser is not a rules library.
Implementing every move rule from scratch adds correctness risk. Browser chess
state as another authoritative implementation creates disagreement. Indexed file
storage was considered; the owner selected SQLite for the library and opening
statistics. Transaction, indexing and migration policies follow resolved decisions.

The approved [study tools](contracts/study-tools.md) also require explicit data
source, exact CSV schema, scale, report interpretation, and detailed puzzle
policies. PGN opening games/studies, Polyglot .bin books, and puzzle CSV imports
are approved. Puzzles include filters, progressive source/destination/next-move
hints with assistance tracking, review, saved progress, and timed sessions.
Record failed attempts and permit practice retries on the continuing countdown.
Accept dataset-listed solution lines; label other legal moves unlisted without
claiming they lose or verifying alternatives with engine searches. Reports start automatically after games played in the app complete, covering
the played main line with a configurable 3-second-per-position default and
depth/node alternatives. Imported completed games require explicit report
requests. Solved puzzles show solution review and wait for Next without charging
session time during review; active hints keep the countdown running.
Variations are analyzed on demand; reports pause during
live games and resume afterward. The owner approved move labels and an
app-calculated accuracy percentage, with versioned formulas/thresholds selected in decisions.md. Displayed
search evaluations are White-positive. Timed puzzles offer whole-session and
per-puzzle countdowns, defaulting to configurable 5-minute sessions and 60-second
puzzles, with one countdown mode per timed run. Manual pause and app focus loss
pause puzzles and hide board/solution until resumed. Session timeout ends the run;
per-puzzle timeout records the result and advances. Shuffle filtered puzzles
without repeats until exhausted. Save local per-puzzle attempt history, outcomes,
assistance, elapsed time, and session summaries. Preserve paused runs and remaining
time across sleep/relaunch; always require explicit Resume. The Settings tab includes
common options and advanced engine/resources. Appearance applies immediately;
other settings require Save, with deferred safe engine application where needed.
The approved inventory and application policy are tracked
in [the UI contract](contracts/ui-experience.md#settings-tab). Starter
data plus personal imports are approved within a 2 GB installed starter-data
budget excluding app/engines. The opening book is study-only and never supplies
opponent moves.
Selected CC0 sources and formulas are resolved in decisions.md; source files
remain unacquired by this planning revision.

Confirmed live-game behavior keeps clocks running on app focus loss and pauses
all live games and engine searches on computer sleep pending explicit Resume.
Both modes allow takebacks:
previous human turn against Reckless, one move locally. Restore clocks before
undone moves, reversing increments. Local confirmation is configurable per game
and required by default. Clocks continue until local consent is accepted; then
restore snapshots. No request expiry; reuse matching variations in stable order,
with terminal-result cancellation under the selected race policy.
Manual Pause in both game modes freezes clocks/search pending explicit Resume.
Engine faults preserve the game and pause clocks; restart only on explicit Retry
and require Resume after recovery. This resolves the stop-timeout restart conflict.
The adjustable initial new-game preset is 10 minutes plus 5 seconds per move.
Untimed engine play defaults to a configurable 3 seconds per move. Retain removed
takeback moves as alternative variations. Approved clock families include untimed,
sudden death, Fischer increment, staged/cyclic, simple delay, and Bronstein delay.
Reckless has no delay token; the selected native delay/refund accounting and
single-movetime search allocation are in resolved clock decisions and require
controlled-time and pinned-engine tests.
History browsing keeps game/clocks/search running and offers
Return to Live.

The library does not replace application responsibilities: repetition history,
claimable versus automatic draws, authoritative clocks, suspend policy, complete
dead-position/flag-fall rules, and game-tree persistence must be implemented and
verified explicitly. Include difficult dead-position and possible-mate fixtures;
do not equate `is_insufficient_material` alone with full adjudication.

Sources:

- [shakmaty 0.30.2](https://github.com/niklasf/shakmaty/tree/shakmaty-0.30.2)
- [shakmaty capabilities and license](https://github.com/niklasf/shakmaty/blob/shakmaty-0.30.2/README.md)
- [Matching pgn-reader manifest](https://github.com/niklasf/shakmaty/blob/shakmaty-0.30.2/pgn-reader/Cargo.toml)
- [pgn-reader 0.29.1 documentation](https://docs.rs/pgn-reader/0.29.1/pgn_reader/)

## 4. Process lifecycle, transport, and capacity

**Decision**: Retain engine/storage actors and the capacity coordinator. Root
owns one Tokio 1.53.3 runtime, bounded Rust message ports, latest-value telemetry
and a Bevy App. Domain state remains ordinary Rust, while Bevy resources/entities
represent presentation and scene instances. Associated system methods receive
ports/snapshots without constructing services inside render systems.

**Rationale**: UCI lacks correlation and ignores configuration during search.
Typed in-process messages remove redundant UI serialization but retain revision,
idempotency, stale-generation and cancellation requirements. Repeated UI passes
or Bevy updates must not create repeated state transitions.

Tokio serves asynchronous UCI/network/application work outside Bevy update
systems. Bevy's task pools are not a Tokio reactor; do not run Tokio-dependent
I/O on a pool without its owned runtime context. CPU domain work stays on the
existing bounded executor. Partition native worker capacity before initializing
Bevy global pools, including render/asset work, Rayon and active engines.
TaskPoolOptions::with_num_threads alone is not a strict combined cap at small
counts because default IO/async/compute minima can each allocate a worker.
Use explicit assignment policies and the [capacity design](decisions.md#native-engine-capacity-partition).

Use reactive Bevy updates with explicit wakeups after backend publication.
WinitSettings::desktop_app defaults to long reactive waits, so it is insufficient
for visible clocks/animation. EventLoopProxyWrapper.send_event(WinitUserEvent::WakeUp)
can wake the native loop. Pinned bevy_egui only forwards immediate repaint requests;
nonzero egui request_repaint_after alone is not a native timer. A presentation
scheduler must own the next clock/animation deadline and native wakeup.

Disable automatic close on the main window; close/menu/quit/update all use one
owned confirmation/save/stop/reap state machine. Use WindowPlugin close_when_requested=false and ExitCondition::DontExit;
only successful completion sends final AppExit::Success. Native macOS Cmd+Q
requires an application-owned quit/menu adapter to intercept confirmation, plus
an idempotent bounded cleanup resource holding its own required handles. Bevy
may clear/drop resources without returning from App::run; do not rely on drop
order or only post-run cleanup. Inject a bounded platform-termination fallback and preserve crash
recovery. Native focus is distinct from desktop sleep; a platform power adapter
is required because winit suspended does not guarantee desktop sleep reporting.

**Alternatives considered**: Independent full-capacity pools, detached CPU work,
blocking UI futures, hidden Tauri runtime, trusting egui repaint timers alone or
starting asynchronous saves only on final AppExit fail the selected boundaries.

Sources: [Bevy winit configuration](https://github.com/bevyengine/bevy/blob/v0.20.0/crates/bevy_winit/src/winit_config.rs),
[Bevy task-pool options](https://github.com/bevyengine/bevy/blob/v0.20.0/crates/bevy_app/src/task_pool_plugin.rs),
[plugin source](https://github.com/vladbat00/bevy_egui/blob/v0.43.1/src/lib.rs),
[Tokio](https://docs.rs/tokio/1.53.3/tokio/runtime/struct.Runtime.html),
[message contract](contracts/desktop-ipc.md).

### Local profile ownership and activation

**Decision**: Enforce the clarified single-writer profile policy with Rust's
`File::try_lock` on a permanent control file and interprocess 2.4.5 with its Tokio
feature for activation-only local sockets. Acquire the exclusive guard before
opening user databases, starting migrations/recovery or allocating engines.
The root retains one un-cloned handle through complete shutdown. The selected
algorithm and limits live in [profile ownership](decisions.md#local-profile-ownership-and-activation).

**Rationale**: `try_lock` is stable since Rust 1.89, within the existing 1.99 pin,
and distinguishes contention from an I/O failure. OS lock lifetime avoids stale
PID-file ownership. Locks are advisory on Unix: every application/helper follows
the protocol, and the lock file is never unlinked, replaced, truncated for
metadata or inherited by children. A restore replaces data, not this anchor.
Successful exclusive acquisition is the authority for a new writable lifetime;
stale metadata, a PID lookup or activation timeout cannot grant that authority.
No claim of coordination with unrelated programs that ignore this protocol is
made. Supported profiles use validated local filesystems.

Interprocess supplies safe async Unix-domain sockets and Windows named pipes.
Use its generic local-socket API on Unix: `ListenerOptions::create_tokio`, async
`accept`/`Stream::connect`, `reclaim_name(false)` and `ListenerOptionsExt::mode`.
Select filesystem sockets in user-private directories on Unix, not Linux
abstract names or implicit `/tmp`
mapping. The pre-bind socket mode option may return Unsupported on macOS;
private-directory protection and verified peer effective-user identity remain
required there. Windows pipes reject remote clients by default; provide an
explicit current-user/SYSTEM security descriptor rather than a default DACL.
Use the concrete Windows `PipeListenerOptions`/Tokio duplex byte-stream API.
The generic Windows client ignores its requested wait mode and can enter an
uncancellable blocking connect. Select `connect_by_path_with_wait_mode` with
`ConnectWaitMode::Timeout(Duration::ZERO)` and application-owned async retry
within the overall deadline. Written pipe streams can also create hidden linger
workers on drop. A narrow stream owner always calls safe `assume_flushed()`
before dropping, including cancellation; do not start an underlying pipe flush.
The [activation protocol](contracts/desktop-ipc.md#local-activation-interface)
acknowledges receipt consumption before the client closes. On timeout,
discarding incomplete activation bytes is acceptable and never grants ownership.

Reuse sysinfo 0.39.6's current-process effective-user query for Unix directory
and peer-credential validation. On Windows select winsafe 0.0.29, `advapi`
feature, for safe fallible current-token SID acquisition, plus widestring 1.0.2
for the interprocess descriptor parser. Build an explicit protected DACL granting
the current SID and SYSTEM, not a permissive default or unchecked PID identity.
Winsafe is MIT with Rust 1.87; widestring is MIT OR Apache-2.0 with Rust 1.58.
Reciprocal server authentication uses the kernel pipe peer PID and safe Winsafe
limited-query process/token access to compare the server's User SID. A DACL
alone does not authenticate a pipe replacement after an owner exits. Query
failure rejects activation; PID lookup never authorizes profile takeover.
Reserve handler capacity before accepting: the Windows listener creates its
replacement instance before returning the connected stream, so the selected
four handlers need one additional pending instance and pre-accept admission.
Interprocess is 0BSD OR Apache-2.0 and declares Rust 1.75. These are source/API
findings, not installed-target authentication or locked-build evidence. Winit
0.30.13 is a direct matching dependency for the adapter's native attention types,
sharing Bevy's resolved instance rather than adding a second event loop.

Activation requests reach the owner's native thread through bounded messages.
Bevy exposes `Window::set_minimized(false)` and focus requests where supported.
Its compatible winit 0.30.13 `request_user_attention(Informational)` uses
xdg-activation on Wayland; the compositor still decides focus. Wayland does not
support Bevy's ordinary focus/unminimize setters. Track requested versus actually
observed focus and explain manual window selection when the OS refuses; never
open a competing writable session. Startup-token setters for an existing window
are not supplied by this winit version and are not invented here.

A durable installation intent closes the parent/helper ownership gap. Every
new guard holder checks it before mutable initialization. An ordinary launch
cannot initialize a profile while installation is pending. The helper observes
actual parent exit, acquires the same guard and revalidates before installing;
interruption leaves explicit recovery rather than timeout-based takeover.

**Alternatives considered**: PID-only files, heartbeat expiry, deleting a locked
file, SQLite's busy timeout or unique socket binding alone do not coordinate
all clocks, subprocesses, migrations and installers. TCP listeners add an
unnecessary network boundary. A framework-only focus flag is insufficient on
Wayland. Inherited lock transfer is not selected because its cross-platform
lifetime/duplication semantics are unnecessary with the durable intent.

Sources: [Rust File locking](https://doc.rust-lang.org/1.99.0/std/fs/struct.File.html#method.try_lock),
[Rust 1.89 stabilization](https://blog.rust-lang.org/2025/08/07/Rust-1.89.0/),
[interprocess 2.4.5](https://docs.rs/crate/interprocess/2.4.5/source/Cargo.toml.orig),
[listener options](https://docs.rs/crate/interprocess/2.4.5/source/src/local_socket/listener/options.rs),
[Unix security options](https://docs.rs/crate/interprocess/2.4.5/source/src/os/unix/local_socket.rs),
[Windows security options](https://docs.rs/crate/interprocess/2.4.5/source/src/os/windows/local_socket.rs),
[pipe connection implementation](https://docs.rs/crate/interprocess/2.4.5/source/src/os/windows/named_pipe/tokio/stream/impl/ctor.rs),
[pipe flush suppression](https://docs.rs/crate/interprocess/2.4.5/source/src/os/windows/named_pipe/tokio/stream/impl/send.rs),
[generic Windows client](https://docs.rs/crate/interprocess/2.4.5/source/src/os/windows/named_pipe/local_socket/tokio/stream.rs),
[Winsafe token query](https://docs.rs/crate/winsafe/0.0.29/source/src/advapi/handles/haccesstoken.rs),
[sysinfo effective identity](https://docs.rs/sysinfo/0.39.6/sysinfo/struct.Process.html#method.effective_user_id),
[Bevy window updates](https://github.com/bevyengine/bevy/blob/v0.20.0/crates/bevy_winit/src/system.rs),
[winit Wayland window](https://docs.rs/crate/winit/0.30.13/source/src/platform_impl/linux/wayland/window/mod.rs).

## 5. Visual design and accessibility

**Confirmed direction**: Dark-first premium styling, richer effects and animation,
and a full light theme. The owner selected the Chess.com-inspired layout with
collapsible left navigation, a large board/player bars/clocks, and resizable
right-side tabs. Configurable board aids, a command palette, keyboard shortcuts,
and optional move/check/result sounds remain approved. The native UI choice
supersedes earlier TypeScript/Vite approval.

**Confirmed appearance defaults**: Graphite surfaces, emerald accents, green/cream
board, and warm light theme. Legal-move dots appear on selection; last-move/check
highlights and captured pieces are initially enabled. Sounds default to enabled
during active play and silent in background. The owner chose sculpted 3D-style
pieces, bundled modern sans-serif UI/notation with tabular clock digits, and
configurable 180 ms piece moves/160 ms panel transitions with subtle elevation,
respecting reduced motion. Real-time 3D board/pieces with camera rotation/zoom,
ivory/ebony satin materials, and a 1280x800 native-logical-point minimum window are confirmed.
The default camera is centered at 20 degrees from top-down with all squares
visible. Maximum manual zoom-in keeps the full board visible. Reset View restores
default pose/framing/tilt while retaining White/Black orientation. Normal piece dragging,
right-drag drawing, and visible camera rotation/zoom controls are confirmed.
The board is 3D-only, without a selectable or fallback 2D board. Remember the
last camera view across launches with Reset View. The evaluation bar appears
only in analysis/review. Clickable report findings navigate to their position,
show alternatives, and offer an analysis branch. Move-list/explorer/book tabs
share the selected board position. Quality defaults to Automatic
with manual Low/Medium/High overrides. Graphics failure preserves the game,
pauses clocks, and offers Retry; explicit resume is required after recovery.

**Resolved presentation details**: Exact tokens, native scene integration, original GLB
meshes/synthesized WAV, Inter 4.1/SVG figurines, panel sizes, camera framing,
quality/adaptation, keyboard/audio and context recovery follow
[presentation decisions](decisions.md#presentation-assets-and-input). Animation
durations and selected hardware baselines remain targets rather than measured
performance. Accessibility and native graphics/window validation remain release work.

**Rationale**: Appearance must remain coherent during play, analysis, errors,
resizing, promotion, and keyboard navigation. Semantic squares enable keyboard
movement and assistive technology alongside the selected real-time 3D scene.
The owner chose real-time 3D with rotation/zoom over pre-rendered and fixed-camera
options. The selected architecture uses egui widgets and Bevy scene rendering
in one native host. Native logical points retain the approved apparent layout size;
physical framebuffer dimensions are derived at the scene boundary.

One typed theme owner supplies widget and scene tokens. AccessKit semantics and
keyboard controls accompany rendered squares. Neither custom mesh rendering nor
installing egui proves VoiceOver/NVDA/Orca behavior. Actual native target checks
remain required, including focus, promotion, reduced motion and display scaling.

**Alternatives considered**: Drag-only interaction, visuals without a semantic
board or adopting unstyled stock widgets as completed premium design would fail
the approved experience. [The UI contract](contracts/ui-experience.md) retains
full visual/interaction acceptance.

Sources: [egui accessibility](https://github.com/emilk/egui/blob/0.36.0/README.md),
[AccessKit](https://github.com/AccessKit/accesskit),
[WCAG 2.2](https://www.w3.org/TR/WCAG22/).

## 6. Cross-platform packaging and testing

**Decision**: Use framework-independent cargo-packager 0.11.8 for Windows NSIS,
macOS .app/.dmg, Linux .deb/AppImage. It has an egui example and does not require
Tauri. Native builders retain Windows 11 x86-64, macOS 14+ ARM64 and Ubuntu
22.04+ x86-64 baselines; Linux validation includes X11 and Wayland.

**Rationale**: The selected installer formats survive the UI change. Framework-
independent packaging avoids keeping obsolete WebView dependencies. Bevy needs
actual native GPU/window/driver checks, not browser CSS tests.
Cargo-packager is MIT/Apache-2.0; inspect its pinned config/resource mapping and
installer outputs before acceptance. Platform ad-hoc signing and install/update
helpers remain integration work, not guaranteed packager behavior.

**Testing decision**: Match egui_kittest 0.36.0 to the plugin's egui 0.36 graph,
without its standalone renderer. Test semantic trees, inputs, focus and command
submission with fixed clocks/seeds. Headless Bevy App tests cover adapter
scheduling, snapshot-to-entity updates, wake/deadline logic and cleanup. Real
native scene tests run the production Bevy host on the OS main thread using a
Cargo `harness = false` executable, temporary data and bounded orchestration.
The explicit target-selection policy below keeps this native execution outside
ordinary hermetic tests without dropping its compile, lint or acceptance checks.

Capture through Bevy Screenshot::primary_window or Screenshot::image and observe
ScreenshotCaptured, after rendering. Limit captures to one in flight; require
completion before a deadline and verify scene/widgets/expected dimensions.
Matching-egui image utilities can compare these images; do not replace production
GPU testing with a separate kittest renderer. Former eframe request_screenshot,
run_native/on_exit and WDIO APIs are not selected testing/lifecycle mechanisms.

**Alternatives considered**: Tauri bundling/WDIO would preserve an obsolete
runtime. Software/headless widgets alone do not establish production native GPU rendering, sleep,
OS dialogs or screen-reader support. Cross-compilation alone does not establish
minimum-platform compatibility.

Use unsigned Windows installers and nested ad-hoc macOS signing, with tested OS
warning/approval guidance. No paid signing, notarization or services. No WebView2
or WebKitGTK runtime is needed by the final app. Linux dependencies may use
internet during installation, with offline-preparation instructions. Native
file dialogs may require portal/D-Bus services independently of the renderer.

Sources: [cargo-packager 0.11.8](https://docs.rs/cargo-packager/0.11.8/cargo_packager/),
[packager repository](https://github.com/crabnebula-dev/cargo-packager),
[egui_kittest 0.36](https://docs.rs/egui_kittest/0.36.0/egui_kittest/),
[Bevy screenshot source](https://github.com/bevyengine/bevy/blob/v0.20.0/crates/bevy_render/src/view/window/screenshot.rs).

### Native test target selection

**Decision**: Register the `native` target at `tests/native.rs` with
`harness = false` and `test = false`. Keep default Cargo test selection for
hermetic suites and library doctests. Execute the native target explicitly by
name through `make native-tests`, using optimized production Bevy rendering,
fixture-only storage and bounded orchestration on the OS main thread.

**Rationale**: The [Cargo test target-selection reference](https://doc.rust-lang.org/cargo/commands/cargo-test.html#target-selection)
states that a named `--test` selection overrides `test = false`. The
[harness field](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-harness-field)
controls the entry point, not default execution. Source for the installed Cargo
1.99 commit `5f94df4789f005f9a352888e8355ffc645b7ed0e` confirms that
[`--all-targets` selects all enabled target classes before named filters](https://github.com/rust-lang/cargo/blob/5f94df4789f005f9a352888e8355ffc645b7ed0e/src/ops/cargo_compile/compile_filter.rs#L113-L167),
but [the all-tests rule filters on `Target::tested`](https://github.com/rust-lang/cargo/blob/5f94df4789f005f9a352888e8355ffc645b7ed0e/src/ops/cargo_compile/unit_generator.rs#L465-L495).
Consequently it omits this `test = false` target. Lint and optimized/profiling
build gates need an additional separate named `--test native` invocation,
without `--all-targets`; the build invocation compiles/links without running it.
`make native-test-build` exposes that optimized nonexecuting check. Actual
GPU/OS execution is separate, required release evidence and fails when required
resources are unavailable. Test controls stay in the harness executable and
are absent from application entry points and installed packages.
The integration target's `cfg(test)` does not enable test-only helpers in its
normal library dependency. Drive real application ports with harness-owned
adapters in `tests/native.rs` and `tests/native/`, including multiprocess fixture
launches, rather than adding an automation mode to the shipping executable.

**Alternatives considered**: `harness = false` alone still runs under default
Cargo tests. Optional feature gating does not establish separation while the
test gate uses `--all-features`. Appending a named target to `--all-targets`
does not force selection in the pinned Cargo implementation. Ignoring tests or
returning success when GPU prerequisites are missing would conceal acceptance
failures. A second renderer would not exercise production Bevy behavior.

## Evidence limits and next-phase checks

The resolved choices are recorded in [the decision review](review.md). Remaining verification belongs to
implementation/release acceptance, not an assumed successful result:

| Evidence | Current status | Required disposition |
| --- | --- | --- |
| Engine source/options/commands | Tagged source inspected | Protocol fixtures and real-engine tests for every matrix entry |
| Official macOS architecture | ARM64 header inspected without execution | Rebuild portable ARM64 and execute on minimum Apple Silicon baseline |
| SPSA feature | Static missing-symbol finding | Controlled optional-feature build; classify unavailable unless upstream supplies a stable repair |
| Linux x86-64 NUMA profile | Source configuration inspected | Native compile, compatibility, libnuma, and hardware topology tests |
| UI quality and responsiveness | Design only | Recorded target-system measurements and visual/accessibility review |
| Model distribution rights | No explicit model LICENSE found | Authoritative terms before external distribution |


## Native egui renderer and integration

**Decision**: Bevy 0.20.0 + bevy_egui 0.43.1 and the matching egui 0.36 graph.
Use Bevy's compatible wgpu dependency graph; do not force standalone eframe's
wgpu 30.0.1 into it. Enable the required PBR/glTF/camera/window/native-backend
features and bevy_egui AccessKit explicitly. Package a single host/device, no
browser scene or parallel eframe renderer.

**Rationale**: Bevy provides GLB asset loading, reusable physically based
materials, lighting/shadows, camera, scene hierarchy and native render lifecycle.
The chess-specific adapter owns instance selection, board layout, snapshots,
interaction, quality and error/recovery policy. It does not rewrite a general
renderer or move chess correctness into ECS.

### Scene, assets and viewport

The pinned GLB has one scene with 12 overlapping named color/kind parents, one
mesh child each, two metallic-roughness materials and no textures, extensions or
external buffers. Preserve the existing 1,376,224 bytes and SHA-256
`ee0c9bf8cd98f1239046b0566748030c4c320f049fd6ab3232f1f7e9691a020e`.
Do not spawn the entire library scene as the board. Resolve named Gltf nodes,
descendants and mesh/material subassets into a reusable typed piece registry,
retaining authored transforms. Bevy 0.20 scene assets use WorldAssetRoot and
Handle<WorldAsset>; old SceneRoot tutorial APIs are not the selected source.

A camera renders the board into a persistent image displayed inside an egui
widget. Official plugin examples demonstrate this arrangement. The adapter owns
current egui rectangle, clipping, logical/physical sizes, camera viewport and
render-target generation. Resize/DPI changes update these together. Scene input
must first map from the visible image rectangle into the camera's physical
viewport; generate an explicit camera ray for mesh picking/board-plane mapping.
Bevy's ordinary window-pointer RayMap does not automatically traverse egui image
indirection. Reject points outside the current clipped board viewport.

Map hits back through the adapter's stable piece/square registry, then submit
native game intents. Legality remains in BoardPosition. Test every square,
all allowed camera bounds, drag/click/right-draw/promotion and revision changes.
An alternate direct-camera viewport was considered, but cached offscreen rendering
better separates unchanged 3D work from clock/widget repaints.

Use PBR satin materials, directional shadows and the selected quality tiers,
without speculative postprocessing. Reuse GPU/asset instances and update only
changed snapshot fields. Static scene image content is reused during unrelated
widget/clock updates. On-demand scheduling still services pending assets/GPU work,
requested animations and native events. Validate actual frame pacing and idle
CPU; WinitSettings alone does not establish that the renderer stops work.

### Native APIs and render failure

Bevy's automatic backend settings support native Metal/Vulkan/DX12. macOS uses
native Metal without MoltenVK; Windows may select Vulkan or Direct3D according
to available adapters, and Linux supports Vulkan. Do not promise one default
Windows API or force a Vulkan-only mask. Surface/driver compatibility remains a
native acceptance gate on all selected minimum systems.

Bevy 0.20 exposes RenderErrorHandler and RenderErrorPolicy including StopRendering
and Recover(RenderCreation). Its default log/exit behavior does not meet the
product's preserved-state/Retry policy. Configure a custom handler, serialize
one application interruption, invalidate the scene generation and rebuild GPU
resources only through explicit Retry. Recovery reruns render startup; every
adapter-owned resource and pending screenshot/pick must follow the new generation.
Bevy's default automatic initialization still contains panic paths for missing
adapters/device request failures. Use an intentional fallible native initialization
adapter, not catch_unwind around default startup, and prove safe startup/error UI
in the first migration slice. This is project integration, not a claim that
friendly startup and every driver-loss scenario work automatically.

Safe manual constructors exist for RenderDevice, RenderQueue, RenderAdapterInfo,
RenderAdapter and RenderInstance, passed through RenderCreation::manual. Wgpu
adapter/device requests return Result. However, RenderPlugin build needs them
before the normal winit runner creates its window; headless preflight does not
prove actual surface compatibility. The first native bootstrap spike must
resolve real-window timing without authored unsafe code or vendor changes.
The official render_recovery example establishes Bevy recovery mechanisms, but
bevy_egui caches GPU resources in Plugin::finish rather than RenderStartup. Add
a recovery adapter reinitializing its public renderer caches/pipelines on each
device generation and test actual restored widgets. Merely selecting Recover
does not establish complete egui recovery.

**Alternatives considered**: Eframe/wgpu requires more scene work; sgl-3d is young;
Glow/three-d is OpenGL; Rend3 is archived; Godot adds a C++ host and an unsafe
extension-policy conflict. Bevy is selected under owner delegation.

**Acceptance gates**: One locked compatible graph, original assets/materials,
render-to-image picking/scaling/clipping, frame/idle metrics, safe GPU startup,
custom device-loss/retry policy, real target screenshots, input and resource
cleanup. Native accessibility uses the separate explicit semantic board.
No performance or minimum-platform guarantee follows from source inspection.

Sources:

- [Bevy render settings](https://github.com/bevyengine/bevy/blob/v0.20.0/crates/bevy_render/src/settings.rs)
- [Bevy GLB loader](https://github.com/bevyengine/bevy/tree/v0.20.0/crates/bevy_gltf/src)
- [GLB example](https://github.com/bevyengine/bevy/blob/v0.20.0/examples/gltf/load_gltf.rs)
- [Bevy-egui render-to-image](https://github.com/vladbat00/bevy_egui/blob/v0.43.1/examples/render_to_image_widget.rs)
- [Render errors](https://github.com/bevyengine/bevy/blob/v0.20.0/crates/bevy_render/src/error_handler.rs)
- [Renderer initialization](https://github.com/bevyengine/bevy/blob/v0.20.0/crates/bevy_render/src/renderer/mod.rs)
- [Official recovery example](https://github.com/bevyengine/bevy/blob/v0.20.0/examples/app/render_recovery.rs)

## Bundled font candidates, license review 2026-10-16

**Status**: Owner selected Inter, English-only interface, and letter/figurine
move display with letters initially selected. PGN retains standard letters.
No font files are installed or pinned.

- [Inter](https://github.com/rsms/inter) has a verified
  [SIL Open Font License 1.1](https://github.com/rsms/inter/blob/master/LICENSE.txt).
- [Geist Sans](https://github.com/vercel/geist-font) has a verified
  [SIL Open Font License 1.1](https://github.com/vercel/geist-font/blob/main/OFL.txt).

Inter is selected as the UI/notation family; Geist Sans remains an evaluated
alternative.
Exact release files, license notices, glyph coverage for approved languages,
tabular digit behavior, font loading, and notation-symbol coverage need validation
before bundling. License inspection is not a rendering or performance test.


## Desktop updater research, 2026-10-16

**Owner decision**: Include signed desktop OTA through GitHub Actions with
private binaries, automatic build/publication on version-tag pushes, and
nonblocking startup plus manual Settings checks. Use a repository-scoped
fine-grained Contents-read token in the OS credential store. Automatically
download updates; install only after explicit Install, confirmation, and successful
pause/save of all live games; restore them paused. Credential failures show a
persistent nonblocking Settings notice with token controls. Failed downloads stop
and offer Retry without automatic retries. Relaunch automatically after successful
installation where supported; explain any manual-reopening requirement. This is
an owner-selected policy, not verified cross-platform lifecycle behavior. Stable
releases only use vMAJOR.MINOR.PATCH tags matching the application version.
Before installation require a fresh successful safety backup of user data/settings
at the configured destination, excluding datasets and credentials; failure blocks
installation with Retry. Retain the latest five successful pre-update backups plus
any needed for unresolved recovery, separately from daily/manual retention.
Failed migrations stop startup, preserve data, and offer previewed, confirmed
pre-update backup recovery, potentially requiring compatible-app reinstallation.
Installation failures offer explicit Retry only where safe and manual-recovery
guidance otherwise, without automatic retries. These policies do not establish
tested migration or binary-rollback support. Keep published private releases and
assets until explicit deletion. GitHub Actions encrypted secrets hold the OTA
signing key and passphrase, with an owner-controlled offline backup. Ask before
downloads on OS-detected metered connections; cross-platform detection and
deadline enforcement remain unvalidated. Ask also when connection status is
unknown or unavailable. Approved deadlines are 15 seconds total for each check
attempt across discovery/metadata, excluding binary downloads; downloads stop
after 60 seconds without received bytes or 30 minutes total, whichever occurs
first, with manual Retry. These are selected policy values, not measurements. Cancel stops downloads,
discards partial bytes, and waits for explicit Retry. Completed downloads survive
relaunch and require revalidation before installation. Every metered/unknown
attempt, including Retry, requires approval. There is no Remove Download action;
retain completed downloads until installation/replacement. Keep a downloaded
version when a newer stable release appears and offer explicit Download Newer
Version under the same approval rules. On detected change to metered/unknown,
stop, discard partial bytes, and ask before a fresh attempt. Platform detection
and detailed cache/replacement mechanics remain unvalidated.
Remaining delivery policies
are selected in [resolved decisions](decisions.md).

**Decision**: Replace Tauri update/plugin/action assumptions with native
application-owned policy, cargo-packager 0.11.8 installers, reqwest 0.13.5 async
streaming, keyring 4.2.0 credentials, minisign-verify 0.3.0 verification and
minisign 0.10.0 release signing tooling. Exact wire/install details are in
[packaging](contracts/packaging.md#desktop-ota-and-github-actions).

**Rationale**: Private GitHub discovery and downloads need authenticated API
requests, redirect credential isolation, bounded disk staging and all selected
cancellation/timeouts. A small explicit orchestration layer composes established
HTTP, signature, credential and packaging libraries. It does not implement
cryptography or assume the native scene engine is an updater framework.

**Verified facts and limits**:

- Cargo-packager is framework-independent, MIT/Apache-2.0 and supports the four
  selected formats. Its separate cargo-packager-updater 0.2.3 uses blocking
  in-memory downloads, offers no cancel callback, does not reverify signatures
  during install and handles Linux AppImage rather than the approved .deb path.
  Do not make it the policy owner or assume it meets our staged-cache contract.
- Minisign-verify exposes streaming verification. Verify the exact signed manifest
  bytes before JSON parsing, then bind every installer to its target/version,
  length and SHA-256 in that signed manifest. Revalidate cached manifest and
  installer bytes before handoff. Use no new bespoke signature algorithm.
- Native platform installer adapters/helper must wait for process exit, validate
  installed destination, perform safe package handoff/replacement and report
  retry/relaunch capabilities. This is project integration work, not a proven
  universal self-update API. Detailed target behavior stays in packaging.
- Keyring 4.2.0 uses OS credential stores. Linux lacking a working Secret Service
  shows explicit setup status or user-selected session-only credentials, never
  silent plaintext storage. Keep credentials outside export/backup/log state.
- [GitHub asset retrieval](https://docs.github.com/en/rest/releases/assets#get-a-release-asset)
  supports fine-grained Contents-read tokens and octet-stream responses, direct
  or redirected. [Discovery](https://docs.github.com/en/rest/releases/releases#get-the-latest-release)
  is separate from asset download. Authenticate only approved GitHub API requests;
  follow approved asset redirects without forwarding the token to other origins.

**Alternatives considered**: Tauri updater requires its removed framework;
cargo-packager-updater fails current policy needs. Generic binary replacement
without platform-aware installer/backup handling is insufficient. A paid update
service is excluded. No authenticated end-to-end update or installer test was
performed by this planning revision.

Sources: [cargo-packager](https://docs.rs/cargo-packager/0.11.8/cargo_packager/),
[updater source](https://docs.rs/crate/cargo-packager-updater/0.2.3/source/src/),
[minisign-verify](https://docs.rs/minisign-verify/0.3.0/minisign_verify/),
[minisign](https://docs.rs/minisign/0.10.0/minisign/),
[reqwest](https://docs.rs/reqwest/0.13.5/reqwest/),
[keyring](https://docs.rs/keyring/4.2.0/keyring/).

## Selected starter-data direction

Follow [starter provenance/selection](decisions.md#starter-datasets-and-provenance),
[puzzle interchange](decisions.md#puzzle-interchange-and-lifecycle), and
[book generation](decisions.md#book-generation-and-explorer-semantics). Standard
starters, two selectable CC0 online collections including a titled-master subset,
broad puzzle difficulty, manual updates and the 2 GB installed cap are selected.
Exact file checksums and measured collection sizes are preparation outputs,
not unanswered preferences or already acquired data.

### Deterministic starter subset preparation

**Decision**: Use [starter subset selection v1](decisions.md#starter-subset-selection-v1)
for exact rating/time/theme strata, canonical ordering, quota remainders and
installed-byte prefix reduction. Keep the full ratings, TimeControl and theme
metadata separately from these preparation-only derived values.

**Rationale**: [Lichess export documentation](https://database.lichess.org/)
describes WhiteElo/BlackElo and a list of puzzle Themes; it defines no canonical
primary puzzle theme. The [time-control FAQ](https://lichess.org/faq#time-controls)
uses base seconds plus 40 increments, with Bullet below 180 seconds, Blitz below
480, Rapid below 1500 and Classical above that boundary; UltraBullet is grouped
into Bullet for this application. Correspondence is explicit source metadata:
the [pinned Lichess exporter](https://github.com/lichess-org/lila/blob/cec1a24e965583b69cbb9790735021bf43817ecc/modules/game/src/main/PgnDump.scala#L129-L131)
emits days-per-move TimeControl text. The
[internal speed enum](https://github.com/lichess-org/scalachess/blob/b3d8b8d9efb91c404629ed05cddc8866deb805d6/core/src/main/scala/Speed.scala)
also uses a long-clock boundary; the selected FAQ-based preparation policy
intentionally requires explicit day-per-move metadata for Correspondence.
The versioned policy resolves missing-side ratings, multi-theme records, empty
strata, quota remainders and cap reductions without source-order dependence.

**Alternatives considered**: Using whichever player's rating is present changes
the meaning of known bands. First-listed themes depend on source token order.
Inferring correspondence from missing clocks conflates unavailable metadata with
a known class. Nested quotas overweight sparse groups. Unseeded sampling and
worker-arrival allocation prevent independent reconstruction. Single-record
full rebuilds can waste preparation work; bounded prefix reduction preserves a
reproducible collection without promising maximum possible counts.

## Final resolution research

The owner delegated remaining decisions on 2026-10-16. Selected policies are in
[the resolution document](decisions.md); facts below were checked during this
planning pass. No application dependencies or datasets were installed.

### Dataset licensing and schema

[The official Lichess database](https://database.lichess.org/) gives CC0 terms for
standard online games and puzzles. Its **broadcast section overrides that with
CC BY-SA 4.0**. Therefore do not generalize the top-level CC0 statement to
broadcasts. Select titled-master online games from standard CC0 exports, label
them truthfully, and exclude broadcast data. December 2025 standard-game exports
are the selected source snapshot; freeze downloaded file hashes during data
preparation. Online PGN supports WhiteTitle/BlackTitle metadata where available;
exclude games missing required titles from the master subset.

The puzzle export advertised update date is 2026-10-05. The documented CSV adds
optional DailyDate after OpeningTags. Its FEN precedes the first opponent move;
apply Moves[0] before presenting the puzzle and start the accepted solution at
Moves[1]. The plan supports both the older header and this extension. No large
export was downloaded; collection counts and installed sizes remain unmeasured.

[Opening-name data](https://github.com/lichess-org/chess-openings) has an explicit
[CC0 dedication](https://github.com/lichess-org/chess-openings/blob/master/COPYING.txt).
Freeze Git revision a6189a30dc273ccb21fc2536a9a2fefd5592a67a (resolved during this
pass) and actual TSV hashes when preparing data. Do not fetch a moving branch
for reproducible releases. Provider discovery via Stripe Directory returned no
matches for free chess PGN/puzzle datasets; official source documentation supplied
these selections. No payment or provider provisioning occurred.

### Selected stable library versions

Native versions replace former npm and Tauri pins. Resolve one compatible stable
locked graph during implementation; these selections are not a built native app.

| Area | Selected version |
| --- | --- |
| Rust / edition | Existing 1.99.0 / 2024 |
| Bevy / bevy_egui | 0.20.0 / 0.43.1 |
| egui / egui_kittest | Matching 0.36 family / 0.36.0 test harness |
| wgpu | Bevy-compatible 30 family, frozen in Cargo.lock; no separate eframe/wgpu graph |
| winit | Bevy-compatible 0.30.13; direct matching dependency for native attention types, sharing the same resolved instance |
| Tokio / Rayon | Existing 1.53.3 / 1.12.0 |
| shakmaty / pgn-reader | Existing 0.30.2 / 0.29.1 |
| rusqlite / schemars / Serde | Existing 0.40.2 / 1.2.3 / 1.0.229 |
| rfd / directories | 0.17.2 / 6.0.0 |
| Profile activation | interprocess 2.4.5 with `tokio`; pinned std file locks |
| Windows activation security | winsafe 0.0.29 with `advapi`, widestring 1.0.2; target-specific only |
| reqwest / keyring | 0.13.5 / 4.2.0 |
| cargo-packager | 0.11.8, build-time packaging |
| minisign-verify / minisign | 0.3.0 runtime verification / 0.10.0 release tooling |
| Inter | v4.1, retain font/notice |

Bevy and egui are MIT/Apache-2.0; bevy_egui is MIT; cargo-packager MIT/Apache-2.0;
minisign-verify and minisign MIT. Existing chess copyleft and NNUE provenance
gates remain. Inspect resolved transitive licenses and features. No Tauri,
eframe, Node or Bun package is a final runtime prerequisite. Keep Bevy asset
watchers/processors, dynamic linking and development-only features out of release
builds; select features required by the product and verify resulting native
window/audio/portal dependencies. Approved PCM WAV sounds require the WAV
decoder feature rather than assuming the default Vorbis audio profile suffices.
Embedded fonts, LUTs and other resources retain their own notices; Inter and
any enabled default font payloads are not covered by a blanket crate license.

### Native runtime, dialogs and accessibility acceptance

Main constructs Bevy, ports, capacity and owned Tokio runtime before the native
runner. Publish bounded backend results, then issue a native wake. Visible clock
updates and animations use one explicit presentation deadline scheduler. Normal
close/menu/quit waits for pause/save/reap; final AppExit occurs only afterwards.
Power-notification adapters distinguish sleep from focus. Headless tests cannot
establish actual OS quit, sleep, dialog or GPU behavior.

Enable bevy_egui's optional accesskit feature and the relevant Bevy platform
accessibility support, including accesskit_unix for Linux. Plugin integration
uses the Bevy window AccessKit adapter directly and does not automatically merge
ordinary scene-node semantics. The board needs 64 named, stable, visible semantic
square controls as part of the egui tree. Invisible widgets may be omitted.
The pinned plugin does not forward AccessKit ActionRequest to egui; add a narrow
application adapter translating native actions to egui AccessKitActionRequest
after EguiPreUpdateSet::ProcessInput and before BeginPass. Verify roles, focus, activation, square labels, legal moves,
dialogs and throttled announcements with matching-egui semantic tests plus actual
VoiceOver/NVDA/Orca. Tree inspection alone is insufficient evidence of usable
screen-reader interaction.

Use rfd 0.17.2 via the dialog port, creating platform-required dialog handles on
the owning window thread and awaiting through one bounded owned actor. Bevy
window/raw-handle integration replaces former eframe Frame use. Validate macOS
main-thread creation, Linux portals and supported parent handles. Rfd cancellation
results cannot prove that every backend failure has a distinct error.

Use directories 6.0.0 behind the data-path adapter. Preserve the existing
nl.sadra.chess data identity through explicit discovery; no silent empty-library
migration. Resolve read-only Bevy asset roots separately from writable data,
especially macOS Contents/Resources versus Contents/MacOS. Credential buffers
stay outside snapshots, exports and backups; unavailable Secret Service gives
the selected setup/session-only policy.

Sources: [plugin manifest/source](https://github.com/vladbat00/bevy_egui/tree/v0.43.1),
[egui_kittest](https://docs.rs/egui_kittest/0.36.0/egui_kittest/),
[rfd](https://docs.rs/rfd/0.17.2/rfd/),
[directories](https://docs.rs/directories/6.0.0/directories/).

### CI feasibility

[GitHub Actions billing documentation](https://docs.github.com/en/billing/managing-billing-for-your-products/managing-billing-for-github-actions/about-billing-for-github-actions)
confirms private-repository quotas and potential billing beyond included usage.
Verify hard stops for compute, artifact and release storage before activation.
Use standard included runners only and immutable verified action commits. Native
egui does not remove the zero-cost gate. No account settings, signing keys,
workflow activation or release publication occur during planning.

### Rules and exactness risk

[FIDE Laws 5.2.2 and 6.9](https://handbook.fide.com/chapter/E012023) concern
checkmate possibility by any legal sequence, including cooperative play. An
engine's best-play WDL draw, or an ordinary insufficient-material shortcut, is
not equivalent. The selected proof adjudicator must return Unknown rather than
invent a terminal result when its bounded proof is incomplete; practical
coverage/latency remains an explicit release blocker until demonstrated.
This is a selected correctness boundary, not a claim of a solved efficient
algorithm for every arbitrary constructed position.

Polyglot entry-format reference: [python-chess v1.11.2 polyglot.py](https://github.com/niklasf/python-chess/blob/v1.11.2/chess/polyglot.py)
uses the big-endian QHHI record layout: 64-bit key, 16-bit move, 16-bit weight,
32-bit learn field. This confirms the 65535 unsigned weight ceiling; it is a
reference for fixtures, not a selected Python runtime dependency or copied code.
