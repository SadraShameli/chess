# Typed engine option application

Validated 2026-10-16 from pushed HEAD `52b65eb`, using Rust 1.99.0 and debug
builds. T077's backend option registry/application lives in
`src/engine/{capabilities,enums,options,resource_config,session}.rs`.

## Ownership and behavior

`EngineEnumOption::ALL` remains the eight-entry discovery and coverage inventory.
`EngineCapabilitiesVersion` identifies registry version 1 and rejects unsupported
versions. `EngineOptionsSetting` privately owns a validated typed value/version;
the receiving session revalidates against its actual discovered descriptor before
I/O or lifecycle mutation. This includes dynamic per-session Threads bounds,
Hash/overhead policy, exact millisecond overhead and MultiPV 1..256. Unknown
advertisements still retain their name/descriptor in the typed coverage error.

`EngineSession::configure_option` is usable idle-only configuration. The existing
resource batch and variant/new-game/search setters use its same typed formatting
and response path. One session-owned `EngineOptionsApplied` holds resource,
variant, Minimal, MultiPV and tablebase state; no parallel option registry or
actor was added. A resource change clears `resources()` during its ambiguous
phase and publishes the exact updated configuration only after acknowledgment
and readiness. Without a previously complete resource configuration it remains
unknown rather than claiming an older or invented allocation. Applied options
are unavailable during cancelled preparation or failed state. Cancellation
retains the process and forces observed cleanup before reuse.

CPU/hash admission remains in the native resources controller, which still
acquires the allocation before session configuration. The low-level option API
does not acquire, resize, clamp or release leases, and no desktop route bypassing
that controller was added. Paused-game child/hash release from T076 is unchanged.

## Pinned protocol semantics

The exact pinned `src/uci.rs` was read from the prepared source for commit
`0e92358f5acd66e5ac77b1bf558202e47c515435`; its
[`src/tb.rs`](https://github.com/codedeliveryservice/Reckless/blob/0e92358f5acd66e5ac77b1bf558202e47c515435/src/tb.rs)
was also checked. Application waits for these actual responses:

- Hash, Threads, MoveOverhead, UCI_Chess960 and MultiPV: their exact option/value
  `info string set ...` acknowledgment, then readiness.
- Clear Hash: `setoption name Clear Hash` has no value; wait for
  `info string Hash cleared`, then readiness.
- Minimal: emits no option acknowledgment; use its readiness barrier.
- SyzygyPath: wait for explicit positive loaded cardinality on stdout or the
  explicit load failure on stderr before readiness. Return typed Loaded or
  Unavailable, retaining diagnostics. `readyok` alone, zero or malformed
  cardinality cannot claim loaded tables. A failed replacement replaces the old
  loaded status with Unavailable.

The default advertised path is empty/unconfigured. The pinned setter requires
one value token, and `tb::initialize` returns failure when largest cardinality is
zero. No empty-value unload command is fabricated. Paths are bounded, nonempty,
representable single-line text without whitespace/control characters; formatting
adds no quotes or filesystem copies. Variant state is shared with new-game and
position setup, so a later Standard/FRC search restores its required setting.

## Validation

New contract tests were added before implementation. The initial exact filtered
run exited 101 because the typed setting/version and session application API did
not exist. Ten new tests now cover every registry entry, valid/invalid values,
version/path boundaries, active-search refusal, exact acknowledgments, button
syntax, Minimal readiness, cancellation of resource/button/tablebase waits,
per-session revalidation, resource consistency, MultiPV boundaries, variant
coherence and loaded/unavailable distinction. Existing unknown-descriptor,
analysis/root legality, diagnostic, lifecycle and Retry checks remain passing.

- `cargo fmt --all`: passed.
- `cargo clippy --locked --lib --test engine --test desktop --test native -- -D warnings`:
  passed without warnings.
- `cargo test --locked --lib --test engine --test desktop`: 52 library,
  118 engine and 201 desktop tests passed; zero failures or ignored tests.

- `cargo test --locked --profile dev --test native -- --engine-options resources/engine`:
  passed 8/8 registry entries against the actual packaged Reckless, with one
  owned CPU thread and 16 MiB Hash. Exact acknowledgments, Minimal readiness,
  button syntax, restored variant/Minimal, MultiPV, explicit empty-directory
  Syzygy failure, resource consistency and observed cleanup passed. The switch
  exists only in the native test executable.
- `cargo test --locked --profile dev --test native -- --game-composition`:
  passed once after final source edits. Two real-engine mate games, two
  post-result rejections, durable restoration and frozen clocks passed. Both
  bootstraps recovered 14/14 CPU slots, acquired a full-worker lease, observed
  zero direct children and reacquired the exclusive profile.
- `cargo fmt --all --check`: passed.

The existing macOS `__eh_frame section too large (max 16MB)` linker warning
remains unsuppressed. No new warnings were introduced.

## Scope

T077 is complete: 66/295 tasks completed, 229 pending. It covers usable
low-level typed application for all eight options. Managed
Syzygy aliases, native directory selection, positive real-table load/cardinality
acceptance and T073's full tablebase corpus remain T080/T073 work. Desktop
handlers/UI and the full capability runner remain their own tasks. No optimized
performance, platform/package or full-release acceptance is claimed.
No dependencies, release/all-target/all-feature builds, owner-profile writes,
new branches/worktrees/agents, staging, commits or pushes were used.

No extension hooks were registered (`.specify/extensions.yml` is absent).
