# Position editor storage foundation

This source-only T150 handoff starts at commit
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68` with the domain dependency applied.
It does not mark T150 or the US6 journey accepted.

## Dependency and ownership

Dependency patch:
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/position-editor-domain.patch`.
Its SHA-256 is
`a645f2ad378016ebf0b957796d9247eb4ab2480c002a03127b069fbacb3f59f8`.
The verified dependency index tree is
`101c8284a830354963376f8102175a5d7ef4dbf1`.

The incremental patch contains only the editor repository and its transaction
module, migration 0004, the migration registration append, editor command/client
dispatch in the existing document store, the focused storage fixture and this
evidence. It excludes the domain dependency and all module/test root registration.
No library service, planner or UI files are changed.

## Public contract

`StorageEditorRepository::new(StorageDocumentClient)` binds the actual store.
Draft creation and identity validation remain owned by `BoardEditorDraft` and
`BoardEditorDraftId`. There is no game identity, fake game session, title,
selection or history field in the repository model.

The repository exposes these operations:

- `save(&BoardEditorDraft, StorageEditorRepositoryRevision)` synchronously admits
  a write and returns `StorageEditorRepositoryWrite`.
- `delete(BoardEditorDraftId, StorageEditorRepositoryRevision)` synchronously
  admits deletion and returns the same retained write owner.
- `read(BoardEditorDraftId)` asynchronously returns an optional
  `StorageEditorRepositoryRecovered`.
- `list(Option<StorageEditorRepositoryCursor>)` asynchronously returns a bounded
  `StorageEditorRepositoryPage`.

All operations return `StorageEditorRepositoryError` on failure. The storage
revision has canonical decimal-string serde, an explicit `ABSENT` value for new
identities, checked successor calculation and no conversion to domain revision.
The canonical domain ID rejects nil or noncanonical UUID input. Cursors wrap
that exact typed ID. Entries expose ID, storage revision, domain revision and
recomputed valid/invalid status.

Save and delete perform no implicit retry or identity replacement. The write
owner retains its exact ID, expected storage revision, saved domain revision
when applicable, and its original result receiver. `wait(&mut self)` borrows
that receiver across the await. Dropping a pending wait future preserves the
accepted operation and its original eventual receipt. Consuming the completion
twice returns `CompletionConsumed`. Callers must retain the write owner while
canceling its wait; dropping the owner cannot undo an already accepted write.
Receipts contain the actual ID, committed storage revision and a typed Saved or
Deleted result. Saved also carries the saved domain revision and validity.

## Storage and validation

Migration 0004 adds the dedicated strict `editor_drafts` table and a partial
index over saved identities. Draft rows do not enter game/document inventories.
The body uses the domain's existing validated serde directly. It stores raw
fields, domain revision, separate identity and explicit validity. Deserialization
recomputes validity and rejects an inconsistent saved status, duplicate piece or
right declarations, unknown fields and malformed typed fields. Invalid chess
positions and signed out-of-range counters remain saveable correction data.
Actionable findings are recomputed through domain validation after reopening.

Repository format version 1 is separate from database schema version 4 and from
both revision counters. Future draft formats are rejected before domain decode.
Stored ID/body mismatch, invalid revision encodings, zero durable revisions and
oversized payloads fail without replacing the original row. Save and delete
validate the existing row before mutation, so a malformed or future row cannot
be silently repaired by overwriting it.

Each save/delete runs an immediate transaction on the same existing serialized
storage actor connection. That connection already enables WAL, synchronous FULL
and foreign keys. Expected storage revision is checked inside the transaction.
New identities use INSERT; updates require an existing matching revision.
Revision overflow and transaction/commit failure produce no success receipt.
Receipts are constructed only after commit succeeds. An unresolved transaction
retires the actor through its existing unresolved-transaction error path.

Delete clears the body and advances a retained revision tombstone. Read treats
it as absent and lists omit it. This prevents an old expected-zero request from
recreating a deleted identity, including after reopening. Even an exact tombstone
revision cannot revive the ID. Only the ID, format and latest storage revision
remain; no deleted position fields or editing history are retained.

Reopening exposes the decoded draft and storage revision. `into_session()` uses
`BoardEditorSession::from_draft`, so selection and Undo/Redo start empty. Moving
the draft into the application session releases the transport reservation and
transfers domain ownership to that session.

## Capacity and bounded work

The existing store's actual admission semaphore and 64 MiB memory semaphore are
used. The editor client acquires both before invoking the closure that clones
the fixed-size domain draft or allocates the request box. No JSON encoding occurs
on the submitting async worker. Rejection releases partial admission.

Each draft operation reserves 64 KiB for the bounded raw board, canonical serde
records, encoded body and decode/validation scratch. Bodies are capped at 16 KiB
in the schema and repository. The raw domain has at most 64 occupied squares
and 64 distinct rights, so this bound does not depend on session history length.
The bound is a conservative design allowance, not measured allocation evidence.

Read/list SQL guards the byte lengths of body, ID and revision before returning
text to Rust. A separate null marker distinguishes a deleted body from a body
refused by the size guard. Borrowed SQLite values are checked before domain
deserialization. One canonical row decoder serves read, list, save and delete.
There is no second codec or database connection.

Lists use binary UUID keyset ordering, a typed exclusive cursor, a partial index
and a LIMIT of 100. A full page supplies its last ID as continuation; a final full
page can be followed by an empty page. Pages contain metadata only. Each list
reserves the metadata array plus one draft's decode allowance, decodes one row
at a time and retains its reservation with the returned page. Read similarly
retains its reservation with the returned draft. Separate page requests reflect
intervening explicit saves/deletes; they do not claim a persistent snapshot.

Encoding, SQLite work and bounded decoding run inside the existing blocking actor
under its shared `CapacityBudget` lease. No new runtime, CPU pool, blocking task,
connection actor or background receipt reconciler is created.

## Test source and verification limits

The initial focused test source was authored before production implementation.
There are 13 tests in `tests/storage/editor_repository.rs`, using real temporary
SQLite databases and real domain objects. They cover valid/invalid durable
round trips, raw invalid counters, empty Undo/Redo/selection after reopen, stale
saves/deletes, duplicate identity refusal, tombstone persistence, exact original
save/delete completion after canceled waits, stable 100-entry keyset pages,
shared queue and memory refusal, malformed/status/identity/unknown-field/oversize
and future-format preservation, deferred foreign-key failure at COMMIT,
migration continuity/future-database rejection, canonical identity/revision
boundaries, maximal raw fields and exhausted storage revisions.

Cancellation and queue fixtures hold an explicit one-worker capacity lease and
poll once, then release it and observe accepted completions. They use no sleeps.
All fixture-side database mutations occur with the application store closed.
The deferred foreign-key fixture permits UPDATE to execute and makes COMMIT
fail, checking that neither save nor delete returns a receipt or changes the row.

Executed behavior tests: **0**. No baseline red, compile, Cargo command, build,
Clippy, runtime acceptance or CPU diagnostics ran. The fixture is not registered
locally and has not been compiled. Source inspection, the permitted pinned
standalone formatter, whitespace checks and patch/tree integrity checks are the
only verification in this handoff. They do not prove Rust type checking,
transaction behavior or the US6 journey.

## Parent integration

The parent must register `pub mod editor;` in `src/board/mod.rs`,
`pub mod editor_repository;` in `src/storage/mod.rs`, and the focused fixture in
`tests/storage.rs`. Those parent-owned files are excluded. No visibility mirror
is needed or included in this patch.

Migration 0004 is appended without changing migrations 0001 through 0003 or the
existing migration algorithm. At this baseline there is no database-schema
expected-3 assertion requiring a parent edit. Specifically,
`tests/storage/puzzle_repository.rs:188` derives the expected version from
`StorageMigration::core().len()`, while
`tests/storage/document_store.rs:96` correctly expects zero after rollback.
`tests/storage/game_document.rs:741` expects supported game-document codec 3;
that is unrelated to database schema 4 and must remain 3.

The parent owns fixture registration, compiler/Clippy repair, all execution,
service/UI integration, saved-list journey behavior and acceptance evidence.
T150 here is a source foundation only. T151 through T155 and complete US6
acceptance remain outside this patch.
