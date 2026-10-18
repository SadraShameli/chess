# Game editor snapshot prerequisite

Source-only T106/T110 prerequisite. This artifact does not accept either task or
claim compilation, runtime, mounted-editor, persistence-command, or performance
verification. The parent retains the shared verification lane and source-module
registration.

Baseline commit: `e618533aa6f2f74da54b3d745f2b7b4ac19ecbc3`.
Baseline tree: `4d3366387d83ea197de2ff222124e45657fea648`.
The checkout was clean at that commit before authoring.

The design follows [US3](../spec.md), the [native message contract](../contracts/desktop-ipc.md),
the [study contract](../contracts/study-tools.md), and the
[canonical engineering rules](../../../.specify/rules/README.md).

`DesktopGameEditor<'a>` borrows the actual source immutably. Its played
constructor derives identity, revision, and selected view from `GameSession`.
Its notation constructor requires a notation `StorageLibraryRecord`, derives
that record's ID, accepts the authoritative notation owner's `GameRevision`,
and rejects an unknown selected node. `NotationGame` itself has neither an ID
nor a revision. The parent must supply its retained storage revision and
notation revision rather than inventing a played session.

`DesktopGameEditorBinding::new(storage_revision, epoch, generation)` supplies
the retained owner binding. `DesktopGameEditorEpoch::from_request_id` uses an
injected request ID. Generation advances with checked arithmetic. Context
equality includes game ID, storage revision, reopen epoch, local generation,
selected node, source variant, and actual source revision. Snapshot and both
page methods validate the complete expected context before projecting data.
The source owner must issue a new epoch on reopen and advance the generation
for accepted local changes. A projection borrow adds no second mutation owner.

`snapshot(expected)` owns the selected canonical `DesktopBoardSnapshot`, one
copy of selected annotations, metadata, root, and actual study mainline tip.
Canonical native legal moves and the declared variant come from
`DesktopBoardSnapshot::from_position`. Played state carries the real live tip,
phase, and outcome. Notation state carries the actual result, optional original
result header and time-control text, and optional selected clock annotation.
Notation has no fabricated seats, running clock, phase, outcome reason, or live
tip.

`tree_page(expected, cursor)` projects at most 100 insertion-order nodes.
Inspection of `GameTree::nodes` confirmed that it returns the retained vector's
slice iterator. Rows carry stable ID, parent, canonical SAN, child count, first
child, and separate notes/NAG/markings presence. The cursor retains full context,
the next insertion position, and the exact next node ID. Both position and ID
must still match the canonical iterator.

`children_page(expected, parent, cursor)` projects at most the same 100 rows
directly from the parent's ordered child slice. Its cursor additionally retains
the parent and validates the offset and next child ID. Promotion order therefore
comes from the domain rather than insertion order. Every returned page includes
its full context, including the last page. Cursor fields are private. No tree,
full history, sibling map, complete children vector, or annotation text is cloned
by either page method. No database operation, SQL offset, sibling search, RNG,
executor, or effect-boundary trait is introduced.

The parent needs only `pub mod game_editor;` in `src/desktop/mod.rs`. The source
includes `tests/desktop/game_editor.rs` under `cfg(test)`, so no integration-test
root registration is required. Provider, command, supervisor take/replace,
authoritative notation ownership, and presentation wiring remain parent work.

Tests were authored and staged before the first production file. The initial
14-test checkpoint SHA256 was
`ecfe51bdad59f90caa7ffc52c6c35238a50014cbd31bdd8fc9e005df0f8de4be`.
Static review subsequently refined those tests to use completed sessions for
annotation saves, retain private cursors, and exercise actual domain promotion.
There is no existing-behavior repair in this patch and no observed red run.

Final authored tests: 14. Executed tests: 0. Unrun tests: 14.
Coverage authored against actual notation records and played sessions includes:

- Unknown selected ID and wrong source-kind refusal, injected epoch identity,
  checked generation exhaustion, and exact context comparison across every
  binding field, including equal FEN and distinct selected nodes.
- Original notes, NAGs, markings, metadata, notation result/header/control/clock
  annotation, canonical native promotions and Chess960 moves, and real played
  phase, result, historical view, live tip, and independently promoted study tip.
- More than 100 tree nodes, a structurally valid promoted-material position with
  more than 100 legal root children, repeated page determinism, all canonical
  child ordering, leaf pages, unknown parents, stale cursors, and wrong canonical
  cursor positions under deliberately reused context.
- Source immutability through repeated projections and retention of an owned
  snapshot after the actual played source accepts an annotation change.

The wide-parent fixture's actual legal-move count and every behavior assertion
remain unrun. No measured memory, latency, or throughput result is claimed.

Static checks performed: standalone Rust 1.99.0 installation's rustfmt
formatting and `--check`, working/index diff checks, source-content review,
forward patch check against a scratch baseline index, and reverse patch check
against the applied scratch index and owned working files. The scratch index
round trip reproduces the baseline tree. These checks do not establish Rust
name resolution, compilation, Clippy, or runtime behavior.

No Cargo, build, tests, Clippy, CPU diagnostics, extra agent, extra worktree,
commit, or push ran. The final patch is exported to
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/game-editor-snapshot.patch`.
Its SHA256, staged tree, and full staged manifest are reported with the handoff
because embedding this document's own hash would be circular. The worker freezes
after the export checks.

Owned manifest:

- `src/desktop/game_editor.rs`: `ee069266d8735403d2b07c8d872f56de928ca69c`.
- `src/desktop/game_editor/context.rs`: `a2f5b1a8f1cd60bcdcd426e0a8d51a34848a4b68`.
- `src/desktop/game_editor/snapshot.rs`: `7217d8850a06e5d365756c2941d5461e179eaa1b`.
- `src/desktop/game_editor/tree.rs`: `18073115fe3dc6e90cf32d25d27308ab1d47545a`.
- `tests/desktop/game_editor.rs`: `4eec098d73dfa48ed16b3a05c88fe5460a0cd1d8`.
- `specs/001-reckless-desktop-chess/evidence/game-editor-snapshot.md`: this artifact.

## Parent canonical integration

[Verified editor integration](library-editor-integration.md) supersedes the
isolated unrun status for the integrated snapshot. It records 1,703 passing
tests, all three Clippy selections, both optimized builds, the final shared
record/clock-side refinements, durable logs and the remaining mounted/native
acceptance boundaries.
