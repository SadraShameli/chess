# T080 local tablebase preparation and status

## Delivery and verification state

This isolated implementation starts at `f74c168`. It adds the preparation/status
core and 13 hermetic tests. The tests were authored before the implementation.
The isolated lane performed only standalone Rustfmt and staged diff checks.
Parent integration at `f0317cf` then passed formatting, targeted Clippy and all
160 engine tests, including these 13 cases and 14 diagnostic-job cases. There
were no failed or ignored tests. No positive native table load, Windows build,
or release was performed. T080 remains open for authentic native-table and
Windows acceptance. The existing macOS linker warning remains unsuppressed.

The governing behavior is in
[engine-protocol.md](../contracts/engine-protocol.md#tablebase-paths)
and
[decisions.md](../decisions.md#engine-workbench-and-optional-data).

## Public API

- `EngineTablebaseLocationSelection::new(Vec<PathBuf>)` accepts individual
  owner-selected directories. Source paths are never parsed as a delimiter-joined
  string, so spaces and Unix colons in source names do not change selection.
- The composition boundary injects either
  `EngineTablebaseLocationPolicy::unix_temporary(parent)` or
  `windows_owner_selected(parent)`. There is no implicit use of the username,
  home directory, or process temporary directory. Select a parent on the native
  platform with a whitespace-free complete canonical path; when that is
  impossible, surface the typed actionable error and leave ordinary chess usable.
- `EngineTablebaseLocation::prepare_blocking(selection, policy, is_cancelled)`
  validates actual directories, traversal/read permissions, readable nonempty
  regular table files, material filenames, and alias collisions. It creates a new
  UUID-named directory, with Unix permissions set to `0700`, and links each
  accepted file using its canonical source path. Root syntax rejects whitespace,
  controls, invalid Unicode, and the native path-list delimiter. Both the supplied
  parent and its canonical target are checked. Unix uses `:` and Windows uses
  `;`; a Windows drive colon is allowed.
- `engine_path()` returns the existing validated `EngineOptionsSyzygyPath`;
  `source_directories()`, `path()`, and `file_count()` expose the selection and
  current managed links. No parser quoting, truncation, table downloads, or
  silent copies are involved.
- `cleanup_blocking(&mut self)` checks root identity and each recorded link's
  identity/target, unlinks only operation-owned aliases, and removes only an
  empty owned root. It does not recursively remove directories or follow links
  for deletion. Foreign entries, replaced links, and replaced roots are
  preserved with typed failures. Successful cleanup is repeatable.
- `EngineTablebaseStatus::configured(&location)` records selected directories
  and the prepared engine path, initially in `Configured` state. Its async
  `configure(&mut EngineSession<P>)` calls the actual
  `EngineSession::configure_option`, returning the resulting typed state.
  `record_configuration` also accepts that existing typed result for event
  delivery. Only an explicit `EngineOptionsEnumTablebases::Loaded` for the same
  path produces `Loaded { pieces: NonZeroU32 }`. `Unavailable`, unexpected
  outcomes, mismatched paths, and session errors have distinct states/causes.
  A failed replacement overwrites an older loaded status. No readiness message
  or readable fixture file is interpreted as a successful native load.

## Blocking boundary and ownership

All directory scans, metadata checks, link operations and cleanup are exposed as
synchronous adapter work with `_blocking` names. The component creates no runtime,
thread, subprocess, or worker pool. The native composition caller must submit
preparation and cleanup to its existing `CapacityExecutor`, using the injected
budget and `CapacityExecutorCancellation::is_cancelled` callback. Pure status
transitions perform no filesystem I/O, and status configuration awaits the
existing asynchronous engine-session API.

Cancellation is checked before setup, between scan entries and link operations,
and after the final link. Failure or observed cancellation after root creation
attempts safe cleanup before returning. Cleanup failures retain both the original
operation and cleanup causes, including the managed path for recovery. An
uninterruptible filesystem syscall cannot be cancelled until it returns.

The caller must retain and observe the preparation job through completion. It
must keep a successful location until all engine use of the alias has ended,
then explicitly submit and observe `cleanup_blocking` on the shared executor.
There is intentionally no filesystem work in `Drop`: abandoning a successful
location does not delete data, but leaves its temporary aliases for explicit
recovery. Do not detach preparation jobs or discard a successful result during
cancellation. If cancellation arrives after successful preparation, the owner
must schedule cleanup of that result. A later launch prepares a new root and new
links from the selected source directories; alias paths are not persisted or
reused. Crash leftovers are not scanned or recursively swept.

## Fixture coverage and native limitations

The 13 new tests cover complete-path syntax and native path-list semantics;
space-containing source paths; canonical parents with spaces; Unix colon source
names; private permissions; real symbolic links to tiny local files; missing,
unreadable, empty and nonregular sources; invalid material names; duplicate
material collisions and repeated directory selection; link creation failure;
cancellation after a created link; preservation of sources/foreign entries;
root replacement; configured/load/cardinality/unavailable/failure distinctions;
and the existing session API's unavailable and readiness-only outcomes.

The tiny files contain fixture bytes, not authentic Syzygy tables. The positive
cardinality fixture feeds an explicit typed load outcome into the status owner;
it does not demonstrate native Reckless/Fathom loading. The existing channel
process fixture is reused for session behavior. Its unavailable outcome leaves
the session idle and able to complete another readiness request for ordinary
chess. A readiness-only protocol violation remains a typed session failure under
the existing session contract; this component does not alter that behavior.

Unix uses stable safe `std::os::unix::fs::symlink` and compares link device/inode
identity. Windows uses stable safe `std::os::windows::fs::symlink_file` and records
link target plus creation/write timestamps and file attributes. Windows file-link
privileges or Developer Mode may be unavailable: that is an explicit typed setup
limitation, not an administrator prerequisite for ordinary play. Directory
links are not used. An opt-in Windows junction fallback
(`EngineTablebaseLocationPolicy::with_junction_fallback`) now exists: after a
file-link privilege failure it removes the partial aliases and creates one
junction through `cmd.exe /D /C mklink /J` (system directory from winsafe). It
requires exactly one selected directory, exposes that whole directory, refuses
paths with cmd metacharacters, never copies, and reports typed errors
(`JunctionUnavailable`, `JunctionNeedsOneDirectory`, `JunctionSyntax`). It is
written but uncompiled and unverified on Windows. The owner-selected
parent is canonicalized and validated; file-link failure is surfaced without a
copy fallback. Windows link permissions, junction/ancestor behavior, link
identity guarantees, and native loading remain untested on this Mac. The Windows
branches use portable standard APIs, but cross-target compilation has not run.

Authentic positive native-table load/cardinality, root WDL/DTZ and rule-50 evidence
remain unavailable because there is no supplied real-table fixture. No network
acquisition was attempted and no native acceptance result is claimed.

## Parent check commands

After serial registration integration, these consolidated checks passed using
the parent's existing target directory and pinned toolchain:

```sh
cargo fmt --all
cargo clippy --locked --lib --test engine -- -D warnings
cargo test --locked --profile dev --test engine
```

The owner-selected debug workflow leaves optimized/full verification outstanding.
Native Windows build/link and authentic table fixture acceptance need their own
actual environment; a Mac-only green suite cannot close those gates. The isolated
source-only formatter verification was:

```sh
rustfmt --edition 2024 --config skip_children=true --check src/engine/tablebase/mod.rs src/engine/tablebase/location.rs src/engine/tablebase/status.rs tests/engine/tablebase.rs
git diff --cached --check
```
