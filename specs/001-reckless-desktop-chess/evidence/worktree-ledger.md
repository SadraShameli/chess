# Implementation worktree ledger

Last reconciled: 2026-10-16. Integration branch:
`001-reckless-desktop-chess`. Last pushed checkpoint: `ff20477`.

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
| `plan_review` | `engine-lifecycle` | `work/001-engine-lifecycle` | Desktop game supervisor, lifecycle and save scheduling | Tests integrated; corrected source pending |
| `selection_review` | `profile-foundation` | `work/001-profile-foundation` | Board input controller, native game application UI | Controller patches delivered; application mounting in progress |
| `story_coverage` | `native-harness` | `work/001-native-harness` | Reckless resource configuration and exact option acknowledgments | Resource tests delivered; source in progress |

The main checkout at `/Users/sadrashameli/Personal/chess` owns the native game
bridge, coordinator, composition root, scene synchronization and integration.
There are three active child agents and eight Git worktrees including this
checkout. The historical agent names in older session metadata are not active
assignments.

## Inactive worktrees

| Worktree | Branch | Recorded base | State |
| --- | --- | --- | --- |
| `activation-foundation` | `work/001-activation-foundation` | `945c863` | Dirty; preserve and compare before cleanup |
| `board` | `work/001-board-split` | `7de3a7b` | Dirty; preserve and compare before cleanup |
| `bootstrap` | `work/001-bootstrap` | `ce2bb9d` | Dirty; preserve and compare before cleanup |
| `bootstrap-validation` | `work/001-bootstrap-validation` | `6bb1434` | Dirty; preserve and compare before cleanup |

These directories contain historical work, including files from the removed
web frontend. Their contents have not been fully reconciled against current
native implementation. A dirty file count does not establish unique unfinished
work because many earlier patches were integrated without advancing the agent
branch. No inactive worktree has been deleted or declared fully integrated.

## Pending integration queue

| Delivery | Integration | Verification |
| --- | --- | --- |
| Game supervisor | Tests present; corrected source awaited | Pending assembled desktop checks |
| Immutable game snapshots and reducer | Source/tests present; reducer registration pending | Snapshot extensions and reducer pending |
| Native input controller | Tests/source delivered | Pending |
| Application game UI | Tests delivered; source pending | Pending |
| Reckless resource configuration | Tests delivered; source pending | Pending |
| Sibling continuation reuse regression | Tests/source delivered | Red-first check pending |
| Save capacity waiting | Executor integrated; repository patch pending | Executor tests passed; repository integration pending |

## Verification boundary

The last recorded focused green run preceded the newest bridge/coordinator
changes: 28 library, 164 domain and 32 presentation tests. The current assembled
working changes have not yet passed verification. Production playable wiring is
still in progress. Task completion remains governed by `../tasks.md`, not agent
delivery or the existence of a source file.
