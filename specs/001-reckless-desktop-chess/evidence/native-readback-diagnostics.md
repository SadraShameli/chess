# Native compatibility readback diagnostics

## Status and scope

This is a diagnostic-only source increment based on
`74a8c314e8646cc7ed3a2a6573dd268e7937fe6c`. The new diagnostics are **UNRUN**:
no Cargo command, compilation, test execution, Clippy, native execution, or CPU
diagnostic ran in this implementation lane. The previously observed optimized
compatibility failure remains the actual red result. This increment establishes
neither a repair nor native acceptance.

The owned paths are `tests/native/compatibility.rs` and this evidence file.
Production code, dependencies, vendor sources, test roots, and build commands
are unchanged. The default compatibility fixture does not compose
`DesktopNativeHost`, `SceneRenderer`, or production renderer recovery; its failure
does not justify changing those components.

## Frozen failed attempt

The actual attempt was at
`294ae72ae5da65813b795fb37bdfc2fb3f01dfe8`. A source comparison against the current
base found no changes in `tests/native/compatibility.rs`, `tests/native.rs`,
`tests/native/asset_comparison.rs`, `Cargo.toml`, `Cargo.lock`, `Makefile`, or
`rust-toolchain.toml`.

The parent executed exactly:

```sh
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make native-tests NATIVE_PROFILE=release
```

The original log is
`/tmp/claude-chess-pgo-diagnosis-20261016/current-native-compatibility-release.log`.
The persistent archive and manifest are under
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/current-native-compatibility-20261017/`.
Both log copies were read and their SHA-256 hashes matched the manifest:
`9f24dd6c3b588dd3b58e8669ca8b034ae323f2b24730cd0b4149ef2caa84f2da`.
The manifest records make exit 2 and native executable exit 1.

The log records Metal initialization on Apple M4 Pro, a loaded original model
and loaded asset/dependency states, 580 UI frames, and 192 screenshot callbacks
before the ten-second deadline. The recorded first and latest readbacks are
`Bgra8UnormSrgb`, 2560 by 1600, with zero first/last bytes, zero RGB maxima,
zero nonblack pixels, zero geometry pixels, and zero text pixels. The window
report is `visible=true`, `focused=false`, `occluded=None`, with matching physical
dimensions. The frozen log exposes the first and latest images' summaries, not
an individual record for every intermediate callback.

## Read-only source findings

The inspected dependency sources are the locked local registry versions:
`bevy_render`, `bevy_camera`, `bevy_material`, `bevy_shader`, `bevy_extract`,
`bevy_app`, and `bevy_window` 0.20.0, plus `wgpu-hal` 30.0.1. Paths below are
relative to each dependency's `src/` directory.

- Bevy `view/window/screenshot.rs:322` (`prepare_screenshots`) prepares the
  screenshot texture and readback buffer from `SurfaceData` configuration and
  substitutes an output attachment. This can happen without an acquired drawable.
  `submit_screenshot_commands`, around lines 580 to 585, skips the window copy
  when the extracted swap-chain view format or view is absent.
  `collect_screenshots`, around line 703, maps every prepared buffer. A callback
  therefore does not establish that the window-copy commands were encoded.
- `wgpu-hal` `metal/surface.rs:354` to `369` checks the hosting `NSWindow`'s
  `occlusionState` and returns `SurfaceError::Occluded` if its visible bit is
  absent. Bevy `view/window/mod.rs:343` handles
  `CurrentSurfaceTexture::Occluded` without logging. Bevy's `Window.visible` and
  `WindowOccluded` message history are separate observations and cannot establish
  drawable acquisition or the absence of this Metal outcome.
- Bevy `view/window/mod.rs:265` also gates drawable acquisition on a matching
  sorted window camera with write output, except for the initial present.
  Missing extracted cameras, missing `ViewTarget` output, or pipelines that are
  not ready remain alternative boundaries to inspect. None was established as
  the cause by the frozen log.

These are source-backed hypotheses about observable boundaries, not a diagnosed
production defect. No raw Metal calls, unsafe code, vendor instrumentation, or
panic interception were introduced.

## Added observations, UNRUN

The compatibility fixture alone installs an observer in `RenderApp`'s `Render`
schedule, inside `RenderSystems::Render`, explicitly after
`RenderSystems::PrepareViews` and before the public `render_system`.
It replaces a single latest typed observation containing:

- The number of primary extracted windows; the first one's main-world entity,
  physical dimensions, `SurfaceData` presence, swap-chain texture/view presence,
  texture format, and view format. More than one primary window is visible in
  the count. `SurfaceData` configuration fields are private in this version;
  their actual configured values are not accessed or reconstructed.
- Counts of matching `SortedCameras` and `ExtractedCamera` entries, matching
  write-output cameras, matching cameras with `ViewTarget`, and matching view
  targets with an output texture or output format. Matching uses the normalized
  window entity, rather than assuming the render-world and main-world entity
  identifiers are equal. An absent primary window leaves camera observations
  absent; an absent sorted-camera resource is distinct from a zero count.
- Optional `PipelineCache` counts for ready (`Ok`), pending (`Queued` or
  `Creating`), and error (`Err`) states. Up to four errors retain the actual
  error text, descriptor label, cache index, render/compute/mesh kind, and typed
  shader-cache failure kind. Each detail has a fixed 1024-byte UTF-8 buffer,
  explicit truncation and formatting-failure flags, and the actual error text
  precedes the label. Additional errors are counted without retaining strings.

Every added numeric counter uses checked increment with a sticky typed
`Overflowed` result. Two cumulative counters record pre-render observations and
visits to the actual `RenderGraphSystems::Finish` set in the public `RenderGraph`
schedule. The finish counter is not inferred from UI updates or screenshot
callbacks.

The bridge follows Bevy's documented `SubApp::take_extract`/`set_extract` wrapper
pattern in `bevy_app` `sub_app.rs:182`. It copies the previous render observation
into the fixture's main-world state at the existing extraction callback boundary,
then calls the original extraction callback exactly once. With the existing
disabled `PipelinedRenderingPlugin`, `SubApps::update` runs that callback before
the next render update. Diagnostic render systems mutate only their own resource;
they do not mutate the UI/window/compatibility state from a render worker. The
bridge uses owned `Copy` values, without a lock, channel, queue, blocking I/O, or
per-frame heap strings. The deadline and successful-report formatting expose the
transferred snapshot; formatting into heap strings occurs only at those terminal
report boundaries. No observation history or per-frame log is retained.

The public APIs and re-exports were checked directly in the pinned sources before
use: window components in `view/window/mod.rs`, camera types in `camera.rs`,
`ViewTarget::out_texture`/`out_texture_view_format` in `view/mod.rs`,
`PipelineCache::pipelines` and `CachedPipelineState` in
`render_resource/pipeline_cache.rs`, descriptor variants in `bevy_material`
`descriptor.rs`, shader failures in `bevy_shader` `shader_cache.rs`, and render
scheduling in `lib.rs` and `renderer/mod.rs`.

## Interpretation limits

The transferred snapshot describes an earlier render update, not necessarily the
frame behind the latest asynchronous screenshot callback. It is overwritten on
each observation and cannot prove that a condition held throughout the attempt.
Before any snapshot has transferred, `render=None` is reported; unavailable
resources remain explicit `None` values.

The graph-finish hook occurs before `render_system`'s screenshot submission,
presentation, and collection code. Its count establishes graph schedule progress,
not drawable presentation, successful screenshot copying, completed GPU work, or
correct pixels.

Bevy's pipeline queue processing system is private. The pre-render observer's
ordering relative to that system is not established by the public constraints.
`PipelineCache::pipelines()` exposes the processed cache, not the private
new-pipeline queue. Its counts can therefore precede queue processing and do not
prove that every newly requested or camera-specific pipeline is ready. Some
shader-cache errors represent retryable unavailable shader/import states.

Absent drawable fields would narrow the observation to the acquisition boundary,
but would not identify Metal occlusion as its unique cause. A present drawable
does not prove successful presentation or correct scene rendering. The parent
must interpret these fields together with camera/output and pipeline evidence.

## Static handoff and remaining verification

Standalone formatting uses the installed Rust 1.99.0 distribution's
`/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt`, with edition 2024, the existing
formatter configuration, and child traversal disabled. Formatting, its check
mode, source-preservation comparisons, whitespace checks, and staged path/patch
checks are the only executable validation performed for this increment. They do not establish compilation,
scheduling validity, native behavior, or a repaired readback.

The ten-second deadline, pixel classifier and thresholds, capture retries,
asset requirements, backend selection, synchronous pipeline setting, disabled
pipelined rendering, task-pool policy, focus behavior, and window flags remain
unchanged. No activation, focus, resizing, movement, or owner-setting change was
used to force a passing result. No diagnostic-assignment tests were added.

The explicit staged binary patch is exported to
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/native-readback-diagnostics.patch`.
Its final SHA-256 and exact two-path scope are included in the implementation
handoff. No commit or push was created.

The parent retains the exclusive compilation/native lane. When available, it
must compile and execute the same command recorded above, retain the new result
alongside the frozen failure, and decide the next diagnosis. Any controlled
regression or justified production repair requires that evidence and remains
outside this source-only increment.

## Parent execution on 2026-10-17

The first named native Clippy attempt stopped on PGN library findings, before
checking this harness. An optimized native compilation then exposed a missing
`ContainsEntity` trait import in the diagnostic query. The parent added that
import. Named native Clippy subsequently passed with `-D warnings`.

The parent reran the unchanged command
`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make native-tests NATIVE_PROFILE=release`.
The named optimized harness built successfully in 1m55s. Its actual execution
again failed the ten-second deadline: Make exited two and the native executable
exited one. It recorded 583 UI frames, 193 zero-filled BGRA readbacks, loaded
assets and an initialized Apple M4 Pro/Metal adapter.

The bounded diagnostics recorded 583 pre-render observations and 583 graph
finishes. The final observation had one primary window with SurfaceData but
no acquired swapchain texture or texture view. Its stored texture format was
BGRA sRGB, while the acquired view format was absent. One matching sorted and
extracted camera was present, with one writer, but no ViewTarget or output
attachment. All 45 pipelines were ready; pending and error counts were zero.
These are final sampled states and accumulated observation counts, not a claim
that every earlier frame had the same resource state.

This narrows the observed failure to unavailable drawable/output resources;
screenshot callbacks alone did not establish submitted texture copies. It does
not establish why acquisition was unavailable or prove a renderer defect.
The pinned Metal occlusion path is a source-backed possibility, not an observed
native occlusion receipt from this run.

A later read-only IORegistry check at 2026-10-17 02:00:09 UTC reported
`screen_locked=true` and `on_console=true`; the login-done key was unavailable.
The check emitted only those visibility fields, without account identifiers.
This post-run observation is consistent with a missing visible drawable but
does not prove causation during the run. No owner setting, focus, backend,
window policy, deadline or pass threshold was changed. Further native retries
are deferred while the desktop remains locked; T049/T051 and platform/GPU
acceptance stay open. No matching native or Cargo process remained after the
attempt.

Exact failed/successful compile and Clippy logs, runtime output, filtered OS
probe source/output and verified SHA-256 manifest are retained under
`.git/parallel-patches/native-readback-diagnostics-integration-20261017/`.
