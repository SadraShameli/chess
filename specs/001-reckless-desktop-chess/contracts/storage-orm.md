# Storage ORM compatibility contract

The owner-directed ORM migration implements the existing storage obligations in
[FR-004, FR-014, FR-021 and FR-025](../spec.md), with the adapter selected in
[the plan](../plan.md#storage-orm-migration). It introduces no player-facing
workflow or new public database API.

## Connection and transaction ownership

The existing storage actor owns one Diesel SQLite connection. Its public clients
and repositories retain their typed request/result contracts. Capacity admission,
queue bounds, memory reservations, close admission and retained worker completion
remain with their existing owners. No UI/runtime worker performs blocking SQL.

Every content save, organization change, import, Trash operation and puzzle
checkpoint keeps its current atomic transaction boundary. A successful receipt
follows confirmed commit. Revision conflicts and failed commits expose errors
without a successful receipt or partial content/projection/journal replacement.
An unresolved transaction or page-ceiling restoration retires the connection
through the existing typed failure path; a timer cannot establish cleanup.

## Compatibility and mappings

Existing schemas 1 through 3 and canonical stored document bodies remain
readable without rewriting identity, revision, text counters or body bytes solely
because the ORM changed. Future schema versions are rejected before migration.
Migration failure rolls back its complete schema/version change. Existing table,
index, foreign-key, trigger, JSON and STRICT constraints remain unchanged.

Typed mappings cover documents, library state/entries/tags/folders, puzzle
collections/runs/events/attempts/checkpoints and supported existing game schema.
Ordinary record reads and writes use Diesel mappings and query operations.
PRAGMA, schema inspection, recursive hierarchy traversal and physical dbstat
queries may use scoped typed Diesel SQL. Production does not retain a rusqlite
connection or a replacement API that imitates its row/statement interface.

Field validation remains at the existing boundary. Canonical unsigned counters
remain text, optional metadata remains optional, and invalid stored values remain
typed corruption/errors. IDs, body/schema versions, journal digests, ordering,
cursor identity, modified-time policy and duplicate matching retain their current
meaning. New ORM rows do not become an alternative domain model.

## Required validation

Use real isolated SQLite fixtures and actual actor/repository operations for
legacy reads, fresh migrations, rollback, revision conflicts, durable receipts,
library metadata/organization/Trash, heterogeneous keyset paging, atomic import
space ceilings and puzzle journal/checkpoint recovery. Preserve cleanup before
final assertions in newly authored fixtures. Independent raw SQLite setup and
inspection is allowed as a test-only dependency.

Compare the same tests before and after migration. Exercise the existing native
application storage consumers through their hermetic suites. Formatter/source
checks cannot replace compilation, lint or behavioral evidence. Final evidence
records actual commands/counts, schema continuity and every unrun platform gate.
