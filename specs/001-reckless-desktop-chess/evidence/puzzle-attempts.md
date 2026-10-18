# Puzzle attempt progression: T176, T182 and T183

Base: `8300a5e`. This slice extends the merged immutable definition boundary.
Source is ready for parent verification. Cargo remains reserved for T082; no
Cargo command has run in this lane and no passing Rust test result is claimed.

The selected policy comes from [FR-021 and story 8](../spec.md), the
[study-tools puzzle contract](../contracts/study-tools.md#puzzles), the
[puzzle data model](../data-model.md#reports-openings-and-puzzles), and
[selected lifecycle decisions](../decisions.md#puzzle-interchange-and-lifecycle).
The [canonical rules](../../../.specify/rules/README.md) govern this work.
All 56 checklist items are checked; the implementation prerequisite script
resolved the feature using `SPECIFY_FEATURE_DIRECTORY` because this isolated
worktree has no `.specify/feature.json`.

## Implemented boundary

`PuzzleSession` owns one current `PuzzleAttempt`, its injected nonzero session
and attempt identities, its checked revision, and an immutable countdown
configuration. The configuration stores exactly one mode and one positive
nanosecond-bounded duration. Defaults are 300 seconds for session mode and 60
seconds for per-puzzle mode. There is no player rating or points model.

Every mutating session operation requires a `PuzzleSessionContext`. Session,
attempt, complete source identity including dataset version, and revision are
checked before attempt changes or randomness. Revision overflow is checked
before the operation. Invalid phases, illegal moves, exhausted hints and invalid
random indices leave the session unchanged.

`PuzzleSolution` follows the definition's original node identities and child
ordering. Listed input advances to that exact child. Other input is checked by
the existing `BoardPosition::play` boundary. Illegal input returns its typed
board error without recording failure or changing the board. Legal unlisted
input records a failed primary outcome and a separate unlisted event, leaves the
board at the failed decision, and enables an explicit practice retry. No engine
port, search, generated alternative or second chess rules implementation exists.

Hints use the first source-listed continuation at the current node: source
square, destination square, then the exact move including promotion. Hints do
not play a move. Each accepted solver move resets the stage; assistance remains
attempt-wide. A retry resumes the same failed decision and accepted path,
preserving the current hint stage because no new solver turn was reached.
Repeated practice failures and later completion retain the failed primary
outcome. Practice retries/completion and assistance remain separate state/events.

Opponent turns are explicit transitions through an injected
`PuzzleRandomProvider`. `PuzzleRandomProviderSeeded` wraps the existing pinned
Rand implementation with an explicit seed. Only indices into listed children
are accepted. Each chosen reply records its move, node identity and original
child index in the attempt's in-memory typed event sequence. All source solution
lines and alternatives remain intact for future review.

Attempt-level skip and timeout operations finalize only unfinished attempts.
An unresolved attempt receives one skipped or timed-out primary outcome; a
failed attempt retains failure and gains a secondary event. Timeout records
the configured mode. Solving enters the review phase. These operations neither
start a countdown nor implement collection advancement or session expiry.

## Fixtures and checks

The 20 new `tests/puzzle/solution.rs` tests were written before implementation.
They cover valid, branching and transposed-path progression; illegal foreign-position and pinned
moves; legal unlisted failure; all hint stages, exhaustion, reset and assistance;
failed then practice solved; repeated retries; skip/timeout primary preservation;
deterministic listed replies and invalid random choices; terminal opponent
replies; Black to move; exact underpromotion reveal; configuration limits;
stale contexts; and illegal phase transitions.

The definition fixture's identity/line/definition constructors now live in
`tests/puzzle/fixture.rs` because both suites use them. Existing definition test
assertions remain intact. Both suites are registered in `tests/domain.rs`.

Executed checks:

- Standalone pinned `rustfmt` on explicitly owned and registration files: passed.
- `git diff --check`: passed.
- Source and registration inspection: 20 new tests; no authored comments, lint
  suppressions, dependencies, engine imports, wall sleeps or global randomness.

Rust compilation, targeted Clippy, test execution and a runtime red-first result
remain unverified until the parent grants Cargo access. Tests-first file order
is not presented as an executed failing test result.

## Remaining scope

T184 through T192 still own countdown readiness and charging, injected time,
pause/focus/minimize/sleep/relaunch behavior, review/Next and confirmed End Run,
collection selection/shuffling/exhaustion and pinned dataset lifecycle, durable
random decision persistence, checkpoint cadence and lag, storage failure
handling, histories/deletion/summaries, and desktop/UI integration. This slice
does not implement or verify those behaviors. The event sequence is in memory;
it is not a durable checkpoint or replay/import API.

The parent owns task markers, review, Cargo verification, publication and
worktree removal. This lane creates no commits or pushes.

## Parent verification

Integrated into `81e1566` with the countdown and summary consumers.
`cargo fmt --all` and targeted Clippy for the library, domain, engine and
presentation roots passed. All 241 domain tests passed, including all 18
definition cases and 20 new solution/attempt cases. All 55 library tests
also passed. Read-only timer/session review found no concrete blocker.
T176, T182 and T183 are accepted for this pure domain boundary. Native
lifecycle, persistence and product integration remain in their owning tasks.
The existing macOS linker warning remains unsuppressed.
