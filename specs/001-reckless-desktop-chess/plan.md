# Implementation Plan: Reckless Desktop Chess

**Branch**: `001-reckless-desktop-chess` | **Date**: 2026-10-16 | **Spec**: [spec.md](spec.md)

**Input**: Existing feature specification, request for native egui UI and native
GPU backend selection, followed by delegated scene-engine selection and the
clarified one-writable-owner local-profile policy.

This revision also resolves the analysis findings for mode-specific takeback
confirmation, native-test execution, reproducible starter selection and stale
workflow status. Existing task IDs and completion history are retained.

**Status**: Bevy + egui selected under the owner's delegated recommendation policy.
Phase 0 research and Phase 1 design revised. Existing implementation covers
T001 through T028 of the former stack. Former T029 is staged and former T030
preparation is unstaged; both are preserved and neither is accepted as completed.
The task workflow has reconciled the native work in [tasks.md](tasks.md), retaining
completed IDs and mapping former pending IDs. Native migration and platform
acceptance remain implementation work. Planning changes no source/configuration.

## Summary

Build the complete offline chess application using **Bevy + egui in Rust**.
Retain Reckless play, local two-player, Standard/Chess960/DFRC, concurrent games,
full analysis/capability workbench, library, PGN/FEN, notes/variations, editor,
reports, explorer, study-only book, puzzles, Settings, backups and private OTA.
No approved product behavior is removed by the frontend change.

The owner could not choose among the researched engines. Select Bevy as the
application/3D host and bevy_egui for native Rust desktop panels under the earlier
instruction to pick the best recommendation. This replaces the proposed eframe
host. Bevy/wgpu selects supported native APIs, including Metal on macOS and
Vulkan/Direct3D on other targets. No Vulkan-only mask, browser scene or required
Vulkan-to-Metal portability layer is selected. The decision is recorded as an
assistant selection under delegation, not a quoted explicit Bevy answer.

The fully three-dimensional board retains the original GLB ivory/ebony sculpted
pieces and premium graphite/emerald dark and warm light styling. The native scene
integration and exact dependency selections are documented in
[research](research.md#native-egui-renderer-and-integration).
Reckless remains a separate native process communicating through UCI pipes.

This selection supersedes Tauri, Vue, TypeScript, Vite, Tailwind, shadcn-vue,
Bun app tooling, ESLint, Prettier and Three.js/WebGL2. They remain in the current
checkout until migration implements and verifies their replacement. Existing
[implementation evidence](evidence/) describes the previous stack; it does not
prove native-egui acceptance. Preserve the staged and unstaged implementation
work while deriving reusable backend contracts.

## Technical Context

| Area | Selected design |
| --- | --- |
| Language/Version | Pinned Rust 1.99.0, edition 2024, one root Cargo package |
| Native UI | Bevy 0.20.0 + bevy_egui 0.43.1, matching egui 0.36 family; centrally styled widgets/view models; explicit AccessKit integration |
| Graphics | Bevy scene/assets/PBR/cameras/shadows/picking through its compatible wgpu graph; automatic native backend selection; no separate renderer/device or eframe dependency |
| Engine | Reckless v0.9.0, commit `0e92358f5acd66e5ac77b1bf558202e47c515435`; verified packaged native UCI child |
| Rules/notation | Existing shakmaty 0.30.2 and pgn-reader 0.29.1; separate proof-adjudication boundary |
| Storage | Existing bundled rusqlite 0.40.2, serialized storage actor, WAL/FULL transactions, separate starter indexes |
| Internal interface | Typed in-process Rust commands/events, checked revisions and correlation; no UI JSON serialization or generated TypeScript |
| Profile ownership | Standard-library exclusive file lease before mutable startup; interprocess 2.4.5 activation-only local sockets; authenticated user scope, bounded requests and durable installer intent |
| External formats | Serde, bounded validated PGN/FEN/CSV/Polyglot, versioned settings/backup/update schemas; persistence compatibility retained |
| Concurrency | One explicitly owned Tokio 1.53.3 runtime, existing capacity coordinator/Rayon 1.12.0 executor and engine leases |
| Native services | Injected rfd dialogs, native data paths, keyring 4.2.0 credentials, network-cost/lifecycle adapters; cargo-packager 0.11.8 and explicit installer helper |
| Testing | Default Cargo domain/contract/concurrency and headless semantic/adapter suites; separately selected production Bevy GPU/native harness, compiled and linted explicitly; OS accessibility/installer acceptance |
| Target Platform | Windows 11 x86-64, macOS 14+ Apple Silicon, Ubuntu 22.04+ x86-64 on X11 and Wayland |
| Project Type | Native offline desktop application, no browser runtime |
| Delivery | cargo-packager 0.11.8: unsigned NSIS .exe, ad-hoc-signed .app/.dmg, .deb/AppImage; reqwest 0.13.5 + minisign-verify 0.3.0 for private signed OTA |
| Performance Goals | 60 fps active scene target, on-demand idle frames, p95 feedback <=100 ms, game start <=3 s, p95 engine stop <=1 s |
| Constraints | 8 GiB RAM/four usable logical threads baseline; 6 GB installation space plus staging; starter data <=2,000,000,000 installed bytes; zero payments |
| Scale/Scope | Complete 13 user journeys, FR-001 through FR-026, SC-001 through SC-014; multiple live games subject to shared CPU/memory admission |

The stable versions in research are design pins. Adding dependencies requires
actual locked compilation and API/license verification. No installed native-egui
compatibility, package execution or performance result is asserted here.

## Behavioral ownership and native execution

Bootstrap first resolves the existing local profile and obtains its
[exclusive profile lease](decisions.md#local-profile-ownership-and-activation).
Inspect pending installation state before database open, migration or recovery.
A contending launch performs only bounded activation of the existing owner,
then exits or explains Busy; it initializes no games, engines or full worker
topology. A timeout never authorizes ownership. The lease outlives all mutable
services, including canceled/failed quit and data replacement. This adds no
user-facing profile-management scope.

After admission, one root constructs a Bevy App, native service adapters, typed application ports,
one owned Tokio runtime and the shared capacity owner. Bevy owns the native
window/event loop, render device/queue, assets and scene scheduling. bevy_egui
owns widget integration. A board camera renders to an image displayed in an egui
widget, leaving native panels around the board. The application retains one
logical board viewport with consistent hit testing, scaling and clipping.

Authoritative board/game/clock/search/storage/report/puzzle owners remain plain
Rust components. No domain type imports Bevy, egui, ECS entity IDs or graphics
handles. Presentation/scene adapters map validated snapshots to Bevy entities;
ECS is for view resources and scheduling, not a second rules implementation.
Owning-type system methods coordinate typed messages. Repeated egui passes and
repeated frame updates do not duplicate commands, autosaves or engine searches.
The [message contract](contracts/desktop-ipc.md) retains expected revisions,
request identities, bounded delivery and stale-result rejection.

Tokio owns application actors and async UCI/network operations; UI and Bevy
systems never block on those results. Native wake notifications connect actor
completion to reactive Bevy updates. CPU imports/indexing/proofs remain on the
existing executor. Configure Bevy task pools and schedule parallelism through
[the selected capacity partition](decisions.md#native-engine-capacity-partition)
before plugin initialization, including Bevy asset preparation, render extraction,
Tokio/blocking work, Rayon and engine leases. Framework defaults must not create
independent full-capacity worker pools. Resource allocation is a design budget,
not a measured utilization result.

Retain reactive/on-demand scheduling: animation, camera input, clocks, pending
GPU work and backend results schedule needed updates, while unchanged scenes
stop rendering. Avoid treating a reactive event loop as proof that an active
render sub-app or repeating timer is idle. Measure the complete process.

Disable automatic close on the main window. Normal close, quit, menu and update
requests route through one confirmation/pause/save/stop/reap state machine while
the event loop stays alive. Use WindowPlugin close_when_requested=false and ExitCondition::DontExit;
only successful completion permits final AppExit::Success. A native quit/menu
adapter intercepts macOS Cmd+Q confirmation before termination; an idempotent
bounded cleanup resource owns its required handles for nonreturning exit paths.
Failures keep the newest state/window available. OS termination paths need an
idempotent bounded cleanup fallback and crash recovery; no framework can promise
to save on force kill. Native platform power adapters detect sleep separately
from window focus: focus loss leaves live clocks running, hides/pauses puzzles
and mutes audio; sleep interrupts games until explicit Resume.

Use Bevy's explicit render error handler/policy and device-generation invalidation.
The default log-and-exit path does not satisfy preserved game state plus Retry.
A fallible initialization adapter must handle missing adapters/devices before
default panic-based initialization. Runtime errors interrupt games once, stop
clock/search advancement and expose Retry. Retry may reconstruct render/window
resources using preserved CPU/application state; it never resumes play. Native
GPU startup/recovery behavior is a first migration acceptance gate.

## Existing work and migration sequence

1. Preserve the current implementation and pending T029/T030 diffs. Inventory
   backend code and test contracts independently of the obsolete UI shell.
2. Use the reconciled [tasks.md](tasks.md), which preserves completed task
   IDs/evidence and replaces obsolete remaining UI/packaging work without
   dropping requirements. This revision aligns affected descriptions only;
   it creates no new tasks and accepts no implementation as complete.
3. Establish a locked native UI/scene/accessibility compatibility slice and native
   test harness, including profile arbitration before mutable startup and native
   activation on all targets. Reuse original GLB resources, board/clock logic, UCI lifecycle,
   capacity coordinator and storage actor. Decouple Tauri-specific bootstrap,
   dialogs, paths and shutdown from reusable application orchestration.
4. Connect typed Rust message ports and native widgets; migrate board interaction,
   themes, player bars, clocks, workflows and accessibility with contract tests.
   An internal transition build may retain both shells, but only Bevy is the
   final window/event-loop/render host. Avoid duplicate rules or state ownership.
5. Once native parity and relevant gates pass, remove obsolete frontend packages,
   generated browser validators, Tauri build/config/capabilities/plugins and
   runtime dependencies in the same migration scope as their final callers.
   Replace the Bun-based GLB generator with a reproducible native tool while
   preserving asset identity or recording an explicitly verified replacement.
6. Adapt Makefile verification/build/run/native-test commands, packaging and
   update adapters. `make run` launches the optimized native application and
   appends an argument separator only when ARGS is nonempty.
7. Resume the full remaining product implementation and platform acceptance.
   A native shell migration is an increment, not completion of the chess app.

## Native test selection and data preparation

Register `tests/native.rs` as the explicit `native` test target with
`harness = false` and `test = false`. Cargo's default test selection retains
hermetic suites and library doctests without executing a native event loop.
The target is still required: `make native-tests` selects it by name and runs
the production Bevy host on the OS main thread with isolated fixture storage.
Missing required graphics or platform services fail that command and leave
the corresponding acceptance gate outstanding.

Cargo's `--all-targets` omits a `test = false` target. Native migration therefore
adds a separate named `--test native` selection to lint and nonexecuting
build/profile checks, without combining it with `--all-targets`. Keep the
ordinary all-target checks. `make native-test-build` performs the optimized
named harness build without executing it; `make verify` never runs GPU/OS
acceptance implicitly. Harness controls belong only to the test executable,
and installed packages contain no test target or automation listener. The
[research decision](research.md#native-test-target-selection) records the Cargo
reference and exact pinned-source behavior.

Starter preparation follows the versioned
[subset-selection algorithm](decisions.md#starter-subset-selection-v1).
The preparation manifest records source hashes, normalized strata, ordered
selection and cap reductions. Fixtures reproduce selection from permuted input
and cover missing metadata, class boundaries, theme ties and exhausted strata.
The [study contract](contracts/study-tools.md#starter-preparation-reproducibility)
defines validation obligations; acquisition and measured installed size remain
implementation evidence.

## Visual acceptance and performance

[Presentation decisions](decisions.md#presentation-assets-and-input) retain exact
colors, Inter 4.1, camera fit, quality tiers, input, audio and adaptation policies.
Former CSS-pixel layout dimensions mean native logical points at scale 1.0;
1280x800 minimum and 1440x900 initial/reset size preserve apparent size under
OS scaling. Use one typed theme/layout owner for egui and the 3D scene.
Retain 20-degree initial tilt, full-board camera bounds, 180 ms piece/auto-flip
and 160 ms panel transitions with reduced motion. Native GPU compatibility and
minimum reference hardware require actual tests.

Measure p95 input feedback <=100 ms, active frame pacing toward 60 fps,
start <=3 seconds, p95 stop <=1 second with 2-second safety deadline, zero stale
mutations in 1,000 controlled lifecycle cycles and idle CPU <1% of one logical
core under the defined idle fixture. Static scenes stop frames; visible clock
changes and active animation schedule only needed repaints. Compare optimized
one-worker and maximum-capacity workloads and record whole-process plus child
memory, GPU resources and cache memory separately. Keep shared CPU admission,
equal engine search shares and 10%-of-startup-available-RAM hash cap of 2 GiB.
No speed improvement over the former UI is claimed without measurements.

The [acceptance measurement protocol](decisions.md#acceptance-measurement-protocol)
defines exact timing boundaries, reference conditions, per-run percentiles,
sample counts, whole-process idle CPU and one/max-worker comparisons. Native
validation must use that protocol; unspecified timing windows are not evidence.
Input geometry invalidation, modal/reduced-motion behavior and GPU-independent
recovery controls follow [the UI contract](contracts/ui-experience.md).
The [desktop contract](contracts/desktop-ipc.md#runtime-ownership) defines finite
command admission and urgent-control capacity even when ordinary queues are full.

## Packaging and release boundaries

The native package has no WebView2, WebKitGTK or browser CSS prerequisite. Package
the actual native graphics/window/dialog dependencies and portable engine profiles
for each declared minimum platform. Test NSIS, ad-hoc-signed .app/.dmg, .deb and
AppImage on clean target systems, including Linux glibc and graphics drivers.
Offline chess includes all original assets, fonts, engines and starter datasets.

Replace the Tauri updater with the native authenticated download/signature and
installer adapters in [packaging](contracts/packaging.md). Preserve every selected
private-update timeout, metered-network approval, cancellation, cache, backup,
installation and recovery policy. Bevy does not supply the selected package/update policy.
A standalone updater needs platform-specific installation/relaunch acceptance.
The [helper ownership contract](contracts/packaging.md#profile-ownership-during-installation)
retains a durable intent across parent exit and helper lock acquisition; a new
launch cannot start migration or games in that gap.

No paid signing, Apple membership, service or runner is selected. Version-tag
publication stays gated on verified included-allowance and hard billing stops.
No workflow activation, credentials, release, tags or external conveyance occurs
in this planning task. Existing model-license, exact adjudication and platform
acceptance gates remain in [the evidence register](decisions.md#release-evidence-register).

## Constitution Check

[Constitution v1.1.1](../../.specify/memory/constitution.md) incorporates
[the canonical catalog](../../.specify/rules/README.md). Policy remains there.

| Rule | Before research | After Phase 1 design |
| --- | --- | --- |
| [Code quality](../../.specify/rules/code-quality.md) | PASS: artifact-only revision with pending implementation preserved | PASS: canonical policy linked; no source/config changes or duplicate rules |
| [Verification](../../.specify/rules/verification.md) | PASS: existing feature/branch selected by setup-plan | PASS for design: explicit native-target compile/lint selection preserves default Cargo tests/doctests; document/link/ID checks apply; implementation gates remain unperformed |
| [Architecture](../../.specify/rules/architecture.md) | PASS: retained domain ownership and effect ports | PASS: one composition root and exclusive profile lease precede mutable services; explicit runtime/native adapters, UI emits intents |
| [Structure](../../.specify/rules/structure.md) | PASS: retain root package | PASS: native presentation and scene components with downward dependencies; final separate web package removed |
| [Rust style](../../.specify/rules/rust-style.md) | PASS: existing typed board/clock/engine contracts reusable | PASS: component-prefixed Rust values/messages; external deserializers preserve validated invariants |
| [Performance](../../.specify/rules/performance.md) | PASS: optimized native UCI and one capacity owner retained | PASS: bounded delivery, nonblocking UI, shared capacity/phase policy, repaint-on-demand and selected measurement budgets |
| [Testing](../../.specify/rules/testing.md) | PASS: retain domain fixtures and task history | PASS: default hermetic suites separated from required explicit GPU/OS acceptance; selection reproducibility and mode-specific takeback assertions assigned to existing tasks |

No constitutional exceptions. Post-design review includes the selected Bevy
adapter, plain Rust domain and shared capacity partition. Platform,
accessibility, resource and exactness acceptance are evidence gates, not waivers.

## Project Structure

### Documentation (this feature)

Existing spec, decisions, research, data model, review, quickstart, contracts,
checklists and evidence remain under `specs/001-reckless-desktop-chess/`.
The subsequent task workflow reconciled [tasks.md](tasks.md), preserving historical
completion and pending implementation inputs. It executes no implementation tasks.

### Source Code (repository root)

Proposed final layout; existing reusable modules remain in place. New modules
are created only as their owning feature is implemented.

```text
Cargo.toml
Cargo.lock
src/
  main.rs
  lib.rs
  desktop/
    bootstrap.rs
    application.rs
    messages.rs
    profile/
    native/
  presentation/
    app.rs
    theme/
    board/
    game/
    analysis/
    library/
    editor/
    report/
    opening/
    puzzle/
    settings/
    workbench/
  scene/
    renderer.rs
    camera.rs
    assets.rs
    picking.rs
  board/
  game/
  clock/
  engine/
  capacity/
  storage/
  notation/
  report/
  opening/
  puzzle/
  workbench/
  update/
tests/
  domain.rs
  engine.rs
  storage.rs
  desktop.rs
  presentation.rs
  scene.rs
  update.rs
  native.rs
  fixtures/
benches/
resources/
  engine/
  pieces/
  fonts/
  sounds/
  datasets/
tools/
  main.rs
  engine/
  assets/
  packaging/
```

**Structure Decision**: One Rust package, native UI and scene as outer adapters.
Scene consumes board presentation snapshots, not storage or engine clients.
The application consumes domain ports; concrete OS/process/database/GPU adapters
are wired in bootstrap. Source tests mirror their owners. `ui/`, Tauri config
and browser contract tooling are migration inputs, excluded from the final layout.

## Phase 0: Research

[Research](research.md) resolves the native shell, compatible renderer, async
and profile ownership, authenticated activation, testing/accessibility, packaging and updater choices. Existing engine,
data, report/puzzle and exactness findings are retained. No owner questionnaire
is reopened. Sources distinguish actual upstream facilities from project
integration work and target-system evidence still required.

## Phase 1: Design and Contracts

- [Data model](data-model.md): native ownership, typed message values and lifecycle.
- [Desktop messages](contracts/desktop-ipc.md): in-process operations, bounded streams and the separate activation-only local protocol; filename retained for stable document references.
- [Engine protocol](contracts/engine-protocol.md) and [capabilities](contracts/reckless-capabilities.md): unchanged native UCI responsibilities.
- [UI experience](contracts/ui-experience.md): native widget, 3D, focus, sizing and accessibility acceptance.
- [Study tools](contracts/study-tools.md): unchanged full product scope through Rust message ports.
- [Packaging](contracts/packaging.md): native dependencies, private signatures and installer handoff.
- [Decisions](decisions.md) and [review](review.md): selected technical decisions and latest owner stack override.
- [Quickstart](quickstart.md): current implementation boundary and future native validation commands/scenarios.

All FR/SC identifiers and 13 user journeys are retained. Phase 0 research and
Phase 1 design are complete for this revision, including Story 9.8 / FR-004 /
SC-006 repeated-launch acceptance. Task reconciliation is complete, and the
affected existing task descriptions are aligned with this design. Native source
migration and full platform/release evidence remain outstanding.

## Complexity Tracking

No constitutional exceptions. Bevy owns the window/input/render loop and
supplies reusable scene facilities; bevy_egui supplies application widgets. The application still owns chess
behavior, private update orchestration and native installer safety. Engine
subprocesses and isolated workbench children remain justified by upstream UCI
and noninterruptible diagnostic behavior. Full product implementation remains
required despite the frontend migration.
