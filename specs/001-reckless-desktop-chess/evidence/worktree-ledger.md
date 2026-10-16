# Implementation worktree ledger

Last reconciled: 2026-10-16. Integration branch:
`001-reckless-desktop-chess`. Last pushed checkpoint: `abaff93`.

The coordinator owns registrations, integration, verification, task markers,
commits and pushes. Agent branches deliver explicit-path staged binary patches.
Agent delivery, patch integration, passing checks and task completion are tracked
as separate states. Only the coordinator runs Cargo, using focused debug checks
during development.

## Active ownership

All worktree paths below are relative to
`/Users/sadrashameli/Personal/chess-worktrees/`.

| Agent | Worktree | Branch | Current ownership | State |
| --- | --- | --- | --- | --- |
| `plan_review` | `engine-lifecycle` | `work/001-engine-lifecycle` | Supervisor/registry cleanup and capacity teardown | Runtime tests and terminal admission integrated; retained Quit admission fix awaiting green |
| `selection_review` | `profile-foundation` | `work/001-profile-foundation` | Host preparation, graphics interruption and confirmed Quit boundary | Geometry regression observed red; fix integrated; graphics cause and teardown patches in progress |
| `story_coverage` | `native-harness` | `work/001-native-harness` | Saved per-game search budget and Auto-flip | Allocator integrated; US1 gap review delivered; configuration persistence regression queued |

The main checkout at `/Users/sadrashameli/Personal/chess` owns the native game
bridge, coordinator, composition root, scene synchronization and integration.
There are three active child agents and eight Git worktrees including this
checkout. The historical agent names in older session metadata are not active
assignments.

## Inactive worktrees

| Worktree | Branch | Recorded base | State |
| --- | --- | --- | --- |
| `activation-foundation` | `work/001-activation-foundation` | `945c863` | Dirty; audited; no unique useful integration candidates |
| `board` | `work/001-board-split` | `7de3a7b` | Dirty; audited; historical web-shell state |
| `bootstrap` | `work/001-bootstrap` | `ce2bb9d` | Dirty; audited; integrated foundations and historical artifacts |
| `bootstrap-validation` | `work/001-bootstrap-validation` | `6bb1434` | Dirty; audited; historical web-shell state |

Read-only reconciliation against `ec355b3` and the current integration checkout
found all four branch heads are ancestors, with zero unique committed changes.
Their dirty patches contain previously integrated or superseded foundations,
older alternatives, removed web-shell files and generated artifacts. Package and
activation work is preserved in `75d219f`, `9345071` and `f7c3901`; historical
board/bootstrap work is preserved in `945c863` and its obsolete web shell was
deliberately removed in `cf03da8`. No uniquely unintegrated useful implementation
was found. No inactive worktree has been reset or deleted.

## Pending integration queue

| Delivery | Integration | Verification |
| --- | --- | --- |
| Game supervisor, save receipts and responsive resource waits | Source/tests integrated | Existing desktop regressions passed; new runtime retry race observed red |
| Game registry Create activation | Source/tests integrated | Create and prior shutdown regressions passed; retained Quit fix awaiting green |
| Immutable game snapshots and reducer | Source/tests/registrations integrated | Save-failure snapshot regression passed in 172-test desktop run |
| Native input controller and mounted game UI | Source/tests/registrations integrated | New presentation verification pending after engine-suite interruption |
| Reckless resource configuration and native factory | Source/tests/registrations integrated | 71 engine tests passed; corrected Chess960 fixture still awaiting green |
| Shared engine resource allocator | Source/tests/registrations integrated | 14 allocator tests passed |
| Sibling continuation reuse regression | Source/tests integrated | Both regressions passed within 167-test domain run |
| Save capacity waiting | Executor/repository source and tests integrated | Executor tests passed; new inventory/storage checks pending |
| Quit after committed game response | Coordinator regression and fix integrated | Regression passed in assembled desktop suite |
| Retained native runtime and shared fixtures | Source/tests/registrations integrated | Two new runtime checks passed; cleanup retry check exposed Busy race |
| Terminal capacity teardown | Source/tests integrated | Fixture borrow correction integrated; behavioral checks pending |
| Native host geometry preparation | Red regression and fix integrated | Readiness overwrite observed red; green pending |
| Configuration persistence | Regression integrated | Behavioral red queued; source not yet delivered |

## Verification boundary

The previous assembled run recorded 28 library, 172 desktop and 167 domain
passes, followed by 71 engine passes and one incorrect Chess960 fixture failure.
Its fixture correction is integrated. A subsequent desktop run with runtime tests
recorded 175 passes and one cleanup retry failure. The new reserved Quit admission
fix is integrated and awaits green. Focused Clippy passed before the newest
runtime/host/teardown changes. The macOS debug linker still reports an oversized
unwind-section warning. No optimized or cross-platform acceptance is claimed.

Production playable composition remains unfinished. The coordinator is wiring
the native runtime into startup and restoring unfinished games through bounded
document inventory. The custom search budget/Auto-flip persistence and
intended-move draw-claim UI are known US1 gaps. Task completion remains governed
by `../tasks.md`, not agent delivery or the existence of a source file.
