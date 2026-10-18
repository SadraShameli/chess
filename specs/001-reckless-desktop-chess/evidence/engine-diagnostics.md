# Bounded engine diagnostic retention

Validated 2026-10-16 against starting HEAD `b0f65d3`, with Rust 1.99.0 and
sequential debug checks. This implements the diagnostic-retention portion of
T076. Task markers remain unchanged at 64/295.

## Ownership and bounds

`src/engine/diagnostics.rs` owns an ordered tail of complete validated process
lines, including stdout/stderr identity. The 1 MiB limit counts the owner,
reserved deque entry storage and exact boxed text allocations. A separate
4,096-entry bound handles empty/tiny floods. Whole-line eviction preserves UTF-8
and the latest line; saturating evicted-line/byte counters expose lost history.
Allocator bookkeeping and the existing bounded transport queue are separate from
this owned payload/entry budget. This is an ownership bound, not an RSS claim.

The session's common receive boundary records every line before interpreting it,
including startup, readiness, resource acknowledgments, malformed search output
and stop waits. The owner moves from `EngineSessionStartup` into `EngineSession`
without copying its tail. Borrowed `diagnostics()` and owned `take_diagnostics()`
remain available after retained cleanup. Consuming start/shutdown failures carry
`EngineSessionError::Diagnosed`; `operation()` exposes the typed original error,
including both startup/operation and cleanup failures. Debug formatting reports
counts only. No automatic output or logging was added.

Native observed shutdown drains the same bounded channel while waiting for the
child and reader handles, rather than closing the receiver and discarding queued
output. The child state was extracted into `EngineProcessChild` to borrow these
owners independently. All three futures are joined directly, with no new tasks,
locks or borrowed guards. Collection errors remain on the native owner across
cancellation and are combined with process/reader failures. Expected shutdown EOF
is accepted. Other process implementations inherit existing cleanup behavior
through the observation hook without fixture-wide rewrites.

The 64 KiB line bound, 256 pending output lines, exact readiness/bestmove barriers,
typed analysis parsing, receipt-based 2-second stop deadline and 3-second cleanup
bound remain in place. Only validated complete lines enter the tail; invalid or
oversized input remains a typed process failure rather than a raw output dump.

## Red-first evidence

After adding the tail API and tests, before connecting session receives/transfer,
`cargo test --locked --test engine engine_diagnostics` exited 101:

```text
2 passed; 4 failed; 93 filtered out
startup_failure_and_cleanup: expected stderr/rejected stdout, got []
startup_transfer_readiness_and_configuration: expected startup context, got None
cancelled_handshake_and_cleanup: expected startup context, got None
malformed_search_and_cleanup_failure: expected malformed info line, got None
```

The two passing cases checked long/empty/tiny floods, owned-memory and entry
bounds, cross-pipe order, latest-line preservation, Unicode and safe Debug output.
The four failing cases then passed after receive-boundary integration. Additional
regressions cover consuming startup failures with both typed causes, startup
handshake timeout, cancelled stop waits, stop timeout, cleanup collection beyond
queue capacity, and collection cancellation retaining both data and its failure.
All new fixtures use controlled futures, in-memory channels or paused Tokio time.

## Final checks

- `cargo fmt --all`: passed.
- `cargo clippy --locked --lib --test engine --test desktop --test native -- -D warnings`:
  passed without warnings.
- `cargo test --locked --lib --test engine --test desktop`: 52 library, 102 engine
  and 201 desktop tests passed; zero failures or ignored tests.
- `cargo test --locked --profile dev --test native -- --game-composition`:
  passed once. Both real-engine mate games completed, with two post-result
  rejections. Durable one-move restoration, frozen clocks and completed-tab
  exclusion on relaunch passed. Both bootstraps recovered 14/14 CPU slots,
  acquired a full-worker lease, observed zero direct children and reacquired the
  exclusive profile.
- `git diff --check`: passed.

## Remaining acceptance

The control-bound and explicit failed-engine Retry gaps recorded by this increment
are now closed by [engine Retry and bounded controls](engine-retry-controls.md).
That follow-up traces the actual engine-facing command owner and records the
new regression, cleanup, generation and paused-until-Resume acceptance.

T077 remains incomplete: full eight-option application, tablebase path/load and
configuration UI remain future work. T062, T071, platform and release gates stay
open. No dependencies, vendor edits, release/all-features/all-target rebuilds,
profile writes, staging, commits or pushes were performed. The existing macOS
`__eh_frame section too large (max 16MB)` linker warning remains unsuppressed.
