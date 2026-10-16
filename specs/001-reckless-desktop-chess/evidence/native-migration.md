# Native migration execution

## Obsolete shell removal

T050 removes `ui/`, Tauri build scripts/configuration/capabilities, browser
validators, the browser envelope/schema and concrete Tauri adapters. Native
commands, bounded streams, request deduplication and canonical identifier
serialization remain in their Rust owners. The Rust asset generator and the
reviewed assets are retained under [their acceptance evidence](native-assets.md).

All application and authoring commands now use Cargo. `make run` is an incremental
debug launch. Dry runs produced `cargo run --locked` without arguments and
`cargo run --locked -- --help` with explicit arguments. The lockfile lost 270
packages and added no package versions. No Tauri, Wry or Schemars package remains.

Following the owner's instruction to use debug iteration, post-removal validation
used targeted debug commands:

- Clippy for the library, binaries, desktop tests and native harness with the
  asset-tools feature passed.
- The desktop suite passed all 132 tests, including the preserved bounded stream,
  canonical serialization and deduplication cases.
- The production native lifecycle harness passed startup, actual Reckless
  identity, graphics readiness, canceled quit, confirmed quit, observed cleanup,
  exclusive profile reacquisition and durable SQLite reopen on Apple Silicon.
- `cargo fmt --all` and `git diff --check` passed.

The debug linker still reports a compact-unwind section size warning. Bevy still
logs a skipped Destroyed event during window teardown; production cleanup returns
success. Neither warning is suppressed or represented as resolved.

T049 and T051 remain open: no optimized performance acceptance, complete-process
idle measurements or Windows/Linux native execution is claimed by this cleanup.
Actual graphics-failure recovery, native power events and active native-dialog
cancellation also retain the limits in [foundation evidence](native-foundation.md).

## Development scope

The native shell is a working migration checkpoint. Full playable games and the
remaining product stories are tracked in [tasks](../tasks.md); removing the old
frontend does not complete those requirements. During implementation, subsequent
checks select changed components and reuse the debug dependency cache. Final
optimized verification remains a separate acceptance gate.
