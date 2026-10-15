# Code quality

## No code comments

- Never write line comments, block comments, Rust doc comments, prose docblocks,
  file headers, dividers, TODO comments, or commented-out code in authored source,
  tests, examples, benchmarks, scripts, or configuration. This includes `//`,
  `/* */`, `///`, and `//!` comments. Explain decisions in Markdown documentation,
  specifications, or commit messages. Preserve comments in untouched third-party
  and tool-generated files; do not edit vendor assets to remove them.
- Use self-explanatory names and types. Do not add lint suppression attributes,
  lint expectations, ignore pragmas, ignored tests, or checker configuration that
  exempts a finding. Correct the implementation. Documentation-comment lints are
  not enabled because this project keeps prose outside code.

## DRY and scope

- Extract the second occurrence of logic, arithmetic, validation, or another
  maintained rule while writing it. This applies to all maintained file types.
  Put the canonical implementation on the owning type or component. Documentation
  must link to an existing rule rather than reproduce its obligations.
- Reuse existing types, enum behavior, configuration defaults, standard-library
  facilities, and project abstractions. Never re-derive a centralized value.
- Update callers and remove obsolete names together. No compatibility shims,
  aliases preserving a removed implementation, dead code, unused flags, empty
  architectural scaffolding, or placeholder implementations.
- Names state actual semantics. Rename misleading methods and vague abstractions.
  Match the surrounding style; do not reformat or refactor unrelated code.
- Address the requested scope fully. A proposed product feature needs its own
  specification; examples in rules do not select features or dependencies.

## Information handling

- Never print secrets, credentials, full environment dumps, or sensitive account
  objects. Report keys or redacted identifiers. Inspect error and `Debug` output
  before exposing external objects.
- Keep rationale in Markdown. Do not introduce em dashes or double hyphens used
  as punctuation into authored files or messages. CLI flags and code operators
  retain their meaning.
