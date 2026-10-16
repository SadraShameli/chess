# Validation Guide: Reckless Desktop Chess

**Status**: Bevy + egui architecture selected under owner delegation. Native
migration tasks are reconciled in [tasks.md](tasks.md); execution is in progress.
This guide distinguishes
existing implementation from future native acceptance.

## Current checkout

The repository has partial native Rust/Bevy/egui implementation, including
verified engine packaging/startup and original 3D assets. Legacy Tauri/Vue files
remain until the native replacement passes its migration gates.
Tasks T001 through T028 retain their recorded completion. Former T029 staged
changes and former T030 unstaged preparation remain preserved and unaccepted;
[the reconciliation table](tasks.md#pending-task-reconciliation) maps their new owners. Existing
[evidence](evidence/) describes its specific previous-stack checks; it does not
prove native-egui operation or the full product.

Read [the actual Makefile](../../Makefile) before using commands. At this point
`make run` now launches the native Bevy/egui executable in debug mode. Its
current shell is an integration milestone, not a complete playable product.
`make run ARGS="--help"` lists supported arguments; use
`make run ARGS="--profile-directory /tmp/chess-development"` for an isolated
profile. Invalid arguments and help return before opening application services.

## Prerequisites after migration

- The pinned Rust toolchain and locked native UI/scene dependencies.
- Windows 11 x86-64, macOS 14+ Apple Silicon, or Ubuntu 22.04+ x86-64;
  Linux validation includes X11 and Wayland.
- Working hardware graphics using Bevy's selected native backend, a
  1280x800 logical-point usable window and baseline 8 GiB RAM/four usable threads.
- Packaged original GLB/font/audio/shader resources, matching verified Reckless
  binary/embedded model and starter datasets. Test data and user data are separate.
- Native desktop dependencies/dialog services and optional credential service
  for private updates. Chess itself requires no network or update credential.

## Build and run validation after migration

The migration must implement the native definitions of these existing entry
points before they establish native acceptance:

```sh
make verify
make run
make run ARGS="--help"
make native-tests
```

`make verify` owns the ordered final checks. Native commands use Cargo/Rust
instead of frontend toolchains. `make run` launches a debug development build; no empty
argument separator is appended without ARGS. The actual app argument surface
must be documented when implemented, including the above help smoke check.
The native test command must start the production Bevy renderer on the OS main
thread with temporary storage and deterministic orchestration. Do not interpret
headless widget tests as a substitute for native graphics/platform execution.

The planned `native` target at `tests/native.rs` has `harness = false` and
`test = false`. Default Cargo tests run the hermetic suites and library
doctests. Native lint/build checks select this target explicitly in a separate
invocation, because `--all-targets` does not include it. After migration,
`make native-test-build` compiles/links the debug harness without running it;
`make native-tests` runs it on the OS main thread. Required native prerequisites
missing from a test machine produce a failed check and an outstanding evidence
gate. See [the target-selection decision](research.md#native-test-target-selection).

The existing engine-only tool remains separately useful where its verified
resources exist:

```sh
cargo run --locked --bin chess-tools -- engine-check
```

Expect verified package/model identity, readiness and clean process shutdown.
This checks an engine boundary, not playable native UI or complete capability
coverage. Required real-engine scope remains in [the capability matrix](contracts/reckless-capabilities.md).

## Required end-to-end scenarios

| Area | Validation and expected result |
| --- | --- |
| Native startup | Offline app opens at selected logical size with original 3D pieces, correct font/theme and actual adapter/backend shown in diagnostics |
| Complete play | Standard/Chess960/DFRC against Reckless and local two-player; special moves, every clock family, draw claims/results, premoves and exact takebacks match [the spec](spec.md) |
| Concurrent games | All games' clocks/turns stay visible in overview; admission, equal shares, rebalance and hash cap obey [capacity decisions](decisions.md#capacity-and-memory) |
| Lifecycle | Pause/resume, focus loss, sleep, engine/renderer/save faults, close/quit and crash recovery preserve state and explicit-resume rules |
| Profile ownership | Simultaneous/second launches activate the existing owner without duplicate writers, clocks or engine allocations; unreachable owners refuse takeover under [the profile policy](decisions.md#local-profile-ownership-and-activation) |
| Analysis/workbench | Every usable matrix capability, bounded streams, stale-result rejection, stop/cancel, tablebase setup and isolated diagnostics |
| Library/study | PGN/FEN round trips, invalid imports, duplicate preview, notes/variations/markings, folder/tags/Trash, positions and editor-save prompts |
| Reports | Three-second default/mainline-only automatic reports; pause for live games; versioned scoring, incomplete results and clickable alternatives |
| Opening data | Accurate corpus filters/denominators/deduplication; book weights and study branches; never chooses opponent moves |
| Puzzles | Both timer modes, board-ready start, pause hiding, hints, failed/practice/unlisted/illegal distinctions, skip/timeouts, no-repeat exhaustion and persisted histories |
| Settings/backups | Immediate appearance versus saved settings, safe engine boundaries, JSON merge/migration, no paths/credentials in exports, daily/manual backups and confirmed cross-platform recovery |
| Native graphics | Full camera range, picking/drawing/drag/promotion, resize/DPI/clipping, lighting/shadows/materials, quality adaptation, on-demand idle and retry without game resume |
| Accessibility | 64 semantic keyboard squares, modal focus return, palette/remapping, reduced motion, both themes and real VoiceOver/NVDA/Orca |

Use [desktop messages](contracts/desktop-ipc.md), [UI experience](contracts/ui-experience.md),
[study tools](contracts/study-tools.md) and [the data model](data-model.md) as the
owning contracts. These scenarios retain all FR-001 through FR-026 and SC-001
through SC-014; this guide does not shorten the approved scope.

## Repeated-launch and handoff validation

Use isolated temporary local data and controlled process barriers in the native
harness. Launch the production executable twice against the same fixture;
its test-only profile selection must not become a new shipping profile feature.
Record writer/engine admission counts and state revisions, not just window count.

1. Race first launches before owner metadata exists. Exactly one obtains the
   lease; the other activates or reports Starting/Busy within the selected bound
   without opening storage or initializing a full worker topology.
2. Repeat with the owner starting, migrating, restoring, minimizing or holding a
   quit confirmation. A failed save or canceled quit retains the same lease and
   newest in-memory state. Replacing database files must not replace the lock.
3. Make the live owner unreachable and leave stale/invalid endpoint metadata.
   The second process refuses ownership after timeout. Then terminate the owner
   and launch again: only actual exclusive acquisition admits paused recovery.
4. Exercise wrong-user peers, malformed/oversized frames, stale generations,
   unknown versions, Windows server substitution and connection floods. Reject them within documented bounds
   without mutation or extra unbounded connection/flush workers.
5. Request activation on macOS, Windows, X11 and Wayland. Separate observed
   native focus from attention requested; test OS/compositor denial or missing
   activation support and visible manual-selection guidance. No activation
   resumes a game/puzzle or dismisses its pending confirmation.
6. Stop at the parent-exit/helper-acquisition boundary and launch another copy.
   The durable intent blocks ordinary initialization. Repeat with helper failure,
   interrupted replacement, delayed system-installer completion and failed
   relaunch migration; preserve one coordinator and explicit safe recovery.
7. Lose the helper-ready acknowledgment, cancel preparation and later quit
   normally. The Prepared/revoked generation must never install. Crash before
   and after durable commit, and terminate the helper while its installer keeps
   running; no timeout, free lock or stale PID permits intent clearance or Retry
   before confirmed installer quiescence/outcome.

These validate Story 9.8, FR-004 and SC-006. They are required future native
scenarios, not checks executed by this documentation workflow. The
[activation](contracts/desktop-ipc.md#local-activation-interface) and
[installation](contracts/packaging.md#profile-ownership-during-installation)
contracts own detailed behavior and limits.

## Native performance and packaging

Record optimized target measurements against specification budgets, with fixed
fixtures, actual hardware/backend/driver, one-worker and maximum-budget searches,
whole-process/child/GPU memory and active versus idle frame scheduling. No
throughput or smoothness claim follows from screenshots or framework selection.
Verify the phase-based CPU policy at four usable threads and at detected maximum;
there must be no unowned Bevy asset/pipeline work during engine-reserved phases.

Test clean offline installation and resource resolution for unsigned Windows
NSIS, ad-hoc-signed ARM macOS .app/.dmg and Linux .deb/AppImage. Validate actual
native dependencies rather than bundling obsolete WebView prerequisites.
AppImage's maintained stable tooling and reproducibility remain an explicit
release gate. Installer sizes must meet GitHub's per-asset limit independently
of the client's download cap. [Packaging](contracts/packaging.md) owns details.

## Starter preparation validation

Use frozen local miniature source fixtures before preparing the large starter
exports. Follow [subset selection v1](decisions.md#starter-subset-selection-v1)
and the [study acceptance cases](contracts/study-tools.md#starter-preparation-reproducibility).
Permute candidate arrival order and compare one-worker versus maximum-worker
selection: identities, stratum order, prefix lengths and selection digests must
agree. Cover rating/time/theme boundaries and deterministic cap reductions;
invalid or impossible preparation must fail explicitly. Large-source checksums,
actual collection counts and installed bytes are later preparation evidence.

## Private update validation

Use a deterministic local transport fixture for missing/expired credentials,
signed-manifest verification, asset identity/digests, wrong target/version,
credential-safe redirects, deadlines/cancel/network-cost approval, durable cache,
manual Retry, save/backup prerequisites and native helper handoff. Completed
payloads are revalidated before installation. Failures preserve data and expose
selected recovery actions; no automatic binary rollback is promised. Native
installed-target tests then verify actual installer/relaunch behavior.

No workflow activation, payment, signing key, secret, version tag or release is
created by this planning revision. Hard no-overage controls and all outstanding
[release gates](decisions.md#release-evidence-register) must pass before release
support is claimed. The reconciled [tasks](tasks.md) retain historical IDs and
include the native migration. Implement them and record actual results here.
Use Bevy ScreenshotCaptured for native image completion, matching
egui_kittest 0.36.0 for semantic checks and explicit OS accessibility actions
through the plugin adapter. Eframe and WDIO are not the native test host.
