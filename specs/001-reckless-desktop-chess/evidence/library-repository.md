# Library backend source handoff

## Status and scope

T101 through T104 owned backend source is authored in the isolated
`t3/library-backend-foundation` worktree at baseline
`39994327f2742cb51c80a02d54d9d72943be3269`. This is source-authored, uncompiled
and unrun work in this lane. Parent-observed baseline failures and the subsequent
source-only repair are recorded below. It is not task acceptance. No task or
checklist markers changed.

The frozen codec dependency was applied locally from
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-codec-integration-current.patch`.
Its SHA-256 is
`28f2ab7bacb4e36f965d510d312be863d7c090f0fabbe026b1a5ea6b4493e0a2`.
Its 13 paths are excluded from this handoff. The codec is unchanged, including
its accepted selected interior notation tip. Root's reported codec verification
is separate upstream evidence, not execution evidence for this backend.

Required global/project guidance, constitution, all seven canonical rules,
applicable feature spec/plan/data-model/decisions and the study-tools contract
were loaded. The Spec Kit prerequisite command needed the explicit
`SPECIFY_FEATURE_DIRECTORY=specs/001-reckless-desktop-chess` environment value
because this isolated branch has no feature selector. All three existing
checklists have no unchecked entries. No extension hook file exists.

## Frozen root integration contract

`StorageLibraryEnumTransactionRequest` and
`StorageLibraryEnumTransactionResponse` live in
`src/storage/library_transaction.rs`. Their nine variants and payload types
match the agreed actor contract. The exhaustive synchronous dispatcher takes
`&mut rusqlite::Connection`, the request and `&Arc<tokio::sync::Semaphore>`.
It opens no connection or executor and does not decode concrete game bodies.
Every mutation uses an Immediate transaction. Reads that need multiple
statements share one read transaction. A response escapes only after commit.

The review repair replaces the original request `transport_bytes` method with
`StorageLibraryEnumTransactionRequest::unreserved_admission_bytes(&self)`,
returning `Result<usize, StorageLibraryError>`. Save and Adopt carry mandatory
prepared reservations and report zero additional bytes; other requests report
their complete unreserved admission charge.

`StorageLibraryPending` lives in `src/storage/library_repository.rs`.
`new(oneshot::Receiver<Result<StorageLibraryEnumTransactionResponse,
StorageLibraryError>>)` returns `Self`. `wait(&mut self)` returns the response
result and keeps the receiver in the owner if the wait future is cancelled.
A second completed observation returns `AlreadyObserved`.

Root supplies only the promised client entry point:

```rust
pub(crate) fn begin_library(
    &self,
    request: StorageLibraryEnumTransactionRequest,
) -> Result<StorageLibraryPending, StorageLibraryError>;
```

The single actor Library command retains its admission and input-memory
reservations through dispatch under the existing actor capacity lease. Pages,
adoption inventories and recovered records own output-memory reservations.
`StorageLibraryMutation::wait(&mut self)` provides a public retained mutation
owner with checked response-kind matching. Lifecycle owners should retain it
until observing the outcome, including after cancellation of an individual wait.

## Implemented source

Migration 3 preserves existing documents, legacy tables, bodies, IDs and content
revisions. It adds organization tables, checked domain discriminants, foreign
keys and query indexes. The approved structural extension adds
`identity_initial_fen`, `identity_mainline_uci` and `content_sha256`, plus the
conservative `(rules_variant, identity_initial_fen, plies, game_id)` index.
FEN and UCI come from the decoded record's canonical identity; SHA-256 covers
the exact accepted body. This is not a replacement identity serialization or a
hash-only duplicate decision.

Prepared writes decode the exact supplied body on the injected executor, check
its outer game ID, derive the projection and retain the private accepted data.
The transaction CAS checks only `documents.revision`, advances it once, preserves
organization and rejects Trash writes, kind replacement and overwriting
unadopted documents. New content requires `Changed(timestamp)`. A Checkpoint
preserves modified time; Changed uses the maximum of the existing and injected
timestamp. Adoption inserts default organization with Unknown modified time and
never rewrites the document. Organization CAS uses a distinct domain revision.

The organization owner validates names, canonical non-nil UUID IDs, revisions,
nonnegative Unix-millisecond timestamps and distinct bounded tag sets. Mutations
cover create/rename/reparent folders, create/rename/delete tags, favorite, folder
assignment and tag replacement. Missing references and cycles fail. No-op
assignments preserve revisions and modified time. Displayed folder-path and tag
changes touch affected games in bounded 100-row batches without rewriting bodies.

Queries support every agreed filter, six sorts, explicit missing-field exclusion
and at most 100 results. Text normalization calls root's canonical public T099
methods. Player '?' semantics remain separate from generic text. Date intervals
compile on the executor to at most 366 month/day windows with validated calendar
witnesses and leap-year checks. All-unknown dates are excluded under an active
date filter. Entries disclose partial-date matches. Keysets contain null rank,
every sort component and canonical ID; cursors bind the normalized query and
catalog revision. No OFFSET is used. Query and load preserve modified time.

Every relevant changed content/adoption/organization/Trash/delete commit bumps
the checked catalog revision. Content Checkpoints conservatively invalidate
cursors too. No-op organization and AlreadyCurrent adoption preserve the stamp.
Query pages expose their stamp and exact unindexed-or-stale document count.
`StorageLibraryEnumIndexCoverage` distinguishes every document indexed from an
unindexed-document count. Generic documents remain conservatively unindexed;
this count does not certify that an adoption pass has classified them.

Destruction uses an immutable exact plan and a private approved-plan wrapper.
Root alone can mint approval and closed-game evidence through crate-level
constructors. Folder removal revalidates subtree counts/catalog state, unfiles
Library and Trash entries, touches their organization revisions, then deletes
the folders atomically. Bodies, tags and favorite state survive. Trash preserves
the row, body and grouping. Restore updates the same row without ID replacement.
Permanent deletion requires current approval and Trash, then deletes the document
so foreign keys remove dependent entries. There is no expiry.

Adoption inventories contain at most 100 UUID/revision candidates and a catalog
stamp. Generic UUIDs, including nil generic document IDs, remain valid inventory
boundaries; game IDs are validated when a recognized body is prepared. A
boundary-only envelope probe distinguishes unrelated documents. Version-plus-game
fields, even null/corrupt fields, declared played/notation kinds and existing game
associations require the canonical decoder. Recognized failures carry ID,
observed revision and a typed source. Preparation and transaction both detect
revision changes. Adopt returns Inserted, AlreadyCurrent or Changed, preserving
existing organization and all document bytes/revisions.

## Authored contracts and verification

The first missing-functionality contract tests were authored before production
implementation. Additional source contracts cover organization, destructive
plans, adoption and bounded ownership. No tests were executed in this lane.
The original export claimed no semantic red or green and repaired no existing
source defect. The later parent-observed reds authorize the review repair below.

Private transaction contracts are included by `#[cfg(test)] mod tests` from
`src/storage/library_transaction.rs`; do not register those a second time.
They use real canonical notation records, in-memory SQLite, injected timestamps
and the existing bounded executor. Coverage includes independent CAS,
Checkpoints and monotone time, partial-calendar witnesses against an enumerated
reference, all six keyset sorts, stale cursors/plans, folder descendants and
Trash entries, no-op/cycle/reference refusal, displayed grouping changes,
permanent delete, adoption idempotency/CAS, catalog-overflow rollback,
recognized corrupt/future envelopes, generic UUID migration continuity,
retained cancelled waits, exact-body identity/hash projection and memory permits.

Public boundary tests live in `tests/storage/library.rs`. Root must include
that file once from the existing storage integration-test root. They cover
canonical revision/name/timestamp validation and complete ordered calendar
interval boundaries.

Allowed verification is pinned standalone rustfmt formatting/checking, Git
whitespace checks, explicit staged-path inspection, codec reverse-apply
verification and handoff forward/reverse-apply checks. These are static checks,
not compilation or runtime evidence. No Cargo, build, tests, Clippy, CPU
diagnostics, agents, extra worktrees, commits or pushes ran in this lane.

## Root-owned integration and acceptance

Root registers the five new storage modules, `game::library_organization`, migration
3 and the public test include; supplies the public canonical_player/canonical_text
normalization methods; and adds the single client/actor Library dispatch.
The codec dependency and local root registrations are excluded from the export.

Root must guard generic saves against overwriting indexed games and route all
production game content writes through the prepared transaction before exposing
organization. Any remaining generic writer that can create/change a game-envelope
candidate must also invalidate the catalog stamp; otherwise adoption readiness
or T100 duplicate-preview stamps could miss a phantom candidate.

Production saves still need the real injected wall-time and typed mutation-cause
boundary. `GameStorageSaveRequest` has neither; game revision changes alone do
not identify content edits. No timestamp is invented here and no shared played
save, autosave, session/tree or lifecycle source changed.

Root owns the full adoption scan coordinator and readiness receipt. Retain each
Changed candidate for bounded retry; retain issues and unresolved changes as
incomplete. A completed duplicate preview must first establish settled adoption
and then use one catalog stamp across all candidate pages. This backend exposes
no false settled-adoption result. Future T100 candidate/read/insert-only actor
hooks and import_session remain separate work.

T104 runtime prerequisite acceptance remains root-owned. Root must serialize the
real confirmation, pause/stop, successful durable save and tab closure. A failed
save prevents close and Trash. Durable prerequisite saves invalidate older plans;
root must retain the original confirmed target/scope and actual pause/save/close
receipts, validate their exact before/after binding, then obtain a current plan
for that same action. Changed targets or subtrees require their own authorization;
an anticipated prerequisite sequence does not require a second confirmation.
This lane supplies no production proof that
a game is closed. Current plans and matching lifecycle content revision are
required for game destruction.

After integration, root must compile and run focused contracts and canonical
gates, inspect diagnostics, verify caller/cancellation ownership and perform
actual save/autosave/lifecycle tests. Scale, latency and all task acceptance
remain unverified. T101 through T104 are intentionally left unchecked.

The staged binary patch is exported to
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-repository.patch`.
Its SHA-256 is reported with the handoff instead of embedded in this file.

## Parent-observed review repair

The original full export remains unchanged with SHA-256
`f9b63b2d00700ed5f0cfd7c7dcd34127a15c022efd9af22bb582fa4d3f40780c`.
Before repair, the exact incremental parent artifact
`library-backend-parent-compile-and-deep-test.patch` was applied to that delivered
snapshot. Its SHA-256 is
`9126b5537be5ef664be9a374c683ca10ebef8b2bfa7c10765e5c10b01ea4766a`.
It contains the signed SQLite conversion corrections and the tests-only deep
folder regression. The resulting baseline was frozen as Git tree
`0cfae2f31908c08bff3fb81270b08c47cb639fd9`, without creating a commit.

The exact baseline blobs for every repaired path are:

- `src/storage/library_prepared.rs`: `3cd7b14c50d061aefb05e7c686578d4b19f3b840`
- `src/storage/library_repository.rs`: `f10685ba60dd0cbffa548c0c216005a0300d1934`
- `src/storage/library_transaction.rs`: `de30aaba4ea91369db08457b2d2386150fc52432`
- `src/storage/library_transaction/content.rs`: `f39a61ff9645ba7143b1e1516a40622e6f3ba8fc`
- `src/storage/library_transaction/destruction.rs`: `b9ceaf5e6cbdac1dc9e60df6923e3d4588b654b5`
- `src/storage/library_transaction/organization.rs`: `8bed2676d0ba1328eb363ad4d30661f5cb7261af`
- `src/storage/library_transaction/rows.rs`: `665c15be0d6be7effe4abe0b836ea56acf6945e7`
- `src/storage/library_transaction/tests.rs`: `f4f1cb9e7064fd2d5e5514aea585c4c8dd5921c0`
- This evidence: `37c150488a4aada20581bf05f6d1556de0c34f09`

The actual parent commands and red evidence were read before production repair:

```text
cargo test --locked --all-features --test storage storage_library_actor:: -- --nocapture
exit 101: 1 passed, 3 failed, 138 filtered out
cargo test --locked --all-features --lib deeply_nested_confirmed_folder_removal_ -- --nocapture
exit 101: 0 passed, 1 failed, 273 filtered out
```

The first log is
`/tmp/claude-chess-pgo-diagnosis-20261016/library-actor-semantic-red.log`,
SHA-256 `702927a2f8f3cc3baa60f2859cddb3f5f0c68db7e2e92011af957f8b1a3b06bc`.
Its failed contracts are retained prepared writes escaping the shared byte
budget, retained prepared adoptions escaping that budget and generic overwrite
of an indexed game. The one-worker cancelled-wait contract passed. This log also
records the existing macOS linker `__eh_frame section too large` warning; it is
separate from these semantic failures.

The second log is
`/tmp/claude-chess-pgo-diagnosis-20261016/library-deep-folders-red.log`,
SHA-256 `ee9091dda222a91a0181cdb9cad945ccfee40133cbf4c35526e0f1e1f50a1f24`.
Its valid 1,100-folder chain failed confirmed removal with SQLite
`too many levels of trigger recursion`. The parent-owned actor test source was
read at blob `74762a30e60a06d615f8fb335bf788bc9ef90b0d`; it is neither edited nor
exported here. Generic overwrite/catalog-phantom repairs, the actual confirmation
adapter and all actor/client integration remain parent-owned.

### Mandatory prepared ownership

`StorageLibraryPreparedMemory` owns a nonoptional shared byte permit. Public
write preparation reserves from the existing client before its first executor
await, using the owned body's capacity. Adoption retains the actor-read
reservation while acquiring its prepared reservation before executor admission.
Both private constructors require valid prepared memory and retain it in every
successful public prepared output. There is no unreserved constructor, optional
permit or uncharged successful public owner.

The conservative reservation is four times the encoded capacity/length plus
projection/request overhead. It remains charged with the prepared output even
when an adoption discards its source body. The final owned payload and request
size are checked against the retained permit before yielding success. Owned
write and identity String capacities, including normalized-key buffers, are
included. This is bounded admission, not a claim that peak codec/executor process
memory has been measured. Oversized reservations remain subject to the existing
64 MiB byte-budget refusal.

The exact new root boundary is:

```rust
pub(crate) fn reserve_library_memory(
    &self,
    bytes: usize,
) -> Result<OwnedSemaphorePermit, StorageDocumentError>;
```

It is a nonblocking wrapper around the existing shared byte budget. It exposes
no semaphore internals. Root must update begin_library to use
`request.unreserved_admission_bytes()` at the same integration point. Save/Adopt
return zero because their mandatory prepared permits already cover payload and
request storage. Moving the prepared value into the request transfers ownership;
no second payload reservation is acquired. The dispatcher borrows the complete
owned request and explicitly drops it only after `transaction.commit()` returns.
Error unwinding retains the request until transaction rollback/drop. The actor's
existing admission and any extra reservation still survive dispatch as before.

Private write preparation now takes `StorageLibraryPreparedWriteOptions` and
cancellation. Its typed options contain ID, expected revision, body, change and
mandatory prepared memory. Private adoption preparation takes ID, observed
revision, body, mandatory prepared memory and cancellation. Every direct private
transaction fixture explicitly reserves from its own shared test budget. Public
repository method signatures are unchanged.

### Deep folder removal

Confirmed removal still revalidates the exact catalog plan, unfiles every
contained Library/Trash game and touches only organization revisions/times.
It then selects at most 100 current subtree leaves, deletes those leaves with a
second child-absence guard, and repeats until the root is gone. No delete starts
a recursive descendant cascade. The exact deleted-folder count must match the
approved plan. There is no new maximum depth, SQLite limit change, checker
change, extra connection or entire-subtree Rust collection.

All work remains in the same Immediate transaction. A failed leaf/root delete,
count mismatch or catalog increment rolls back unfiling, organization changes
and already-deleted folders together. Document bodies, content revisions, tags,
Trash state and unrelated folders retain their existing semantics. Runtime and
scale evidence after this repair remain unverified.

### New source contracts and export

The parent deep-folder regression is preserved byte for byte. It is not copied
into the incremental export. Existing private fixtures are updated only to
supply mandatory permits. Four genuinely new source contracts cover:

- Prepared request transfer at an exhausted byte budget and release after dispatch.
- Dropping an unqueued adoption and cancelling preparation before executor admission.
- Transaction rollback after descendant deletion and unfiling but before root deletion.
- Actual one-worker actor enqueue at a full byte budget, retained cancelled wait,
  no second payload charge and re-reservation of all bytes after the commit receipt.

Those new contracts were not run here. Pinned standalone rustfmt/check covers
new production/fixture source; the unchanged parent case and two parent SQL
conversion expressions retain their inherited formatting. A byte comparison
against the frozen baseline verifies the parent case. Formatter checks for the
two affected production files also use root's already-formatted read-only base
with just this incremental patch applied in scratch. Git whitespace, scoped incremental
paths, baseline forward applicability and local reverse applicability are checked.
No Cargo, build, tests, Clippy, CPU diagnostics, agents, extra worktrees, commits
or pushes ran for this repair.

The incremental production/evidence/fixture repair is exported as
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-backend-review-fix.patch`.
It is relative to the frozen delivered-plus-parent tree, not to repository HEAD.
It excludes the original full backend, already-applied parent test hunks, codec
dependency and all root-owned actor/client/confirmation/generic-save files.
Root must apply it after the parent increment and run the observed red contracts,
new contracts and required integration gates. No repaired behavior is claimed
green and no task acceptance or T104 runtime prerequisite completion is claimed.

## Parent integration and verified checkpoint

The parent integrated migration 3, the source modules and tests into the existing
serialized actor. Signed SQLite count/limit conversions were corrected during
compilation. The actor now retains library request ownership through commit,
uses the shared preparation reservation without charging it twice, and checks
the preparation's budget origin before admission.

The original retained-adoption probe used notes that adoption intentionally
discards. It did not independently establish retained payload pressure. The
corrected fixture instead retains one MiB of White metadata. Against the
unrepaired implementation it failed with 0 passed, 1 failed and 158 filtered
in 22.57s. `library-retained-player-adoption-red.log` is the authoritative red
for that contract; the earlier note-based claim is superseded here.

Two additional real two-store tests demonstrated that prepared writes and
adoptions paid for by store A could enter an exhausted store B and mutate it.
The correct baseline command ran two tests: both failed. The earlier mistyped
filter ran zero tests with 280 filtered and establishes no behavioral result.
The repair compares the retained permit's semaphore identity with the receiving
client's shared budget at `begin_library`. Foreign preparations return typed
`ForeignPreparation` before admission. Both repaired regressions pass without
destination mutations. No alternate budget or copied semaphore policy was added.

Generic saves now reject already indexed content after revision validation.
Generic candidate writes also advance the canonical catalog inside the same
transaction, invalidating stale query/import snapshots; custom migration stores
without library tables retain their prior behavior. Both observed regressions
pass. Mandatory prepared ownership and bounded leaf deletion repair the memory
and 1,100-folder recursion failures without a depth cap or SQLite limit change.

Focused verification passed five actor cases, 22 private transaction cases
before the two foreign-origin additions, both foreign-origin cases, 16
heterogeneous filter cases, both generic-write cases, three lifecycle-evidence
adapter cases, ten document/inventory compatibility cases and 21 puzzle storage
compatibility cases. The final canonical suite includes all 24 private
transaction cases. The shared public normalization boundary passed 26 identity
cases and preserves generic `?` text while treating unavailable player names
through the canonical player rule.

`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify`
exited 0 on this combined library/annotation snapshot on 2026-10-17. All three
Clippy selections passed. All 1,507 tests passed with zero failures, ignored or
filtered tests. Both optimized builds passed: all targets in 5m 01s, and the
named native harness in 49.90s. No Cargo parallelism cap was applied. The known
macOS debug linker `__eh_frame section too large` warning remains unsuppressed;
Clippy itself was clean.

All original and corrected baseline logs, repaired focused results, filter and
compatibility results, and the combined canonical log are preserved with a
SHA-256 manifest under
`.git/parallel-patches/library-backend-integration-20261017/`.

The typed Trash confirmation adapter requires exact observed closed content
revision from an injected lifecycle observer. It does not orchestrate the
actual native pause/save/close barrier. Production played saves, bounded legacy
adoption/readiness, typed played recovery and mounted library/editor controls
remain open. T104's native prerequisites and T106/T107/T109 are not accepted by
this backend checkpoint. No whole-process memory or large-library throughput
claim follows from bounded preparation admission tests.
