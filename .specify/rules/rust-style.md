# Rust style, naming, and type safety

## Names and ownership

- Type names use `UpperCamelCase` with their owning component path as a prefix.
  Modules, files, methods, fields, and variables use `snake_case`; constants use
  `SCREAMING_SNAKE_CASE`. Acronyms use Rust casing, such as `Api` and `Fen`.
- Apply component prefixes to public and private owned types. Omit `src/`, the
  crate root, and organization-only filenames such as `mod.rs` from the prefix.
  Do not add a redundant `Chess` prefix to every type.

| Location | Type name |
| --- | --- |
| `src/board/piece.rs` | `BoardPiece` |
| `src/board/enums.rs` | `BoardEnumColor`, `BoardEnumPieceKind` |
| `src/engine/search/config.rs` | `EngineSearchConfig` |
| `src/engine/search/error.rs` | `EngineSearchError` |
| `src/engine/evaluator/base.rs` | `EngineEvaluatorBase` trait |
| `src/engine/evaluator/material.rs` | `EngineEvaluatorMaterial` |
| `src/engine/evaluator/registry.rs` | `EngineEvaluatorRegistry` |

- Enum names include `Enum` after their owning path. Variants express the case
  without repeating the type, such as `BoardEnumColor::White`. Error enums use
  the conventional `Error` suffix and semantic variants.
- Nested or extracted types retain their parent's semantic prefix. Avoid bare
  `Manager`, `Helper`, `Utils`, `Data`, or `Base` names. Use `is_` and `has_` for
  predicates. Port traits may use a semantic role suffix such as `Provider` or
  `Repository` rather than a meaningless interface prefix.
- Keep enums in their component's `enums.rs`, or a focused file when substantial.
  CLI options belong to their command level. Do not make the domain import them.
- Import through `crate::`, an external crate name, or a narrow public re-export.
  Avoid chains of `super::`, wildcard imports, and glob exports.

## Types and ownership

- Model payloads, configs, commands, and results with concrete structs, enums,
  newtypes, or bounded generics. Use `Option` for absence and `Result` for failure.
  Keep fields private and expose typed operations preserving invariants.
- Parse and validate untrusted input once using fallible constructors, `TryFrom`,
  or `FromStr`. Deserializers must not bypass validation, including through derives.
  Keep validated types intact after the boundary.
- Use newtypes for values with different semantics, ranges, or units. Model
  closed selections as enums and match exhaustively. Do not hide new enum cases
  behind wildcard branches or represent invalid state combinations with booleans.
- Define domain strings, numbers, thresholds, and selections once on their typed
  owner, as enum behavior or associated constants. Derive names and paths in typed
  methods. Rust formatting inside such a method is valid; stored placeholder
  templates for callers to substitute are not domain values.
- Keep fields statically typed. Dynamic JSON, string-based field lookup,
  heterogeneous maps, and `Any` downcasts remain outside domain logic. A
  homogeneous map keyed by a typed identifier is valid.
- Default to immutable values and borrowing. Select `&self`, `&mut self`, or
  consuming `self` deliberately. Fix ownership or lifetimes rather than cloning
  to appease the compiler. Derive `Copy` or `Clone` only when semantically valid.
- Shared ownership and interior mutability require a demonstrated need. Use safe
  Rust only, without `unsafe` or unchecked conversions. Use checked numeric
  conversions and arithmetic where input ranges or overflow affect invariants.
- Prefer constructors returning `Self` or `Result<Self, OwnerError>`. Above five
  parameters, use a typed options object. Return values callers must handle can
  use `#[must_use]`; do not discard an error or required task result.

## Errors and outcomes

- Own errors on the component as enums or structs implementing
  `std::error::Error` and `Display`. Include typed context, preserve sources, and
  propagate with `?`. Human-readable messages belong to the error type.
- Production code must not use `unwrap`, `expect`, `panic!`, `todo!`, or
  `unimplemented!`. Represent expected conditions as typed outcomes. Never turn a
  failure into an empty collection, `None`, `false`, or another apparent success.
- An intentional fallback must be explicit in the contract and surfaced at the
  effect boundary. Reuse the logging facility there and preserve silent success;
  pure calculations must not acquire hidden logging effects.
- See [performance](performance.md) for allocation, dispatch, and synchronization
  decisions and [code quality](code-quality.md) for source-content restrictions.
