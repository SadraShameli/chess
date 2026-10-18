# Native engine and puzzle persistence integration

This increment starts from `7ce747f` on `001-reckless-desktop-chess`.
The parent integrated the isolated source patches, retained their authored
regressions, and performed the combined checks on the shared checkout.

## Executed hermetic checks

Both canonical Clippy selections passed with all features and `-D warnings`,
including the separately selected native target. The final full test selection
passed all 975 tests, with no failed, ignored or filtered cases:

| Suite | Passed |
| --- | ---: |
| Library | 84 |
| Tools | 3 |
| Assets | 5 |
| Desktop | 213 |
| Domain | 293 |
| Engine | 222 |
| Presentation | 102 |
| Scene | 11 |
| Storage | 42 |

The application binary and library doctest selections contain no tests. All
commands use the pinned Rust 1.99.0 and the shared target directory with Cargo's
detected parallelism. No checker suppression or dependency change was added.
The debug linker reports its existing macOS `__eh_frame` size warning; this
warning remains unsuppressed and is separate from the warning-free Clippy runs.

Observed failing regressions before their corresponding fixes include:

- Four same-revision selection receipt cases, followed by a late export receipt.
- Nine initial Quit/export routing cases, then four additional original-deadline
  cases without pending engine work or while awaiting a native dialog.
- Divergent journal-prefix admission with matching event count and storage
  revision, and delayed save completion acknowledged using pre-await time.
- Live admission acquiring spare capacity before the active analysis cleanup
  was observed, and stale diagnostic progress following terminal delivery.

The canonical `make verify` sequence passed, including both optimized build
selections. The optimized active-search composition check passed in a temporary
macOS resource-layout fixture, and the optimized standalone workbench acceptance
passed all five operations and cleanup checks. Both development and optimized
native game-composition acceptance passed both real-engine mates and both 14/14 CPU,
zero-child and exclusive-profile recovery checks. The independently observed
immediate-Stop upstream race is documented in
[composition evidence](native-analysis-composition.md#parent-integration-and-immediate-stop-boundary).

The final optimized application SHA-256 is
`0e83260e0413a3c279523e0936ff737f6e3500293c89192c48a0f8ce472c0709`.
The optimized native harness SHA-256 is
`60391f6b0747185b461592ff73e48737f7baca5a10a11997cbd2542c4171b616`.
Executed gate logs and baseline regression failures are retained locally under
`.git/parallel-patches/integration-20261016/`.

## Worktree reconciliation

All six completed worker checkouts and their six local branches were removed.
Only the root checkout remained when that cleanup finished. The subsequent
T087 and T088 source-only tasks have two explicitly tracked active worktrees.
Complete archives, staged and unstaged patches,
untracked setup, ignored feature metadata, reconstruction checks and parent
refinement comparisons are retained under `.git/parallel-patches/` in:

- `completed-worktree-cleanup-20261016T202156Z/`
- `completed-worktree-cleanup-20261016T203301Z/`
- `completed-worktree-cleanup-20261016T203536Z/`

Cleanup preserved the parent's index and working state, current and release
branches, and remote refs. Worker source reports remain historical evidence of
their own limited checks; this report records parent integration results.

## Remaining acceptance

This increment does not complete all 13 journeys. Puzzle native checkpoint
cadence, freeze/error orchestration and history/UI integration remain open.
Authentic positive tablebase loading, Windows alias-parent selection, actual
GPU and accessibility acceptance, packaged platform coverage, capability-build
workflows and measured performance retain their own task and evidence owners.
The upstream immediate-Stop race has no product workaround in this increment.
