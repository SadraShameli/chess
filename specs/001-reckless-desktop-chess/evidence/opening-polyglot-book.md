# Standard Polyglot studybook core

This isolated source slice starts at
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68` and advances the core portions of
T158/T169. It owns `src/opening/mod.rs`, `src/opening/book.rs`, its focused child
files, `tests/opening/book.rs`, its fixture helper and binary fixture, and this
evidence. It does not complete either task's integrated acceptance.

The governing behavior is [FR-020 and US7](../spec.md), the
[selected book policy](../decisions.md#book-generation-and-explorer-semantics),
[study-tools contract](../contracts/study-tools.md#opening-book) and
[OpeningBook model](../data-model.md). The implementation follows the
[constitution](../../../.specify/memory/constitution.md) and the seven canonical
rules: [code quality](../../../.specify/rules/code-quality.md),
[verification](../../../.specify/rules/verification.md),
[architecture](../../../.specify/rules/architecture.md),
[structure](../../../.specify/rules/structure.md),
[Rust style](../../../.specify/rules/rust-style.md),
[performance](../../../.specify/rules/performance.md) and
[testing](../../../.specify/rules/testing.md).

## Boundary and identity

`OpeningBook::validate(bytes, source, expected_checksum, limits, cancellation)`
borrows immutable bytes, consumes the existing
`StorageDatasetPackageSourceIdentity`, and checks the actual SHA-256 before
admission. `OpeningBookIdentity` binds that canonical source/version/manifest
identity to the actual book checksum. A query retains that binding and the full
`BoardPositionIdentity` alongside its Polyglot key. There is no new source
registry, file access, serialization, executor, worker pool, async runtime,
engine command or opponent-move selection operation.

The approved parent-owned gateway is
`BoardPosition::standard_polyglot_key() -> Result<BoardPositionPolyglotKey, BoardPositionError>`.
The key exposes `value()` and remains owned by `board::position`. The gateway
uses the installed `shakmaty = 0.30.2`
`Position::zobrist_hash::<Zobrist64>(EnPassantMode::PseudoLegal)` for Standard
and refuses Chess960 with `UnsupportedPolyglotVariant(BoardEnumVariant)`.
Its source API, en-passant mode definition and fixed Polyglot vectors were
inspected read-only in the installed vendor source. Opening never accesses
`BoardPosition::chess`, reparses a board through FEN, implements hashing, or
copies the 781 Polyglot constants. No gateway mirror is included in this patch.

Pseudo-legal en-passant hashing is intentionally different from repetition/full
identity's legal en-passant normalization. A pinned adjacent pawn still affects
the Polyglot key. Query legality continues to come from the actual board. The
Polyglot format supplies a 64-bit lookup key rather than a full source position;
legal filtering cannot establish the historical position behind a forged hash
collision. This core never merges full positions using that key.

## Bounds and validation

`OpeningBookLimits::new(NonZeroUsize)` requires an explicit caller input limit
and refuses a limit above 67,108,864 bytes, the selected 64 MiB decoded-buffer
boundary. An input above the caller's limit is refused before validation. This
supports the selected 50,000,000-byte starter book component without treating
that starter cap as a universal personal-book limit. This is a bounded borrowed
byte core, not an arbitrary-size personal-file importer. The adapter must bound
staging and allocation before creating its slice. Input bytes, canonical source
metadata, validation state and query output all remain charged to the parent's
capacity and memory accounting; the input limit is not a measured total-memory
claim.

Validation visits immutable bytes in at most 65,536-byte chunks, without a
decoded vector of all records. Records are 16-byte big-endian `QHHI`: key,
encoded move, raw weight and opaque learn value. Empty input, any incomplete
tail, descending keys, the reserved top move bit, promotion codes 5 through 7,
and checksum mismatch are typed refusals. Keys need only be nondecreasing;
records within a key can appear in any move or weight order. Duplicate
`(key, encoded move)` records are refused even when their weights differ or
another move separates them. No duplicate-weight aggregation policy is invented.
The same encoded move under a different key is permitted.

Duplicate tracking uses a fixed 32,768-bit bitmap and a touched-word list of at
most 512 machine indices. New keys clear only touched words. State survives
chunk boundaries and does not grow with the file's number of positions. With
the accepted promotion range and unique move encodings, a validated key group
has at most 20,480 records. Counter and offset arithmetic stay within the checked
input bound; record offsets and weight/share arithmetic also use checked
operations. These are source-derived bounds, not measured performance results.

Cancellation is injected as `impl Fn() -> bool`, compatible with
`|| cancellation.is_cancelled()` from the existing executor. It is checked
before admission, between chunks, for each record, before checksum finalization
and return, during binary search, legal-move preparation, group scanning and
share calculation, and before a completed query is returned. No partial book or
query result escapes cancellation. SHA-256 processing of one bounded chunk,
board legal-move generation and small legal-result sorting are synchronous
operations between cancellation checks. The root must invoke validation/query
work on its existing admitted executor and retain operation ownership. No
executor scheduling, elapsed-time bound or runtime responsiveness was tested.

## Study lookup

Lookup binary-searches the validated bytes, then scans only the selected key
group. It encodes the existing board's legal moves once and matches records to
those real `BoardMove` values. Standard castling encodes the king's origin and
castling rook square, then returns canonical Standard UCI such as `e1g1`.
Ordinary king-destination castling records do not masquerade as Polyglot
castling. Promotions use the Polyglot knight/bishop/rook/queen codes, and
en-passant legality comes from the board.

Structurally valid records that are illegal for the selected position are
excluded and counted in `rejected_illegal_records()`. Their weights never enter
the denominator. An absent key returns an empty result with zero rejections;
an all-illegal group returns an empty result with explicit rejections. Raw zero
weights remain present for legal moves. Results sort by descending stored u16
weight, then canonical UCI ascending. Raw learn values remain opaque metadata.

Shares use checked integer nearest rounding to tenths of a percent from legal
stored weights. `OpeningBookShare` exposes integer `tenths()` and formats exactly
one decimal plus `%`. All-zero legal totals give `None`, and a zero-weight move
beside a positive weight has an available `0.0%` share. Display code must explain
that rounding may not total exactly 100%. These weights are relative book
preferences; no evaluation, win probability, source frequency reconstruction or
weight rescaling is inferred. Starter book generation and original-count
provenance remain in the parent's tools/data scope.

## Authored fixtures and verification status

The public-contract fixtures were written before the production implementation.
No baseline regression was executed, and no red-to-green result is claimed.
There are 19 authored test functions, covering nine fixed external Polyglot key
vectors, pinned en-passant hashing, literal big-endian record fields, actual
source/checksum binding, legal-weight denominators, sort ties, zero/unavailable
shares, max-u16 stored weights, half-up rounding, castling for both colors,
all four promotions for both colors, legal and illegal en-passant, absent keys,
typed variant refusal, empty/truncated/malformed/duplicate/unsorted records,
checksum and byte-limit refusal, chunk-boundary state and deterministic
cancellation. The study fixture passes prepared moves to the existing real
`GameSession::append_study_variation` boundary, checking active-game refusal,
branch reuse, and preservation of played history/result. It adds no engine
integration and is not a substitute for native command/presentation acceptance.

The synthetic literal fixture `tests/opening/fixtures/start-e2e4.bin` contains
key `463b96181691fc9c`, move `031c` (`e2e4`), weight `0102` (258), and learn
`01020304`. Its independent file SHA-256 is
`d841dbc2c738761294ef867ab94138815163a1dd8884b77bb9213199312b623d`.
Known key constants come from the published Polyglot reference sequence also
present in shakmaty's installed reference tests; the fixture expectations never
call the new lookup implementation to produce expected keys or move encodings.
The single pinned-en-passant delta is the reference d-file mask
`1c99ded33cb890a1`, not a replacement hashing implementation.

Executed checks are limited to the authorized pinned `rustfmt`, source/diff
inspection, patch applicability checks and SHA-256 handoff checks. Exactly zero
Cargo, build, test, Clippy, application runtime or CPU diagnostic commands were
executed. Formatting and patch checks establish neither compilation nor
behavior. No agents, additional worktrees, commits or pushes were created.

The parent owns the canonical gateway and its tests, `src/lib.rs` module
registration, `tests/domain.rs` registration as `opening_book`, existing
`game_transition_fixture` registration, shared roots, executor/file/storage
adapters, immutable import previews, dataset lifecycle, study commands,
presentation, dataset preparation and final verification. This patch does not
mark T158/T169 complete, prove full US7/US13 acceptance, acquire licensed real
data, or prove that any installed starter book exists.
