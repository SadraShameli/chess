# Native annotation panels source handoff

This is a source-only T095/T110 handoff. It does not establish completed US3,
native input acceptance, durable application routing, or platform acceptance.
No task markers were changed. Parent composition and verification remain required.

The frozen starting commit is `465837a95c0de9129460171ea57a5edb2d054aac`.
The applied `library-service-actor-dependency.patch` SHA256 is
`3a8fd30ab4d987148dad1678c08afaf4b40cc0e368ded496cba408b448a96d67`.
Its staged tree was verified as `3af7406383b36908adae237eba518ef801d92c13` before
any owned files were authored. The export is incremental against that tree.
The export manifest records the resulting tree, patch checksum and owned paths.

## Ownership and registrations

The patch adds only these source components, their three mirrored test files,
and this evidence file:

- `src/presentation/analysis/editor_intent.rs`
- `src/presentation/analysis/annotations.rs`
- `src/presentation/analysis/variation_tree.rs`
- `tests/presentation/analysis/editor_intent.rs`
- `tests/presentation/analysis/annotations.rs`
- `tests/presentation/analysis/variation_tree.rs`

The parent must register `editor_intent`, `annotations`, and `variation_tree` in
the existing analysis module. Tests are privately source-included from the new
components; the editor intent test module exposes its fixture only within the
crate's test build. No integration test root edits are required for these tests.
No existing source files, PresentationApp, host, coordinator, messages, scene
bindings, task lists, or canonical owner modules are changed by this increment.

## Public mounting contract

`PresentationAnalysisEditorIntent` contains a stable `DesktopRequestId`, the
complete `DesktopGameEditorContext`, and `DesktopAnnotationCommandsEnumAction`.
Its public methods are `new`, `request_id`, `expected`, `action`, `into_action`,
and `admit`. `admit(actual_receipt)` checks the receipt request identity and
constructs `DesktopAnnotationCommands`; the UI never creates a receipt or time.
The parent must preserve the intent identity when admitting the actual request.

Both panels provide `new`, `synchronize(&publication, pending)`, `context`,
`show(ui, request_id)`, `attach_receipt(&intent, actual_receipt)`,
`observe_result(&receipt, original_expected, &result)`, and
`observe_failure(&receipt, original_expected, &DesktopFailure)`.
`failure()` exposes the retained typed application failure. Each actual input
needs its own parent-supplied request identity. Repeated delivery of an emitted
identity produces no second intent. The bounded identity ledger refuses further
input after 256 distinct submitted actions for that panel instance.

For a panel's own reply, call its correlated observation method before forwarding
the returned publication through general `synchronize` calls. Independent source
publications always go through `synchronize`. Pending statuses must accompany
their publications. A pending wrapper is never an edited or saved success.

### Notes, markings and guarded switches

`PresentationAnalysisAnnotations` also exposes:

- `notes`, `nags`, `set_notes(expected, text)`, `set_nags(expected, nags)`,
  `has_notes_changes`, `has_guard`, `is_conflicted`, `pending_intent`,
  `save_notes(request_id)`, and `save_markings(request_id)`.
- `request_switch(request_id, destination)` and
  `resolve_guard(choice, save_request_id)`.
- `prepare_checkpoint(request_id)`,
  `attach_checkpoint(original_edit_id, original_expected, actual_receipt)`,
  `observe_save(&checkpoint_receipt, &actual_save_progress, &publication)`,
  `observe_checkpoint_failure(&checkpoint_receipt, &failure)`,
  `retry_save(request_id)`, and `discard_draft()`.
- `marking_input(expected, input)` and `markings()` for the parent board scene.
  The returned marking view borrows `saved()` and `temporary()` slices and
  carries its complete `context()`. Inputs are typed `Toggle`, `ClearTemporary`,
  and `ClearSaved`. Rendering and board input must respect that context.

Destinations are `Navigate(GameMoveId)`, `AppendStudyBranch { node, movement }`,
and `Leave(DesktopRequestId)`. The last is an opaque destination token owned by
the parent. The board supplies the actual typed `BoardMove`; this component does
not parse or validate study moves. Returned effects are `Command(intent)` or
`Leave { request_id, expected, destination }`.

Route tree navigation and selected-position-changing board study input through
`request_switch`. A changed notes draft retains the original destination and
request identity. Stay retains the draft and selection. Discard restores the
latest authoritative notes before emitting that original destination. Save
retains the destination through the following complete sequence:

1. Admit the returned notes intent and attach its actual receipt.
2. Deliver its correlated canonical `Edited` result to `observe_result`.
3. Call `prepare_checkpoint` with a fresh stable request identity. Admit the
   returned `Checkpoint` intent, then attach that actual receipt using the
   original edit identity and original expected context.
4. Deliver real owner `DesktopAnnotationOwnerEnumSaveProgress` values to
   `observe_save`. `Committing` releases nothing. `Saved` must agree with the
   retained receipt outcome, actual storage acknowledgment, covered source
   revision, game, selected node, epoch, generation and returned publication.
5. Route a returned continuation using its source-returned expected context.

Changed drafts have an independent edit version. A later save acknowledgment
cannot consume edits made after submission, including edits reverted to the
submitted text. It preserves them and requires reconciliation. An independent
source change preserves the bound draft and refuses rebasing or continuation.
A correlated old-source edit may be retained solely to finish its original save
while `observe_result` reports conflict; it never replaces the displayed newer
publication. Once that bound save is durably observed, the original draft and
destination remain for explicit Stay or Discard.

Notes and NAGs are one explicit save action. Markings are a separate explicit
save action. Drawings clear when authoritative context changes, including equal
FEN nodes and different games. Keyboard-operable square and color controls are
included. Drawing input is refused while a save or source update is pending.
Saved drawings and temporary overlays are never stored by FEN.

After a checkpoint failure, `retry_save` emits a bound canonical `RetrySave`
intent with the new supplied identity. Attach its actual receipt through
`attach_checkpoint`, then continue observing its actual save progress. Before an
edit result exists, recovery retains the original intent and receipt identity
for parent-owned recovery of that admitted operation. A failure notice alone
does not prove source work has stopped or permit a speculative navigation.

### Bounded tree and variation review

`PresentationAnalysisVariationTree` also exposes `request_tree(id, next)`,
`request_children(id, parent, next)`, `preview_variation(id, request)`,
`request_scope(id, scope, next)`, `confirm_variation(id)`,
`append_study_move(id, expected, node, movement)`, `cancel_preview`,
`dismiss_failure`, `pending_intent`, and borrowed page/preview getters.

`show` returns `Command(intent)` or
`Navigate { request_id, expected, selected }`. Route the latter through the
annotations guard. The parent must serialize other source-changing actions
against an unsaved notes draft and obtain a fresh variation preview after a
notes save changes the source context. A confirmation token is never rebased.

The component retains one canonical tree page, one ordered children page and
at most one page per proof scope. Every page contains at most 100 entries.
Next-page actions reuse the retained opaque cursor; cursor provenance is
validated by the canonical source. Replies must match the pending receipt and
complete expected context. Children pages also match their parent; scope pages
match the retained token, scope, count, offset-derived length and next offset.
The UI never reconstructs a traversal or synchronously fetches a complete line.

Variation review displays actual previous/resulting tips, line sizes, deleted
subtree size and bounded owner-issued scope pages. Move names come from canonical
tree/children pages. Positions outside the loaded page are labeled truthfully;
bounded move-name loading and position review are explicit controls. Only the
retained `DesktopAnnotationCommandsVariationPreview` token is used to confirm.
Full context changes invalidate the confirmation. Played-history refusal remains
canonical source policy and is displayed through the typed failure seam.

Recorded notation is identified as a game or study. No live session, seats,
clock or outcome is manufactured for it. The new production components create
no service, storage client, executor, clock, session or local legality engine.

## Authored verification and outstanding acceptance

There are 23 authored tests and zero executed tests in this lane. The tests were
authored before the new APIs they first exercise. They use actual notation
records, admitted application receipts, permit-owning publications, canonical
owner operations and proof scopes, a temporary storage client, and an injected
executor and fixed clocks. They contain no wall-clock sleeps.

Coverage authored here includes guards and retained board destinations,
independent note/marking saves, full binding changes, async receipt rejection,
actual durable completion, retained storage failures and explicit retry intent,
newer draft versions, repeated publication observation, native egui repeated
passes, keyboard modal containment/cancellation, bounded tree and subtree pages,
ordered children, confirmation tokens and invalidation, exact equal-FEN distinct
nodes/games, pending-source refusal and authoritative typed board movements.

Static formatting and Git patch integrity checks are reported in the export
manifest. They do not establish compilation or behavior. No Cargo, tests,
Clippy, application runtime, build, CPU diagnostics, agents, extra worktrees,
commits or pushes were run. The parent owns shared source registration, mounting,
actual admitted service/application routing, compilation, test execution, all
canonical/native/platform acceptance, accessibility and task completion markers.
