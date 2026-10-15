# Rust testing

- Add checks for meaningful behavior, invariants, error boundaries, and contracts.
  A bug fix includes a regression test observed failing against the original
  defect before the fix. Do not add scaffolding for instruction-only changes or
  tests that merely repeat the implementation.
- Use local `#[cfg(test)]` unit-test modules for private behavior. Public API
  integration tests mirror their owning source component under `tests/`. Nested
  files must be included from a Cargo-discovered test root; arbitrary nested
  integration files are not discovered automatically.
- Prefer real domain objects and small in-memory implementations of boundary
  traits. Place shared fixtures next to their component. Do not expose internal
  methods solely for testing or mock away the behavior being checked.
- Tests are deterministic and hermetic: fixed seeds, explicit clocks, temporary
  paths, no real network, no wall-clock sleeps, no live credentials, and no writes
  to the owner's data. A missing expected fixture is a failure.
- Drive contract coverage from the canonical registry. Fail on an empty registry
  or a member never exercised. Cover distinct behavior without multiplying
  unrelated parameter axes. Do not hide missing coverage behind successful
  early returns; apply the checker policy in [code quality](code-quality.md).
- Concurrency changes need checks for cancellation, error propagation, bounded
  capacity, ordering where promised, and graceful shutdown. Use controlled
  synchronization and time instead of flaky sleeps. Test one-worker and shared
  maximum-worker behavior when scheduling can affect outcomes.
- For implemented make/unmake, serialization, or incremental computations, verify
  a round trip or compare with full computation where that is the invariant.
  Performance evidence follows [performance](performance.md).
- Test functions may use assertions and return typed `Result` values. Use `?`
  for setup errors instead of panic-based helpers. Report actual test counts and
  unverified behavior; zero starter tests do not prove chess correctness.
