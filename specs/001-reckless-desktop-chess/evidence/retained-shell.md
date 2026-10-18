# Retained shell checkpoint

Recorded 2026-10-16 on the macOS ARM64 development host before native-host
replacement. This checkpoint preserves the staged application composition and
the recoverable changes from the earlier worktrees.

The earlier `work/001-bootstrap` and `work/001-board-split` commit histories are
already ancestors of the feature branch. The bootstrap worktree's staged source
matches the preserved main index. The board worktree contributes three unique
file changes: deferred scene/GLTF loading and production chunk separation.
Those changes were recovered by binary patch in an isolated validation worktree.
An AbortController preserves teardown checks across both asynchronous loading
boundaries while satisfying strict control-flow analysis.

The retained application composes the shared capacity owner, SQLite actor,
verified Reckless startup and bounded desktop snapshots. SQLite operations hold
capacity only during execution. The former browser host remains until native
parity is implemented and verified. This checkpoint does not implement gameplay.

The optimized native smoke initially failed with SIGABRT before readiness.
Direct launch identified `resource_dir()` returning UnknownPath: a custom-protocol
test executable was outside a macOS application bundle. Explicit native-test
builds now select workspace resources. Production bundled resource selection is
unchanged. The same smoke then passed against the corrected optimized build.

Executed in the isolated validation worktree:

- Strict formatting, generated-contract consistency, TypeScript, ESLint,
  Stylelint and all-target/all-feature Clippy passed with no warnings.
- All 53 Rust tests passed, including storage admission, capacity release,
  transactional receipts, engine protocol and validated position boundaries.
- All 40 Chromium tests passed across ten files.
- The optimized frontend and all-target Rust release builds passed.
- The optimized WebKit desktop smoke passed: Reckless was ready, its selected
  engine matched the pinned package, the 3D canvas loaded and 64 semantic squares
  were available. A screenshot was saved under ignored test results.
- No task-owned chess or Reckless process remained after the smoke.

The application used temporary test storage. No owner profile was opened.
These results verify only the retained shell. Bevy/egui graphics, native
accessibility, production profile ownership and other operating systems remain
separate migration gates in [tasks.md](../tasks.md).
