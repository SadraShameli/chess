# Library application routing

Source-only handoff on 2026-10-17 for the authorized all-13-journey feature.
This slice connects Library to the existing application and coordinator. Native
composition, presentation, service repairs, and execution remain with their
assigned owners.

## Baseline and dependency

- HEAD: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
- Dependency: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-red-baseline-20261017/source.patch`.
- Dependency SHA-256: `80194a403857ca571fc3063cdeb5f108e3288c50601f942199fd4fe2e8aed18f`.
- Applied with `git apply --index`; the resulting index tree was verified as
  `67da2aed144a86d9cdaa7822ae42e71eb6c3854c`.
- The dependency is excluded from the incremental routing export. Its manifest
  records unresolved semantic service regressions. Applying it does not establish
  service acceptance.

## API handshake

The transport handshake was sent before production edits:

- `DesktopEnumCommand::Library(DesktopLibraryServiceOperation)`.
- `DesktopCommand::library(operation)` takes the operation's canonical optional
  expected revision without wrapping `None` or replacing it with zero.
- `DesktopEnumValue::Library(Box<DesktopLibraryServiceEnumResult>)`.
- `DesktopMessagesEnumResult::LibraryFailure(Arc<DesktopLibraryServiceError>)`.
- `DesktopCoordinator::with_library(workspace, Arc<dyn DesktopLibraryServiceProvider>)`.

The service error remains the exact shared typed error returned by the provider.
UI code can inspect it and call the service-owned `disposition()` once the
separately owned accepted seam is integrated:
`DesktopLibraryServiceEnumDisposition::{Refused, Retained { request:
DesktopRequestId, expected: Option<DesktopGameEditorContext> }}`. That enum and
method are absent from the supplied dependency and are prerequisites for UI
disposition handling. Routing neither synthesizes a disposition nor constructs
a `Continuation` error. The service worker must use its continuation wrapper
only when an actual pending continuation remains owned.

## Ownership and completion

Library closing availability and mutation identity delegate directly to
`DesktopLibraryServiceOperation`. Ordinary editor actions consume the existing
bounded ordinary admission budget. They add no urgent slot or channel. The
existing application mutation map excludes competing work for the actual game,
including work submitted through a different workspace identity.

The coordinator checks the canonical optional revision and configured workspace
before calling the provider. It passes `work.receipt().clone()` to
`DesktopLibraryServiceRequest::new`, preserving request identity, receipt time,
cancellation state, and shared outcome. The existing commit boundary distinguishes
cancellation before dispatch from an operation already handed to the provider.
The retained pending entry owns both the real application work and its future.
Application admission bounds the number of these entries.

The existing pending-future owner now supports a typed error parameter; Game,
Engine, and Puzzle retain their existing default error type. Library results move
from the provider into pending response ownership and then into the application
receipt. The application acquires and validates its state before taking that
response. A Busy retry preserves the response, and this completion path clones
neither the service result nor a library record/tree. Dispatch retains the
existing single command clone required by the provider's owned-operation API.
That operation contains command inputs, not an independently copied game payload.

Success carries exactly `result.revision()`. Service failures retain the original
`Arc<DesktopLibraryServiceError>`. Coordinator refusals before provider dispatch
use the existing typed `DesktopFailure`. Stop and Quit propagate cancellation
through the actual receipt; dispatched service responses remain authoritative
and are delivered even after cancellation was requested. Quit waits for pending
Library responses before shared lifecycle cleanup. Expired quit attempts retain
the work and require the existing explicit Retry quit path.

Completion also observes whether the application receipt already owns the
response when native waking fails. It releases the coordinator entry in that
case, preserving the recorded outcome and avoiding another completion attempt
or provider dispatch after the caller resumes `run()`.

## Focused test sources

Four application tests and ten coordinator tests were authored before production
API edits. The initial tests-only staged binary patch is:

`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-application-routing-20261017/tests-first.patch`

SHA-256: `e6cf41efc40407ff50fdb7bf0febbac952ae4e8a1ac230d19ba03bb3087bdb36`.

The final test sources additionally include formatter changes and source-review
corrections. The initial patch deliberately names APIs absent from the dependency.
No compile failure or semantic red was executed or claimed.

The application cases cover canonical classification, receipt deduplication,
competing edits to one actual game, ordinary capacity with reserved Quit, queued
cancellation, and cleanup admission while closing. The coordinator cases cover
actual receipt delivery, single provider dispatch across replay, canonical query
and editor-result revisions, typed error identity, workspace/revision refusal,
responsive Bootstrap while Library waits, dropped `run()` waits, cancellation,
Stop reconciliation, retained Quit ownership, explicit retry after deadline,
wake failure, and bounded dispatched work.

The fixtures use the real application/coordinator, real local two-player session
and editor publication, and actual temporary SQLite query results. Controlled
substitutes implement only injected clock, wake, dialog, lifecycle, and external
Library provider boundaries. Provider futures are controlled by bounded channels
and oneshot responses. Deadline coverage advances paused Tokio time and uses no
wall-clock sleeps. Successful fixture cleanup closes storage and observes shared
executor shutdown. These sources test routing; they do not establish correctness
of the independently owned service implementation or its continuation policy.

## Exact integration prerequisites outside owned files

Root must register the integration suite in `tests/desktop.rs` as:

```rust
#[path = "desktop/library_routing.rs"]
mod desktop_library_routing;
```

The nested coordinator tests and fixtures are included by this suite. They use
that exact module name. The new production child modules are declared from the
owned `application.rs` and `coordinator.rs`; no crate-root registration is needed
for them. The dependency already declares `desktop::library_service`.

The new exhaustive variants require these unowned source additions. Locations
refer to the unchanged dependency baseline:

| File and match | Required variant handling |
| --- | --- |
| `src/presentation/app.rs:699`, game result observation | `DesktopMessagesEnumResult::LibraryFailure` |
| `src/presentation/app.rs:742`, retained game command match | `DesktopEnumCommand::Library` |
| `src/presentation/app.rs:766`, `accept_response` | `DesktopMessagesEnumResult::LibraryFailure`, retained as typed Library state |
| `src/presentation/app.rs:793`, `accept_value` | `DesktopEnumValue::Library`, routed to the actual Library owner |
| `src/presentation/engine_panels.rs:703`, `receive_response` | `DesktopMessagesEnumResult::LibraryFailure` as an incompatible engine response |
| `src/presentation/puzzle/panel.rs:737`, response observation | `DesktopMessagesEnumResult::LibraryFailure` as an incompatible puzzle response |
| `tests/presentation/puzzle_panel.rs:224`, fixture operation match | `DesktopEnumCommand::Library` |
| `tests/desktop/native/bootstrap_games.rs:161`, fixture revision match | `DesktopEnumCommand::Library(operation) => operation.expected_revision()` |
| `tests/native/engine_composition.rs:68`, fixture revision match | `DesktopEnumCommand::Library(operation) => operation.expected_revision()` |
| `tests/native/engine_composition.rs:96`, fixture outcome match | `DesktopMessagesEnumResult::LibraryFailure` |
| `tests/native/game_composition.rs:412`, fixture revision match | `DesktopEnumCommand::Library(operation) => operation.expected_revision()` |
| `tests/native/game_composition.rs:452`, fixture value extraction | `DesktopMessagesEnumResult::LibraryFailure` |

These are source-inspection findings, not compiler diagnostics. All listed files
remain untouched by this patch. Root also owns wiring the real workspace and
service port through `with_library` in native bootstrap/host, service shutdown
ownership through the existing shared lifecycle, and the presentation Library
owner. Routing adds no native-host or UI placeholder. Until those assignments
and the service repairs are integrated, the combined application is not accepted
or claimed to compile.

## Verification and export

Read the global guidance, applicable project bridge, constitution, and all seven
canonical rule files. Inspected user path-rule frontmatter; none applies to the
authored Rust and Markdown paths. No nested guidance was found for these paths.

Allowed verification consists of pinned standalone rustfmt parsing/formatting,
Git whitespace checks, ownership and prohibited-content source inspection,
dependency hash/tree verification, and isolated-index patch replay. Exact export
paths, file hashes, final tree, and executed-check results are recorded in the
adjacent artifact manifest under the parallel-patches directory.

No Cargo, build, test, Clippy, runtime, CPU diagnostic, commit, push, agent, or
extra worktree operation was performed. All 14 tests remain uncompiled and
unexecuted. Root must run the appropriate registered checks after integrating
the listed prerequisites. No performance, native-window, platform, release, or
all-journey acceptance result is asserted.
