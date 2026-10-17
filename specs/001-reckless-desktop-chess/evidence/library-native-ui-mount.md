# Native Library UI mount

Status: source handoff awaiting parent compilation, behavioral checks and native acceptance. No task is marked complete.

## Provenance and ownership

The starting HEAD is `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`. The supplied dependency patch at `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-native-mount-dependency-20261017/source.patch` was verified with SHA-256 `41954367eb94b0d1200cc79549a254de717139395d2236d7fce327661cea4472`. Applying it to the clean index produced exactly `a02aa91579c030c35e2ffcfb5221e713f1a11bf6`.

The later parent Open recovery repair is a separate prerequisite. Its four service files were mirrored unstaged from `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-open-application-recovery-repair-20261017/source.patch`, verified with SHA-256 `9968a02b14b41226dda45360f1137d860d361386ab9189024ebe79ae5f95e912`. They are excluded from this incremental UI export. That repair's stated service baseline is `b11bd8b2c14558b2b6fcfba52d30384bbef4687b`; its hunks applied to the supplied mount dependency without modification.

The export contains only the app mount and focused private files, native host connection fields, the truthful host fixture field, app workspace tests, the explicitly extended Library controller/intent/view recovery seam and focused tests, and this evidence file. Other dependency source, services, messages, DTOs, coordinator, storage, board and task markers are excluded.

## Mounted behavior

- The native bootstrap passes the constructed Library workspace to `DesktopNativeHostServices`. The host mounts it on the actual `PresentationApp`. The existing native service construction, `.with_library` route and retained cleanup remain intact.
- The header has a Library navigation button. Its workspace mounts the existing query view, including filters, metadata, bounded pages, refresh and Open Saved controls.
- Query and metadata effects retain the original operation and prepared `DesktopRequest` before submission. Busy and entity contention retry that same unadmitted request. Actual application receipts are recorded before controller binding. There are at most five panel lanes and one serialized editor action.
- The Library board is selected directly from the permit-owning `DesktopGameEditorPublication`. An unopened Library workspace has no board and does not fall back to the bootstrap position. It has no synthetic game snapshot, clock or outcome.
- Focused app notes and tree controls use the existing annotation and variation owners. They synchronize actual publications and directly observe service results and canonical service errors. Concrete editor requests are prepared before passing their actual request IDs to these owners.
- Notes save completion consumes actual Played or Notation save receipts. An unchanged edit completes through the existing owner without adding a checkpoint. Pending snapshots and data/protocol failures retain the draft and its leave guard.
- Save/Discard/Stay protects Open Saved, workspace leave, New Game, game-tab changes and Quit. A typed root target is associated with the annotation Leave token. The frozen Open intent keeps its original game and storage revision through a save; Stay abandons it before admission.
- Tree loading uses actual opaque continuations and the existing 100-row proof checks. Navigation uses canonical editor commands. The study-move selector lists only canonical moves from the actual publication, including their actual promotion and castling encodings, and submits legal Append Study Branch commands.
- Actual live-game snapshots with a newer revision preserve the earlier board and notes and expose a source-change conflict. They do not construct or silently replace an editor publication.

## Certified Open recovery

`PresentationLibraryController::retry_open` retains the frozen original Open intent, prepared request, actual receipt and service-certified lower publication context. Each retry has a fresh intent and application request. Repeated egui passes or Busy admission retain that prepared fresh request.

Only a retained Open carrying `Some(certified_context)` exposes Retry opening. Legacy `None` continuations remain visibly retained without a retry control. Query and metadata still refuse a context-bearing continuation.

Continuation validation tracks the original request ID separately from the current fresh receipt. A later error may identify the older original application request. It must still carry the exact original certificate. The fresh request dispatches the ordinary application Library Editor RetrySave command through the coordinator; the UI does not call a provider directly or redispatch the original cached application request.

Recovered Opened results are checked against the frozen game and certified game/source/epoch owner. Newer revisions and selections are accepted under the canonical editor snapshot checks. A Saved result cannot substitute for Opened. The original application failure remains immutable; only the new application request completes with the recovered publication.

The parent supplied an observed baseline red, 0 passed and 1 failed, at `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-open-application-recovery-baseline-20261017/semantic-red.log`, SHA-256 `b67284ce7d9d0e612603d62a78a5a18175d8a33157da0905aebd1f943fec2ee0`. The parent repair green log at `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-open-application-recovery-repair-20261017/parent-green.log` has verified SHA-256 `0db5535e099f925e5425a212bb4620d746579f1ae1458f6e34b053a1aba23581`. The parent reported 1 passed, 0 failed and 657 filtered for that service regression, including exact lower Publishing certification, a fresh application Opened result, refusal of an older actual epoch, unchanged source/durable state and observed cleanup. These are parent service results, not verification of this UI increment.

## Test source and verification

The initial app tests were authored before the UI implementation. The controller recovery integration test was authored before the controller seam. Twelve new tests are registered across the private app fixture and the focused controller recovery module. They use the real application queue, service, registry, repository and permit-owning publications. The egui interaction harness uses the matching 0.36 family.

The source covers visible native controls and repeated passes, exact selected board projection, save-before-open ordering, Stay abandonment, guarded root targets, Busy request identity, foreign receipts, failed durable saves, real tree navigation, legal study branches, Played receipt completion, live-source draft preservation, repeated fresh Open retries with older origin IDs, foreign actual publications and fresh retry admission under Busy pressure. The existing puzzle workspace test now also checks the third workspace. The native host fixture truthfully uses `library_workspace: None`.

Executed here: standalone pinned rustfmt with child traversal disabled, patch SHA/tree verification and source/diff inspection. Tests run here: **0**. No Cargo, build, Clippy, runtime, GPU, CPU diagnostics, extra agents, extra worktrees, commits or pushes were run. Neither a compilation result nor an observed red/green UI result is claimed.

## Remaining acceptance seams

- Direct 3D board clicks remain read-only. The live-game move-selection controller requires an actual `DesktopGameSnapshot`. A shared gateway consuming canonical board moves and editor context is required to reuse click/drag, ambiguous castling and promotion selection for study input. The legal study selector is usable without fabricating a live game or adding another rules engine.
- The focused mounted view provides notes save, existing NAG display, tree paging/navigation and legal branch append. The standalone annotation and variation view widgets, including marking editing and variation promotion/deletion dialogs, are not all mounted by this increment. Their owners and receipt checks are reused; full annotation UI acceptance remains open.
- Retained annotation/tree save recovery still needs the canonical fresh application retry protocol. The mounted view exposes retained state and does not offer a Retry Save button that resubmits a cached terminal application receipt. The new certified Open path is separate and requires the parent service repair above.
- Newer live-source notifications retain the draft and block stale editor commands. A source-certified refreshed publication feed and conflict reconciliation remain acceptance work; the mount never reconstructs the missing publication from a game DTO.
- The unowned `src/desktop/native/puzzle_window.rs` readiness condition still names Games as its non-puzzle workspace. The parent should make it treat every workspace other than Puzzles as hidden. The app's own readiness method already does this for Library, but the native interruption path needs its corresponding condition before acceptance.
- The parent must compile and execute the focused tests, review all warnings, run the required wider checks and perform native GPU/accessibility interaction acceptance. Headless scene handles in test source do not establish actual 3D rendering. Existing GPU/platform/release blocks remain unchanged.

Handoff artifacts are `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-native-ui-mount-20261017/source.patch` and `manifest.json`. The manifest records the exact owned path list, staged tree and incremental patch SHA-256 without including the frozen dependency or the unstaged parent service repair.

## Parent integration checks

The initial parent run exposed a real duplicate-node error in study append and
aborted during headless texture cleanup. The log is
`/tmp/claude-chess-pgo-diagnosis-20261016/library-native-mount-tests-2.log`,
SHA-256 `44813951fe80c11643e7f1811069523ad0e45c8174ea273694d072468fadad40`.
The mounted action now receives a fresh typed node identity at the input
boundary. The domain UUID gateway rejects nil identities without reparsing
strings. The headless fixture intentionally clears unrendered texture deltas
before error propagation while retaining shapes for semantic assertions.

The save-failure fixture captures the guard, notes and unadmitted destination
before removing its synthetic failure trigger. Cleanup then reads the retained
accepted source and uses the supported fresh service RetrySave operation before
closing the service. This fixture cleanup does not claim that native save
recovery is mounted.

A separate actual regression found the native puzzle-window readiness path
reporting BoardNotReady in Library. It now reports Hidden in every workspace
other than Puzzles. The baseline log is
`/tmp/claude-chess-pgo-diagnosis-20261016/library-puzzle-readiness-baseline.log`,
SHA-256 `aea2f264720a7b7bd8942350791654005705fd48bf940b3adc2289a7e53c2231`.

The parent full library-unit run passed all 670 cases, including the ten mounted
Library cases and the readiness regression, with no ignored tests. Its log is
`/tmp/claude-chess-pgo-diagnosis-20261016/library-report-integrated-lib-tests.log`,
SHA-256 `1eb06b684918a9da895b9397de4290e74d3d7397a1f63bde40fcc489bd1f8aed`.
This is hermetic application/service/scene-adapter evidence. The direct board
input, remaining annotation widgets, native save recovery and actual GPU,
accessibility and platform acceptance listed above remain outstanding.
