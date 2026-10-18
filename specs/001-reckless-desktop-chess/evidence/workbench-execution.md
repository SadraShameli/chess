# Isolated workbench execution

T082 is prepared in `/Users/sadrashameli/.t3/worktrees/chess/t3-fe1c1bc0`, based
on pushed `b8c5dae0c14c456e3445411d363d1ae8ec64cced`. The parent owns integration,
the task marker, commit, push and worktree removal. The model contract is
[the T081 job evidence](workbench-job.md).

`WorkbenchExecutor<P>` takes an already admitted `WorkbenchJob` and the caller's
current `ClockInstant`. It binds the remaining absolute job deadline to Tokio's
monotonic clock, including elapsed admission/queue time. It never discovers CPU
capacity, creates a pool, clamps resources, acquires another lease, restarts a
child, accepts an executable path, or executes a shell. The injected process
provider receives only the exact selected package retained by the job. The native
provider launches that verified app-owned package directly.

The public lifecycle is `new(job, now)`, `start(provider)`, `run()`,
`cancel(job_id)`, and `shutdown_retained(deadline)`. `job()` exposes typed
state/progress/outcome and bounded diagnostics; `is_shutdown_complete()` reports
observed resource completion. Callers inspect the job outcome after `start` and
`run`: an operation failure is a terminal typed job outcome, while a cleanup
error returns an executor error and retains the owner for another explicit
cleanup wait. A failed setup can already finish the job, so callers run only
jobs still in Running state. `start` cannot be repeated after interrupted setup.
The retained cleanup method remains available after any interrupted wait.

The executor stores the startup owner, established session, incremental parser,
pending completion and first cleanup failure on itself. Dropping a borrowed
setup, run, cancel or shutdown future leaves those fields, the child and the
job's actual CPU lease with the caller. There are no detached executor tasks.
Only observed child exit and completion of both existing bounded pipe readers
permit `finish_reaped` and release of the job lease. Cleanup failures retain the
operation failure and cleanup cause. Retried cleanup preserves an earlier
cleanup failure in the final outcome. Cancellation requests retain their reason
and time even when cleanup fails. Cleanup waits are capped by the canonical
three-second native bound.

`engine/diagnostic.rs` is a crate-private closed effect boundary, consumed only
by the executor. The session exposes crate-private diagnostic begin/read/complete
and forced retained cleanup methods. It reuses the canonical UCI identity and
capability handshake, typed resource and variant settings, acknowledgments,
readiness, line limits, bounded diagnostic collector and process shutdown.
There is no engine-to-workbench dependency or desktop raw-command API. Setup
configures the exact admitted thread count and benchmark hash. Bench's own
private search pool receives those same validated values; the outer UCI pool
is idle while bench runs.

All parsers were based on the actual pinned upstream files at
`0e92358f5acd66e5ac77b1bf558202e47c515435`: `src/uci.rs`,
`src/board/parser.rs`, `src/tools/perft.rs`, `src/tools/bench.rs`, and
`src/threadpool.rs`. The prepared UCI and threadpool snapshots and
[engine provenance](engine-build.md) were checked before implementation. The
perft and bench files were fetched by exact commit, without editing vendor code.

- Inspector validates the complete ASCII board, ranks, files, separators and
  final FEN against the requested validated domain position and history.
- Eval parses the upstream bare signed White-perspective NNUE integer through
  `EngineAnalysisStaticEvaluation`. It never converts it to search centipawns.
- Compiler requires the three pinned named lines in order and checks the target
  against the selected package. Compiler/features remain bounded named fields.
- Perft requires the upstream table grammar and closing separator, sequential
  root indices, domain-legal moves, every legal root exactly once, checked totals,
  and depth-one root counts. Displayed seconds and kN/s are converted with checked
  decimal arithmetic, avoiding floating-point acceptance of NaN/infinity.
- Bench requires the upstream pretty UCI table, rows 0 through 44 exactly once,
  checked aggregate nodes and final summary/closing separator. Indexed rows
  advance typed progress. Default parameters remain 16 MiB, one thread and depth
  12. Semantic native acceptance uses explicit depth one rather than claiming a
  full default-depth performance benchmark.

Perft and bench are synchronous on the pinned engine's main command loop. An
interrupted operation uses forced terminate/kill and observed reap, never stdin
`stop` or an `isready` completion sentinel. Complete valid output permits normal
quit and observed cleanup. Malformed, incomplete, oversized, EOF, failed setup,
expired and cancelled output cannot become successful results. Meaningful stderr
and informational output remain in the existing bounded diagnostic tail.

Tests were authored before implementation. Cargo was explicitly reserved by the
parent during source work, so no preimplementation red test run is claimed.
After the exclusive lane grant, all checks reused
`CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target` sequentially. Only
owned files were formatted. No dependency, Cargo configuration, task marker,
presentation, puzzle, tablebase, vendor, commit or release change is included.

Verification results are recorded below after the final source checks. The
existing macOS linker warning `__eh_frame section too large (max 16MB)` remains;
the linker states that `linker_messages` ignores `-D warnings`. No suppression
was added.

The work does not establish full application integration, UI/export behavior,
full-performance or 1,000-cycle acceptance, PGO, cross-platform execution, or
release readiness.

Final sequential checks after the last source/test edits:

```sh
rustfmt --edition 2024 --config skip_children=true src/engine/mod.rs src/engine/diagnostic.rs src/engine/session.rs src/workbench/mod.rs src/workbench/executor.rs src/workbench/output.rs src/workbench/job.rs tests/engine.rs tests/native.rs tests/native/workbench.rs tests/workbench/executor.rs
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo clippy --locked --lib --test engine --test native -- -D warnings
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --test engine
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --profile dev --test native -- --workbench /Users/sadrashameli/Personal/chess/resources/engine
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --profile dev --test native -- --game-composition
rustfmt --edition 2024 --config skip_children=true --check src/engine/mod.rs src/engine/diagnostic.rs src/engine/session.rs src/workbench/mod.rs src/workbench/executor.rs src/workbench/output.rs src/workbench/job.rs tests/engine.rs tests/native.rs tests/native/workbench.rs tests/workbench/executor.rs
git diff --check
```

Clippy passed without warnings. The complete engine integration suite passed
175 tests, zero failed, zero ignored and zero filtered out. This includes all
15 new executor cases and the existing 14 T081 cases. The new cases cover the
five-operation inventory, transcripts, malformed/oversized/partial/EOF output,
perft root legality/completeness/uniqueness/counts, raw-NNUE/search-cp distinction,
compiler target mismatch, benchmark indices/metrics/overflow/defaults, explicit
32 MiB/two-thread/depth-one identity, admission saturation, queue/setup/run
deadlines, cancellation, dropped setup/run/cancel/shutdown waits, spawn failure,
combined operation/cleanup failures, retained retry, exact lease recovery, and
live-session isolation. The initial 11-case executor-only run also passed before
the four additional boundary cases were added. The first Clippy invocation
found a large enum variant; the session variant now uses one owned box and all
subsequent Clippy checks passed.

The focused native workbench mode passed all five actual pinned-engine
operations. It checked inspector history, raw evaluation, compiler target,
standard depth-two perft total 400, all 45 benchmark positions at explicit depth
one, unchanged parameter defaults 16/1/12, and forced cancellation/reaping of a
real Kiwipete depth-twelve perft. A separate live session retained its process
generation and answered readiness afterward. Every diagnostic recovered its
exact allocation. Missing engine packages fail selection; there is no skip path.

The final native game-composition run, after all source/test fixes, passed two
real-engine mate games, two post-result rejections, durable one-move restoration,
frozen clocks, and completed-tab exclusion on relaunch. Both production
bootstraps recovered 14/14 CPU slots, acquired a full-capacity lease, observed
zero direct children and reacquired the exclusive profile. The existing pinned
binary was copied into the worktree's ignored `resources/engine/bin/` solely to
supply the production bootstrap's normal resource lookup. No binary is staged.
Both native modes verified the same existing portable macOS ARM package, SHA-256
`ab4128cd33149b14713475e049db8ddaff702ee79bfb412ecfbde91655273689`.
Scoped formatter checks and `git diff --check` passed. No Cargo checks remain
running in this lane.

## Parent integration correction

Integration at `8300a5e` exposed duplicate, reordered diagnostic history: the job
recorded received output and then appended the session's complete startup/output
tail again during cleanup. A new regression failed with the board separator as
the first entry instead of the UCI identity.

The session/startup owner now retains the single canonical tail while active.
`WorkbenchExecutor::diagnostics()` borrows that owner; only observed final cleanup
moves its tail into the job. Dropped cleanup waits retain the original tail.
Per-line job timestamps still advance without copying diagnostic payloads.
Two parent regressions cover ordering, exactly-once result lines, live borrowed
diagnostics, observation time and final allocation recovery.

Final parent checks passed: `cargo fmt --all`,
`cargo clippy --locked --lib --test engine --test native -- -D warnings`,
`cargo test --locked --profile dev --test engine -- --quiet` (177 passed),
`cargo test --locked --profile dev --test native -- --workbench resources/engine`
and `cargo test --locked --profile dev --test native -- --game-composition`.
The diagnostic run again covered all five operations, real perft cancellation
and lease recovery. Both game bootstraps again recovered 14/14 CPU slots and
zero children. The existing linker warning remains. T082 is accepted after
these checks; desktop wiring, exports and broader release gates remain separate.
