# Played editor forwarding source handoff

This is an unrun implementation handoff for the owned played-source portion of
US3 T106/T110. It does not establish acceptance. The parent owns the exclusive
shared Cargo lane, production composition, runtime verification, and task status.

The checkout began clean at `465837a95c0de9129460171ea57a5edb2d054aac`.
The two ordered dependency patches were SHA-256 verified before application:

| Dependency | SHA-256 | Resulting index tree |
| --- | --- | --- |
| `library-import-metadata-current.patch` | `ea4e51690e2484f1058842196bc3cf27487801d141a90433c4c7ac005bf82156` | `e3e55b6eaf62c1730ebda2dc85c4bd806610ba6b26` |
| `notation-annotation-owner.patch` | `5d4cccabfee60e1e32a4e10ac6557f5b913d128df025f3564525cbe874939821` | `7be9f35fd05daeffb608b3c17cbae6e7c8b70127` |

The second tree is the frozen incremental export baseline. The patch and its
sidecar manifest live under the shared Git directory's `parallel-patches/` as
`played-editor-forwarding.patch` and `played-editor-forwarding.manifest.json`.
The manifest records the final patch SHA-256, applied index tree, owned paths,
and static checks. Dependency-only hunks are excluded.

Read guidance included the global Claude file, root Claude bridge, constitution,
all seven canonical rule files, scoped-rule frontmatter, the desktop IPC,
study-tools and UI contracts, and the actual editor, annotation, supervisor,
session, save and indexed storage implementations. There were no applicable
nested instruction files in the owned source or test paths.

The original thirteen runtime contract tests were authored before production
APIs. Eleven further cases and focused assertions were authored during retained
save/publication integration. All twenty-four tests remain unrun. No existing
domain defect repair is included. The parent supplied the canonical append
effect contract while preserving its existing session revision behavior.

## Owned implementation

`DesktopGameSupervisorPort` admits game and editor envelopes through its existing
32-slot ordinary channel. Existing reserved controls keep their separate finite
admission. At most one ordinary envelope is retained by the scheduler while an
earlier save blocks ordinary game dispatch. Editor outcomes have a bounded
32-entry correlation cache; pending entries cannot be evicted. Queue refusal is
typed, and a failed retry admission preserves the pending outcome.

The implementation uses the existing `DesktopGameSupervisorContent` mutex
take/replace payload and executor. No second session, session clone, selected
global port lookup, runtime, database connection owner, serializer, or transaction
implementation is introduced. Registry forwarding selects an already-owned actor
by the supplied actual game ID under nonblocking `try_read` admission.

Open initializes the actor's editor epoch exactly once from its actual admitted
application receipt. Later Open requests preserve that epoch. Snapshot and edit
contexts bind the actual game ID, played revision, current storage revision,
checked generation and selected view. Ordinary view changes also invalidate
previous editor generations. Bound actions do not rebase their expected context.

Session methods own notes/NAGs, markings, branch insertion, variation preview and
confirmation, phase policy, played-history protection, live position and outcome.
The non-Clone `GameAnnotationsSessionPreview` remains on the source. Context and
token checks and UTC/revision/generation preflight precede consuming its proof.
Domain revision effects identify annotation and promotion changes, including
sibling reordering. Append consumes the parent's canonical `.change()` effect.
It does not infer a new tree policy from node count or study tip.

Only actual Changed effects record a new accepted content watermark and UTC.
Navigation, unchanged annotations, reused branches and checkpoints do not record
a new modified timestamp. Each editor Save captures the canonical source with
`GameStorageSaveRequest::capture_with_change` and uses the existing indexed
`StorageLibraryPlayedRepository`. Retry Save enters the existing recovery path;
it retains the original accepted timestamp and newest owned source. Older
acknowledgments cannot clear a newer accepted watermark.

The scheduler continues polling its owned save future while CPU computation is
pending. A completed save is retained until the session payload is available for
acknowledgment. The reply reports the truthful opaque `Saving { computing }`
state; it does not claim visibility into private preparation/commit internals.
Urgent controls precede editor work at scheduler boundaries, and deadline and
storage branches precede queued editor dispatch and projection. No responsiveness
or timing measurement was run.

After an actual acknowledgment, the reply retains `PendingProjection` and the
acknowledgment's exact covered/storage revisions. A fresh bound publication is
created on the executor before a successful snapshot result is delivered. A
cancelled projection wait retains its computation. Obsolete expected storage
contexts fail against the authoritative binding.

Snapshot production uses the parent's shared receiving-budget publication
factory on the executor. Publication pressure after mutation retains the source,
original UTC, original application receipt and accepted result kind. It exposes
`Publishing { context }` and a typed `EditorPublicationPending` error. Resubmitting
the same admitted receipt schedules publication only. It does not replay the
edit or obtain another UTC. Foreign actions cannot consume that pending source.
Pending publication also blocks falsely successful shutdown acknowledgment.

## Exact integration surface

The existing supervisor options and ordinary game command/receipt contracts do
not gain required fields. The dedicated supported surface is:

```rust
DesktopGameSupervisorPort::try_open_editor(
    &self,
    receipt: DesktopApplicationReceipt,
) -> Result<DesktopGameSupervisorEditorReceipt, DesktopGameSupervisorError>

DesktopGameSupervisorPort::try_submit_editor(
    &self,
    command: DesktopAnnotationCommands,
) -> Result<DesktopGameSupervisorEditorReceipt, DesktopGameSupervisorError>

DesktopGameRegistryPort::try_open_editor(
    &self,
    game: GameId,
    receipt: DesktopApplicationReceipt,
) -> Result<DesktopGameSupervisorEditorReceipt, DesktopGameRegistryError>

DesktopGameRegistryPort::try_submit_editor(
    &self,
    command: DesktopAnnotationCommands,
) -> Result<DesktopGameSupervisorEditorReceipt, DesktopGameRegistryError>
```

The cloneable editor receipt exposes `request_id()`, `application_receipt()`,
`pending()`, `try_result()`, `wait(&mut self)`, and
`storage_receipt() -> Option<GameStorageReceipt>`. The last method contains only
that operation's actual repository acknowledgment in a shared OnceLock. It is
independent of a later source/publication revision. Open and read-only requests
have no fabricated storage acknowledgment. No public receipt constructor or
default is added. Repeated waits and re-admission share the same outcome.

`wait` retains pending projection until observed. Publication failure returns its
typed pending error so the caller can release receiving memory and explicitly
resubmit the original application receipt. It must not mint a new action or
substitute a newer application's receipt. Successful saves may return the latest
authoritative source publication while `storage_receipt()` still identifies the
original edit's actual covered revision.

The parent must register its shared `annotation_commands`, `annotation_owner`
and editor prerequisites in the desktop root. Only the owned private editor
submodule and its mirrored test inclusion are registered here. The existing
snapshot-only `DesktopGameBridgeFuture` is unchanged. The admitted library
service must route played versus notation requests, preserve application commit
and cancellation ownership, reconcile typed pending results, and map owned
errors to its outer messages. Ordinary game commands retain their existing
public API.

## Parent prerequisites excluded from export

The parent must first provide
`GameSession::append_study_variation -> Result<GameAnnotationsVariationApplied,
GameSessionError>`, with canonical insertion-or-study-tip change detection and
its real `.change()`, `.view()` and `.mainline_tip()` accessors. The parent's
session/tree/notation/caller edits are not mirrored or exported here.

The publication prerequisite is `annotation-snapshot-memory-repair.patch`,
SHA-256
`74485be0c37a1b2c3b9126a612ce94234c5257f78e1ae07334a4e7ce3a0cdec1`.
The local working tree mirrors only its `game_editor.rs`,
`game_editor/publication.rs` and `annotation_commands.rs` producer/protocol
changes, plus the parent's `Pending::Saving { computing: bool }` addition.
These mirrors are unstaged and excluded from the owned export. The dependency's
old failure-injection fixture is untouched. The eight-path repair did not apply
wholesale to that older fixture, so no unrelated owner/test hunks were copied.

The required factory is
`DesktopGameEditor::publish(expected, &StorageDocumentClient) ->
Result<DesktopGameEditorPublication, DesktopGameEditorPublicationError>`.
Its publication/error types must remain re-exported by `desktop::game_editor`.
Snapshot/Edited protocol variants contain the permit-owning publication directly.
The source's actual receiving storage client is carried in the focused editor
computation context. No storage internals need wider visibility.

## Authored verification coverage

| Contract | Authored cases and assertions |
| --- | --- |
| Identity | Actual first receipt epoch, same-actor focus, new actor epoch, equal-FEN foreign game/epoch/storage/view rejection |
| Admission | Shared ordinary/editor capacity and reserved urgent Stop; registry existing actor forwarding and absent game refusal |
| Annotation persistence | Independent notes/NAG and markings saves, indexed durable readback, actual correlated storage acknowledgment |
| Variation policy | Real path and subtree preview, scope paging, foreign token/context preservation, failed UTC retains proof, promotion retains played history, protected deletion refusal |
| Content effects | Reused branch Unchanged despite session revision advance, unchanged notes, navigation/checkpoint modified time, Checked overflow before mutation |
| Recovery | Failed UTC unchanged source/draft, failed save original UTC, older completion versus newer watermark, failed older save plus drained newer write followed by newest-source Retry |
| Cancellation | Abandoned result wait, retained computation, preparation and SQL commit waits, post-save projection, cancellation before admission, application cancellation after accepted edit |
| Receiving memory | Accepted edit retained under publication pressure, foreign/stale requests preserve pending source, original-receipt publication retry without edit replay or another UTC |
| CPU/read delivery | Executor-produced snapshots, bounded tree/children/confirmation pages, stale cursor refusal, due live clock before queued editor snapshot |

The mirrored test module is included privately from the owned editor source.
Fixtures use actual supervisors, real temporary SQLite storage, domain sessions,
actual application-admitted receipts, an injected monotonic clock/UTC provider,
capacity leases, and controlled SQL locking. No test-only public API is exposed.

Executed checks are source parsing/formatting with the installed Rust 1.99.0
toolchain's `rustfmt 1.10.0`, `git diff --check`, owned-path and patch-manifest
inspection, and reverse applicability against the staged result. Rustfmt is not
compilation evidence. No Cargo command, build, test, Clippy, application runtime,
CPU diagnostics, agents, extra worktrees, commit, or push ran in this lane.

The parent must compile and execute the twenty-four cases, run relevant existing
supervisor/registry/save suites, inspect every finding, and complete its canonical
verification lane. This handoff makes no passing-count or performance claim for
the authored source.

## Remaining integrations

This scope does not implement running-registry DB OpenSaved for absent completed
games, a single-game correlated close/Trash certificate, outer
messages/coordinator/UI integration, or GPU acceptance. The parent retains all
of those integrations and full T106/T110 and all-thirteen-journey acceptance.
No task marker is changed by this handoff.
