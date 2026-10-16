# Typed diagnostic job core

T081 source and public-contract tests were prepared in the isolated worktree
`/Users/sadrashameli/.t3/worktrees/chess/t3-c982b06c`, based only on committed
`f74c168ab4c7fef7c782644392df8dad2445f8d9`. This evidence proposes acceptance of
the model increment; the parent owns integration, task markers and commits.

`src/workbench/` owns the job, tagged parameters, operation inventory, validated
results, cancellation and transitions. The operation inventory covers inspector
`d`, raw static evaluation, compiler diagnostics, perft with a root divide, and
the built-in 45-position benchmark. Tests exercise every inventory member through
an admitted start and successful terminal result.

The job retains the actual `EnginePackageSelected`, including its verified
executable, digest, target and profile. Position operations retain the existing
`EngineSearchPosition` and its validated history. Inspector results must match
the requested board. Raw evaluation reuses `EngineAnalysisStaticEvaluation` and
keeps its White-perspective integer distinct from search centipawns. Compiler
results validate their package target and bounded named informational fields.
Perft results reuse domain legal moves, require every root exactly once, check
the total with checked arithmetic, and match the requested position and depth.
This adds no chess parser or rule engine.

The default deadline is 60 seconds, with a positive duration capped at ten
minutes. Its absolute instant is computed at construction and includes queue
time. Perft defaults to depth 4 and accepts 1 through 12. Bench keeps upstream
hash/thread defaults from discovered canonical option descriptors and depth 12.
Explicit resources use `EngineResourceConfig::number`; depth uses
`EngineSearchLimit::depth`. Dynamic advertised thread bounds remain supported.
The model adds no memory-budget owner or resource-discovery mechanism.

All CPU leases arrive from the caller's shared `CapacityBudget`. Admission
requires an exact worker match, including explicit benchmark threads. The job
never discovers capacity, acquires a lease or creates workers itself. Start
requires prior admission. Queued/admitted jobs have no child; cancellation may
release those allocations immediately. Running cancellation enters Cancelling
and retains its lease, typed identity, deadline, progress and diagnostics until
`finish_reaped` receives observed executor completion. Result, process, analysis
and combined operation/cleanup failures remain typed, with their error sources.
A late result is retained under a timeout outcome rather than accepted as
success. Stale identity/time, repeated transitions, regressing/excess progress,
wrong operation and mismatched result context fail explicitly.

Time and job identity are supplied by the caller. The model reads no clocks and
performs no I/O. Progress is bounded by one operation or 45 benchmark positions.
Informational output reuses `EngineDiagnostics` and its existing retention
bounds. Compiler strings and perft roots are stored as boxed slices after
validation so spare input-buffer capacity is not retained.

The following targeted checks ran sequentially after the parent granted the
exclusive Cargo lane, reusing the established target directory:

```sh
rustfmt --edition 2024 --config skip_children=true src/workbench/mod.rs src/workbench/job.rs src/workbench/enums.rs src/workbench/parameters.rs src/workbench/result.rs tests/workbench/job.rs
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo clippy --locked --lib --test engine -- -D warnings
CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --test engine workbench_job
rustfmt --edition 2024 --config skip_children=true --check src/workbench/mod.rs src/workbench/job.rs src/workbench/enums.rs src/workbench/parameters.rs src/workbench/result.rs tests/workbench/job.rs
git diff --check
```

Final results: Clippy passed without warnings; scoped formatting and diff checks
passed; 14 Workbench tests passed, zero failed and zero ignored, with 118 existing
engine tests filtered out. The test build emitted the previously recorded macOS
linker warning `__eh_frame section too large (max 16MB)` for the existing
application binary. The linker reports that `linker_messages` ignores
`-D warnings`; no suppression was added. See [US1 evidence](us1.md).

Tests were written before source. The first authorized test invocation failed
with E0583 for the then-missing `workbench/job.rs`; this demonstrates the absent
API and is not a claimed behavioral regression run. The first implemented
version passed 11 tests. Review then added discovered resource bounds, compact
result buffers and three additional boundary/context checks; the final 14-test
run and Clippy both passed.

T082 must own the job together with its isolated child through cleanup. It must
call `start` before attempting child creation, while admission still denotes no
child, and observe that no child remains before calling `finish_reaped`. A spawn
failure after start can be finished with its typed process error because no
child was created. Dropping a cancellation future must leave that owner alive;
this model does not provide detached cleanup or an executor. Actual subprocess
execution, output parsers, kill/reap behavior, diagnostic UI, export adapters,
native integration, whole-project verification and measured performance remain
T082/later or parent integration scope. No diagnostic process was launched and
no release or task-completion marker was changed for this increment.

Parent integration at `f0317cf` preserved the analysis and tablebase test
registrations when resolving the shared test-root merge. Consolidated
`cargo fmt --all`, `cargo clippy --locked --lib --test engine -- -D warnings`
and `cargo test --locked --profile dev --test engine` passed. The integrated
engine suite contains 160 passing tests, including all 14 job cases and 13
tablebase cases, with zero failures or ignored tests. T081 is accepted after
these checks. The same existing macOS linker warning remains.
