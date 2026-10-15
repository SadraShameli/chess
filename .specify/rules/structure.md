# Source structure and boundaries

## Current repository

The application is the `chess` Cargo package, edition 2024, with a starter
`src/main.rs`. Keep one package until independent compilation, reuse, or dependency
boundaries justify a workspace. Create source modules only when a specified
feature needs them.

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | Thin executable entry point and composition root |
| `src/lib.rs`, when needed | Deliberate reusable public API and module declarations |
| `src/<component>.rs` | Small cohesive component |
| `src/<component>/mod.rs` | Growing component's narrow module surface |
| `src/<component>/<concern>.rs` | Owned model, behavior, contract, or implementation |
| `tests/<suite>.rs` | Cargo-discovered public-contract integration test root |
| `tests/<component>/` | Included integration cases and local fixtures |
| `benches/` | Representative benchmark targets when there is behavior to measure |
| `examples/` | Compilable examples only when they serve an actual use case |
| `specs/` | Feature specifications, plans, tasks, and evidence |
| `.specify/rules/` | Canonical engineering policy |

## Dependency direction

- Model reusable domain components independently of infrastructure. Board or game
  state, when implemented, must not import commands, transport, persistence, or
  UI code. Engine components may consume domain contracts; adapters and command
  interfaces consume application APIs; the composition root wires implementations.
- A lower-level component owns the trait for an upward need. Dependency arrows
  never reverse through a concrete adapter. No cyclic component imports.
- Keep the crate root and `mod.rs` focused on declarations and deliberate exports.
  A tiny cohesive implementation can stay in its component's single source file;
  split by concern when it grows. Do not put unrelated behavior into `mod.rs`.
- Put configuration, enums, errors, and helper types beside their owning
  component. Cross-component types are shared only when they have genuinely
  shared semantics. Do not create generic `models`, `services`, `utils`, or enum
  dumping grounds.
- Expose the smallest visibility required: private, then `pub(super)` or
  `pub(crate)`, then `pub` only for a supported external contract. Public APIs
  must preserve validated invariants and remain testable without exposing internals.
- Module and type names follow [Rust style](rust-style.md). Test discovery and
  placement follow [testing](testing.md).
