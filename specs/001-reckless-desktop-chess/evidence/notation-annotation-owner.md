# Annotation snapshot receiving-memory repair

This is a frozen, isolated source handoff. No Cargo command, compilation, test,
Clippy run, build, CPU diagnostic, agent, extra worktree, commit, or push was
performed in this slice. Static checks do not establish runtime correctness.
The constitution, all seven canonical rules, and applicable guidance were read.

## Baseline and ownership

HEAD is `465837a95c0de9129460171ea57a5edb2d054aac`.
The applied dependency is `library-annotation-review-current.patch`, SHA256
`4cad29d61a745483cf92cff20a08b7e1365c5f378e16201de555c779243c5d27`.
Its verified index tree is `d6aa39759f9be323023c0f4c4e6e404542c2c72d`,
containing 41 changed dependency paths. Unchanged dependency paths are excluded
from the incremental export. The handoff supplies the resulting tree and patch
SHA256 separately to avoid embedding a self-referential tree or patch digest.

The incremental manifest is exactly:

- `src/desktop/annotation_commands.rs`
- `src/desktop/annotation_owner.rs`
- `src/desktop/annotation_owner/source.rs`
- `src/desktop/annotation_owner/saving.rs`
- `src/desktop/game_editor.rs`
- `src/desktop/game_editor/publication.rs`
- `tests/desktop/annotation_owner.rs`
- `specs/001-reckless-desktop-chess/evidence/notation-annotation-owner.md`

The requested export is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/annotation-snapshot-memory-repair.patch`.
The command tests and pure snapshot implementation remain unchanged. Shared
roots, domain/session code, study callers, played supervisor and registry,
storage/import/metadata sources, public schema, and feature task markers were
not authored by this repair.

## Actual semantic red

The parent ran the unchanged regression
`published_snapshot_clone_retains_receiving_bytes_until_its_final_drop` against
the frozen dependency. Its command exited 101 with 0 passed, 1 failed, and 500
filtered. Initial open, 256 KiB accepted notes, save settlement, source/result
release, final clone release, and fixture cleanup all passed. The final assertion
failed because the full receiving budget could be reserved while only the
published snapshot clone remained alive.

The preserved log is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/annotation-snapshot-memory-baseline-20261017/red.log`,
SHA256 `9e0a57327e82fd5b278dfd1ba216f82eb1500d7378e3d3f7664334f164c0c700`.
Its hash was verified here. The regression body is unchanged. The parent also
reported prior green runs for the owner and dependencies; those are prior
verification evidence, not green evidence for this repair. The separate
checkpointed-space red is outside this slice and remains untouched.

## Producer and consumer API

The exact shared producer method is:

```rust
DesktopGameEditor::publish(
    &self,
    expected: DesktopGameEditorContext,
    client: &StorageDocumentClient,
) -> Result<DesktopGameEditorPublication, DesktopGameEditorPublicationError>
```

The caller supplies its actual borrowed played or notation editor and receiving
client. Call the synchronous factory on the existing bounded CPU executor.
`DesktopGameEditorPublicationError` has `Editor(DesktopGameEditorError)`,
`Memory(StorageDocumentError)`, and `FootprintOverflow` variants.

`DesktopAnnotationCommandsEnumResult::Snapshot(publication)`,
`Edited { change, snapshot: publication }`, and
`DesktopAnnotationCommandsEnumAdmission::PendingSnapshot { snapshot, pending }`
now contain `DesktopGameEditorPublication` directly. They no longer box the
pure snapshot. `PendingProjection` retains its existing boxed result wrapper.

The publication exposes `context`, `root`, `mainline_tip`, `board`, `annotations`,
`metadata`, and `state` getters. Its Clone only clones one private Arc. That Arc
owns both the pure snapshot and the receiving byte permit. There is no public
constructor from an unreserved snapshot, optional guard, public backing field,
raw snapshot getter, or Deref exposing the cloneable pure snapshot. Explicit
pure `DesktopGameEditor::snapshot` projections remain available at their existing
low-level boundary.

## Allocation and ownership behavior

Publication first validates context and measures borrowed input, then obtains the
receiving client's permit, then captures annotations, metadata, notation time
control, and board projection. It reserves inline backing and Arc counters;
notes; NAG and marking storage; every known metadata string, opening code,
extension struct/name/value; and notation time control. Board allowance covers
FEN buffers, square-bounded piece storage, and move buffers bounded by the actual
pinned shakmaty MoveList capacity, including temporary/growth allowance. Dynamic
sums use checked addition. It introduces no memory pool, current-client lookup,
new runtime, or executor.

The canonical source remains one Arc<StorageLibraryRecord>. Projection backing
owns no additional record or domain session. Source cache, observer cache,
Snapshot/Edited results, pending admissions, and cloned wrappers share the same
backing. Coordinator snapshot admission and refresh_observation only clone the
constant-sized publication. Identical contexts reuse the retained projection,
including unchanged domain edits when no receiving bytes remain.

## Refusal, cancellation, and durable projection

After a domain edit or navigation changes canonical context, the source retains
its typed Snapshot/Edited completion before attempting publication. Receiving
pressure retains the accepted domain state, revision, original accepted UTC,
completion kind, and actual application receipt. `status().has_compute()` remains
true. Reads identify `DesktopAnnotationCommandsEnumPending::Publishing { context }`;
usable snapshot access and submissions return
`DesktopAnnotationOwnerError::PublicationPending { context }` until publication
is observed. An older cached snapshot is available only through the explicitly
pending read contract.

The service retries with `observe_compute(&original_receipt)`. Retry only projects
the retained canonical source. It does not replay mutation, reread wall time,
consume a variation proof again, or discard an accepted completion because the
application receipt was canceled. A dropped observe future retains its executor
submission/join. Foreign receipts are rejected before touching genuine work.
Refusal returns a typed error, never Ready with the old context. There is no
self-waking loop; the service must schedule another observation only on an
explicit retry or relevant external capacity change.

A post-commit projection keeps the actual checked SQL receipt in shared ownership.
Byte refusal returns its phase to Completed, which still reports Projecting and
blocks obsolete contexts. The source projects the prospective storage binding
before assigning it. The original save receipt retries projection without
reissuing SQL. Once publication is observed, durable revision advances only to
the receipt's covered revision; a newer accepted edit and original UTC stay dirty.
Existing exact game/revision checks, proof checks, no-op outcomes, and genuine
receipt correlation remain in place.

## Authored coverage and parent verification

There are 38 authored tests in the retained owner/command suites: the original
25 owner tests, the unchanged parent-run memory red, 11 additional owner cases,
and the unchanged command test. The first seven additional cases were written
before implementation. The other four extend the same publication contract.
None of the additional cases or repaired cases were run here.

Additional coverage exercises final result/cache/admission clones, pointer-shared
board/metadata/annotations under full pressure, retained pending publication,
foreign receipts, cancellation before retried executor admission, release after
owner/pending/result drops, initial refusal and retry, unchanged annotations under
pressure, post-commit refusal and retry, older durable receipts versus newer UTC,
cloned PendingProjection results, receiving-client isolation, all large metadata
and annotation payloads, and both actual borrowed played and notation publishers.
Tests use real owners, records, sessions, application receipts, bounded executors,
and temporary SQLite. The existing saturation fixture now reserves the remaining
budget through lawful reserve attempts, preserving its acceptance/UTC/retry
assertions without exposing test-only production getters.

Permitted checks are standalone rustfmt with skip_children, whitespace and
no-comment/suppression review, original-test preservation, exact manifest and
frozen-dependency checks, and binary patch apply/reverse checks. Parent Cargo,
Clippy, focused owner/command checks, combined integration checks, and the final
canonical gate are required before acceptance. No measured speedup or runtime
allocation result is claimed. The inherited recovered-record permit conversion
and the later mounted canonical export gateway remain separate work.
