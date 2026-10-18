# PGN export and declared-variant FEN source handoff

Recorded 2026-10-17 for T098 and the export/roundtrip portion of T093. This is
source-authoring evidence. Neither task is accepted by this handoff.

## Owned files and interfaces

The patch contains only `src/notation/pgn_export.rs`, `src/notation/fen.rs`,
their two mirrored integration-test files, and this evidence file. Library/test
module registration, aggregate ownership, importer ownership, game identity,
dependencies, destination commit/rename, and executable verification belong to
the parent integration lane.

`NotationPgnExport::new(&NotationGame)` borrows the validated aggregate and
defaults to `NotationPgnExportEnumSelection::Full`. `with_selection` accepts
`Full` or `Mainline`. `write(&mut W, cancellation)` accepts an injected
`std::io::Write` destination and `FnMut() -> bool` cancellation callback. It
returns `NotationPgnExportError` and does not flush or close the caller's writer.
`NotationFen::import(fen, variant)` delegates to `BoardPosition::from_fen`;
`NotationFen::export(&position)` delegates to `BoardPosition::fen`. The existing
board owner enforces its 512-byte bound and position validation.

## Written behavior

The exporter serializes canonical SAN from existing tree nodes. It follows the
explicit `mainline_tip` path, emits its selected child first, and emits the other
children as variations in their existing relative order. Inside alternative
lines, the first ordered child provides their continuation. Mainline export
stops exactly at the selected tip. Full export refuses a selected tip that has
continuations, because standard PGN cannot encode those continuations while
retaining that exact selected endpoint.

Supported known metadata uses canonical tag names; extension tags preserve
their order and display values. Quote and backslash escaping applies to tag
values. Typed unavailable-rating warnings re-emit the exact original per-color
Elo value without fabricating a known numeric rating. Missing Result headers
remain absent, independently of the required
movetext result marker. Every export declares Standard or Chess960. Custom
Standard positions and all Chess960 starts include SetUp and the canonical FEN;
the board owner supplies Chess960 rook-file castling rights. Move numbers come
from the immutable parent positions, including Black starts and alternatives.

Notes remain plain text. Brace comments preserve backslashes literally and add
no padding. Notes containing either brace character use standard semicolon line
comments. Each original newline separates consecutive payloads, including empty
first/final payloads; every emitted line terminates before a subsequent move or
variation delimiter. Marking and clock directives append to the same final
payload without added whitespace. Root and move notes, typed NAGs, ordered
markings, and clocks are included. Clock fractions retain exact nanoseconds.
Unknown directives remain literal note text; the borrowed source aggregate's
warnings remain available to its caller.

Literal recognized clock/marking directive text is explicitly refused because
the importer would interpret it as structured data. Brace-containing notes with
carriage returns and tag values containing control characters also return typed
unsupported-representation errors. There is no custom backslash-close-brace
grammar, HTML rendering, silent replacement, or annotation truncation.

Writes use at most 4096-byte requests and observe cancellation before each
request, including partial writes and interrupted retries. I/O errors and
zero-length writes stop export immediately. The implementation uses an iterative
traversal, a mainline ID path, and a typed clock lookup; it never clones the tree
or materializes the complete PGN. Output ordering requires sequential writes.
No measured throughput, allocation, latency, or scaling result is claimed.

## Authored checks and pending gates

The initial 27 PGN tests and seven FEN tests were authored before either
production file. Additional source-review coverage brings the authored total to
39 PGN tests and seven FEN tests, 46 tests overall. None were executed. Missing
production APIs were not counted as semantic regression failures.

The cases include independent fixed PGN expectations, full/selected-mainline
variation ordering, optional Result headers, metadata escaping/display/order,
root/move annotations, all marking colors/shapes, exact clock fractions and the
native maximum, brace/semicolon/newline/backslash preservation, unsupported
representations, Black starts, both castlings in both variants, all four
promotions, check/checkmate, en passant, writer ownership, bounded/partial/
interrupted writes, cancellation, destination errors, and 512/513-byte FEN
boundaries. Roundtrip cases call the real importer through its coordinated
public API and use real board/tree/aggregate values.

The required guidance was read from the global instructions, project bridge,
constitution and all seven [canonical rules](../../../.specify/rules/README.md).
Applicable feature requirements, design decisions, research, data model and
study contract were checked against existing board, start, tree, metadata,
annotation and clock APIs. The design retains the existing effect boundary and
component-prefixed types; no comments, suppression attributes, unsafe code,
dependencies or unrelated root changes are introduced.

Only standalone formatting with
`/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt --edition 2024`, source inspection,
and Git/static content checks are authorized in this lane. Cargo, compilation,
tests, Clippy, runtime diagnostics, commits and pushes were not run. Parent
module registration, importer integration, semantic red/green where needed,
the canonical verification sequence and full T093/T098 requirement acceptance
remain pending. This evidence must be updated with actual executed counts by
the integration owner before task acceptance.

The standalone formatter and its check passed on all four owned Rust files.
Static content inspection found no prohibited code comments, suppression/ignore
attributes, unsafe code, panic/unwrap/expect calls, or em dashes in those files.
The staged whitespace check and exact five-file patch-scope check passed. The
binary patch is exported to
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/notation-pgn-export.patch`.

## Parent integration

The parent integrated the importer, aggregate, exporter, FEN boundary and
independent combined fixtures after `19b44cf`. Initial compilation passed.
All export/FEN and combined round-trip fixtures passed during the frozen
semantic baseline; four separately added importer regressions failed and were
repaired only after observation. The final focused run passed 11 private
importer and 104 selected domain cases, including the 39 exporter and seven FEN
contracts. That broad filter also includes existing identity and game cases;
it is not a count of newly authored export tests.

The exporter privately groups its traversal inputs to satisfy the existing
argument-count lint. No checker change, public API change or behavior change
was required. All-target, all-feature Clippy subsequently passed with
`-D warnings`. Final canonical verification and T093/T098 acceptance remain
pending. Exact command logs and checksums are retained with
[the importer integration](pgn-import.md#parent-integration-and-observed-regressions).

The subsequent canonical checkpoint passed 1,403 tests, all three Clippy
selections and both optimized builds, as recorded in
[the final importer checkpoint](pgn-import.md#canonical-checkpoint).
T098 is accepted for supported canonical full/mainline PGN export and declared
variant FEN interchange, including its explicit unsupported-representation
errors. Desktop/native file handles, atomic destination publication and library
UI remain T106/T108 acceptance. No GPU or platform execution is accepted here.
