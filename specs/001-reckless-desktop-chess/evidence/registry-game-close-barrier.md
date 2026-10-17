# Registry single-game close barrier

This is an uncompiled source handoff for the authorized US3 T104/T106 and US9
close producer. It does not establish task completion or a native journey.

Base HEAD is `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`. The applied frozen
dependency is `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-native-mount-dependency-20261017/source.patch`,
SHA-256 `41954367eb94b0d1200cc79549a254de717139395d2236d7fce327661cea4472`.
Its verified index tree is `a02aa91579c030c35e2ffcfb5221e713f1a11bf6`.
The exported patch is the owned staged delta from that tree and excludes the
dependency itself. The adjacent export manifest records exact file and patch
hashes.

The applicable guidance was read from the global Claude instructions, project
bridge, constitution, and all seven files required by the
[canonical catalog](../../../.specify/rules/README.md). The behavior follows
[FR-001 and FR-004](../spec.md), [game and library ownership](../data-model.md),
[storage decisions](../decisions.md#storage-imports-and-backup-recovery),
[desktop commands](../contracts/desktop-ipc.md),
[study tools](../contracts/study-tools.md), and
[native game and library behavior](../contracts/ui-experience.md).

## API and ownership handshake

`DesktopGameRegistryPort::try_close_target` checks the caller's game revision
and storage revision and captures the actual installed supervisor event channel.
That channel is the registry owner epoch. A replacement supervisor cannot reuse
the captured identity, even with the same game ID and revision. Targets have no
public constructor. `try_not_open_close_target` records an expected storage
identity for the absence path; it grants no database authority.

`try_close_game` installs a source-owned admission barrier and returns a retained
`DesktopGameRegistryCloseReceipt`. Identical accepted targets join the original
operation. Admission is bounded by the existing registry creation capacity.
`operation_id` and `try_reconcile_close` recover the same accepted operation after
the caller abandons a wait. `wait` borrows the receipt. Its cancellation does not
cancel a supervisor receipt, task join, retained actor, or cleanup future.

Before close validation, accepted creation, installation, activation and saved
load/create/editor phases settle under their existing owners. Previously queued
saved opens receive Busy. A small sender wrapper protects both saved-open
entry points without changing `game_registry/saved.rs`. Because that request's
target is private to the frozen module, this wrapper refuses all saved opens
while any close barrier is held. Unrelated installed games retain their clocks,
commands, factories and owners. New-game identity admission and direct restore
also check the game-specific barrier.

The registry retains the original Quit response future. The supervisor owns the
real pause, save, engine stop and resource cleanup. Completion of a newly
submitted Quit requires both the successful response and the observed actor
return with `is_shutdown_complete`. An already returned matching retained owner
uses its observed cleanup and real final acknowledgment without repeating
factory effects. Failed commands retain their operation, and incomplete
returned actors remain owned. `try_retry_close` reconciles that operation using
Retry Save when required or retained cleanup. Stale owner/revision rejection
never rebases onto a replacement. Rejected requests can be explicitly canceled;
any incomplete returned actor remains with the registry's retained owners.

The only supervisor prerequisite adds seven source lines: retain the actual
`GameStorageReceipt` after successful session acknowledgment in `finish_save`,
initialize it to None, and expose `last_storage_acknowledgment`. No receipt is
constructed from an event, query, expected revision or game ID. The closed proof
checks this actual receipt against the final supervisor snapshot and save status.
Only then does the registry remove the session, source memory reservation,
snapshot watch and published snapshot. The certificate retains the final
snapshot and actual acknowledgment.

An absent source is certified only after registry admission is barred and all
accepted installation phases have settled. `NotOpen` reports registry absence
and the caller's expected storage revision. It does not assert that a database
row exists, is current, or can be deleted.

`DesktopGameRegistryCloseCertificate` is non-Clone. Dropping it relinquishes
the right to observe that certificate, while the registry continues holding the
barrier. The original receipt can reacquire the proof. Its lifecycle-provider
implementation supports `DesktopLibraryCommandsConfirmation`; its evidence
method also produces the existing storage evidence only from this registry
proof. Evidence extraction does not release admission.

## Parent integration

The Library service must retain the certificate with its actual storage
mutation, including across a dropped caller future. Close performs a real save
and can advance content/catalog revisions. Reconcile the original confirmed
intent against the resulting revisions without approving unrelated changes.
The existing confirmation refuses an old content revision; the source test
exercises this explicitly with a fresh repository preview after close.

After observing the current mutation's actual SQL commit acknowledgment, call
`try_finish_close` with `TrashCommitted(&receipt)`. A receipt for another game
or action is refused. The storage owner must associate this receipt with that
mutation; the registry does not claim database transaction authority. A confirmed
cancellation before submission, or an observed rollback/failure after all
accepted database work is settled, uses `CancelConfirmed` or `RollbackObserved`.
Await `wait_released` to observe source cleanup of the barrier. The API trusts
these explicit caller observations; dropping a wait or certificate is not one.

After observed cancellation/rollback release, a later explicit saved open may
load the paused source. No release path starts or resumes a game. Full service,
application and native UI wiring remain with the parent. No storage, Library
service, source gameplay, shared module roots or task markers were changed.

The parent must register `tests/desktop/game_registry/closing.rs` as a private
child of `desktop::game_registry`, alongside its existing private suites. Its
focused fixture lives in `tests/desktop/game_registry/closing/fixture.rs` and
uses the existing Library service fixture. Shared test roots remain frozen.

## Source cases and verification limits

Eleven deterministic test cases were authored, with the first contract cases
written before the new API implementation. They use real Registry,
GameSupervisor, application receipts, SQLite actors and storage acknowledgments.
Time is injected. Executor admission and factory cleanup use explicit permits
and notifications; there are no wall-clock sleeps or fabricated close/session
certificates.

Cases cover final acknowledgment/snapshot removal, SQLite save failure and
original-operation Retry Save, repeated close with unchanged source bytes,
certificate drop and proof reacquisition, bounded absence admission, in-flight
saved-load constructor failure, completed uninstalled source without writes or
factory creation, abandoned waits, held factory cleanup and retained source
memory, owner epoch/game/storage conflicts, unrelated active clock continuity,
and a certificate retained through an actual Trash repository acknowledgment.

Executed behavior tests: **0**. Cargo, build, Clippy, runtime/native checks and
CPU diagnostics executed: **0**. Pinned standalone rustfmt and Git/static
artifact checks are the only verification performed here. Formatting establishes
Rust parsing, not compilation, test discovery or behavior. No existing-component
production bug fix was attempted without a parent-observed regression.

Parent gates remain: register the suite, compile the exact integrated source,
run the focused cases and required Clippy gates, and validate real engine
stop/terminate/reap, deadlines, platform lifecycle and mounted Trash flows.
The cleanup fixture gates the real supervisor factory boundary for a local
game; it does not establish native subprocess or end-to-end evidence.
