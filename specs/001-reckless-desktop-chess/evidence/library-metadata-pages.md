# Library metadata pages: isolated source handoff

This is the source-only prerequisite for mounted library organization and
breadcrumb navigation in T106, T107 and T109. It does not establish task
acceptance, mounted UI behavior, compilation or successful execution.

Baseline commit: `e618533aa6f2f74da54b3d745f2b7b4ac19ecbc3`.
Baseline tree: `4d3366387d83ea197de2ff222124e45657fea648`.
All four owned paths were absent at that baseline. No shared file is included.

The global/root guidance, constitution and all seven
[canonical rules](../../../.specify/rules/README.md) were read. The design follows
[US3](../spec.md#user-story-3---play-locally-and-keep-games-priority-p1),
[study tools](../contracts/study-tools.md#saved-games-pgnfen-and-annotations) and
the [Library workspace](../contracts/ui-experience.md#workspaces). The parent
accepted the focused metadata rows and actor handshake before production source
was written. Canonical validated `FolderId`, `TagId`, `Name` and `Revision`
values already exist; canonical folder/tag read aggregates do not exist at this
baseline, so the focused read rows compose those existing values.

## Owned file manifest

| Path | Purpose |
| --- | --- |
| `src/storage/library_metadata.rs` | Typed rows, opaque scoped cursors, retained pages, cancellable read owner and client facade |
| `src/storage/library_transaction/metadata.rs` | One-connection transaction helper, bounded keyset reads and canonical value parsing |
| `tests/storage/library_metadata.rs` | 20 deterministic public-contract cases using temporary SQLite and real organization writes |
| `specs/001-reckless-desktop-chess/evidence/library-metadata-pages.md` | Integration handshake and limits |

Source SHA256 values at export:

```text
2694facbbcb68ca31d29761c76d3b9d1ec61011904d6eaf47e31460ea27d365b  src/storage/library_metadata.rs
7b82d42b83d4bf0891670fe2276da679f5384a7ce6841f27c70981e321903666  src/storage/library_transaction/metadata.rs
c2442d1a12da7e29ca7a863fb21b4cfdac61a491350c9a21ca3d8be3d90a76a6  tests/storage/library_metadata.rs
```

## Public API and data contract

`StorageLibraryMetadataRepository::new(StorageDocumentClient)` uses the existing
client, admission path and actor. It creates no executor, connection or pool.

```rust
pub fn begin(
    &self,
    request: StorageLibraryMetadataEnumRequest,
) -> Result<StorageLibraryMetadataRead, StorageLibraryError>

pub async fn folders(
    &self,
    parent: Option<GameLibraryOrganizationFolderId>,
    cursor: Option<StorageLibraryMetadataFolderCursor>,
) -> Result<StorageLibraryMetadataFolderPage, StorageLibraryError>

pub async fn tags(
    &self,
    cursor: Option<StorageLibraryMetadataTagCursor>,
) -> Result<StorageLibraryMetadataTagPage, StorageLibraryError>

pub async fn folder(
    &self,
    id: GameLibraryOrganizationFolderId,
    catalog: StorageLibraryCatalogRevision,
) -> Result<StorageLibraryMetadataFolderRead, StorageLibraryError>
```

`StorageLibraryMetadataRead::wait(&mut self)` returns the typed response enum.
Dropping a wait future preserves its existing `StorageLibraryPending` receiver
on the read owner. A second observation after completion returns
`AlreadyObserved`. Convenience reads transfer ownership to the existing actor
while they wait; callers needing resumable observation retain `begin`'s owner.

Both page types expose borrowed `entries`, `next_cursor` and `catalog_revision`.
Folder reads expose borrowed `folder` and `catalog_revision`. Folder rows expose
typed `id`, `parent`, `name` and `revision`; tag rows expose typed `id`, `name`
and `revision`. Names come from SQLite and the canonical Name constructor.

First pages read the actual catalog inside the transaction. A folder cursor
contains that revision, its exact optional parent and the exclusive full typed
last ID. Tag cursors have their own full typed last ID and catalog. Neither
cursor can be fabricated through public fields or constructors. Folder scope
mismatch returns `CursorQuery`; stale catalog returns `StaleCursor`. One-folder
reads validate the requested catalog before resolving the ID.

The page size reuses `StorageLibraryQuery::PAGE_SIZE`, currently 100. SQL reads
at most 101 rows to determine continuation, materializes at most 100, and uses
strict ID range predicates for continuation with no OFFSET. Canonical UUID
text ordering preserves full UUID order. Root reads use `parent_id IS NULL`;
orphaned rows keep their stored parent and never become synthetic roots.
Missing explicit parents return `MissingFolder`; existing parents with no
children return a successful empty page. SQLite failures remain `Sqlite` and
shared-budget refusal remains `Store(Saturated)`.

## Reservation and lifecycle ownership

The transaction helper receives the actor's existing `&Arc<Semaphore>` and calls
`StorageLibraryMemory::new` before allocating the page Vec, name Strings or
retained Arc body. No additional parent budget helper or visibility is needed.
The shared client admission reservation covers the fixed typed request.

Checked arithmetic reserves the complete retained payload, page wrapper, Arc
header, permit Vec element, full row capacity, maximum name bytes for every
row, an extra maximum name allocation for canonical construction, and bounded
UUID/revision parsing and binding scratch. The name bound uses the actual
`GameLibraryOrganizationName::MAX_BYTES` of 1024. SQLite text is borrowed before
checking the canonical byte bound, so corrupt overlong names cannot cause an
unreserved Rust String allocation. IDs, parents and revisions use their actual
canonical `FromStr` implementations; names use their actual constructor.

Each page or lookup result contains an Arc of a private retained payload whose
last field owns `StorageLibraryMemory`. Cloning the public result shares that
same payload and permit. The allocation and its reservation survive until the
final result clone drops. Error paths drop the local reservation. Actor admission,
CPU-lease ownership, abandoned-response disposal and connection ownership remain
with the existing actor.

## Exact parent integration

Apply only after the atomic-import worker exports its shared-file changes. These
snippets describe parent edits and are deliberately absent from this patch.

In `src/storage/mod.rs` add:

```rust
pub mod library_metadata;
```

In `tests/storage.rs` add:

```rust
#[path = "storage/library_metadata.rs"]
mod storage_library_metadata;
```

In `src/storage/library_transaction.rs` add the module and imports:

```rust
mod metadata;

use crate::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataEnumResponse,
};
use crate::storage::library_transaction::metadata::StorageLibraryTransactionMetadata;
```

Add exactly one variant to each existing transaction enum:

```rust
Metadata(StorageLibraryMetadataEnumRequest),
```

```rust
Metadata(StorageLibraryMetadataEnumResponse),
```

Add `Self::Metadata(_)` to the successful fixed-request group in
`StorageLibraryEnumTransactionRequest::validate_memory_owner`. Add it to the
zero-payload group in `unreserved_admission_bytes`; retain that method's existing
outer-enum-size charge. Metadata requests have no allocated payload or prepared
memory owner to validate.

Add `StorageLibraryEnumTransactionRequest::Metadata(_)` to the
`TransactionBehavior::Deferred` group. Add this exhaustive dispatcher arm:

```rust
StorageLibraryEnumTransactionRequest::Metadata(request) => {
    StorageLibraryEnumTransactionResponse::Metadata(
        StorageLibraryTransactionMetadata::execute(&transaction, request, memory)?,
    )
}
```

The new helper's exact visibility/signature is already present in its owned file:

```rust
pub(super) fn execute(
    transaction: &Transaction<'_>,
    request: &StorageLibraryMetadataEnumRequest,
    memory: &Arc<Semaphore>,
) -> Result<StorageLibraryMetadataEnumResponse, StorageLibraryError>
```

No shared error, document actor, schema, dependency, prepared-write, import or
organization-mutation changes are required by this slice. Preserve all new
atomic-import request variants and memory-owner handling during integration.

## Authored verification and remaining gate

20 tests are authored; 0 are executed; all 20 remain unrun. The first 17 tests
were written and staged before production source. Three subsequent boundary
cases add exact-100 termination, reparented-row metadata and orphan-root refusal.
No existing production defect was repaired and no observed-red result is claimed.

| Coverage | Cases |
| --- | --- |
| Root/child pages beyond 100, full UUID ordering, exclusive continuation, tags and foreign scope | 5 |
| Empty versus missing parent, reparented metadata and actual orphan parent preservation | 4 |
| Catalog invalidation, breadcrumb names/entity revisions, stale lookup and unchanged read revision | 4 |
| Maximum multibyte UTF-8 names | 1 |
| Shared exhaustion and final-clone release for folder pages, tag pages and folder lookups | 3 |
| Cancelled wait retains accepted read; closed actor refusal | 2 |
| SQLite failures stay distinct from successful empty pages | 1 |

The memory cases use 100 actual maximum-length folder/tag names, fill the real
actor byte budget through public reads, and prove that intermediate clone drops
do not release capacity. Final drop permits one replacement result; releasing
all retained results restores admission. Cancellation is coordinated with the
injected one-worker capacity lease and explicit polling, with no sleeps. All
fixtures use temporary paths. The orphan case closes the actor before inserting
one deliberate inconsistent row into its temporary database, then reopens it.

Static checks: standalone pinned `rustfmt --edition 2024 --check` for the three
Rust files; staged and working-tree `git diff --check`; owned-file and source
restriction inspection; forward and reverse `git apply --cached --check` using
a temporary index seeded from the baseline; exact exported-tree equality; and
restoration of the baseline tree after reverse application. The temporary index
is scratch data, not another checkout or Git worktree.

The patch is `git diff --cached --binary HEAD --` for exactly the four manifest
paths. The final handoff supplies its SHA256 and staged tree. No Cargo, build,
tests, Clippy, CPU diagnostics, agents, extra worktrees, commits or pushes were
run. Compilation, lint and behavior verification require the parent's combined
registered lane after atomic-import integration. Source checks cannot accept
these tasks or establish measured performance.
