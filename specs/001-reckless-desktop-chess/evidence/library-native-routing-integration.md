# Native Library routing integration

Implementation continues from committed checkpoint
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`. This increment does not accept
the complete Library journey or all feature tasks.

## Implemented ownership

The existing application/coordinator now routes typed Library operations and
retains their actual application receipts. Service errors remain shared typed
`DesktopLibraryServiceError` values in `LibraryFailure`. The service owns
revision mapping and continuation disposition.

The native bootstrap constructs the Library service using the actual running
game registry, storage client, executor and wall-time provider. Its worker owns
the service through observed completion. Canceled cleanup waits retain both
the shutdown receipt and actor. A deadline preserves ownership for explicit
retry; an observed historical actor failure remains reportable after cleanup.

Library and puzzle cleanup share one typed owner implementation. Each future is
polled with its owner retained outside the bootstrap state lock. Both services
must finish before game, engine and storage cleanup proceeds. No worker is
aborted or detached to declare successful shutdown.

The Library query panel and direct annotation completion consumer remain under
integration. Generic application response handling cannot substitute for their
correlated request owners. Full import/export, organization and Trash flows
retain their existing task ownership.

## Observed targeted checks

All commands used the recovered exact Rust 1.99.0 toolchain through
`/tmp/claude-chess-pgo-diagnosis-20261016/with-pinned-rust`, the shared target
directory and default Cargo/test parallelism. Root Rust source was frozen during
each Cargo command.

- `native-library-service-tests-2.log`: three passed, zero failed or ignored,
  625 filtered. Real SQLite query correlation, canceled cleanup and expired
  cleanup recovery were exercised. The earlier first run exposed a test
  assumption that historical timeout errors would disappear after cleanup;
  the final test requires both truthful error reporting and observed completion.
- `library-bootstrap-tests-2.log`: eight passed, zero failed or ignored,
  621 filtered. This includes retained Library and puzzle cleanup, existing
  preparation/terminal capacity checks and local-game composition/recovery.
- The first routing integration run recorded 13 passed and one failed. Its
  saturation fixture incorrectly required final Quit to finish in one manual
  poll after exhausting Tokio cooperative capacity. The fixture now awaits
  the real coordinator under a bounded deadline. No production policy was
  changed for that fixture. The final registered routing run passed all 15
  cases, including over-budget refusal before application retention.

The same integration compiled and executed the position-editor domain suite:
19 passed, zero failed or ignored, 468 filtered. That suite is registered only
once through `tests/domain.rs`. The existing macOS linker `__eh_frame` warning
was emitted while compiling the application binary and remains unsuppressed.

## Remaining verification

The service suite subsequently passed 327 private desktop checks, including
the four original review regressions. The additional queued-payload check
admits 31 one-MiB inputs before its transport budget refuses more; all actual
canceled requests finish with certified preacceptance refusals, unchanged
source context and no wall-time sampling. Their exact nested error shape
depends on whether refusal occurs before source lookup or inside the notation
owner. The test checks that actual cancellation and refused disposition,
rather than requiring only the top-level cancellation variant.

The combined presentation filter passed 91 cases, including all three observed
annotation regressions, actual service save completions and Library pages.
The import filter passed all 55 cases after the separately documented fixture
corrections. The Polyglot core passed 19 cases plus two hash-gateway checks.
The report filter passed all 26 report cases and one existing puzzle-summary
case whose name also matched the filter.

Mounted query/open/annotation workflows and final canonical gates remain
pending. These targeted checks are not GPU, native accessibility,
supported-platform, package, performance or release acceptance.

## Initial Open recovery through the application

A further regression used the actual application/coordinator and Library
service. An initial played Open under receiving-memory pressure retained its
lower publication but exposed no context for fresh admission. Re-submitting
the original application request correctly returned its cached terminal
failure. The preserved baseline recorded zero passed and one failed, SHA-256
`b67284ce7d9d0e612603d62a78a5a18175d8a33157da0905aebd1f943fec2ee0`.

The source now records the exact original lower Publishing context on its
retained reply. A new application RetrySave bound to that context resumes the
original lower application receipt through Publish. It never calls Open again.
The original and fresh service receipts reconcile to Opened; the original
application failure remains immutable and the fresh application receives the
actual recovered publication. Source clocks, revisions, durable bytes and
modified time remain unchanged, with one factory and no save or wall-time
replay. A foreign old epoch cannot consume the original watcher.

The complete regression passed after cleanup. Its log SHA-256 is
`0db5535e099f925e5425a212bb4620d746579f1ae1458f6e34b053a1aba23581`.
The four-file source increment, exact baseline tree and log are preserved under
`.git/parallel-patches/library-open-application-recovery-repair-20261017/`.
The native UI consumer must use fresh admission with that certificate; a
cached original application request cannot perform recovery.
