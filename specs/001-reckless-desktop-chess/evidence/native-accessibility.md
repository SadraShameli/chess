# Native accessibility implementation evidence

This increment implements the native action bridge and camera-independent
keyboard portion of T044/T307. Its written obligations are
[FR-012 and SC-008](../spec.md), the
[native accessibility contract](../contracts/ui-experience.md#accessibility-and-state)
and the camera-orientation policy in that same contract. It does not accept the
remaining OS accessibility, idle CPU or complete native foundation gates.

## Keyboard regression

The previous semantic grid selected arrow-key neighbors by projecting squares
through the freely rotated camera. Two meaningful regressions failed before
the repair. At White orientation with 90-degree yaw, Arrow Up from d4 focused
c4 instead of d5. Down from the White a1 edge moved to b1 instead of staying at
the edge. The tests use the actual egui focus/action path and explicit expected
destinations, with each fixture dropped before final assertions.

The fitted camera frame now retains its typed White/Black orientation. The
direction owner computes checked neighboring coordinates from that orientation,
independently of yaw. The former projection-search implementation and unused
direction vector API are removed. Both regressions and all eight existing
board presentation cases passed: 10 passed, 0 failed, 0 ignored.

## Native action bridge

The production host installs `DesktopNativeAccessibility`. One message cursor
owns action consumption, and the existing bounded presentation input owner
retains at most 64 requests. Collection binds board requests to the ready scene
generation before application updates; delivery checks the current ready
generation immediately before the egui pass. Missing generations and saturation
remain typed refusals, logged at the native boundary and presented through the
host's existing failure surface. A refused board action does not discard an
unrelated valid native control.

Requests wait while the primary context is unavailable. Requests published after
the egui pass request a subsequent frame, without replaying already consumed
requests. The adapter uses actual Bevy action messages, egui inputs, schedules
and mounted widgets. Its generation-change test uses the actual `SceneRenderer`
and camera transition. The existing renderer fixture is shared with native host
puzzle cases; no production test API or alternate renderer implementation was
introduced. The test explicitly despawns the renderer before final assertions.

Six adapter cases and all existing native library cases passed together:
106 passed, 0 failed, 0 ignored, 451 filtered. The initial headless adapter run
failed because its fixture dropped egui texture deltas without consuming them;
the fixture now clears those deltas after each actual pass. That setup failure
is not represented as a production semantic regression. The camera-update test
system uses Bevy's actual fallible-system result type.

## Verification records

All commands use the existing pinned Rust wrapper. Logs are under
`/tmp/claude-chess-storage-orm-20261017/`.

| File | Result | SHA-256 |
| --- | --- | --- |
| `keyboard-orientation-red.log` | 0 passed, 2 failed before repair | `247c3ec4c5f8f2c96035c8ecd1bfb8508f449ccc14950a98eefc51f4cae0cc93` |
| `keyboard-orientation-green.log` | 10 board cases passed | `0abdb04677cf28bdd01b71df4655d87a38853c28332c81594a9d1439f4bf583c` |
| `native-accessibility-tests-fixed-fixture.log` | Initial five adapter cases passed | `7162690ab113697eb42146c5e08df86c483897568ea815ef57acd6df2f1d4770` |
| `native-accessibility-and-host-tests.log` | Six adapter and 100 existing native cases passed | `1e64fcec8cf69620c30826cc83740603e20159b46412293cf57b5d994df33e18` |

After the cursor consolidation, the ordered `make verify` gate is running in
`native-accessibility-make-verify.log`. Its completion is not inferred from the
earlier focused results. The existing macOS compact-unwind linker warning is
unsuppressed. Native GPU/AT execution, first-action wake from an idle OS window,
whole-process idle measurements and Windows/Linux acceptance remain unverified.
The pinned Winit action handler queues requests; the bridge's late-message wake
starts when Bevy publishes them. Headless scheduling evidence alone does not
establish the OS callback's initial wake behavior. T044 remains open.
