# Implementation worktree ledger

Last reconciled: 2026-10-16. Integration branch:
`001-reckless-desktop-chess`. Last pushed checkpoint: `ec355b3`.

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
| `plan_review` | `engine-lifecycle` | `work/001-engine-lifecycle` | Desktop game supervisor and registry lifecycle | Supervisor/start activation integrated; shutdown review fixes in progress |
| `selection_review` | `profile-foundation` | `work/001-profile-foundation` | Retained native engine allocation/start/refresh and regressions | UI/controller patches integrated; registry review and inactive-worktree audit delivered; adapter follow-up in progress |
| `story_coverage` | `native-harness` | `work/001-native-harness` | Shared engine resource allocator and retained allocation/refresh | Resource configuration/factory integrated; allocator tests delivered; allocator source in progress |

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
| Game supervisor, save receipts and responsive resource waits | Source/tests integrated | Pending assembled desktop checks |
| Game registry Create activation | Source/tests integrated | Pending; reviewed shutdown races have separate fixes in progress |
| Immutable game snapshots and reducer | Source/tests/registrations integrated | Save-failure snapshot regression observed red; fix awaiting green |
| Native input controller and mounted game UI | Source/tests/registrations integrated | Selection and stream-retry regressions observed red; fixes awaiting green |
| Reckless resource configuration and native factory | Source/tests/registrations integrated | Exact acknowledgment and retained cleanup checks pending |
| Shared engine resource allocator | Tests delivered; source pending | Not yet registered or run |
| Sibling continuation reuse regression | Source/tests integrated | Two failures observed before source fix; green pending |
| Save capacity waiting | Executor/repository source and tests integrated | Executor tests passed; assembled repository checks pending |
| Quit after committed game response | Coordinator regression and fix integrated | Red observed; green pending |

## Verification boundary

The last recorded focused green run preceded the newest bridge/coordinator
changes: 28 library, 164 domain and 32 presentation tests. The current assembled
working changes have not yet passed verification. The latest debug attempt stopped
at compilation; the registry async receiver and test deadline type corrections
are now integrated. Native adapter resource refresh is still awaited before the
next assembled check. Production playable composition remains unfinished. Task
completion remains governed by `../tasks.md`, not agent delivery or the existence
of a source file.
