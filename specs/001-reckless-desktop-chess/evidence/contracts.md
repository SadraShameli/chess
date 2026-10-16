# Desktop contract generation

T023 uses schemars 1.2.3 draft-07 schemas from Rust DTOs, json-schema-to-ts
3.1.1 for inferred wire types, and Ajv 8.20.0 strict standalone validators.
The working `chess-tools` dispatcher was registered during this task because
contract generation requires an executable Rust entry point. T025 extends it
with engine tooling.

Schema properties are inlined to avoid TypeScript's instantiation-depth limit
when resolving the nested response union through repeated local references.
A const-generic schema holder preserves literal types without assertions;
the library's `asConst` declaration maps literal null values to never.

Ajv compiles during generation. Bun bundles its standalone output into a local
ES module under `resources/desktop/`. The player imports the compiled validator
and its generated declaration, so validation requires no runtime code evaluation
or content-security-policy relaxation. Vite and Vitest resolve aliases directly
from the TypeScript configuration.

Verification on the current macOS ARM host:

- Strict TypeScript, zero-warning ESLint/Stylelint and all-target/all-feature
  Clippy passed.
- Eleven desktop tests and fifteen domain tests passed.
- Five browser tests passed, including real Rust-serialized request, response
  and event fixtures, maximum-u64 precision, unknown fields, future protocols,
  malformed identifiers and invalid board/capacity boundaries.
- The production frontend build passed.
- Deliberately appending a newline to the generated TypeScript made
  `contracts:check` fail with the expected drift error. The original was restored,
  and the unchanged generation check passed again.

Packaged WebView integration is covered by the foundation checkpoint, after
the real bootstrap and bridge have been composed.
