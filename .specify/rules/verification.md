# Tooling, verification, and work discipline

## Canonical configuration

| File | Owns |
| --- | --- |
| `rust-toolchain.toml` | Exact stable toolchain and required components |
| `Cargo.toml` | Edition, minimum Rust version, dependencies, lint policy, build profiles |
| `Cargo.lock` | Reproducible resolved application dependencies |
| `rustfmt.toml` | Stable formatter settings |
| `clippy.toml` | Project-specific Clippy thresholds |
| `Makefile` | Verification and build commands |

- Use the pinned stable toolchain. Rustup honors `rust-toolchain.toml`; when using
  standalone Cargo, verify its compiler version against the pin. Do not silently
  install a replacement compiler. Keep minimum-version declarations and the pin
  consistent; a future lower MSRV requires its own checks.
- Standard Clippy lints and selected compatible strict lints are enforced through
  Cargo, including conversions, panic-based error paths, accidental cloning, and
  concurrency hazards. No blanket pedantic/restriction group or documentation
  lint is enabled when it contradicts owner policy. Configuration is not an
  exemption mechanism; do not add allowances or disable checks to pass a gate.
- Keep dependency releases stable and tagged, verify their actual APIs, inspect
  license and security implications, and maintain the lockfile. Add a library
  only for a concrete requirement. Python tooling, including `specify`, uses
  uv-managed Python, never Homebrew Python.
- All build profiles are defined in the manifest. Release uses maximum portable
  optimization: opt-level 3, fat LTO, one codegen unit, no overflow or debug
  assertions, stripped symbols, and unwinding panics because the CPU executor
  isolates worker panics. Dev and test compile dependencies at opt-level 3.
  Benchmark further tuning, such as PGO, according to
  [performance](performance.md). Do not add machine-specific compiler flags.

## Required gates

- `make verify` is the canonical final command. It runs formatters before
  checkers, then Clippy, tests, and an optimized build. Individual workflows are
  `make format`, `make lint`, `make tests`, `make build`, and `make profile-build`.
  `make run` starts the optimized release build, `make debug` a debuggable
  build, and `make run-debugging` an optimized build with full debug info;
  `ARGS` passes application arguments.
  Read their actual command definitions in `Makefile`; do not duplicate them in
  documentation or hooks.
- Complete related edits first, then run final gates sequentially once. Repeat
  only after failure or a subsequent change. During implementation use targeted
  checks for the affected component. Gate ordering does not cap internal CPU
  utilization; see [performance](performance.md).
- A single task or increment needs only the narrowest check proving its change:
  `cargo check` of the touched code and the affected test target or name filter.
  Run `make verify`, full Clippy selections or all suites only at a planned
  checkpoint task, before an owner-requested commit, or when the owner asks.
  Report which focused checks ran and that the full gate did not.
- Generate a missing lockfile before locked commands. `make tests` uses Cargo's
  default test selection so library doctests run automatically when a library
  exists. Clippy and build include all targets. Check feature alternatives
  separately if they cannot all be enabled together.
- Zero warnings and zero errors are required. Source changes need relevant
  behavior checks; instruction-only work needs valid references, resolved
  placeholders, rule consistency, and toolkit health checks. Review architecture,
  naming, no-comment compliance, and performance evidence explicitly because
  compiler checks do not prove them.
- No tools run automatically through hooks. Installed Spec Kit templates,
  scripts, skills, and third-party dependencies remain vendor-managed; use
  documented customization mechanisms.

## Authorization and reporting

- Work inline unless the owner asks for agents. Keep edits surgical. Never
  commit or push unless requested. Use Conventional Commits without trailers,
  stage explicit paths, push only a requested feature branch, and never use bare
  Git stash operations. Deployments, publication, external messages, and
  destructive data operations need owner authorization.
- Report changes, executed checks, actual test counts, and material limits.
  Separate measured performance from design intentions. Do not claim runtime
  parallelism, async behavior, or chess correctness before those features exist.
