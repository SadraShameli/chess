# Confirmed Quit engine quiescence and export destinations

This focused source-only follow-up uses exact copies of the parent's current
`src/desktop/coordinator.rs` and `tests/desktop/coordinator.rs` as its base. The
isolated baseline tree is `fd8c143c24e8a1e60f07188bdfa555609e51e3ef`; its
coordinator blob is `e9ceb63a29bfb7e5ee92a6290af2b7303d751e72`.
The existing integration test file remains byte-identical to that base.
The temporary index excludes earlier root/UI and lower-layer dependency edits.

## Behavior

Every confirmed Quit retains one three-second deadline independently of engine
availability or pending engine commands. Its first window is anchored to the
original Quit receipt's runtime instant. Queuing, dialog closure and game-response
waiting cannot restart it; only explicit Retry records a fresh deadline.

A confirmed Quit with unresolved engine requests also starts an early retained
`DesktopEngineBridgeProvider::quiesce` operation before waiting for those
requests to finish. Its first three-second window is anchored to the original
Quit receipt's runtime instant. Dispatch latency and repeated polls cannot
restart that window.

The coordinator polls the stored future in its existing select loop alongside
engine replies, game replies, native selections, streams and application work.
It creates no per-request task. Dropping a `run()` wait or reaching the deadline
does not drop the quiesce future. Its actual result remains observable on the
same owner when `run()` resumes.

A quiesce refusal or elapsed window retains the owner and original Quit receipt,
uses the existing Retry quit notice, and prevents automatic final shutdown.
Explicit Retry records a fresh three-second window. One previous unresolved
attempt can remain retained alongside the new one; the bound of two represents
one processing lifecycle operation and one queued retry. Further Retry attempts
refuse visibly until an earlier response is observed. No unresolved response is
dropped to make room, and a late previous success cannot satisfy a new attempt.

All engine command responses and all retained quiesce results must be observed
before root lifecycle cleanup begins. Final cleanup receives the same absolute
attempt deadline. If root cleanup itself needs Retry after engine quiescence
was already observed, its new bounded cleanup attempt does not call an already
closed engine port again. Final cleanup without an engine, or without a pending
engine command, uses the same independently retained Quit deadline. Native dialog
pending notices retain their existing behavior when a dialog is actually pending.

`ApplyExport` now consumes its opaque destination as an owner-scoped WriteFile
selection from the same registry used by `PrepareTablebases` for Directory
selections. One closed mapping selects the required handle/purpose pair, and
one shared consumption path applies owner, purpose, expiry and single-use
validation. The resulting value is forwarded in `BridgeRequest.selection`.
`PreviewExport` has no file selection and can reach its owner while the native
file registry is temporarily held elsewhere. Normal application receipt
idempotency prevents duplicate request delivery or destination consumption.

## Authored checks

Ten private library cases in `tests/desktop/coordinator_engine_quit.rs` drive
real application ports/receipts and the real native file-handle registry with a
bounded in-memory engine provider. They cover:

- Early quiescence, continued Bootstrap delivery, original Quit correlation,
  engine receipt observation before root cleanup, and the shared deadline.
- Abandoned `run()` waits without reminting or losing the quiesce receiver.
- Queued Quit latency, deadline expiry, retained previous responses and a fresh
  explicit Retry that cannot reuse an old result.
- Quiesce refusal with no automatic retry after engine requests finish.
- Bounded repeated Retry with every outstanding response retained.
- Single-use export destinations and retained receipt replay.
- Wrong-owner and wrong-purpose refusal without consuming a valid selection.
- Expired destination refusal before engine admission.
- Preview export without registry access or file selection.
- Fresh root-cleanup Retry after observed engine quiescence, without calling the
  engine quiesce port again.

The tests use paused Tokio time, an explicit monotonic fixture clock, bounded
channels and controlled responses. They perform no network access or wall-clock
sleep. Initial regressions were authored before the production change. Red or
green execution is not claimed.

Registration is local to the owned coordinator source through its
`#[cfg(test)]` `engine_quit_tests` module. No shared module or test root changes
are required. Existing public coordinator integration tests remain unchanged.

## Verification and limits

The explicit owned Rust files were formatted and checked with the Rust 1.99.0
installation's `rustfmt 1.10.0`, edition 2024, with child traversal disabled.
Scoped diff checks and a read-only patch-application check against the parent
are part of this handoff. Formatter parsing is not a Rust type check.

No Cargo command, compilation, Clippy run, test execution, optimized build,
actual child execution or native/GPU run was performed. The parent reserved the
exclusive Cargo lane for T187 and will consolidate validation. Parent validation
must include these library tests and the unchanged coordinator integration
suite, then the required sequential project gates. Real engine timing, native
shutdown, pending OS writes and platform behavior are not established by these
controlled provider cases.

## Independent deadline follow-up

The receipt-deadline correction uses exact current-parent copies of this evidence,
the coordinator and its private tests, isolated as baseline tree
`17faab43e96f6fa6a18603da8f466c9eaed59a63`. It does not reapply the initial
quiescence/export work or the parent's subsequent test corrections.

Four baseline-compatible regressions were exported first in
`native-quit-deadline-tests.patch`: queued Quit without pending engine work,
queued Quit without an engine provider, expired no-engine Quit requiring explicit
Retry, and a native dialog held for two seconds before closure. The parent ran
all fourteen private cases before this source correction. The read log
`/tmp/chess-quit-all-deadlines-red-20261016.log` records ten passing existing cases
and all four new cases failing. Both queued cases and the dialog case gave final
cleanup two extra seconds; the expired case incorrectly completed without Retry.

The source now stores the active Quit deadline on the coordinator, independently
of quiescence state. Quiescence retains its own actual futures and responses;
its phase no longer duplicates the active deadline. Final cleanup, readiness
failure checks and deadline wakeups use the retained Quit deadline for every
engine/no-engine path. Explicit Retry alone replaces it with a fresh bounded
window. The existing dialog/game completion gates do not recalculate it.

The final `native-quit-deadline.patch` contains only the new coordinator and
evidence delta; apply the tests-only patch separately first. Pinned explicit-file
rustfmt, scoped diff checks and read-only application checks were performed in
this lane. Cargo remains parent-owned: no compilation or post-fix test execution
was performed here, and no green result is claimed for this correction.

## Parent baseline checks

The parent ran all ten initial private tests against the exact previous
coordinator, with only their local test-module registration added. Nine failed
and the preview-without-selection case passed. The integrated implementation
then passed those ten checks in the full 80-test library suite.

All four additional deadline tests subsequently failed against that first
implementation. Both queued cases extended cleanup by two seconds, the expired
case completed without Retry, and the held-dialog case restarted its deadline.
Their corrected implementation is included in the consolidated verification.
The private fixture preserves mutex diagnostics, typed abandoned response
values and executor-owner counts when reporting a failed boundary.
