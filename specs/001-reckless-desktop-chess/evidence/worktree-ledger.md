# Implementation worktree ledger

Last reconciled: 2026-10-16. Integration branch:
`001-reckless-desktop-chess`. Pre-recovery pushed checkpoint: `c808c0e`.

## Current inventory after cleanup

The owner requested removal of the accumulated branches and worktrees. The only
remaining checkout is `/Users/sadrashameli/Personal/chess`. Keep local and remote
`001-reckless-desktop-chess` and the default `release` branch.

Ten redundant worktrees and twelve temporary local branches were removed after
their snapshots were verified. The three remote `t3/` delivery branches are
removed as part of publishing this cleanup. Their shared `a5ff7ae` history is
merged into the feature branch; its source tree matches `cc9ca3e` exactly.

The local recovery archive is
`.git/worktree-archives/20261016T144224Z/`. It contains a verified Git bundle,
all ten source snapshots, staged and unstaged patches, original statuses, file
checksums and restoration instructions. Source snapshots include untracked and
ignored files except reproducible `target`, `node_modules` and `.venv` caches.
All source files and symlinks were checked again immediately before removal.

The following ownership tables preserve the completed assignments. Their old
checkout paths and temporary branch names are archived history.

The coordinator owns registrations, integration, verification, task markers,
commits and pushes. Agent branches deliver explicit-path staged binary patches.
Agent delivery, patch integration, passing checks and task completion are tracked
as separate states. Only the coordinator runs Cargo, using focused debug checks
during development.

## Archived completed ownership

Active implementation worktrees are managed by T3 Code. Preserved legacy
worktree names below are relative to `/Users/sadrashameli/Personal/chess-worktrees/`.

| Agent | Worktree | Branch | Current ownership | State |
| --- | --- | --- | --- | --- |
| Shutdown implementation | `/Users/sadrashameli/.t3/worktrees/chess/t3-0e132e5b` | `t3/fix-chess-shutdown-recovery` | Supervisor, registry, native runtime and coordinator recovery | Delivered and integrated; reviewed snapshot-forwarding correction also integrated; desktop checks pass |
| Graphics implementation | `/Users/sadrashameli/.t3/worktrees/chess/t3-77791555` | `t3/graphics-lifecycle-recovery` | Native host/bootstrap terminal arbitration and explicit asset Retry | Delivered and integrated; steady-interruption and fresh-identity corrections integrated; debug checks pass |
| Draw controls implementation | `/Users/sadrashameli/.t3/worktrees/chess/t3-718751b4` | `t3/intended-move-draw-claims` | Native typed move DTO and intended-move claim controls | Delivered and integrated; accessible selection closure corrected; presentation checks pass |

The main checkout at `/Users/sadrashameli/Personal/chess` owns the native game
bridge, coordinator, composition root, scene synchronization and integration.
The three original child agents stopped when Codex closed. Replacement read-only
reviews named `plan_review`, `selection_review` and `story_coverage` completed
reconciliation without changing source or running Cargo. Their three follow-up
implementation tasks have completed and their patches are integrated. Each used
the same fixed snapshot of the assembled uncommitted implementation. The
coordinator owns integration and Cargo. Before cleanup there were eleven Git
worktrees: this checkout, three delivered implementation worktrees and seven
legacy worktrees. Only this checkout remains. None of the three delegated implementation tasks is still running.
Historical deliveries do not prove that an agent is still running.

### Durable delegated task identities

| Scope | T3 task ID |
| --- | --- |
| Shutdown | `node:delegated-task:command%3Amcp%3A11df6cd4-192f-4596-a399-909919cb4edb%3Adelegate-task%3Aresume-20261016-shutdown-implementation-r1` |
| Graphics | `node:delegated-task:command%3Amcp%3A11df6cd4-192f-4596-a399-909919cb4edb%3Adelegate-task%3Aresume-20261016-graphics-implementation-r1` |
| Draw controls | `node:delegated-task:command%3Amcp%3A11df6cd4-192f-4596-a399-909919cb4edb%3Adelegate-task%3Aresume-20261016-draw-implementation-r1` |

## Restart checkpoint

The restart preserved the staged, unstaged and untracked implementation work.
The interrupted native composition regression was rerun and reproduced
`MissingRevision`: its fixture submitted game mutations without the required
native revision. The production presentation already supplies this revision.
The fixture now follows that same boundary contract; the test passes. The first
resumed run passed all 33 library tests and 187 desktop tests, with two graphics
fixture failures. Their request timestamps were ahead of an injected clock fixed
at zero. A shared monotonic fixture clock now advances before those requests; all
three graphics lifecycle tests pass. Production timestamp rebasing is unchanged.
The recovered `DesktopGameRegistryBroadcastOptions` patch fixes the confirmed
Clippy argument-count error without suppressions. The resumed debug checks pass
33 library, 189 desktop and 49 presentation tests, including the native
create/play/save/relaunch regression. One presentation fixture also required the
same begin-commit handshake already used by the production coordinator; that
fixture correction passes. Targeted Clippy passes. The macOS debug linker still
reports the existing oversized unwind-section warning. These checks do not claim
release, performance or other-platform acceptance.

Read-only reviews found no newer implementation to recover from the three former
active worktrees. The new implementation queue addresses explicit retained-save
retry, shutdown reservation publication, pending-factory Quit settlement,
terminal graphics event arbitration, failed-asset reload and intended-move claims.
All three deliveries and the reviewed follow-ups are now integrated. The combined
debug run passes 44 library, 199 desktop, 172 domain and 54 presentation tests;
targeted Clippy for those targets passes. Evidence and remaining runtime limits
are recorded in [live-game controls](live-game-controls.md). Incremental debug
verification remains serialized in the integration checkout. The owner has now
authorized committing and pushing the integration and completed worktree
branches. Their final snapshots are reconciled to the same verified integration
tree before publication; earlier delivered patch snapshots remain preserved in
the task evidence and temporary patch exports.

The publication check additionally passes all 72 engine and 28 storage tests.
Together with the completed recovery checks, this is 569 debug tests. Optimized,
GPU-recovery and other-platform acceptance remain outside these checks.

## Archived legacy worktrees

| Worktree | Branch | Recorded base | State |
| --- | --- | --- | --- |
| `activation-foundation` | `work/001-activation-foundation` | `945c863` | Dirty; audited; no unique useful integration candidates |
| `board` | `work/001-board-split` | `7de3a7b` | Dirty; audited; historical web-shell state |
| `bootstrap` | `work/001-bootstrap` | `ce2bb9d` | Dirty; audited; integrated foundations and historical artifacts |
| `bootstrap-validation` | `work/001-bootstrap-validation` | `6bb1434` | Dirty; audited; historical web-shell state |
| `engine-lifecycle` | `work/001-engine-lifecycle` | `945c863` | Former active owner; preserved; relevant source already integrated |
| `profile-foundation` | `work/001-profile-foundation` | `6bb1434` | Former active owner; preserved; native host matches integration |
| `native-harness` | `work/001-native-harness` | `1f243d1` | Former active owner; preserved; no missing native draw patch |

Read-only reconciliation against `ec355b3` and the current integration checkout
found all four branch heads are ancestors, with zero unique committed changes.
Their dirty patches contain previously integrated or superseded foundations,
older alternatives, removed web-shell files and generated artifacts. Package and
activation work is preserved in `75d219f`, `9345071` and `f7c3901`; historical
board/bootstrap work is preserved in `945c863` and its obsolete web shell was
deliberately removed in `cf03da8`. No uniquely unintegrated useful implementation
was found. Following the owner's cleanup request, all legacy worktrees were
archived, verified and removed. Their branch heads were already ancestors of the
feature branch, with no unique committed changes.

The three subsequent acceptance reviews used the shared main checkout and
created no worktrees or branches. Their completed reports are retained in
`acceptance-reviews.json` in the recovery archive. Remaining implementation work
includes adjudication Retry dispatch, premove notices and per-move save coverage;
the feature task list remains authoritative.

## Pending integration queue

| Delivery | Integration | Verification |
| --- | --- | --- |
| Game supervisor, save receipts and responsive resource waits | Source/tests integrated | Real storage failure and retained retry regressions pass |
| Game registry Create activation | Source/tests integrated | Pending factory creation settles after observed cleanup; coordinator Quit regression passes |
| Immutable game snapshots and reducer | Source/tests/registrations integrated | Save-failure snapshot regression passed in 172-test desktop run |
| Native input controller and mounted game UI | Source/tests/registrations integrated | All 54 presentation checks pass, including intended-move claims and repeated accessible selection |
| Reckless resource configuration and native factory | Source/tests/registrations integrated | All 72 engine checks pass, including the corrected Chess960 fixture |
| Shared engine resource allocator | Source/tests/registrations integrated | 14 allocator tests passed |
| Sibling continuation reuse regression | Source/tests integrated | Both regressions passed within 167-test domain run |
| Save capacity waiting | Executor/repository source and tests integrated | All 28 storage checks pass, including bounded document inventory |
| Quit after committed game response | Coordinator regression and fix integrated | Regression passed in assembled desktop suite |
| Retained native runtime and shared fixtures | Source/tests/registrations integrated | Reservation publication, cancellation retention and failed-Quit snapshot recovery regressions pass |
| Terminal capacity teardown | Source/tests integrated | Terminal event arbitration and wrong-generation rejection checks pass |
| Native host geometry preparation | Red regression and fix integrated | Native composition graphics interruption, host asset identity and graphics lifecycle checks pass |
| Configuration persistence | Source/tests integrated | Search budget and Auto-flip save/reload plus schema checks pass in desktop suite |

## Verification boundary

Current checks pass 44 library, 199 desktop, 172 domain, 72 engine, 54
presentation and 28 storage tests. The earlier fixture, cleanup retry and reserved
Quit regressions are integrated and pass. Targeted Clippy passes for the changed
targets. The macOS debug linker still reports an oversized unwind-section warning.
No optimized or cross-platform acceptance is claimed.

Native startup now owns the retained game runtime and restores unfinished games
through bounded document inventory; the current-host local create/play/save/
relaunch regression passes. Custom search budget and Auto-flip persistence checks
also pass. Intended-move draw controls and the reviewed shutdown/graphics recovery
follow-ups are integrated and pass their focused debug checks. Full playable-engine,
actual GPU recovery and cross-platform acceptance are not claimed. Task completion remains governed by
`../tasks.md`, not agent delivery or the existence of a source file.
