Desktop application action tickets are source ready for parent integration.
Executable verification remains with the parent. No Cargo, build, tests, Clippy,
formatter, runtime, or CPU diagnostics were run in this checkout. No test result,
behavioral baseline failure, compilation result, or performance result is claimed.

The implementation follows the [constitution](../../../.specify/memory/constitution.md)
and all seven canonical rules:
[code quality](../../../.specify/rules/code-quality.md),
[verification](../../../.specify/rules/verification.md),
[architecture](../../../.specify/rules/architecture.md),
[structure](../../../.specify/rules/structure.md),
[Rust style](../../../.specify/rules/rust-style.md),
[performance](../../../.specify/rules/performance.md), and
[testing](../../../.specify/rules/testing.md).

The source handoff owns exactly these paths:

- `src/desktop/application.rs`
- `src/desktop/messages.rs`
- `tests/desktop/application_action.rs`
- `specs/001-reckless-desktop-chess/evidence/application-action-ticket.md`

Before source inspection or owned edits, the two production paths were copied
from `/Users/sadrashameli/Personal/chess`, staged, and saved in dependency tree
`67a204c11658a673d07825ff86474a3e534dc28b`. Both checkouts started from commit
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`. The dependency tree includes only
those mirrored parent changes over this checkout's starting index. Other parent
APIs were inspected read only and were not mirrored. Their integration state
is outside this patch.

| Frozen parent path | Git blob SHA-1 | Content SHA-256 |
| --- | --- | --- |
| `src/desktop/application.rs` | `e0ca13bca56d1a54c5ddac0f3f9cf09eb65aea30` | `1f61697685842dbda9272c17850f64d0686c83532916001051cced7a2e5dfbc4` |
| `src/desktop/messages.rs` | `be35780c4c5abb598630aa02fad86f212710f8c8` | `27799c542efb977a1bd3a547af71178ab08df091ee918d1f6eb83892fa5f8d0a` |

The new contract test file was written and staged before production edits in tree
`8e555c57d8f3f2814e1712c1361bd08a9b99d6d1`, test blob
`e967dd66ef06c68c1e06802136bbbd6a796ae6a4`. That snapshot is exported separately
as `contracts-first.patch`. The frozen production files have neither
`DesktopApplicationAction` nor `prepare_action`; the new tests require this new
API. This is source-order evidence only. Those tests were never compiled or run
against the baseline. Subsequent test edits adjusted formatting and implicit
scope cleanup of unbound tickets.

`DesktopApplicationPort::prepare_action()` reserves one checked application
sequence and a canonical fresh request ID. `DesktopApplicationAction` has private
session, sequence, and request-ID fields, no public constructor, and no `Clone`
or `Copy` implementation. Its `request_id()` is stable across presentation passes.
Consuming `bind(command)` validates mutation revision presence and seeds the real
`DesktopRequest` with those exact identities. The request constructor remains
crate private; receipt construction remains private. `port.request(command)`
delegates to preparation and binding.

Preparation and binding do not read the clock, retain the application, create
receipts, touch admission state, reserve channel capacity, start deadlines, or
notify the actor/native event loop. Discarding an unbound ticket consumes only
its reserved sequence and identity. A failed bind also consumes its ticket.
Direct requests check the same canonical revision validator before preparation,
preserving their existing `MissingRevision` precedence and avoiding sequence
allocation for invalid commands. Valid direct requests use the same preparation
and consuming binding path as retained tickets.

All admission, cancellation, receipt retention, deadline, Busy retry, and lifecycle
logic remains unchanged. First submission still records the original timestamp
even if admission returns Busy; retrying the same actual request preserves it.
Separate fresh actions receive separate sequences across cloned ports.

Two inherited semantic limits were communicated before implementation and were
kept within the existing admission contract:

- A ticket reserves its sequence at preparation. If later completed work retires
  beyond that sequence, binding and submitting the held ticket can return
  `ResyncRequired`. Retain a ticket for the current widget operation, then handle
  authoritative resynchronization before preparing a replacement action.
- Independently prepared duplicate controls retain existing control coalescing.
  Their own request IDs stay distinct, but the returned shared receipt can carry
  the first admitted control's request ID. Ordinary admitted actions preserve
  their ticket ID through request, work, and receipt.

Eight new tests use the actual `DesktopApplication`, public command/revision
types, an injected atomic clock, and an injected wake provider. They have no
extra fixture registrations, wall-clock sleeps, tasks, network, filesystem
fixtures, or new dependencies. Authored coverage includes:

- Stable ticket identity through mutation binding, admitted work, receipt,
  completion, cloned request retry, and precise expected revision.
- Dropped unbound tickets leave the full ordinary admission capacity available
  without clock reads or native wakes.
- Delayed control binding and submission start the deadline at actual submission.
- Another application's port refuses the bound request before reading its clock;
  the original port can still admit that same request.
- Binding and direct requests reject mutations with no expected revision.
- Busy preserves the same actual request and its first submission timestamp,
  including refusal while an earlier work item is still running.
- Fresh tickets and direct requests across cloned ports retain distinct outcomes,
  with reverse binding/submission order and no duplicate dispatch.
- Completed requests outside the retention window refuse replay while fresh
  actions continue to dispatch.

The parent must register `tests/desktop/application_action.rs` exactly once in
the existing Cargo-discovered desktop test root, then run the focused and required
verification in its executable lane. This checkout changes no test registration,
shared exports, UI, or unrelated imports.

The incremental staged binary patch is exported as
`/tmp/chess-application-action-ticket-t3-f2af7b81/application-action-ticket.patch`
against the frozen dependency tree, never against the checkout's stale HEAD.
The adjacent `manifest.md` records final file blobs, content hashes, tree and
patch hash. Exact baseline contents are retained under `base/src/desktop/` in
that same handoff directory. Git whitespace checks and forward/reverse cached
patch application checks provide static handoff evidence only. Parent source
copies were compared with the frozen baseline before export.
