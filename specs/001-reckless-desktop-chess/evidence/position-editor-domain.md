# Position editor domain foundation

This source-only handoff covers T146/T148/T149 against parent commit
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`. No task completion marker is changed.
The parent retains integration, Cargo verification and acceptance ownership.

## Implemented contracts

`board::editor` exports a raw `BoardEditorDraft`, distinct canonical non-nil
`BoardEditorDraftId`, checked `BoardEditorDraftRevision`, raw
`BoardEditorDraftFields`, typed validation findings and `BoardEditorSession`.
The draft never represents an accepted `BoardPosition` by itself.

Raw fields retain a shakmaty board, declared Standard/Chess960 variant, typed turn,
explicit original-rook-square castling rights, optional en passant target and
signed counters. They retain missing/extra kings, incompatible rights, illegal
check context, backrank pawns, invalid en passant context and invalid counters.
Changing pieces or turn never silently removes rights, clears en passant or
advances counters. Editor relocation replaces the destination piece without
using chess move legality or advancing a game.

Counter validation uses `BoardPosition::MAX_COUNTER`, accepting halfmoves
0..1000000 and fullmoves 1..1000000 inclusively. Counter findings are returned
before constructing a library setup when either raw counter is invalid. With
bounded counters, `Chess::from_setup` supplies structural validation and
`BoardPosition::from_chess` supplies the canonical accepted position. Library
findings are translated into actionable typed context, including king color/count,
rook squares, pawn squares, turn and en passant target. Canonical causes remain
available through the validation error source. No engine is involved and no
historical reachability proof is added.

`BoardEditorDraft::new(id, selected)` copies the actual selected position, or
Standard if absent. Setup extraction preserves the actual rook squares and
always-mode en passant target, including a target without a legal capture.
FRC/DFRC boards keep their declared Chess960 interpretation and actual rights.
The editor does not construct game sessions, seats, clocks or move trees.

`validate()` returns a real `BoardPosition` only on success. `validated_fen()`
delegates to that position's canonical FEN method. `play_prefill()` returns
`BoardStart::custom` for the validated position. `from_fen(id, fen, variant)`
delegates to the existing canonical parser and creates a new draft value. The
caller owns new identity allocation and unsaved-draft resolution. These APIs
perform no automatic Start or Resume.

## Session transitions

`BoardEditorSession::apply` accepts `BoardEditorSessionRequest` with the draft
identity, expected revision and a closed operation. Both identity and revision
are checked before selection, content or history changes.

Operations are Select, PlaceSelected, Edit, Replace, Undo and Redo. Selection is
None, Palette with color/kind, or an occupied Square. Edits cover placement,
removal, square relocation, declared variant, turn, granting/revoking an explicit
rook-square right, en passant and both raw counters.

Replace requires explicit Confirmed input for ClearBoard or StandardSetup.
Required confirmation returns a typed error without changing the session.
ClearBoard retains the declared variant and resets the position to an empty board,
White to move, no rights/en passant and counters 0/1. StandardSetup restores the
canonical Standard position, including its variant.

Changed position edits, Undo and Redo clear selection and advance the checked
revision. Undo/Redo restore complete raw fields, including invalid values, while
keeping revisions monotonic. Changed edits clear Redo. Unchanged operations
preserve revision, selection and history. Selection-only changes do not create
history or advance the position revision. Empty-square relocation, missing
selection, stale identity/revision, missing history and revision exhaustion
return typed errors before changing authoritative content/history.

History snapshots contain only the fixed board and position fields. Neither a
source game nor the existing history vectors are cloned for an edit. There is no
draft collection or fixed total draft-count cap in this component.

## Persistence boundary

`BoardEditorDraft` and its fields implement typed Serde conversion. The draft
record stores id, revision, position fields and explicit valid/invalid status.
IDs and revisions retain canonical string encodings. Piece placements and rook
rights reject duplicate squares. Square, piece, turn and variant declarations
use their typed validators. Raw counters and chess-invalid fields remain valid
draft data.

Deserializing recomputes validity and rejects a stored status that disagrees with
the actual fields. Findings are recomputed from those fields. Opening cannot
trust a persisted valid flag to authorize play. `from_parts` accepts already
typed identity/revision/raw fields and also derives validity on use.

`BoardEditorSession::from_draft` opens these persisted parts with no selection and
empty Undo/Redo. `into_draft()` discards all session selection/history. The draft
serialization contains neither selection nor Undo/Redo. Repository versioning,
SQLite writes and explicit Save remain T150 and later integration work.

## Parent prerequisites

The parent must add `pub mod editor;` to `src/board/mod.rs` and include
`tests/board/editor.rs` from `tests/domain.rs` as `board_editor`. The existing
`game_transition_fixture` registration supplies the real game fixture used by
the source-isolation test. Existing board-scoped `pub(super)` access already
permits setup extraction and `BoardPosition::from_chess`; no shared-board
gateway edit is required by this patch.

Presentation, storage, application commands, bootstrap wiring, unsaved-edit
dialogs, native input/shortcuts, analysis admission and platform acceptance are
owned by the parent and later task slices. This patch does not establish US6
journey completion or completion of the full application.

## Authored and executed evidence

Nineteen deterministic API tests were authored before production editor source
and exported as `position-editor-domain.tests-first.patch`. The final tests retain
those cases with additional boundary assertions. They use real `BoardPosition`,
`BoardStart`, source `GameSession` and fixed draft/move identities. Coverage
includes Standard/FRC/DFRC copies, original-rook distinctions, raw en passant,
king/check/right/pawn/counter failures, exact counter bounds, source isolation,
palette/selection/relocation, replacement confirmation, invalid-state history,
Redo invalidation, fresh sessions, Serde integrity, canonical IDs/revisions,
overflow atomicity and valid-only declared-variant FEN/play prefill.

Executed tests: **0**. No Cargo command, build, Clippy, application runtime,
diagnostic workload, agent fan-out, extra worktree, commit or push was run.
No existing board bug was fixed without an observed parent baseline regression.

Standalone `rustfmt 1.10.0` through the parent's recovered pinned-toolchain
wrapper parsed/formatted the owned Rust paths. Format checking, staged whitespace
checking, ownership checking and alternate-index forward/reverse patch replay
are static checks. They do not establish compilation, passing behavior or runtime
acceptance. The JSON handoff manifest records their final outcomes and hashes.

Final artifacts are
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/position-editor-domain.patch`
and the adjacent `position-editor-domain.manifest.json`. The final patch contains
only the four new editor source files, this evidence file and the new mirrored
board test file.
