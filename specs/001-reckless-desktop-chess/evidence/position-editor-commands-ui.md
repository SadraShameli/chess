# Position editor commands and native controls

This is a source-only producer handoff for NEW T147, T151, T152 and T153. It does
not claim T154, T155, mounted native editor acceptance, or executed regression
results. The source follows the [canonical rules](../../../.specify/rules/README.md)
and the US6, FR-017, editor data-model, desktop-message and study-tool contracts.

## Frozen inputs

- Base HEAD: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
- Domain patch SHA-256:
  `a645f2ad378016ebf0b957796d9247eb4ab2480c002a03127b069fbacb3f59f8`.
- Storage patch SHA-256:
  `3e8302c57aab0db3c1d4c2e0c72e840df8717e5d3e8a85f4dfe302bdc34f4f59`.
- Applied domain first, storage second, using their absolute paths under
  `/Users/sadrashameli/Personal/chess/.git/parallel-patches/`.
- Exact dependency index tree:
  `6f2026c795a88d412a6292ff4305c8a309f24b39`.
- Parent reported 19 domain tests and 13 storage tests passing before this slice.
  Those results were not rerun or independently verified here.

## Source behavior

`DesktopEditorCommands` owns the actual `BoardEditorSession` and the injected
`StorageEditorRepository`. Its typed operations implement selected-position or
Standard creation, editor edits and selection, validation, explicit save,
read/reopen, bounded listing, revision-checked deletion, FEN export/import, and
valid-only `BoardStart` prefill. Opening identities are injected by the root.
Contexts contain editor session identity, draft identity and domain revision.
Reopening the same saved draft resets domain history and rejects the old session.

Dirty state clears only after checking the actual repository acknowledgement's
draft identity, mutation, domain revision, validation status and exact next
storage revision. The pending owner retains the original repository write,
original draft and actual application receipt while later edits remain on the
active session. A canceled wait can be polled again on the same owned write.
A failed wait retains its write, original draft and typed error, returned through
`WriteFailed`; it does not replay or rebase onto another storage revision.
`pending_error`, `pending_draft`, `pending_application_receipt`,
`has_pending_write` and `last_receipt` expose the root's reconciliation evidence.
The source deliberately keeps an unresolved failed write retained; recovery must
not replace that owner or treat an error as permission to replay it.

Create, read, leave and FEN transfer retain their pending destination while
Save/Discard/Stay resolves the active draft. Saving before a transition moves on
only when that exact saved draft is still current. Newer edits keep the prompt.
Invalid FEN fails before changing the current draft. Confirmed Clear Board and
Standard Setup use domain editor history. Play returns a validated `BoardStart`
without creating a game or starting clocks or analysis.

The native editor panels expose the twelve-piece palette, turn, declared variant,
raw en passant target, original-rook castling rights, raw counters, validation
findings, explicit save and transition choices, setup confirmation, and local
Undo/Redo. Palette drags carry typed, context-bound payloads. The command and field
widgets emit at most one action per egui frame, including repeated passes.
The keyboard adapter implements the approved editor chords, suppresses them while
text fields or editor confirmations own input, and uses the existing actual-scene
neighbor projection for arrow focus.

Library Positions delegates to the editor draft-list component. Both borrow the
same canonical `StorageEditorRepositoryPage`, bounded by the repository's existing
100-row page size. `DesktopEditorCommandsPage` retains that page and its memory
reservation through `Arc`; it does not copy an index or introduce another codec.
Delete confirmation retains the listed entry and storage revision.

## Parent integration contract

The source entry point is:

```rust
DesktopEditorCommands::handle(
    &mut self,
    operation: DesktopEditorCommandsEnumOperation,
    receipt: &DesktopApplicationReceipt,
) -> Result<DesktopEditorCommandsEnumResult, DesktopEditorCommandsError>
```

This is an async method. The receipt must be the actual admitted application's
receipt for the command. The handler checks cancellation before its first effect
and immediately before SQL/session mutation. Awaited Read stages the recovered
candidate and checks cancellation again before replacing the session; List checks
again before returning its page. A canceled preaccept request returns `Cancelled`.
An accepted write retains a clone of the actual receipt and must finish through
`finish_write()`. Canceling that receipt cannot discard the write or auto-leave.
If a post-save destination read fails or is canceled, the matching durable receipt
remains available through `last_receipt` and the current draft remains recoverable.

The parent owns `DesktopEnumCommand::Editor { owner, operation }`,
`DesktopEnumValue::Editor(Box<DesktopEditorCommandsEnumResult>)`, entity admission,
typed domain identity gateways, service scheduling, cancellation, native wake,
runtime lifecycle, root module registrations and application result publication.
No local shared-root or domain mirror was needed or included in this artifact.
The new test sources reference the parent's `BoardEditorDraftId::try_from(Uuid)`,
`DesktopMessagesEntityId::for_editor` and `DesktopRevision::for_editor` gateways.

`PresentationEditorCommandPort` prepares a real `DesktopRequest` once and submits
that same request for the actual `DesktopApplicationReceipt`. The controller
retains request identity across explicit Busy/EntityBusy retries and matches
results to that receipt. Intermediate `WritePending` can update the displayed
snapshot while the receipt remains retained. Final results require the matching
completed application receipt. The root must preserve the owner and accepted
work through the final result; it must not abort a source future and drop an
accepted write. No source-owned runtime, pool or background task is introduced.
Existing storage admission, memory permits and actor execution remain canonical.

Register `desktop::editor_commands`, `presentation::editor` and the other owner's
`presentation::library::positions`. The owned source modules include their mirrored
test files through `cfg(test)` declarations; do not register those files twice.
Do not add a second library list or persistence owner at registration time.

## Exact native scene prerequisite

The existing scene's canonical-position path cannot display invalid drafts.
The parent needs an actual 3D editor adapter consuming
`PresentationEditorController::projection()` and its raw `fields().pieces()`,
`selection()`, `focused()` and `context()`. It must bind editor session/revision
and scene generation, render those raw pieces without constructing
`DesktopBoardSnapshot` or `BoardPosition`, and preserve the native camera,
renderer, picking and accessibility ownership.

Route actual square clicks, square drags, palette drops, removal and keyboard
placement through `click_square`, `begin_drag`, `drag_square`, `drop_palette_payload`,
`remove_square`, `focus_square`, `focus_direction` and `key`. The palette payload
type is `PresentationEditorControllerPalette`. Square drags retain the
`PresentationEditorControllerDrag` captured at drag start and reject a changed
editor context on release. The keyboard method takes the
actual `PresentationBoardScene`. Gate scene input on `controller.can_edit()` and
`!view.is_confirming_setup()`, and apply the same gate to accessibility actions.
The parent must provide the raw-draft semantic labels and focus targets on the
actual scene. This patch supplies no 2D board fallback or fabricated game/clocks.

Route the valid-only Play effect to the parent's New Game prefill. Keep the draft
owner alive while the New Game form is open, resolve unsaved edits before any
actual editor teardown, and require the existing explicit Start action. Analysis,
annotations, bootstrap, lifecycle prompts and mounted scene wiring remain with
T154/T155 and their parent owners.

## Authored verification and limits

The first new test sources preceded production implementation. Further
cancellation tests preceded the receipt-aware handler change. The sources use
real domain sessions, temporary SQLite stores, actual repository receipts, actual
application admission with an injected fixed clock, native fonts and the matching
egui harness. Controlled SQLite transactions and direct future polling stage
cancellation without sleeps. The camera test uses real camera/viewport projection
objects; it does not establish GPU rendering.

The 22 authored cases cover source isolation, initialization and history reset, invalid
save/reopen, blocked play/FEN, exact acknowledgement versus newer edits,
canceled waits, conflicts without rebase, Save/Discard/Stay, separate FEN transfer,
explicit play prefill, bounded pages and delete receipts, cancellation before
first poll and after staged read/list, canceled accepted writes, stale editor sessions,
palette and setup widgets, real application receipts and backpressure,
shared Library/editor page identity, shortcuts, placement/removal/history, stale
palette drops and scene-projected arrow focus.

Executed behavioral tests: **0**. Cargo, tests, builds, Clippy, native runtime,
CPU diagnostics, additional agents, extra worktrees, commits and pushes were not
run. Only the requested pinned rustfmt wrapper and read-only source/Git checks
were used for verification. Formatting and patch checks do not prove compilation,
behavior, performance, native scene acceptance or release readiness.

The binary staged patch is exported relative to the frozen dependency tree and
contains only the owned source, mirrored tests and this evidence file. Its
absolute path, exact owned-path manifest and SHA-256 are recorded beside it under
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/`.
