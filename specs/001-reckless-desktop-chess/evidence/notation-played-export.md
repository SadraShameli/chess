# Borrowed played-game PGN and notation study continuation handoff

Recorded 2026-10-17 for the authorized T098/T106/T110 prerequisites. This is a
source handoff. Compilation, execution, and task acceptance belong to the parent.

## Frozen inputs and ownership

The isolated checkout is
/Users/sadrashameli/.t3/worktrees/chess/t3-322acae2, based on parent commit
647147d9ff95cb745ede46637048ef312e3070af.

The supplied dependency
/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-runtime-import-current.patch
was SHA256-verified as
cee393a24f2a999675a030bdc3199c39d8daf7eda431c5e65954c3a7af40f7a1.
Its 67 paths applied cleanly with git apply --index and produced the exact
supplied tree e9219be3c2255064d9611ac5ae5ac37c8a9fc773.

The parent accepted the API handshake and authorized mirroring one prerequisite:
GameTree::prepare_study changes visibility from pub(super) to pub(crate).
It remains tree-owned and is excluded entirely from the owned export. The frozen
dependency plus that visibility prerequisite has tree
c0541c43c4b9d4ee35c8c009a8f421d2f690c75f. Parent installation of this
visibility is required before compiling the notation API.

The owned patch contains exactly these five paths:

- src/notation/game.rs
- src/notation/pgn_export.rs
- tests/notation/game.rs
- tests/notation/pgn_export.rs
- specs/001-reckless-desktop-chess/evidence/notation-played-export.md

No unchanged dependency path, tree implementation, GameSession, codec, importer,
module/test root, task marker, vendor file, or dependency change is exported.

## API and behavior

NotationPgnExport::new(&NotationGame).with_selection(...).write(...) retains its
public contract. The new const from_played(&GameSession) constructor borrows the
actual session through the private NotationPgnExportEnumSource. Both sources
share headers, selection, iterative traversal, annotation encoding, writer,
cancellation, and typed error policy.

Played export follows GameSession::study_mainline_tip, independently of the
live and view cursors and first-child order. It borrows the actual initial
position, metadata, tree, notes, NAGs, and saved markings. The actual outcome
supplies both the Result header and movetext token; an absent outcome emits *.
The existing notation source retains optional Result-header semantics.

A played clock annotation requires a non-root node, a genuine before snapshot,
and a timed post-move snapshot. The mover is the opposite of the resulting
position's turn. Its stored remaining balance comes from the public typed
snapshot DTO. No current live-clock observation, guessed seat, synthesized
session, persisted conversion, or analysis-branch clock is introduced.
Untimed nodes and new study nodes have no exported clock. Retained genuine
played nodes keep their clocks even when exported in an alternative line.

The session has no raw PGN TimeControl or unavailable-Elo warning field. Played
export emits its actual known numeric ratings and metadata; it does not invent a
raw TimeControl or raw rating. The existing notation source preserves its raw
TimeControl, unavailable-rating warnings and exact source Elo text. No storage
kind changes.

Clock lookup uses the existing notation aggregate index or the actual played
node, avoiding a second aggregate-wide clock map. The public persisted snapshot
DTO clones its time-control value when reading a played clock. Output remains
sequential because byte ordering is required. No runtime performance result is
claimed.

The added API is
NotationGame::append_study_variation(&mut self, parent: GameMoveId, node: GameMoveId, movement: BoardMove),
returning Result<GameAnnotationsVariationApplied, NotationGameError>.
It delegates legality, identity collision checks, matching continuation reuse,
annotation preservation, and clock preservation to GameTree::prepare_study.
The returned view() is the actual inserted or reused identity. Tree cardinality
before and after commit determines the actual Changed or Unchanged effect.
It never advances the selected mainline tip or creates a GameSession.

Adding below the selected tip deliberately preserves that endpoint. Mainline
export still stops there. Full export keeps the existing typed
MainlineHasContinuations refusal until a separate explicit promotion chooses a
representable leaf. Existing recognized-directive, carriage-return line-comment
and unsupported-tag refusals remain on the shared writer path.

## Authored coverage and ordering

Seven played-export tests and all seven study-continuation tests were authored,
staged, and parsed by standalone rustfmt before either owned production source
was edited. The tests-only snapshot SHA256 was
77e9b1161e74010583e8ab793958e466c9e41da785c637fadd813962608585fb.
A further played-export case then covered a restored non-first selected path.

There are 15 new tests: eight played-export tests and seven notation-game tests.
The existing exporter file now has 47 tests, including its 39 retained cases.
Together the two owned test files contain 54 cases. Zero tests were executed in
this lane. Missing new APIs were not presented as semantic regression failures;
source inspection found no existing defect requiring a repair in this handoff.

Fixtures use actual BoardStart, BoardPosition, GameTree, GameSession,
ClockState, notation aggregate, and canonical importer values. Played sessions
use the existing deterministic domain proof fixture. No sleeps, network,
external data, CPU diagnostics, or mock session state is used.

Coverage includes exact Fischer post-clock nanoseconds for both movers;
Standard, custom Black-to-move, FRC and DFRC starts; untimed clock absence;
promoted and restored non-first selected study paths; actual outcome
preservation; genuine clocks retained on former played mainlines; clockless new
analysis branches; metadata, ordered extension tags, NAGs, saved markings and
semicolon comments through canonical importer roundtrips; source preservation
after successful export, early/partial cancellation, writer error and typed
comment refusal. Existing source-rating and unsupported-tag cases remain intact.

Study cases cover legal insertion, actual selected identity, insertion/reuse
effects, stable existing IDs and annotations, retained clocks and raw source
Elo, metadata/result/time-control preservation, illegal or wrong-turn moves,
identity collisions, a deleted stale parent, custom/Chess960 parent legality,
and selected-mainline immutability including the explicit Full refusal.

## Static checks and remaining acceptance

The global instructions, root bridge, constitution, all seven
[canonical rules](../../../.specify/rules/README.md), and the relevant
[study contract](../contracts/study-tools.md), game model, feature acceptance,
decisions and plan were inspected.

Standalone rustfmt formatting and --check passed on all four owned Rust files.
Git working/staged whitespace checks, authored-source policy scans, the exact
owned path manifest, forward application against the frozen dependency index,
reverse application against the current source, and the reconstructed tree
comparison passed. These checks establish patch and formatting integrity only.

The exact staged binary patch is
/Users/sadrashameli/Personal/chess/.git/parallel-patches/notation-played-export.patch.
Its SHA256 and final staged tree are supplied in the handoff response, outside
the self-containing evidence file.

No Cargo, builds, tests, Clippy, CPU diagnostics, agents, commits or pushes ran.
The shared target was never used. Parent must register tests/notation/game.rs
from the domain root and install the tree visibility prerequisite.
Parent owns compilation, targeted execution of the 54 owned cases,
the unchanged surrounding annotation/import/session contracts, and canonical
verification. Native handles, atomic destination publication, application
handlers, UI controls, GPU/platform/release evidence and T106/T110 completion
remain with their respective owners.

## Parent canonical integration

[Verified editor integration](library-editor-integration.md) supersedes the
isolated unrun status for the integrated snapshot. It records 1,703 passing
tests, all three Clippy selections, both optimized builds, the final shared
record/clock-side refinements, durable logs and the remaining mounted/native
acceptance boundaries.
