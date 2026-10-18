# User Story 8 acceptance evidence

Deterministic suites are registered through `tests/domain.rs` (puzzle modules) and
`tests/storage.rs` (`storage_puzzle_repository`). Native presentation suites run in
the library test target (`src/presentation/puzzle/{panel,view}.rs` test modules).

Focused results at this revision: `cargo test --test domain puzzle` 153 passed;
`cargo test --test storage puzzle` 32 passed; `cargo test --lib puzzle` 168 passed.

| Scenario | Owning tests |
| --- | --- |
| 1 accepted solution progression and completion | `tests/puzzle/solution.rs` |
| 2 incorrect or illegal attempt keeps the position intact | `tests/puzzle/solution.rs` |
| 3 invalid data reported before presentation | `tests/puzzle/definition.rs`, `tests/puzzle/csv_import.rs` |
| 4 focus loss, sleep, pause and relaunch hide content and require Resume | `tests/puzzle/checkpoint.rs` (`real_focus_sleep_and_puzzle_window_hiding_preserve_time_until_explicit_resume`, `crash_recovery_loses_only_one_second_after_the_last_committed_countdown`), `tests/presentation/puzzle.rs` |
| 5 progressive hints, unlisted legal move, illegal input | `tests/puzzle/solution.rs`, `tests/presentation/puzzle.rs` |
| 6 failure then practice solve, separate hint and practice counts, no overlap | `tests/puzzle/summary.rs`, `tests/presentation/puzzle.rs` |
| 7 either timer mode expires once, paused and review time excluded | `tests/puzzle/timer.rs` |
| 8 shuffle without repeat, exhaustion, explicit reshuffle, persisted order | `tests/puzzle/selection.rs`, `tests/puzzle/history.rs` |
| 9 history, zero-denominator rates, confirmed deletion with recomputed summaries, active-run refusal | `tests/puzzle/history.rs`, `tests/puzzle/summary.rs`, `tests/storage/puzzle_repository.rs`, `tests/desktop/native/puzzle_service.rs`, `tests/presentation/puzzle_panel.rs` |

Checkpoint evidence: a request is made every 250 ms with at most one pending and
one coalesced snapshot, the countdown freezes before advancing more than one
second beyond the durable checkpoint, a failed save exposes Retry Save, and
recovery always waits for explicit Resume (see `tests/puzzle/checkpoint.rs`).

Deletion evidence: attempt, session and all-history deletion require explicit
confirmation, refuse the run in progress, retain datasets, record attempt
tombstones (migration `0004_puzzle_attempt_deletion`) and recompute summaries
without the deleted attempts.

Attempt-level delete UI: the opened run lists its archived attempts with a
Delete Attempt control (finished runs only) that opens the existing
confirmation modal. Written, not compiled or run; no test added.


Not covered here: real-display minimize behavior on each operating system and
screen-reader output, which need external hardware evidence.
