# Selected saved puzzle recovery service

Final parent integration and gate results are recorded in
[saved workspace integration](puzzle-saved-workspace.md). The command chronology
below retains the scope of the earlier child and parent review runs.

Checkpoint: `d703763b6f729737cb6d375a5f5055ce40e1e63b`.

Policy references: [canonical rules](../../../.specify/rules/README.md) and
[verification](../../../.specify/rules/verification.md). Related evidence:
[inventory](puzzle-run-inventory.md), [runtime](native-puzzle-runtime.md), and
[command owner](puzzle-command-owner.md). The retained
[verification logs](/tmp/claude-puzzle-recovery-service/) remain outside the
repository at the absolute path recorded in the command and SHA256 ledgers.

The initial green handoff was followed by the source-only review corrections
recorded at the end of this file. The parent has recorded semantic red and green
results for those corrections; postrepair lint and full verification remain open.

The parent granted this isolated worktree an exclusive Cargo lane after the
initial source-only handoff. The final affected puzzle run passed 80 tests,
including all 15 service, 20 runtime, and 19 command-owner tests. All-feature
library checking, library Clippy, and library/test-target Clippy passed. Commands,
intermediate failures, exact logs, and hashes are recorded below. The lane is
released with zero active command sessions or Cargo/compiler/test processes.
The worktree is source-only again. No commit or push was made.

The parent checkpoint's 1,091 tests remain baseline evidence only. The first ten
service tests were authored before implementation. The write-admission test was
added during source review, and four focused cases were added during verification.
The nonblocking receipt test was observed failing for the absent method before
implementing it. Full parent integration, release builds, performance measurement,
and native OS acceptance were not executed in this worktree.

## Contract and integration

`src/desktop/puzzle_service.rs` owns the lower management contract.
`DesktopPuzzleServiceEnumOperation` has `Inventory`, `OpenSaved`, `Session`, and
`Interrupt`. The parent wraps it with its stable workspace entity owner in
`DesktopEnumCommand::Puzzle { owner, operation }`. Application admission and
the typed conversions to `DesktopRevision` belong to the parent. The operation's
canonical `expected_revision()` method uses `DesktopRevision::for_puzzle_storage`
for OpenSaved, `DesktopRevision::for_puzzle` for Session, and ZERO for
Inventory/Interrupt. These conversions are supplied by the parent's identity
dependency patch, applied locally and excluded from the export. The manager does not
implement the per-run bridge or manufacture an idle session identifier.

Every application submission supplies an actual `DesktopApplicationReceipt` in
`DesktopPuzzleServiceRequest`. A UI Pause/Hidden operation uses
`Interrupt { session, reason }`; confirmed End uses `Session` with its complete
expected context. Inventory results retain `Arc<StoragePuzzleRunPage>`, including
the page's transport memory permit, across cloned receipts.

`DesktopPuzzleServiceEnumResult::revision()` is the single result-revision
mapping used by ordinary and control completion. Inventory returns None;
Opened and Session return the puzzle revision; Ended returns its storage revision.
`DesktopPuzzleServiceReceipt::try_result()` permits synchronous host observation.
It returns the retained result repeatedly, returns None while pending, and returns
a typed watch `Outcome` failure when closed without a result. Retained success or
failure takes precedence over closure. `wait()` reuses the same extraction method.

`DesktopNativePuzzleService::new` receives `DesktopNativePuzzleServiceOptions`
with the canonical repository, shared `Arc<CapacityExecutor>`, injected clock
and native wake, existing Tokio `Handle`, cloneable scheduler, and replacement
timeout. `start()` spawns the manager worker and returns its stable cloneable
port. Bootstrap retains the service and port; it does not separately spawn
`run()`. `run()` observes the manager worker when explicit driving is wanted.
`shutdown_retained()` and `is_shutdown_complete()` are the storage-close barrier.
The latter requires observation of both runtime and manager workers.

The mirrored test file is source-included under `cfg(test)` from the concrete
service. The parent needs only to register `desktop::puzzle_service` and
`desktop::native::puzzle_service`; no integration-test root edit is required.

## Ownership and recovery

The service retains at most one selected runtime and one recovery future. An
inventory request has its own single future. Ordinary, urgent, native
interruption, and shutdown ingress queues are bounded. At most one forwarded
reply per ordinary, urgent, and native lane remains in flight. No runtime, CPU
pool, storage discovery, fixture, dataset catalog, start factory, latest-run
selection, or history deletion is constructed internally.

Inventory exposes metadata only. `OpenSaved` loads through the canonical leased
SQLite actor, verifies the requested storage revision, and uses canonical replay.
Ended runs return an explicit typed classification and summary without creating
a runtime or writing recovery state. Live recovered owners retain the canonical
seed, shuffle order, decisions, journal prefix, elapsed timer, and paused hidden
state. Storage revision and persisted event count are captured before consuming
`StoragePuzzleRecovered`.

Preparation and `DesktopPuzzleCommands::new` run on the injected executor. The
first paused recovery write is admitted before installing the runtime. If write
admission fails, the command owner is still retained with its typed failure and
Retry Save path. Existing checkpoint rules gate explicit Resume until the
recovery state is durable. Cancellation does not drop the recovery future,
executor result, selected runtime, admitted storage write, or its observer.

Before every per-run forwarding operation, including Snapshot and Summary, the
service checks the selected session ID. Both native and application interruption
paths bind the original ID and then call that same runtime's authoritative-context
interruption path. A stale interruption cannot be redirected to a replacement.
`interrupt_selected` captures the selected ID and reuses the bound method.

Replacement requires a paused, hidden, durable snapshot with no retained write
or pending forwarded replies. It then awaits retained native shutdown and observes
the worker returning `(runtime, outcome)` before releasing the old actor and
starting the new canonical load. A blocked replacement leaves the old owner
retained. Runtime failures retain the returned actor; a later explicit permitted
request can restart its driving worker without replacing its owner.

Shutdown retains its original absolute deadline across cancellation. An expired
wait retains its receipt until the actor's outcome is observed. A subsequent
explicit cleanup attempt may then supply a new deadline. Runtime cleanup receipts
also remain retained after a manager deadline. Failed or expired cleanup leaves
the manager alive for Retry Save, Snapshot, Summary, and bound interruption.
No async mutex or synchronous mutex guard is held across an await.

`StoragePuzzleRecovered::into_owner` drops its encoded transport permit. That
permit is not a decoded-owner or runtime resident-memory reservation. This change
bounds retained owner and load counts; it claims no 64 MiB resident-memory cap.
Production collection-scale storage and memory design remains separate US13 work.

## Authored verification cases

Fifteen deterministic source-included tests use real persisted puzzle runs and
the real repository, with controlled clock/scheduler/wake and capacity boundaries:

- Idle inventory, no placeholder session, paused recovery, initial save, and
  explicit Resume.
- Stale storage revision and missing-run refusal.
- Ended-run classification without a recovery write.
- Corrupt checkpoint refusal despite successful metadata inventory.
- Cancellation while a canonical load waits for shared capacity.
- Active-owner replacement refusal, safe replacement, old Snapshot/Summary
  refusal, and stale bound interruption refusal.
- Expired cleanup with retained Retry Save/Snapshot/Summary/native interruption.
- Failed initial save, cloned retained replies, cancellation, blocked
  replacement, explicit Retry Save, and later cleanup.
- Cancelled shutdown wait preserving the original absolute deadline.
- Application interruption using current context only inside the original ID.
- Cancellation after real write admission while capacity keeps SQLite pending,
  followed by exact owner retention and observation of the real commit.
- A runtime worker returning its original owner after an injected clock failure,
  followed by a bound native request restarting that same owner and a successful
  summary request.
- Ordinary saturation retaining distinct bounded urgent and native admission,
  with every admitted receipt observed before cleanup.
- Cancelled runtime cleanup preserving its original deadline while the real
  SQLite write remains blocked, then serving reads/interruption and observing the
  late commit before a successful explicit cleanup retry.
- Nonblocking receipt observation of pending, retained success, retained failure,
  and closed-without-outcome cases, including the typed error source and matching
  async observation.

All 15 cases passed in the final 80-case puzzle run. The remaining 26 passing
cases cover lifecycle, puzzle presentation, and puzzle domain invariants. Final
parent integration verification, production-sized memory behavior, and actual
native OS lifecycle acceptance remain unverified.

Standalone `/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt --edition 2024` parsed
and formatted the three new Rust files. The only existing-source hunks derive
Debug/Clone/PartialEq/Eq for `PuzzlePersistenceEnumCommand` and
`DesktopPuzzleBridgeEnumOperation`, and Clone for the injected Tokio scheduler.

Local dependencies are the four-path puzzle-run inventory patch and the one-path
application identity patch. Their paths and evidence are excluded from this export.
No shared registration, bootstrap, coordinator, message, DTO, host, app,
dependency, checker, migration, or vendor path is changed by this export.

## Verification environment and outcomes

Compiler: `rustc 1.99.0 (b940084d7 2026-09-28) (Homebrew)`.
Formatter: `rustfmt 1.10.0`, from the same pinned 1.99.0 installation.
No dependency or toolchain installation occurred. All Cargo invocations set
`CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target`, use the locked graph,
and retain detected default parallelism. Gates ran sequentially without worker
caps. Local-only module declarations in `src/desktop/mod.rs` and
`src/desktop/native/mod.rs` made the source-included tests discoverable; neither
root is staged or exported.

The first service run passed 10 and failed one fixture setup: the corruption
case attempted invalid JSON, which the existing SQLite constraint rejected.
Changing that fixture to schema-valid `{}` allowed canonical recovery to refuse
it as intended. Production storage/schema code was unchanged.

The runtime deadline case exposed a test-clock boundary while broad puzzle
tests ran: a real blocked SQLite worker inhibits Tokio automatic time advance,
and Tokio timers use millisecond granularity. Three diagnostic runs were
interrupted while that wait remained pending. The fixture now explicitly advances
one named timer-driver tick after the original absolute deadline, and asserts
that the service still retains that original deadline. No production deadline
was extended. One intermediate compiler failure came from putting the diagnostic
timeout in the wrong test scope; that test-only edit was corrected. The final
clean 80-case run includes the corrected case and no diagnostic print statements.

The synchronous receipt case was first compiled against the absent method and
failed with E0599. After adding the owning method and reusing it from `wait()`,
the final puzzle run and both strict Clippy selections passed. No lint allowance,
ignored test, checker change, or production behavior workaround was introduced.

Logs are retained as requested in `/tmp/claude-puzzle-recovery-service/`. They are
handoff artifacts outside the repository and are not shipped in the patch. Each
command below used `set -o pipefail`; the full Cargo commands and log names follow.

```sh
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo check --locked --all-features --lib 2>&1 | tee /tmp/claude-puzzle-recovery-service/01-check.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib desktop::native::puzzle_service::tests:: -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/02-service-tests.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib desktop::native::puzzle_service::tests:: -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/03-service-tests.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib desktop::native::puzzle_service::tests:: -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/04-service-tests.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib puzzle -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/05-puzzle-tests.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib puzzle -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/06-puzzle-tests.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib puzzle -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/07-puzzle-tests.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib puzzle -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/08-puzzle-diagnostic.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib puzzle -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/09-puzzle-diagnostic.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib puzzle -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/10-puzzle-tests-final.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo clippy --locked --all-features --lib -- -D warnings 2>&1 | tee /tmp/claude-puzzle-recovery-service/11-clippy-lib.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo clippy --locked --all-features --lib --tests -- -D warnings 2>&1 | tee /tmp/claude-puzzle-recovery-service/12-clippy-tests.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib nonblocking_receipt_observes_retained_success_failure_and_typed_closed_channel -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/13-receipt-api-red.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo test --locked --all-features --lib puzzle -- --nocapture 2>&1 | tee /tmp/claude-puzzle-recovery-service/14-puzzle-tests-final.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo clippy --locked --all-features --lib -- -D warnings 2>&1 | tee /tmp/claude-puzzle-recovery-service/15-clippy-lib-final.log
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target /opt/homebrew/Cellar/rust/1.99.0_1/bin/cargo clippy --locked --all-features --lib --tests -- -D warnings 2>&1 | tee /tmp/claude-puzzle-recovery-service/16-clippy-tests-final.log
```

Outcomes by log number:

- 01: exit 0, all-feature library check.
- 02: exit 101, 10 passed and one corruption-fixture setup failure.
- 03: exit 0, 13 service tests passed.
- 04: exit 0, 14 service tests passed.
- 05: exit 101 after SIGINT of the waiting timer fixture; no final test count.
- 06: exit 101, test-only E0425 scope error.
- 07: exit 101 after SIGINT of the waiting timer fixture; no final test count.
- 08: exit 101 after SIGINT, diagnostic trace identifies the pending deadline wait.
- 09: exit 0, 79 puzzle tests passed with diagnostic trace.
- 10: exit 0, 79 puzzle tests passed without diagnostic trace.
- 11: exit 0, all-feature library Clippy with warnings denied.
- 12: exit 0, all-feature library/test Clippy with warnings denied.
- 13: exit 101, focused missing-receipt-method E0599 before implementation.
- 14: exit 0, 80 passed, zero failed/ignored, 77 filtered out. Includes 15 service,
  20 runtime, and 19 command-owner cases.
- 15: exit 0, final all-feature library Clippy with warnings denied.
- 16: exit 0, final all-feature library/test Clippy with warnings denied.

SHA256 log ledger, relative to `/tmp/claude-puzzle-recovery-service/`:

```text
f7196f64435739387d533b3ba4c705a44a77ba554f0e0fef95f65a2294a0b702  01-check.log
805ee06d4047288a32eabe047fda20da30ae6f0a842ecc3b33eed283473cbc67  02-service-tests.log
2f6eff42664ec41cf5b4eff9602344cbd47aca6decbd14f386b317b0aa417ca5  03-service-tests.log
eed170efc4f85fb8e440fc01d1a58b0db74c0744d0b29996de567a879a860417  04-service-tests.log
ac42f07d52404bec713ee414f7592c4910fc3a3ad30fa0a68493fb83093c37bd  05-puzzle-tests.log
763b0444a220b00602b91914ed6fcdd1bf8b4526731cd4f995b5f8a0896b4291  06-puzzle-tests.log
4757629d75af09a8f207ac26d3808646b15f63eee9d69c2bdd590471041d9b78  07-puzzle-tests.log
1cedcb6a1c72ad7de117ee0c924ed1f41548fa811a21539d1ada35284929f78e  08-puzzle-diagnostic.log
add7ad40a74f500ab9e0ebbe1fe41943f85cefb3f13b6f9453af05eaa7b0cb27  09-puzzle-diagnostic.log
54217132654fa37d522fbb407b49b5ff8edb33c037711eda62f34485189177ab  10-puzzle-tests-final.log
b1a556d1ed51cffad42d461676e88df7b83b7ed4fd36fee862dcae74498c7564  11-clippy-lib.log
2bd37b91db9947afca3dec3389ae6f0d69ca119e952e3c662a682a0f571ab802  12-clippy-tests.log
f5a09a6fa964e4ee8ffa412d418ca459f1e2e3b44b6a388f0c1436ccb45357af  13-receipt-api-red.log
32cd718775c3849e287b7be187af723fdddc942e1ba0c0ec72e3ffa05b130713  14-puzzle-tests-final.log
eac45ef9e2c279dd0a13ad365f7cb2a69d6461ca99132af61ac3e0e098607bb0  15-clippy-lib-final.log
042569a97b8e17f5406becc40b45d7a630141e690514702c1c28c35281ab7fc9  16-clippy-tests-final.log
```

SHA256 of the final tested owned Rust files:

```text
f0a1c098ce0012877a5552774235074a6a0b6d708a41d0f799ae9b8dfbb68bb9  src/desktop/puzzle_service.rs
d5cf0b2ddeaf3d03b733c3510a7d8547c29fe2eaecae15523a67bace48185995  src/desktop/native/puzzle_service.rs
6daa9a8fc4b017da588f45d0f39534933f6380db07709f20dcf0a6ae9fa5c886  tests/desktop/native/puzzle_service.rs
5948c17513323c8d7610c441d2a73d08fb32908246179fa32a02be516382430c  src/desktop/native/puzzle_runtime.rs
3af660db22edd144404d8c71bf9f26e2518bf9119c953f5ba7c3f6e3c28ad893  src/desktop/puzzle_bridge.rs
6bc6c1bae7f248476d8aa8aebf8bb652e82d80ca31fa2995c7aec258bbcbf851  src/puzzle/persistence.rs
```

For the initial green handoff, no Rust source changed after its final test/Clippy
selections. Only evidence and explicit staging/export followed its lane release. The full
binary patch is exported to the requested absolute
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-recovery-service.patch`;
its final SHA256 is reported in the handoff because embedding it here would change
the patch hash itself.

## Source-only review corrections

Independent review identified two blockers against the frozen native service
SHA256 `d5cf0b2ddeaf3d03b733c3510a7d8547c29fe2eaecae15523a67bace48185995`.
Production stayed frozen while the incremental tests-only patch was exported:
`puzzle-recovery-service-review-tests.patch`, SHA256
`fabe01dd2d786a602eddd1aaa3ed158d630431002bd2dc6a16846f926fed3f66`.

The parent applied those tests and observed both semantic failures before
authorizing production repair. The exact parent command, run in
`/Users/sadrashameli/Personal/chess`, was:

```sh
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib desktop::native::puzzle_service::tests:: -- --nocapture
```

It exited 101: 15 passed, two failed, zero ignored, 182 filtered out, 0.05 seconds.
The [parent red log](/tmp/claude-chess-pgo-diagnosis-20261016/puzzle-service-review-red.log)
has SHA256 `9e1ea51ec42d4c61fe2ef46916d130c3c5c795f75607cab82e3a2147797db02b`.
The log was read in this worktree; the command was not rerun here.

`failed_shutdown_notification_releases_reservation_before_synchronous_retry`
observed an actual failed receipt from a safe `std::task::Wake` callback and
immediately retried through the actual port. Retry was refused because the old
reservation still belonged to the request being dropped after notification.
The repair destructures that request and drops its reservation before publishing
failure. The test drains any admitted retry envelope before its assertions.

`cancellation_while_recovery_preparation_waits_for_executor_never_admits_write`
uses a one-shot post-load clock hook to occupy real executor admission slots,
cancels the actual application receipt while submission is pending, and then
releases and observes every occupying job. The frozen implementation returned
Cancelled but advanced storage revision from one to two and installed an owner.
Preparation now captures the actual receipt's existing cancellation token, checks cancellation at
CPU preparation entry, and checks again after capture immediately before write
admission. Cancellation before that boundary drops only the unadmitted owner and
queued payload. The existing retained-owner path after write admission remains
unchanged.

Only the already-existing write-admission cancellation fixture needs a private
signature adjustment: it supplies its real receipt before preparation, then
cancels after the write is admitted, preserving the original test boundary.
The incremental repair excludes the two already-applied regressions and all
unrelated original source/test content. Public contracts are unchanged.

No Cargo command was run locally for this repair. Pinned standalone rustfmt and
Git whitespace review were the local checks. At that handoff, repaired-source
compilation, the parent's semantic green run, strict lint, and final merged
verification were outstanding.

The parent then reported a compile-only E0277 from session 13206, exit 101, at
the executor submission. Capturing the full receipt crossed the executor's
`UnwindSafe` boundary through its outcome graph, which can retain non-RefUnwindSafe
workbench providers and job handles. No tests ran in that compilation attempt.
The narrow follow-up captures `receipt.cancellation()` instead and passes
`DesktopApplicationCancellation` to private preparation. This is the same real
receipt's `Arc<AtomicBool>` token, checked with `is_requested()` at the same two
boundaries. No suppression or `AssertUnwindSafe` is used. The private fixture
passes that same existing token. Public APIs and already-applied regressions are
unchanged. This compiler feedback is parent-supplied; no Cargo ran locally for
the follow-up, and the parent owned the next compilation and semantic green run.

## Parent semantic green after the cancellation-token delta

The parent applied the cancellation-token delta with SHA256
`793adca8e44b18864469126e0fb2c3324acddd1386a41b2b4afb74a6e0f2acec`.
The repaired native service revision has SHA256
`e699b9ead0ae4bebb86c0ba9559834742d6b0ce1d5dd79d00a90fcdafeca72bf`.
These results describe that repaired revision, rather than the initial frozen
native source or the intermediate receipt-capturing compilation failure.

The parent reports its merged desktop-private filter passed 138 tests, zero
failed, zero ignored, 66 filtered out, in 1.31 seconds. This includes all 17
service cases and both independently reviewed regressions. The root bootstrap
case `dropped_puzzle_cleanup_retains_ownership_before_database_close` also passed.
The [parent repaired-source log](/tmp/claude-chess-pgo-diagnosis-20261016/puzzle-desktop-repair-tests.log)
was read here and has SHA256
`f6f53dfd85230c91c1f54381dfe9f1b67d862033e300736404abed7e8171df47`.

That log also shows zero executed tests in the separate desktop integration
target because all 215 cases were filtered out. The parent is running that full
target separately; its result is not established by this evidence update. The
log includes a linker warning that the `__eh_frame` section exceeds the compact
unwind offset limit. This is not a postrepair warning-free lint or full-gate claim.
Postrepair strict lint and complete merged verification remain unreported here.

Rust source remains frozen. Only this Markdown evidence was updated, and no
Cargo command was run in this worktree for the parent's semantic green result.
