# Pure puzzle summary core

This patch supplies T188's source and the summary-only portion of T179. Its owned
files are `src/puzzle/summary.rs`, `tests/puzzle/summary.rs`, and this evidence.
It consumes the source-ready T182/T183 API from
`/Users/sadrashameli/.t3/worktrees/chess/t3-12dc45ac/src/puzzle/` without copying
or modifying that dependency. The parent owns integration, verification, and task
markers. T179 and T189 are not complete: persistence, shuffle history, confirmed
deletion, active-run deletion refusal, and dataset retention are outside this
patch.

The behavior follows [the selected puzzle contract](../contracts/study-tools.md#puzzles),
[the lifecycle decisions](../decisions.md#puzzle-interchange-and-lifecycle), and
FR-021 in [the specification](../spec.md). Applicable engineering rules were read
through [the canonical catalog](../../../.specify/rules/README.md): code quality,
verification, architecture, structure, Rust style, performance, and testing.

`PuzzleSummary::from_attempts` reduces borrowed `PuzzleSummaryAttempt` observations.
An observation pairs a `PuzzleSessionId`, the actual `PuzzleAttempt`, and the
caller's authoritative elapsed active `Duration`. It does not substitute an
outcome schema, copy a definition/tree/event log, inspect the clock, infer elapsed
time from countdown defaults, or call an engine or storage adapter.

Each recorded primary outcome contributes exactly once to the finalized total
and one of unassisted solve, assisted solve, failure, timeout, or skip. An attempt
without a primary outcome contributes no denominator entry. A recorded failure
is already a finalized primary result even while practice remains available;
practice completion, a later skip, or later expiry cannot reclassify it. Solve
classification comes from the assistance retained in the primary outcome.

Both solve rates retain their exact `u64` numerator and nonzero finalized `u64`
denominator. A zero denominator returns `None`; zero solves with finalized
failures/skips/timeouts returns a valid zero numerator. No floating-point
conversion, scaled multiplication, percentage rounding, or points system enters
the core.

Assisted-attempt counts use whole-attempt assistance. Hints, practice retries,
practice completions, skip events, session expiry events, and per-puzzle timeout
events are counted independently from the actual event log. These observations
and elapsed time include supplied unfinished attempts, allowing current-run
summaries without inventing a primary outcome. The caller supplies only active
countdown time, including replies, animations, hints and retries, excluding
pauses and between-puzzle review. Each observation's duration is its cumulative
active time, not an increment or a whole-session duration repeated per attempt.

The temporary identity index keys on `(PuzzleSessionId, PuzzleAttemptId)` and
borrows the complete source/version/puzzle identity. Duplicate keys return
`DuplicateAttempt`, including for unfinished attempts or different supplied
durations. A duplicate key with different provenance returns
`DefinitionConflict`. Callers must select one authoritative snapshot per attempt;
the reducer does not silently choose between revisions. The same source puzzle
under another session or attempt is a separate valid contribution.

Recomputation accepts any iterator over retained borrowed observations, so a
history owner can filter its records and recompute exact contributions without
cloning whole records. There is no total-library count limit or fixed worker cap.
The deterministic counter reduction is sequential and depends on prior totals;
the temporary index holds one compact identity key and source reference per
observation. All count increments and duration additions are checked. Counts
report their native `u64` overflow; elapsed totals use the full standard-library
`Duration` range instead of imposing the per-countdown nanosecond range. Errors
return no partial summary.

The 12 public-contract tests were authored before the core. They exercise every
primary outcome and both solve denominators, failed-then-practice-completed
history, repeated retries/hints and assistance, secondary skip and both timeout
modes after failure, empty/unfinished rates, recomputation after state changes,
order-independent aggregation, recomputation from retained records, repeated
source puzzles with distinct identities, duplicate finalized/unfinished records,
source-version conflicts, nanosecond precision above the floating-point integer
range, and the full elapsed-duration boundary. Three private boundary cases cover
finalized/event counter overflow and exact rate integers at `u64::MAX`. All use
real attempt/outcome objects and explicit durations; private counter seeding avoids
allocating billions of attempts merely to reach arithmetic boundaries.

Only the two owned Rust files were formatted. `rustfmt --check --edition 2024`
passed using installed rustfmt 1.10.0, and the staged patch passed
`git diff --cached --check`. Cargo was reserved by the parent and was not run.
No failing-test observation, executed test count, compilation result, Clippy
result, full gate result, or runtime performance measurement is claimed.

After integrating T182/T183, the parent must add `pub mod summary;` to
`src/puzzle/mod.rs` and register these cases in `tests/domain.rs`:

```rust
#[path = "puzzle/summary.rs"]
mod puzzle_summary;
```

Those are proposals, not edits in this patch. With the parent's explicit Cargo
grant, use `CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target` and run
focused library/unit and domain-summary checks sequentially before the owning
repository's remaining gates. Compilation and all 15 tests remain unverified at
handoff. No placeholders, dependency changes, deletion operations, timers,
selection behavior, desktop/UI work, commits, or publication are included.

## Parent verification

Integrated with the actual attempts/configuration and registered both test
roots. Formatter and targeted library/domain/engine/presentation Clippy
passed. All 55 library tests and 241 domain tests passed, including the
three private arithmetic cases and 12 public summary cases. T188 is
accepted for the exact pure summary calculation. T179/T189 history and
deletion, persistence, and native rendering remain open. The existing macOS
linker warning remains unsuppressed.
