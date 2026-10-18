# Native position editor mount: source handoff

This is an isolated source implementation for US6 T151 through T155 integration.
It is not executable, native, accessibility-platform, or GPU acceptance evidence.
The parent exclusively owns compilation, Clippy, test execution, runtime checks,
and final integration. All 11 new tests are UNRUN. No baseline regression was
executed by this writer.

## Pinned inputs

- HEAD: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
- Dependency: `/tmp/claude-chess-editor-report-dependency-20261017/source.patch`.
- Dependency SHA256:
  `aa4c274f18a3a26d1ad52b6a61fe764eec51fda53b19e2782a9816277ac303c6`.
- Verified dependency tree: `bb22ef70a2572073156ea6ca2e8ce272652d825a`.
- The exported staged patch is incremental against that tree. It excludes the
  dependency baseline and every parent-owned repair.
- Parent compact draft-list source was read only at
  `/Users/sadrashameli/Personal/chess/src/presentation/editor/draft_list.rs`.
  Its inspected SHA256 was
  `fdd568b90fd61dc88630873594237cc687630b460f083d8ba0eec93271096160`.
  It was not copied into this checkout or included in the export.

## Mounted source

`PresentationApp` mounts one position editor owner, its actual controller and
view, and the shared saved-position page. Library's Positions tab and the editor
list use the same `PresentationLibraryPositions` instance and canonical page.
The parent-owned compact Open, Delete, and List effects map to the existing
typed desktop editor operations. Read, save, delete, validation, FEN import and
export, and Play use actual application requests and receipts.

The native bootstrap injects fresh draft identities and installs the persistent
editor command owner with `DesktopCoordinator::with_editor`. Opening sessions
come from the actual `prepare_action` ticket's request identity. The prepared
request is retained by the controller, and admission uses
`PresentationEditorApplicationPort`. Busy leaves that exact request available
for explicit Retry. Typed editor failures and generic desktop failures remain
distinct. Reconciliation issues an actual Snapshot request. Failed Snapshot
reconciliation blocks edits and waits for explicit refresh instead of spinning.

Save, Discard, and Stay remain command-owner decisions. Navigation and native
quit wait for the protected draft transition. Opening a saved draft or a new
draft also respects the existing Library notes guard. FEN transfer creates a
separate opening; it does not replace an unresolved draft. The deferred opening
slot is bounded to one, with FEN capacity checked before retention. No request
identity cache or independent editor history was added.

New Draft obtains the actual selected source position, or passes no selection
so the canonical editor owner chooses Standard. Play consumes the validated
BoardStart returned by the actual receipt and prefills the existing New Game
configuration. Start remains a separate action. The editor draft is retained
across this prefill. Selecting Editor does not start or resume a game, request
an engine search, or alter the running-game resource admission owner.

`SceneRenderer::apply_editor` consumes raw draft pieces and typed editor context.
It shares the existing piece entity, template, material, camera, picking, image,
and lifecycle path with validated game positions. It never constructs a
BoardPosition or DesktopBoardSnapshot from invalid fields. Every editor context
change invalidates the scene generation, including field-only edits. Switching
back to a game restores the canonical game scene path.

The mounted image and projected semantic cells use the actual scene. Click,
square drag, palette drop, keyboard, secondary-button removal, and accessibility
activation call the existing typed controller methods. Gestures bind both editor
context and scene generation. Input requires controller edit admission and no
setup confirmation, plus ready matching scene geometry. Keyboard neighbors use
the canonical projected-neighbor implementation. The board label includes raw
variant, turn, castling squares, en passant, counters, validity, and dirty state.
Preparing or stale scenes show preparation text; no alternate board is drawn.
Canonical camera controls are also available in Editor.

## Parent prerequisites

These files are outside this writer's ownership and were not edited:

1. In `src/scene/mod.rs`, register `pub mod editor;`.
2. In `tests/presentation.rs`, include `presentation/app_editor.rs` as
   `presentation_app_editor`. The existing `game_transition_fixture` registration
   is used by the played-source preservation test.
3. In `tests/scene.rs`, include `scene/editor.rs` as `scene_editor`.
4. In the `DesktopNativeHostServices` literal in
   `tests/desktop/native/host_puzzles.rs`, supply `editor: None` unless that fixture
   explicitly mounts a real editor connection. The production literal is wired
   in this patch. Recheck all final callers during parent integration.
5. Apply the parent's compact
   `PresentationEditorDraftListEnumAction::{Open, Delete, List}` repair before
   compiling this consumer. This source intentionally targets that approved
   contract, rather than the frozen tree's older Command variant.
6. Retain the parent's history bounds, controller/field lint repairs, and command
   recovery changes. The mounted retained-write retry uses the unchanged Save
   operation; its receipt and original write ownership remain command-core
   responsibilities.

## Explicit T154 gap

The dependency has no operation that opens a validated draft as a distinct
annotation/analysis document. T154's validated-position analysis/annotation
gateway remains parent-owned missing work, confirmed in the parent handshake.
No document, game, receipt, or publication is fabricated to bridge that gap.

A narrow gateway needs to consume the exact DesktopEditorCommandsContext and
its validated BoardStart, acquire root-injected document/node identities, create
an independent persisted study source, and return an actual
DesktopGameEditorPublication through a correlated application receipt. It must
retain the editor draft, respect the existing notes guard, reject stale context,
and preserve the played source and its history, clocks, and outcome. The mounted
draft editing and Play prefill do not establish this missing behavior.

## Authored verification contracts, all UNRUN

`tests/presentation/app_editor.rs` contains nine contracts using the actual
application, coordinator, editor command owner, SQLite repository, board domain,
and headless egui where relevant:

- FEN transfer resolves the real prompt, saves the prior invalid draft separately,
  lists it, and exports through the actual receipt.
- Generic owner refusal remains distinct and failed refresh does not spin.
- New Draft copies a played position while retaining its source history, clock,
  outcome, revision, and phase in the presentation owner.
- A typed stale failure reconciles from the actual Snapshot before further edits.
- Busy Retry reuses the actual opening request and session identity.
- Headless native workspace controls mount setup confirmation and invalid state.
- Invalid draft save/read restores raw fields without session history.
- Stay preserves the draft; Discard releases the exact deferred destination.
- Play only prefills New Game and retains the draft.

`tests/scene/editor.rs` contains two contracts with actual ECS asset resources and
the existing style of GLB-named, headless mesh fixtures:

- The mounted renderer image and projected accessibility cells route through the
  controller, suppress setup-modal input, and render an invalid replacement.
- Raw invalid fields render without a legal-position conversion; field edits
  invalidate the typed scene binding and obsolete generation.

Fixtures close coordinator/storage owners and dispose scene resources before
behavior assertions. The headless mesh fixture is not production visual proof.
No test publishes an invented editor or game-document result.

## Allowed source checks

The writer uses the standalone formatter from the exact Rust 1.99.0 distribution,
with `--edition 2024 --config skip_children=true`, explicitly naming only owned
Rust files. The formatter reports
`rustfmt 1.10.0-stable (b940084d7e 2026-09-28)`. Its matching dynamic libraries
were extracted into the task scratch folder; no compiler was executed or
installed. Final format, whitespace, incremental forward-apply and reverse-apply
checks, the staged result tree, file hashes, and patch SHA256 are recorded in
the external handoff manifest.

Cargo, builds, Clippy, tests, runtime/native acceptance, GPU/CPU diagnostics,
shared targets, agents, commits, and pushes were not used. The source is frozen
after the patch and manifest handoff. US6 completion and release acceptance are
not claimed.
