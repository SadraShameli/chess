# Puzzle application admission and protocol

Final parent integration and gate results are recorded in
[saved workspace integration](puzzle-saved-workspace.md). The source-only and
filtered-run statements below preserve the scope of their individual handoffs.

This is an implementation slice from root `43105cc84ea926b2af3fc54a1baa260fcdad68c2`.
It does not complete a Spec Kit task or establish native acceptance.

## Observed baseline regression

Before any production changes, the existing public Game Stop operation was used
with deterministic distinct GameIds and no admitted owner mutations. The test
attempted at most 10,000 unresolved controls, checked repeated-key receipt and
deadline retention, independently admitted Quit, drained all 10,001 admitted
work items, observed their receipt outcomes, and checked an empty dispatch queue
before asserting a finite Busy refusal.

The frozen baseline compiled and failed on the intended behavioral assertion:

```text
all 10000 distinct unresolved Game Stop controls were admitted without Busy
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 213 filtered out
```

The exact command was:

```sh
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --frozen --test desktop desktop_application::distinct_game_stop_controls_have_finite_admission_and_reserved_quit -- --exact --nocapture
```

Cargo exited 101. Default build and test parallelism were retained. The log also
contains the macOS linker warning about the `__eh_frame` section exceeding
16 MB. This run is behavioral failure evidence, not a zero-warning gate result.

The preserved production application source blob is
`a41d31fa7907828a5662836115f85b89cc40d98a`.
The separate frozen regression **test source** blob is
`52c0768b497147d9badc55e3a0c7de3eeb66d559`.

Artifacts under `/Users/sadrashameli/Personal/chess/.git/parallel-patches/`:

| Artifact | SHA-256 |
| --- | --- |
| `puzzle-application-admission-baseline-tests.patch` | `3c6df6e0de7837e05fa7797f8bf5933046008da16abef19dc821361baec65244` |
| `puzzle-application-admission-baseline.log` | `f82ba652978b5b883d48d6b5468e41e70eb8322a8190abed824914d7489941cf` |
| `puzzle-application-admission-baseline-source.rs` | `ad3761a2f5dcaa60a7ace96108e7965395dd792e3e91853097b648d6246f2a9f` |

The exclusive baseline Cargo lane was released with no active process or tool
session. The parent acknowledged the semantic failure before authorizing
production changes. Subsequent work is source-only until a new explicit lane
grant.

## Implementation

`DesktopApplication::CONTROL_CAPACITY` derives from the existing 256 ordinary
admission policy and the nine non-Quit control-key families owned by
`DesktopMessagesEnumControlKey::PER_ENTITY_CAPACITY`. This is a command admission
budget, independent of CPU capacity. There are two separately bounded control
budgets: 2,304 idle controls and 2,304 controls targeting tracked mutations. Idle
controls cannot consume a tracked mutation's safety entitlement. The finite
non-Quit maximum is `DesktopApplication::MAX_CONTROL_CAPACITY`, currently 4,608,
plus one separately reserved global Quit. Pending duplicates coalesce before
capacity checks. Original receipts and deadlines are retained. Completion
releases the corresponding control budget.

Puzzle commands use the injected stable workspace owner. OpenSaved and mutating
Session actions use real application mutation admission and commit transitions.
Inventory, Snapshot and Summary do not claim mutation ownership. Pause, Hidden
and confirmed End have independent control keys bound to the original session.
Confirmed End retains its full expected context in the original request.
Interrupt retains its original session and reason for authoritative service
resolution. Controls target matching Session mutations only, leaving OpenSaved
replacement work and other sessions uncancelled.

Mutation tracking retains the original request through an Arc. Target matching
and ownership release therefore remain independent of completed-result cache
eviction. Existing generic and game/engine control targeting is preserved.

Closing admission delegates narrowly to
`DesktopPuzzleServiceEnumOperation::is_available_while_closing()`. Recovery
includes RetrySave, Snapshot, Summary and canonical interruption/pause/hide
operations; it does not reset Quit or reopen general mutation admission.
Quit completion also checks every currently unresolved application entry under
the same state lock, including recovery admitted after Quit dispatch. It returns
CancellationUnacknowledged until this work resolves. Successful Quit completion
seals fresh admission in a terminal Closed lifecycle. Original retained requests
and repeated Quit still return their original receipts without reopening work.

The public protocol uses `DesktopEnumCommand::Puzzle { owner, operation }`,
`DesktopEnumValue::Puzzle(Box<DesktopPuzzleServiceEnumResult>)` and
`DesktopCommand::puzzle(owner, operation)`. The constructor delegates to the
service-owned `operation.expected_revision()`. Typed revision conversions are
`DesktopRevision::for_puzzle` and `DesktopRevision::for_puzzle_storage`.
`complete_puzzle_control` delegates to the existing `complete_owned_control`
algorithm and calls the service-owned `result.revision()` policy. The service
owns both revision policies; callers duplicate neither mapping.

## Test and verification boundary

The baseline regression above was exported and executed before production edits.
The initial protocol tests were also authored and exported before the relevant
application/message production changes. New APIs were absent in this checkout
at that point; no missing-API compile failure is claimed as a semantic red.

Eleven new puzzle protocol tests cover typed precision and payload retention,
commit/ownership admission, read-only operations, urgent access under ordinary
saturation, matching-session cancellation and completion observation,
replacement/new-session isolation, Closing recovery and original Quit deadlines,
unknown-session capacity and duplicate retention, queued cancellation,
irreversible commit outcome observation, typed End output and control-family
validation. Two Game Stop tests cover the frozen finite-admission regression and
capacity release after completion.

`tests/desktop/puzzle_admission.rs` is included by `src/desktop/application.rs`
under `cfg(test)` to use real private snapshot construction. It must not also be
registered in a shared test root.

Owned files were formatted with the pinned standalone rustfmt, and
`git diff --check HEAD` passed. No post-change Cargo command ran in this worker.
The parent's merged compilation and semantic results are recorded below.
Clippy, full `make verify` and native/GPU acceptance remain unverified here.
No performance improvement or supported-platform result is asserted.

The explicit staged patch is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-application-admission.patch`.
It exports only the four owned desktop sources, the two owned test files and
this evidence document. Service/inventory/panel source, module roots,
coordinator/bootstrap/host/app registrations, dependencies, checkers and
migrations are excluded. No commits, pushes, extra worktrees or agents were
created.

## Independent review regressions and incremental repair

The original full patch had two reviewed defects: idle controls could consume
the safety capacity needed by a tracked mutation, and Quit used only its initial
affected list while later Closing recovery could remain unresolved or arrive
after QuitReady.

Production was frozen before appending exactly three `review_` tests. The frozen
application production blob was `411eff27761f3ad06e1cee340b52a77ba42c42c0`.
The separate appended test source blob was
`41d1a120515c7eb56b86ec95188e94fcf451962b`. The tests-only incremental patch is
`puzzle-application-admission-review-tests.patch` under the artifact directory
above, with SHA-256
`b221262886bb10d39fe0cb7b7f43bf3e1377f6ca254ce630f8da04a24fd6020f`.

The parent compiled its merged tree successfully and ran:

```sh
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --frozen --all-features --lib desktop::application::puzzle_admission::review_ -- --nocapture
```

The observed result was exit 101, 0 passed, 3 failed, 0 ignored and 172 filtered.
The exact failures were the tracked mutation's Generic Stop or puzzle
Pause/Hidden being refused after idle Game Stop saturation, Quit completing
with later admitted RetrySave unresolved, and fresh canonical recovery being
admitted after terminal QuitReady. Every test drained admitted work and observed
receipt outcomes before its final blocker assertion. This is an observed
semantic red against compiled existing APIs, not a missing-API compile failure.

The exact parent log was read from
`/tmp/claude-chess-pgo-diagnosis-20261016/puzzle-admission-review-red.log`
and preserved as `puzzle-application-admission-review-red.log` under the artifact
directory above. The parent acknowledged all three reds before authorizing the
two source repairs. No child Cargo command was executed in this review round.

The incremental repair exports only changes since the parent applied the
original full patch and the three review tests. Its baseline staged tree is
`a0388afdd0f6d6b438b08a16c182d947a816fbba`. It changes only the application source
and this evidence document; it repeats none of the already-applied tests.
The artifact is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-application-admission-review-repair.patch`.
Standalone formatting and whitespace checks pass. The parent's partial
post-repair Cargo validation is recorded below; remaining gates stay pending.

## Parent merged private desktop validation

The parent's merged tree, including the admission repair, service cancellation
token and coordinator progress fix, compiled and passed 138 selected private
desktop tests: 0 failed, 0 ignored, 0 measured and 66 filtered out. The exact
log confirms all 14 source-included puzzle admission cases passed, including
all three `review_` regressions. This establishes the observed red-to-green
transition for the reviewed application blockers on that merged tree.

The same filtered invocation selected the desktop integration binary but ran
zero of its cases: 0 passed, 0 failed, 0 ignored and 215 filtered out. These
215 cases are not passing evidence from this run. The parent is running the
full desktop target separately; its result has not yet been recorded here.

The exact log was read from
`/tmp/claude-chess-pgo-diagnosis-20261016/puzzle-desktop-repair-tests.log`
and preserved as `puzzle-application-admission-merged-desktop-tests.log` under
the artifact directory above. It includes the macOS `__eh_frame` linker warning,
so this is not a zero-warning final gate result.

The application production blob remains
`1e4705d4abbca8fc6d25fb1f76367997cc6c2798`; this update changes evidence only.
The parent retains the exclusive Cargo lane. No child Cargo command was run.
Final Clippy, full `make verify`, the complete desktop integration result and
native/GPU acceptance remain pending. No Spec Kit task completion is claimed.
