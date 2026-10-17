The editor application/coordinator route is source ready for parent integration.
Eight regression cases were authored before production edits. None were compiled
or executed, and no observed semantic red or green is claimed. The parent's
executable lane and frozen release build remain outside this handoff.

The work loaded the global guidance, root bridge, [constitution](../../../.specify/memory/constitution.md)
and all seven canonical rules: [architecture](../../../.specify/rules/architecture.md),
[code quality](../../../.specify/rules/code-quality.md),
[performance](../../../.specify/rules/performance.md),
[Rust style](../../../.specify/rules/rust-style.md),
[structure](../../../.specify/rules/structure.md),
[testing](../../../.specify/rules/testing.md), and
[verification](../../../.specify/rules/verification.md).

The incremental handoff owns exactly these eight paths:

- `src/desktop/application.rs`
- `src/desktop/messages.rs`
- `src/desktop/dto.rs`
- `src/desktop/coordinator.rs`
- `src/desktop/coordinator/editor.rs`
- `tests/desktop/coordinator_editor.rs`
- `tests/desktop/coordinator_editor/fixture.rs`
- `specs/001-reckless-desktop-chess/evidence/editor-application-routing.md`

The starting commit is `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
Dependencies were SHA-256 verified and applied in this order, using the absolute
paths under `/Users/sadrashameli/Personal/chess/.git/parallel-patches/`:

| Dependency patch | SHA-256 |
| --- | --- |
| `position-editor-domain.patch` | `a645f2ad378016ebf0b957796d9247eb4ab2480c002a03127b069fbacb3f59f8` |
| `position-editor-storage.patch` | `3e8302c57aab0db3c1d4c2e0c72e840df8717e5d3e8a85f4dfe302bdc34f4f59` |
| `position-editor-commands-ui.patch` | `5598c41c94f14183e0204eb98471f30bad5325a895a1f80ea4ea82d593ff8b17` |

The parent's current four owned source files were then mirrored verbatim from
`/Users/sadrashameli/Personal/chess`, retaining Library integration. This produced
tree `cbf0a39275c5d0a6c0d07b6484584f5b56487e13`. The supplied action-ticket patch
`/tmp/chess-application-action-ticket-t3-f2af7b81/application-action-ticket.patch`,
SHA-256 `235ddb4eb5809007b8a531c0509e277f4133179a782c377d88874a120b067525`,
was then applied without conflicts. The resulting frozen dependency tree is
`151c660e672e90548398fb33fc88d18eb758ba53`. The export is relative to that tree.
It contains no dependency or mirrored-parent changes. The tree records this
checkout plus those explicit dependencies, rather than the entire parent working
state. Shared module registrations and other parent source files were inspected
read only where required and were not mirrored or edited.

| Frozen dependency file | Git blob SHA-1 | Content SHA-256 |
| --- | --- | --- |
| `src/desktop/application.rs` | `a753ae37f000ccfc549cb34699bd6b3814cac647` | `422dceaeaa64f893278102340e2d0daf3df6c6a813180bd9373aa7bf92934283` |
| `src/desktop/messages.rs` | `89ffce467cc6fe7a3b19c82cc48ced8ed727b01c` | `f701bdb708108c9e4d7883f33091411c8a9e6a8c1984d9c82a250828a5ca1ad9` |
| `src/desktop/dto.rs` | `fa0ea8a01791f97c1a1288953c7d97c0e03178f0` | `157e61bdc1a1f01e7b9bc7ae72967c84a7ee79c4c2a8c38679cd5d158945b609` |
| `src/desktop/coordinator.rs` | `3abf2f64e1a039fef5cbc3240bd4dc7b8b929bb8` | `276ae5f8f53a039e3ef054574d330927c08454197a0d318960281cf756000604` |

Before production edits, the tests and focused fixture were staged in tree
`bfa7667fd486faea3c0b8d9b61d070082dbc9e55`. Their blobs are respectively
`8e9f354f1580b4052c510cd0639eed99c1453cc3` and
`ad7a64696d02d3b49718cc99769f73ee7490c00b`. The historical source-order snapshot
is `contracts-first.patch`, SHA-256
`60a941048f34c917e0bcf743e82b2085e767af1b8a4ba27e1146d8cf716e47ff`,
in the export directory. Final test refinements correct the inherited Stop
contract to `ReconciliationRequired` after a durable accepted write, assert the
actual initial application result revision, derive bootstrap capacity from the
shared budget, avoid initializing an unused executor, and adjust source formatting.
The historical snapshot and final cases have no executable baseline evidence.

`DesktopEnumCommand::Editor { owner, operation }` uses the stable workspace owner
for mutations through the producer's canonical `is_mutation()`. Draft and opening
session identities never retarget that owner. `DesktopCommand::editor` supplies
`Some(ZERO)` without an expected context and the canonical editor revision when a
context exists. The same crate-private calculation validates dispatch, so a
manually constructed mismatched native revision is refused. The route also
refuses a wrong workspace, an unavailable owner, and overlapping editor futures.
Existing bounded ordinary application admission and mutation ownership remain
authoritative; no queue, channel, provider trait, runtime or worker pool was added.

`with_editor(owner, DesktopEditorCommands)` injects the actual producer. Its owned
future moves the producer out of the coordinator while `handle` awaits Read or
List. A `WritePending` result continues directly to `finish_write` using that
same producer. The actual application work and receipt remain in the pending
slot, and the producer retains its actual receipt. The select branch receives
both the producer and actual result, restores the original owner, then publishes
once through `DesktopApplication::complete_editor`. That completion preserves
`Arc<DesktopEditorCommandsError>` as `EditorFailure` and refuses to finalize
`WritePending`. Response ownership survives application Busy retry and is consumed
only inside the existing completion transaction. Result revisions come from the
actual snapshot or typed result context.

Cancellation does not drop an accepted write or replace its immutable source
snapshot. The coordinator continues polling to the actual acknowledgement or
producer error. Read and List rely on the producer's existing cancellation check
after their awaited result; the coordinator adds no shortcut that would discard
the source owner or awaited outcome. Unrelated Bootstrap and control dispatch
continue through the existing run loop and select. Quit draining checks both the
in-flight editor operation and a returned producer with a retained pending write.

The known failed-write recovery dead end is preserved for the parent's tests-first
repair. A failed acknowledgement returns the exact typed producer error and the
same producer with its pending source state. There is no automatic retry,
replay, revision rebase, replacement owner, or synthetic success. An unresolved
producer write continues to prevent Quit readiness even after its application
failure receipt has been published. This handoff adds no producer recovery API
or change to its error/retry semantics and claims no executed failure-recovery
coverage.

The eight new cases use actual application action tickets, receipts, coordinator
run futures, producer/domain objects and a temporary SQLite store. An injected
atomic clock and native wake provider control observation. A real `BEGIN IMMEDIATE`
transaction delays storage acknowledgement; no wall-clock sleep, fake receipt,
forged editor DTO or completion-as-WritePending fixture is used. Assertions follow
transaction release, coordinator Quit observation, storage close and temporary
directory cleanup. Covered source contracts are:

- Initial creation with revision zero and a stable workspace owner.
- Mismatched native revision and wrong workspace refusal before mutation.
- Stale domain context with the original typed error and unchanged current draft.
- Save waiting for acknowledgement, same-action deduplication, entity backpressure,
  a single storage revision increment and responsive unrelated Bootstrap.
- Canceled accepted Save retaining its original acknowledgement and source context.
- Pending Save preventing Quit cleanup until actual write observation.
- List cancellation checked after the awaited real page.
- Read cancellation checked after the awaited real row.

The parent must apply the action-ticket dependency before this incremental patch,
register `desktop::editor_commands` and the dependency domain/storage modules,
retain its existing Library submodules and identity/revision gateways, and register
`tests/desktop/coordinator_editor.rs` once in its Cargo-discovered desktop test
root. Native bootstrap, app/UI integration and wider exhaustive enum arms remain
parent-owned. The new coordinator submodule is registered in the owned
`coordinator.rs`; no shared root was edited. The local dependency tree deliberately
has no copied parent registrations or Library submodule source and is not a
standalone compilable integration snapshot.

The binary full-index incremental patch and exact manifest are under
`/tmp/chess-editor-application-routing-t3-394f71c8/`. The manifest records the final
index tree, file blobs, SHA-256 values, patch hash and static applicability checks.
Frozen parent copies before action tickets are in `parent-base/src/desktop/`;
the exact incremental dependency copies are in `base/src/desktop/`.

Executed checks are dependency hashes, source/ownership review, new-file rustfmt,
Git whitespace checks and forward/reverse cached patch application. The standalone
pinned formatter was applied only to the three new Rust files with child traversal
disabled. No Cargo, test execution, build, Clippy, app runtime or CPU diagnostics
were run. Authored tests: 8. Executed tests: 0. No agents, commits or pushes were
created. Static source evidence establishes no compilation, behavioral, native,
performance, release or platform acceptance result.
