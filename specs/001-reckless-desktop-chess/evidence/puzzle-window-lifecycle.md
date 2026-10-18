# Puzzle window lifecycle adapter

Final parent integration and gate results are recorded in
[saved workspace integration](puzzle-saved-workspace.md). The source-only
statements below describe the historical child handoffs.

Status at initial handoff: source authored for parent integration on 2026-10-17. Compilation,
Clippy, test execution, final verification and native acceptance remain unrun.
The owner restricted this child to source work while the parent holds Cargo.
No source-ready or native-success claim is made.

The child starts at `43105cc84ea926b2af3fc54a1baa260fcdad68c2`. Dependencies
were read directly from `/Users/sadrashameli/Personal/chess`, including its
uncommitted service, panel and application sources. Those changes were not copied
into the child checkout. The parent owns module registration, composition,
renderer acknowledgements, application admission and final exit.

This patch owns only:

- `src/desktop/native/puzzle_window.rs`
- `tests/desktop/native/puzzle_window.rs`
- This evidence note

The applicable policy is the [canonical rule catalog](../../../.specify/rules/README.md),
including [code quality](../../../.specify/rules/code-quality.md),
[verification](../../../.specify/rules/verification.md),
[architecture](../../../.specify/rules/architecture.md),
[structure](../../../.specify/rules/structure.md),
[Rust style](../../../.specify/rules/rust-style.md),
[performance](../../../.specify/rules/performance.md) and
[testing](../../../.specify/rules/testing.md).
This contributes the native-window slice of T178/T180/T190/T191/T193 in
[tasks](../tasks.md). It does not close those tasks or alter their markers.

## Integration contract

`DesktopNativePuzzleWindow<P: DesktopPuzzleServiceProvider>` retains the actual
Bevy window `Entity`. Its lifecycle identity is a checked `NonZeroU64` from that
entity's bits. A zero identity and an unavailable bound `Window` return distinct
`DesktopNativePuzzleWindowError` variants.

The parent discovers `PrimaryWindow` and calls `new(window, provider)`. It calls
`update(world, presentation, application, graphics_failed)` before synchronizing
the scene. After actual egui/input and renderer readiness acknowledgement, it
calls `report_readiness(frame, presentation, application)`. The frame borrows
`World` and the currently applied `DesktopBoardSnapshot`, and carries the actual
`graphics_ready` flag. The parent retains the adapter through shutdown and does
not emit final `AppExit` until `is_settled()` and its other owned exit barriers
permit it.

The parent registers only the production module. Its `cfg(test)` declaration
includes the mirrored test source once, permitting the existing crate-private
service receipt channel and state constructor without exported test seams.

## Authored behavior

The adapter owns separate cursors for `WindowFocused`, `WindowOccluded` and
`AppLifecycle`. It also observes the bound `Window.focused` and `Window.visible`
data. Events for other entities do not interrupt the puzzle. Focus loss,
visibility loss, occlusion and suspension use the canonical
`DesktopNativeLifecycle` mapper. `WillSuspend` and `Suspended` form one native
suspension edge; `WillResume` and `Running` clear its flag. Focus gain, visibility,
exposure and resume signals never issue puzzle Resume.

Each render-failure edge captures one Recovery interruption and makes readiness
Hidden. A stable failure does not continually submit it. Successful graphics
recovery clears the native failure flag without resuming a run.

Native interruptions go directly through the existing lower service's
`interrupt(session, reason)` lane. The adapter captures the selected session at
the native event, retains one active receipt and one latest coalesced request,
and never redirects retries to a replacement owner. No selection means no native
pause operation. Equal pending native requests coalesce. Completion is observed
with `try_result()`, including a closed result channel. Busy retains the queued
request until a service publication or a new native event provides progress.
Busy does not schedule its own repaint loop. Terminal errors clear their own
slot, preserve a newer queued request and reach
`PresentationAppError::PuzzleInterruption` with the original `Arc` error.

Readiness is Hidden while the bound window is unfocused, invisible, occluded,
suspended or graphics-failed, and in Games workspace. Otherwise graphics and the
root's `is_puzzle_input_ready()` predicate must permit it. Visible boards require
equal current-controller and applied-renderer FEN and variant. The panel is
synchronized again before that comparison, so a newer watch snapshot cannot be
acknowledged against the old rendered board. Paused runs can receive Ready with
no board, allowing an explicit Resume action to become available. Readiness does
not itself Resume or select an opponent reply; the existing panel owns those
decisions.

Readiness cache comparisons borrow unchanged contexts. Hidden comparisons ignore
revision and pending changes within the same controller session, preventing
Hidden receipts from generating a new Hidden request indefinitely. Other
readiness values observe context and pending completion. The cache records
pending work after reporting, avoiding a follow-up report caused only by its own
new request. A changed Ready report that enables controls rearms one application
wake. Expected application and panel errors use `PresentationApp::report_failure`
and do not become graphics failures.

The adapter constructs no runtime, task, worker pool, synthetic application
receipt, storage client or random provider. Its test provider is an in-memory
implementation of the existing service effect boundary. Board and puzzle state
come from real domain runs and snapshots.

## Authored deterministic coverage

All 24 cases were authored before or alongside the implementation. The initial
mirrored test file was written before the production file. None has been
executed, and no red-first execution is claimed.

| Coverage | Source cases |
| --- | --- |
| Entity and window isolation | Checked entity identity, other-window events, already-refocused event batch, missing bound window |
| Native signals | Focus loss/gain, ordinary application saturation, occlusion/exposure, visibility, suspend/resume, Recovery edge, no selected owner |
| Retained interruptions | Original-session Busy admission, original-session Busy receipt, latest coalesced session, terminal receipt/admission errors, closed channel, active plus queued shutdown barrier |
| Readiness | Paused boardless Ready without Resume, current versus newer rendered board, equal FEN with different variant, graphics/input gates, physical window conditions, Games workspace |
| Coalescing | Hidden revision/pending stability, Ready context/pending invalidation, repeated hidden frames without additional requests or wake |

The board comparison and paused-readiness cases exercise the private frame
calculation with real `PresentationPuzzleController` snapshots. They do not
constitute a production renderer test. Parent composition and the full panel
through actual rendering still require integration checks.

## Executed source checks and remaining evidence

- Read global/project guidance, the constitution and every canonical rule.
- Resolved the feature with the explicit `SPECIFY_FEATURE_DIRECTORY` supplied by
  this task after the child checkout had no feature selector.
- Read-only checklist inventory: `native-desktop.md` has 40 checked items,
  `requirements.md` has 16 checked items, and `native-desktop-review.md` has no
  checkbox markers. These remain requirements-review state, not implementation
  acceptance.
- Inspected actual Bevy 0.20.0 `Entity`, `Window`, lifecycle event and
  `MessageCursor` source, and the parent's current lower-service and presentation
  APIs.
- Formatted the two owned Rust files with standalone
  `/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt`, reporting `rustfmt 1.10.0`.
- Standalone formatting check and `git diff --cached --check` passed. The
  exported three-path patch also passed read-only `git apply --check` against
  the parent checkout before handoff. The parent source was not changed.

Cargo, builds, tests, Clippy, `make verify`, CPU diagnostics and runtime
measurement were not run. No dependency, checker, vendor, module root, host,
bootstrap, application or coordinator edit belongs to this patch. Compilation
requires the parent's uncommitted dependencies and its production module
registration.

Bevy event mapping is source-authored. Actual desktop OS sleep-event delivery on
macOS, Windows, X11 and Wayland remains unverified. Native focus, occlusion,
renderer recovery, accessibility and shutdown acceptance remain unverified.

## Parent semantic review repair, 2026-10-17

This incremental round copied exactly the three owned files from the current
parent checkout before editing. Its production baseline blob is
`c535098a171b2c5d434b41631e1fd1dd46ade31a`, its test baseline is
`0a1d8bf2bd346824c9988f2b8b21d63d0a672d6c` with 27 cases, and its evidence
baseline is `f108210d95917e7dcb4d65e112e6d70690689b56`. No parent host,
bootstrap, application, module root or other source was copied or changed.
Earlier authorship and verification statements above describe the preceding
handoff; this section records the evidence supplied and inspected for this round.

The parent authorized production repair after observing semantic failures. This
child read both complete logs under
`/tmp/claude-chess-pgo-diagnosis-20261016/`:

- `puzzle-native-window-busy-red.log`: parent reported exit 101; the log records
  3 passed, 2 failed and 226 filtered. The coalesced-progress regression expected
  two original-session FocusLost attempts and observed one. The Busy-only
  publication regression expected one attempt and observed two. The other three
  Busy tests passed in that parent run.
- `puzzle-native-window-concealment-red.log`: parent reported exit 101; the log
  records 0 passed, 1 failed and 228 filtered. The already-refocused interruption
  failed to report Hidden before its retained receipt completed.

The parent also reported an earlier 23 passed, 1 failed adapter run caused by an
invalid `Entity::from_bits(0)` fixture, then corrected that fixture using distinct
real entities. The copied 27-case baseline already contains that correction; this
round preserves it. The parent's earlier all-target Clippy success predates these
review repairs and is not validation of this patch.

Read-only inspection of the actual parent service confirmed that `respond`
publishes receipt failures and retains the last failure in later publications.
The local typed progress key therefore compares selected session, activity,
health availability and retained snapshot identity. It excludes service failure
payloads and health error identities. A fresh snapshot counts even when its
context revision is unchanged; cloning the same snapshot Arc does not. A later
snapshot, activity or health change still counts when the retained service
failure is Busy. Native receipt completion is
observed before the watch publication, so same-update semantic progress can
unblock a Busy receipt without being overwritten by it. Identical native
requests still coalesce; a new native interruption edge can unblock a retained
request. Retry session identities remain those captured at their original edges.

Each captured native interruption also retains one concealed session identity.
Both update-time and frame-time readiness remain Hidden while that selected
session still has active content, including after a successful receipt without
an authoritative paused snapshot. An authoritative paused or terminal snapshot
releases the native concealment latch. Paused domain state continues to hide the
board while normal readiness makes explicit Resume controls available. Selection
replacement clears the old latch without changing its retained interruption.
No Resume command, detached task or additional public API was introduced.

All 27 baseline case names remain. The existing concealment regression now mounts
the actual presentation panel through its public application connection, real
temporary saved-run inventory and Open control. It checks pending Hidden
delivery, board and Hint input concealment, continued concealment after native
receipt completion, authoritative pause, usable Resume controls and an explicit
Resume click. Two additional cases cover a mounted replacement while the
original interruption remains outstanding, and activity/health retry progress
with retained Busy plus duplicate-publication blocking. There are 29 authored
cases. These strengthened and additional checks have not been executed by this
child, and their additional assertions have no observed red or green result.
Their controlled rendered-board acknowledgement does not establish native
renderer acceptance.

During read-only export checking, the parent test file had gained
`busy_receipt_preserves_fresh_snapshot_progress_without_a_context_revision`.
It publishes a distinct snapshot Arc with unchanged context. This child read
that addition and included snapshot identity in the progress key. The parent
addition was not copied or appended again: the child still starts from the
specified 27-case blob, and the incremental patch preserves that extra parent
case when applied. This child did not run the new parent case.

Standalone Rust 1.99.0 rustfmt was used for the two Rust files. No Cargo command,
test execution, build, Clippy, CPU diagnostics, performance measurement, commit,
push, additional agent or new worktree was run in this round. Parent runtime
feedback, compiler checks and final verification remain outstanding. No green
claim is made for this repair.

The requested source-only incremental export is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-native-window-review-fix.patch`.
It is relative to the exact copied parent baseline and contains only the three
owned paths listed above.

Executed source checks: standalone rustfmt check, incremental whitespace check,
unchanged public declaration comparison, preservation of all 27 baseline case
names with no duplicate case names, and read-only `git apply --check` against
the current parent all passed. Applicability was checked after the parent's
additional same-context snapshot case appeared. These checks do not compile or
execute the repair, and the patch was not applied to the parent by this child.
