# Canonical project rules

This directory is the single source of project engineering rules. These rules
adapt the owner's TradingBot architecture to stable Rust. No other checkout or
Claude integration is required. The constitution incorporates these files by
reference; feature artifacts record how they comply instead of restating them.

Read every required file in full before acting. Follow relative document links
when they govern the current task. A directory or type named in an example is
not a request to implement it.

| Rule | Read for | Owns |
| --- | --- | --- |
| [Code quality](code-quality.md) | Every task | No comments, DRY, scope, secrets, clear names |
| [Verification](verification.md) | Every task | Config ownership, gates, dependencies, authorization |
| [Architecture](architecture.md) | Rust design, source, tests, feature planning | OOP, contracts, composition, effect boundaries |
| [Structure](structure.md) | Rust design, source layout, feature planning | Modules, dependency direction, file organization |
| [Rust style](rust-style.md) | Rust source, tests, examples, benchmarks, feature planning | Prefix naming, types, enums, ownership, errors |
| [Performance](performance.md) | Design, Rust source, configs, tests, benchmarks, feature planning | Full CPU utilization, async I/O, memory, measurement |
| [Testing](testing.md) | Rust behavior changes, tests, benchmarks, feature planning | Deterministic checks, regressions, contract coverage |

A feature plan's Constitution Check must name the applicable files and provide
evidence for each. Read the complete set for a feature design or broad review.
Global owner instructions still apply. Where a more specific project decision
is needed, record it in the owning rule and refer to it from the feature plan.
