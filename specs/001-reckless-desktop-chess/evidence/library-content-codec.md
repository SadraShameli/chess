# Library content codec foundation

The original source-only handoff was against
`edf27114f2ace3a8b9ef393aee0e39af0fe7691a`. It prepares the content boundary for
T100, T101, T102 and the explicit study-line state needed by later T105 work.
It accepts none of those tasks, T097, or native acceptance as complete.

`StorageGameDocument` remains the canonical versioned codec. Its private
`records.rs` owns shared text, metadata, date, annotation, move and nanosecond
duration records. `notation.rs` owns only the notation payload and its source
extras. There is no second serializer registry, notation-to-session adapter,
actor implementation, migration or organization model in this patch.

## Supported public API

The new module is `storage::library_record`:

```rust
pub enum StorageLibraryEnumContent {
    Played(Box<GameSession>),
    Notation(Box<NotationGame>),
}

impl StorageLibraryRecord {
    pub fn new(id: GameId, content: StorageLibraryEnumContent)
        -> Result<Self, StorageLibraryRecordError>;
    pub const fn id(&self) -> GameId;
    pub fn content(&self) -> &StorageLibraryEnumContent;
    pub fn into_content(self) -> StorageLibraryEnumContent;
    pub fn metadata(&self) -> &GameMetadata;
    pub fn start(&self) -> &BoardStart;
    pub fn result(&self) -> NotationGameIdentityEnumResult;
    pub fn mode(&self) -> Option<GameEnumMode>;
    pub fn identity(&self)
        -> Result<NotationGameIdentity, StorageLibraryRecordError>;
    pub fn identity_line_len(&self) -> Result<usize, StorageLibraryRecordError>;
}
```

`new` rejects an external ID differing from a played session's ID. Notation
retains its existing aggregate contract, including an interior selected tip.
The heterogeneous payloads are boxed because
played content additionally owns runtime history, clock, takeback and play
controls. Compiled layout measurements remain parent-owned and unperformed.

Played identity uses the actual `live_moves()`. Notation uses the path to its
explicit `mainline_tip`. `identity_line_len` counts the corresponding tree path
without allocating a move vector. Counting and `path_to` share one bounded
ancestor traversal on the tree. Display metadata is borrowed unchanged;
normalization stays on `NotationGameIdentity`. Notation mode is `None`. Result
is the semantic result, independently of an absent optional PGN Result header.
There are no content-level organization revisions or timestamps. Legacy
modification time remains Unknown in the parent projection. Favorite and other
organization edits require a separate revision from content autosaves.

The existing played `capture`, `json`, `restore` and
`StorageGameDocumentRepository` signatures are retained. Codec additions are:

```rust
StorageGameDocument::capture_notation(GameId, &NotationGame)
    -> Result<StorageGameDocument, StorageGameDocumentError>
StorageGameDocument::capture_notation_cancellable(
    GameId, &NotationGame, &CapacityExecutorCancellation,
) -> Result<StorageGameDocument, StorageGameDocumentError>
StorageGameDocument::recover_record(&str)
    -> Result<StorageLibraryRecord, StorageGameDocumentError>
StorageGameDocument::recover_record_cancellable(
    &str, &CapacityExecutorCancellation,
) -> Result<StorageLibraryRecord, StorageGameDocumentError>
StorageGameDocument::json_cancellable(&self, &CapacityExecutorCancellation)
    -> Result<String, StorageGameDocumentError>
```

Both recovery APIs use the same envelope and payload decoder. Played `restore`
returns the dedicated `StorageGameDocumentError::ContentKind` for valid notation.
`StorageGameDocumentEnumContentKind::{Played, Notation}` supplies typed error
context. Import serialization/recovery can run inside the existing executor's
owned and joined jobs with cancellation forwarded through these entry points.

## Versions and study state

Schema 3 writes exactly the envelope fields `schema_version`, `kind`, and `game`.
`kind` is `played` or `notation`. Borrowed `RawValue` payload dispatch checks
version and typed kind before payload reconstruction. Unknown envelope/payload
fields, duplicate fields, unsupported older versions and newer versions are
refused. A future kind is not parsed before a future-version refusal.

Schemas 1 and 2 retain their actual `schema_version` plus `game` envelope, with
`kind` absent. Schema 1 retains its existing play-option default migration;
schema 2 still requires play options. Presence of `kind`, including null, is
refused in a legacy envelope. A schema-3 played payload requires an explicit
study tip. Legacy payloads cannot supply that field.

Legacy recovery derives the study leaf from the saved live node, repeatedly
following the existing first child. This is an export/study projection. It
does not extend live history, change identity, metadata, clocks or outcomes,
advance a revision, or write the source document. Tests include a live `e4`
node with retained `e5`: live identity remains the one-ply `e4` line and the
study tip becomes `e5`. The original sibling order remains intact.

`GameSessionRestoreOptions::study_mainline_tip: GameMoveId` and
`GameSession::study_mainline_tip()` are explicit and independent of live/view.
New sessions use root. Accepted played moves select the first ordered leaf
reachable from the accepted live node. Takeback retains the study line;
subsequent alternate play selects its resulting branch. Appending at the
current study tip extends it; appending elsewhere does not promote that branch.
Tree traversal and prepared insertion own the mechanics, while the session
owns revision/state policy. The tip is computed before insertion/history
commit; no new fallible state transition follows a commit. Promotion/deletion
services and confirmation flows remain future work.

## Preserved content and limits

Notation recovery constructs `NotationGame` with the external library ID kept
on the record. It preserves start/variant, stable move IDs, insertion and child
order, parents/canonical moves, root/node notes, NAGs/markings, ordered unknown
tags, partial dates, semantic result, optional Result-header absence, raw
TimeControl, exact duration nanoseconds, warning node IDs and unavailable
ratings. It creates no seats, runtime clock snapshots, outcome reasons or
`GameSession`. Non-null runtime snapshots in notation are refused. Absent
node runtime snapshots are omitted on new writes; old null fields remain
decodable.

The existing bounded move visitor and 100,000 non-root move limit remain shared.
Text validation remains one MiB. Body decoding and the bounded writer use
`StorageDocumentStore::MEMORY_BUDGET`, still 64 MiB. No dependency, schema SQL,
transport limit, executor budget or cancellation ownership changed. This is
source inspection of existing bounds, not measured peak-memory evidence.

## Original handoff contracts and verification limits

Initial contract tests were authored before production edits, with additional
boundary cases added during static review. The original handoff contained 23
new tests, all unrun in that lane:
15 in `tests/storage/game_document.rs`, three in
`tests/storage/library_record.rs`, and five in `tests/game/restoration.rs`.
The three files contain 40 tests including 17 pre-existing cases. Missing new
APIs were not counted as semantic regression failures. No discovered existing
bug repair or observed semantic red is claimed.

The contracts cover legacy continuity and read-only body/revision preservation,
version/kind refusal, played study/live/view independence, retained takebacks,
notation full-tree/source-extra round trips, real PGN import, unavailable modes,
declared starts, runtime-snapshot refusal, duplicate IDs/continuations, forward
parents/order corruption, text/node/body limits, explicit-tip validation, played
identity and deterministic executor cancellation with an observed job join.

Executed checks are limited to the pinned standalone rustfmt, Git whitespace and
patch checks, authored-source restriction scans, static fixture JSON syntax, and
owned-path/test-count inspection. No Cargo, compilation, builds, tests, Clippy,
compiler diagnostics, commits, pushes, extra agents or worktrees ran in this lane.
Behavior, compiled layout, peak memory and platform acceptance are unverified.

## Required parent integration

Register `pub mod library_record;` in `src/storage/mod.rs`. Include
`tests/storage/library_record.rs` as `storage_library_record` in `tests/storage.rs`.
Its fixture imports the already registered `storage_game_document` module.
The private codec submodules are declared by `game_document.rs`. The existing
restoration test root already includes the owned game tests.

There are no restore-options struct literals outside the owned session and
codec files. The parent-owned
`tests/game/config_persistence.rs::legacy_game_schema_migrates_absent_play_options_to_approved_defaults`
currently derives a legacy fixture by only changing the current version and
removing play options. Its fixture adapter must also remove current `kind` and
`study_mainline_tip`, or use the existing actual legacy shape, before asserting
legacy recovery. This patch does not edit that file or rewrite accepted fixtures.

Root retains migration registration, SQL, actor writes, library repository/query
integration, organization revisions, native roots, compilation/Clippy/semantic
checks and task acceptance. The played repository still calls generic
`client.save`; making content/index writes atomic is not implemented here.

## Parent integration and canonical verification

The parent applied the ten-file patch with SHA-256
`af1c6eb43e903056264ee0c73084be0e9a37fff5fb034bfefedd0c1a5067639b`
to `39994327f2742cb51c80a02d54d9d72943be3269`, registered the public record
module and mirrored tests, and updated the existing legacy fixture adapter to
remove schema-3 fields before testing schema 1. The first compile found only a
fixture boundary error: Tokio's oneshot sender is not `UnwindSafe`. The fixture
now uses Tokio's explicitly unwind-safe `Notify` for readiness and retains its
existing blocking worker-release channel. No suppression or production compile
repair was needed.

Independent source review found that the delivered codec and record wrapper
imposed the played study-leaf restriction on notation. Existing `NotationGame`
and Mainline export permit a selected interior tip with retained continuations.
Three parent regressions independently exercised wrapping, capture and recovery.
After correcting a fixture formatting compile error, all three failed against
the unchanged delivered production with `Tree(InteriorStudyTip)`: zero passed,
three failed, 133 filtered. That is the observed semantic red; the earlier
compile errors are not counted as semantic failures.

The repair removes leaf-only checks from those three notation paths. The
validated aggregate still owns tip existence and tree relationships. Played
session study tips retain their leaf invariant. The corruption fixture now
uses an unknown node rather than treating a valid interior notation tip as
corrupt. Full PGN export retains its explicit `MainlineHasContinuations` refusal.

The repaired document suite passed 24 tests, zero failed/ignored, 112 filtered.
The canonical parent command was
`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify`.
It exited zero with 1,433 tests, zero failed, ignored or filtered, all three
Clippy selections and both optimized builds passing. The release selections
took 5 minutes 18 seconds and 50.03 seconds. The canonical run includes all
16 restoration cases, three library-record cases and six configuration
persistence cases. The three mirrored codec/record/restoration files contain
43 cases, including 26 new cases relative to the previous checkpoint.

Every Cargo command used the shared target directory and default internal
parallelism. Gates were sequential and the Rust snapshot remained frozen
during execution. The existing macOS debug linker `__eh_frame section too large`
warning remained visible and unsuppressed; clean Clippy does not imply a
warning-free linker. The canonical command compiled and linted the named native
harness without executing GPU or OS acceptance.

Exact commands, five raw logs and their SHA-256 manifest are retained in
`.git/parallel-patches/library-codec-integration-20261017/`. The canonical log
SHA-256 is `142d456d0e10b43f5496fa098b56bbb7a5bba7afbfdba78ef671beb31bde365b`;
the actual three-red log is
`0855495bb6d6ee7c0c5f3bee459b046a1b132c698d4ca0d455f20bb56bb17d06`.

This accepts the codec and persisted study-line foundation. It does not accept
T100 through T105, complete physical staging under T097, native GPU/platform
acceptance, measured peak memory or cancellation latency. Library schema,
projection, organization, atomic content/index writes and mounted handlers
remain separate implementation work.
