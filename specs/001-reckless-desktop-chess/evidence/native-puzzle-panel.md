# Native puzzle presentation handoff

This isolated slice starts at `5e9f55fd94056050e3ba00d7480c7c87f78ac00c`.
It authors the reusable presentation portion of T180/T191 and the Resume,
exhaustion, summary and confirmed End portions of T192. These tasks remain open
for integration and acceptance. T189 history discovery/deletion and US13
production dataset acquisition remain separate. No history records, collection
files, production counts or hidden seeds are fabricated. All 13 approved
journeys remain in scope; no task or checklist marker changes.

The global instructions, project constitution, canonical catalog and all seven
engineering rules were loaded, along with the actual puzzle specification,
data model, study/UI contracts, lifecycle decisions and existing presentation
patterns. The requested `speckit-implement` procedure was loaded. Its
prerequisites resolved after setting `SPECIFY_FEATURE_DIRECTORY` to the existing
canonical feature directory. The read-only checklist gate found requirements
16/16 and native requirements 40/40 checked, with zero unchecked entries; the
separate native review has no checkbox items. No extension hook file exists.

## Dependency boundary

Before local application, the command-owner patch at the root repository's
absolute `.git/parallel-patches/puzzle-command-owner.patch` was verified as
SHA256 `ec95bd1b1b1e7573e403e38819c3aa98648ed7547a969ffb5721ffecde1e775b`.
`git apply --check` and local application succeeded. All 11 dependency paths
are excluded from this slice's staged export, along with all shared module and
test-root registrations.

The dependency's [owner evidence](puzzle-command-owner.md) reports 62 targeted
passing tests, a clean targeted Clippy selection, and the exact-deadline
Pause/Hidden/End corrections. Those are dependency-owner results, not new panel
results. Its domain runs also reported the existing macOS linker unwind warning;
they are not a zero-warning broad build claim. Full native acceptance remains
open for that dependency and this presentation slice.

## Root-facing API

The parent declares `pub mod puzzle;` in `src/presentation/mod.rs` and creates
`src/presentation/puzzle/mod.rs` exposing `controller`, `view`, `setup` and
`summary`. This export does not edit either shared root.

`PresentationPuzzleController::new(DesktopPuzzleSnapshot)` explicitly binds one
session. Ordinary observations cannot replace that session. Construct a new
controller only when the composition root explicitly binds a different run.
Its public operations are:

- `snapshot()`, `summary()`, `failure()`, `board()`, `selected()`,
  `promotion_choices()`, `is_pending()`, `is_confirming_end()` and `can_input()`.
- `can_request(action)` and `request(action, DesktopRequestId)` returning a
  typed optional `PresentationPuzzleControllerIntent` or controller error.
- `board_action(PresentationBoardAction)` preserves the existing board's native
  click/drag identity. `select_square(square, request_id)` and
  `choose_promotion(kind, request_id)` provide typed input alternatives.
- `report_readiness(PuzzleTimerEnumReadiness, request_id)` records the root's
  actual readiness report and can emit Ready or urgent Hidden. Ordinary
  BoardNotReady/InputBlocked observations never pause an active countdown.
- `begin_end_confirmation()`, `cancel_end_confirmation()` and
  `cancel_selection()` own the local confirmation and selection lifecycle.
- `observe_snapshot(snapshot)` and `observe_state(snapshot, Option<PuzzleSummary>)`
  accept current authoritative publications.
- `observe_success(&original_intent, snapshot, Option<PuzzleSummary>)` and
  `observe_failure(&original_intent, DesktopFailure)` settle the exact retained
  request. The root translates actual application receipts/results; the panel
  never constructs an application receipt.

The intent retains its exact `DesktopRequestId`, full `PuzzleSessionContext` and
`PresentationPuzzleControllerEnumAction`. Actions are Move, OpponentReply, Hint,
RetryPractice, Skip, Next, Pause(reason), Hidden, Resume,
End(PuzzleRunEnumConfirmation), Reshuffle, Ready and RetrySave. `action.command()`
returns the actual `PuzzlePersistenceEnumCommand`, with None only for RetrySave.
The bridge handles that case through its real retry operation.

The parent dispatches OpponentReply only after the player's animation/readiness
acknowledgement. Its gate requires the actual opponent phase, Ready, an open
checkpoint and no pending action. The controller chooses no reply, randomness
or solution. A freeze disables dispatch; recovery still requires explicit Resume
and renewed readiness before dispatch. Repeated passes/receipts cannot emit
another reply for the already accepted action.

Urgent Pause/Hidden uses a separate retained request slot, so ordinary pending
actions cannot disable interruption. It conceals content immediately. The
parent routes interruption through its authoritative-context lifecycle path,
bound to the intent's original session. Only revision drift within that same
run is allowed there; an old intent must never target a newly bound runtime.
Ordinary input retains its original full context and is never rebased.

The root must mount both the scene image and semantic board exclusively through
`controller.board()`. None means neither content nor board accessibility may be
mounted, even if a previous snapshot/scene/attempt Arc remains cached. The view
also omits attempt, hint and promotion content behind this gate. The root owns
scene preparation, visibility, interactivity and native lifecycle delivery.
Report Ready only after those conditions actually hold. Focus or visibility
return does not itself request Resume.

`PresentationPuzzleView::show(ui, &mut controller)` returns an optional intent
and renders local errors. Keep the view/controller retained across egui passes.
It supplies timer, hint, retry, skip, Next, pause/Resume, Retry Save, exhaustion,
promotion and confirmed End controls using existing native theme/widget patterns.
The timer label formats only the authoritative remaining duration, using the
existing clock presentation formatter. There is no local countdown or clock.

## Setup and summary

`PresentationPuzzleSetupCollection::new(dataset, label, available_puzzles, themes)`
accepts caller-supplied installed metadata using actual `PuzzleSelectionDataset`
and `PuzzleDefinitionTheme` identities. It defines a presentation choice, not
an interchange format or loader. Empty/unavailable collections cannot start.

Retain `PresentationPuzzleSetup`. `show(ui, collections, can_start)` emits an
optional `PresentationPuzzleSetupIntent` with request, dataset, validated filter
and `PuzzleSessionConfig`. `observe_result(request_id, Option<DesktopFailure>)`
settles only that Start request. Setters permit injected defaults or future
Settings integration. Difficulty/theme/config validation delegates to actual
domain constructors. The factory still owns canonical matching, no-match
refusal, immutable collection freezing, seeded-provider injection and run
construction. Setup changes cannot mutate the active run's frozen filters.

`PresentationPuzzleSummary::show(ui, &PuzzleSummary)` renders only supplied
canonical counts, distinct assisted/unassisted rates, secondary practice/hint/
skip/expiry events and exact active duration. Rates retain integer fractions,
with unavailable zero-denominator rates and no floating-point rounding or
points system. Summaries are optional publications, requested explicitly or
for terminal views; action/timer observations need no O(history) rebuild.

Ordinary retained/coalesced storage writes display Saving progress and do not
disable accepted input. The authoritative Frozen, RetryRequired, AwaitingResume
or paused state gates advancement. Resume remains explicit and requires the
latest paused context to be durable with no newer unsaved state. Save recovery
does not request Resume. End(Unconfirmed) only arms current full-context
confirmation and emits no mutation. Cancel emits no mutation. A changed context
invalidates arming before End(Confirmed) can be submitted, including through a
future shortcut or palette caller.

Legal move and all four promotion choices come exclusively from the actual
`DesktopBoardSnapshot::legal_moves` and retain typed `BoardMove`. Illegal input
does not record an outcome. A legal unlisted result explicitly says it is not
in the listed solution and that no alternative was verified. Failed practice
completion displays the original failed primary outcome separately. No legality,
solution acceptance, RNG, outcome, scoring or durability calculation is copied
into presentation.

## Authored checks and actual verification

Seventeen deterministic fixtures are authored in
`tests/presentation/puzzle.rs`. The owned controller includes that file as a
private `cfg(test)` module so the controlled storage boundary can use the real
crate-private `PuzzlePersistenceReceipt::committed` constructor. Do not register
the same file again through `tests/presentation.rs`. No fabricated JSON snapshot
or private application receipt is used. All snapshots come from actual
`DesktopPuzzleCommands` and real domain-generated definitions/runs. Storage
completion uses controlled channels; clocks are explicit and tests use no sleeps.

The authored cases cover initial save/readiness; continuing reply/animation
time; once-only mounted Hint with an ordinary retained save; urgent Pause with
an older ordinary result; save freeze and removal of board accessibility;
explicit Retry Save/recovery/Resume; all four legal promotions; legal-unlisted
wording and failed practice completion with the canonical summary; confirmation
cancel/stale context; explicit Next/exhaustion/Reshuffle/readiness; confirmed
End; immediate Hidden while another action waits; stale attempt/foreign session
rejection; reply dispatch across a save freeze; actual filter/config validation;
once-only Start and unavailable empty-summary rates. Two review regressions
also send an actual foreign-session snapshot through `observe_success`, while
the controller owns an ordinary or urgent pending intent. They require
`StaleResult`, retain pending ownership and concealment, and then deliver the
real matching result. The ordinary case additionally preserves a newer watch
snapshot and canonical summary when a legitimate same-session stale result
completes as a no-op.

The first source handoff did not run Cargo. The owner later granted one exclusive
targeted lane after the parent's release checks finished. The installed compiler
was verified as the pinned `rustc 1.99.0`. Local declarations registered only the
two dependency desktop modules and the four puzzle presentation modules. No
compile error needed a fix, and receipt ownership stayed unchanged for both
semantic regression reds.

The frozen `observe_success` consumed its matched pending slot before checking
the response session. Both new tests failed at the expected foreign-session
`StaleResult` assertion. The minimal production fix rejects a response belonging
to another bound session before `finish_request` can consume either slot.
Same-session stale completion behavior and the public API are unchanged.

All Cargo commands used the shared target directory shown below, detected default
parallelism, no job/thread cap and sequential execution. Exact selections and
observed results were:

```text
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib presentation::puzzle::controller::tests:: --no-run
exit 0, compilation passed without source fixes

CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib foreign_success_ -- --nocapture
exit 101, 0 passed, 2 failed, 0 ignored, 118 filtered out

CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib presentation::puzzle::controller::tests:: -- --nocapture
exit 0, 17 passed, 0 failed, 0 ignored, 103 filtered out

CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo clippy --locked --all-features --lib -- -D warnings
exit 0, no warnings or errors
```

The 17 passing cases include all 15 original presentation cases and both receipt
ownership regressions. Their final execution took 1.93 seconds, which is a
deterministic contract result, not a native performance measurement. No further
semantic failures were observed in this selection. These commands emitted no
compiler or linker warnings. Their counts are distinct from the dependency
owner's 62 targeted passes.

Standalone Rust formatting and its final check passed on the five owned Rust
paths using `/opt/homebrew/bin/rustfmt` version 1.10.0, edition 2024 and
`skip_children=true`. `rustup` was absent, so the earlier formatter invocation
through rustup could not start; no tool was installed. Final staged whitespace
and export-scope checks passed. All local registrations were removed and excluded
from the delivered patch.

Raw per-command outputs and the command/exit manifest are delivered at the root
repository's absolute `.git/parallel-patches/native-puzzle-panel-logs/` directory.
All Cargo sessions finished and the exclusive lane was explicitly released with
zero active sessions before this final static evidence/export update. No broad
release/native build, dependency change or CPU diagnostic was run.

The final binary delta is exported to the root repository's absolute
`.git/parallel-patches/native-puzzle-panel.patch`. It contains exactly these four
presentation source files, the mirrored test file and this evidence file. No
dependency paths, module/test roots, app/board/theme/domain/native-runtime files,
commits, pushes, agents or additional worktrees are part of this slice.

Parent mounting/factory/native-bridge integration and its final integrated
verification remain required. Production GPU/window focus/sleep/minimize,
relaunch, OS accessibility, platform packaging and all 13 native journeys remain
unverified here. History discovery/deletion remains T189 and the corresponding
T192 portion; starter acquisition/import remains US13. This evidence does not
mark T180/T191/T192, US8 or any release acceptance complete.
