# Engine search limit acceptance

T075 extends the existing private tagged search-limit model without changing
production code. Its constructors already enforce one exclusive limit and reject
empty scalar budgets, unsupported depths and signed-parser overflow.

Seven new engine tests cover both accepted scalar boundaries, independent
overflow in all four clock fields, optional moves-to-go at its supported
boundaries, exact actor commands for every finite mode and explicit infinite
analysis cancellation. Readiness cannot complete a search or its stop barrier;
finite bestmoves retain their search context and generation. Owned shutdown is
observed after the transcript exercises.

Live-game requests with submillisecond or overflowing move budgets, clocks or
increments fail before new-game configuration, position or search I/O. The actor
remains idle. Staged-clock fixtures advance each side independently and verify
different increments and the active side's moves-to-go, then remove moves-to-go
when both sides enter the final unlimited stage.

Checks for this increment:

```sh
cargo fmt --all
cargo clippy --locked --test engine -- -D warnings
cargo test --locked --profile dev --test engine
```

Targeted Clippy passed and all 88 engine tests passed, including seven new cases.
No production defect or behavior change was needed. T075 is accepted; broader
T072/T073 capability acceptance, real-engine coverage for every analysis mode,
analysis orchestration and presentation remain assigned to their own tasks.
The native harness, optimized builds and full repository gates were not rerun
for this test-only increment. The test command still reports the existing macOS
debug linker warning documented in [US1 evidence](us1.md).
