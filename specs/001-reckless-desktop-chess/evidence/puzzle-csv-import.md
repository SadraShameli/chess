# Puzzle CSV source boundary

The initial isolated T263/T269 slice starts at `43105cc84ea926b2af3fc54a1baa260fcdad68c2`
and adds only `src/puzzle/csv_import.rs`, `tests/puzzle/csv_import.rs` and this
evidence file. It implements the CSV portion of
[FR-021](../spec.md), [puzzle interchange](../decisions.md#puzzle-interchange-and-lifecycle)
and the [import limits](../decisions.md). Production module declaration, public
test-root registration, task markers and consolidated verification belong to the
parent. No catalog, acquisition, installation or dataset preparation behavior is
included. T187/T190 native acceptance and the remaining US13 work remain open.

The implementation follows the [constitution](../../../.specify/memory/constitution.md)
and its seven canonical rules:
[code quality](../../../.specify/rules/code-quality.md),
[verification](../../../.specify/rules/verification.md),
[architecture](../../../.specify/rules/architecture.md),
[structure](../../../.specify/rules/structure.md),
[Rust style](../../../.specify/rules/rust-style.md),
[performance](../../../.specify/rules/performance.md) and
[testing](../../../.specify/rules/testing.md).

`PuzzleCsvImport<R: BufRead>::new(reader, dataset)` takes an existing
`PuzzleSelectionDataset` and performs no reads. `next_row(impl Fn() -> bool)` reads and
validates the header on demand, then returns one `PuzzleCsvImportRow`, a typed
`PuzzleCsvImportError`, or `None`. `headers()` exposes the successfully validated
header in source order after the first call. Logical row and column numbers are
one-based; the header is logical row 1. Embedded line endings do not increment
the logical row number.

The parser owns no executor, runtime or worker pool. The root must run file-backed
`BufRead` parsing and board/solution validation on its existing bounded blocking
or CPU executor, charging that work to its shared capacity budget. Reader
timeouts belong to the injected reader or adapter. Cancellation is checked before
and after every buffer read, during each solution move and before returning a
validated row. It cannot interrupt an already blocked `fill_buf` call or the
synchronous `PuzzleDefinition` constructor. The caller passes the existing
executor token through `|| cancellation.is_cancelled()`. Parsing adds no copied
cancellation flag, detached task or global cancellation state.

The standard-library finite-state scanner accepts UTF-8, LF and CRLF record
terminators, quoted commas, doubled quotes and quoted multiline fields. Line
endings inside quotes retain their original bytes. It rejects quotes inside
unquoted fields, text after a closing quote, unclosed quotes, bare unquoted CR
and invalid UTF-8. It reads only through the current record terminator and never
requests the next record to yield the current one. The injected `BufRead` may
perform its own bounded read-ahead.

`MAX_ROW_BYTES` is 1,048,576 encoded bytes per logical record, including commas,
quotes, escaped quotes and all embedded quoted line endings. The final LF or
CRLF record terminator is excluded. The same bound applies to the header. The
scanner refuses the first byte beyond the bound before appending it. An exact
bound record is accepted with a terminator or EOF. Decoded storage, field offsets
and the current validated definition are proportional to this bounded record;
the parser retains no collection of prior rows. This is a bounded-record design,
not a measured peak-memory or throughput result.

Header names are exact and case-sensitive. `PuzzleId`, `FEN` and `Moves` are
required. All documented optional columns may be omitted or empty, including
`Rating`, `Themes` and `DailyDate`; older ten-column and current eleven-column
headers both work, and column reordering is supported. Empty or duplicate
headers, including duplicate unknown names, are rejected. Every data record
must have the same width as its header. Empty records are validated as records
and are never silently skipped.

`PuzzleCsvImportRow` exposes a validated `PuzzleDefinition`, the typed opponent
setup move and `PuzzleCsvImportSourceMetadata`. The metadata DTO keeps all
source columns in order as a homogeneous `Vec<PuzzleCsvImportExtraField>`;
each entry preserves its exact decoded header and field value. Known columns
also carry a typed field identifier. Thus unknown Unicode header names and
values, original FEN, original complete move text, leading integer zeroes,
opening labels, URLs and optional daily dates remain available. Raw metadata
means the decoded original field contents; CSV delimiter and quoting syntax is
not retained. URLs, opening tags, daily dates and unknown values are inert source
text without execution, fetching or invented semantic normalization.

Difficulty and themes use their existing domain owners. Missing or empty values
are `None`, while explicit rating zero remains an available rating. Themes retain
exact token case and order and pass the existing duplicate-theme validation.
Rating deviation is an optional `u32`, popularity an optional `i32`, and play
count an optional `u64`; ASCII decimal syntax and checked parsing reject malformed
or overflowing values. Only popularity permits a leading sign. The raw strings
remain available independently of these parsed values. There is no second
provenance registry, source factory or dataset identity type.

The original FEN is validated as Standard chess through `BoardPosition`,
including its existing 512-byte and counter bounds. `Moves` requires at least
two canonical UCI tokens separated by single ASCII spaces. Each token is parsed
against the actual preceding board. Moves[0] is applied exactly once as the
opponent setup move. The solver and starting board come from the resulting
position, preserving side to move, counters, castling and en-passant state.
Moves[1..] become one complete line passed to `PuzzleDefinition::new`, which
replays its typed moves and validates the immutable solution tree. A late illegal
move prevents the entire row from yielding a definition. Promotions, including
underpromotions in either setup or solution, retain their selected piece. No
engine evaluates legal unlisted alternatives or performs import verification.

The definition owner supplies the shared 1,024-line and 4,096-ply limits.
CSV checks its single line before legal replay and tree construction; persistence
validation uses the same owner and retains its existing budget-error outcome.
`into_parts()` transfers the validated definition and preserved source metadata
to the caller without cloning the tree.

Every error carries its logical row, optional one-based column, known typed
field when available, source issue and explicit continuation status.
`NextRow` permits another preview call after an entire record has been framed:
width, UTF-8 field content, identity, integer metadata, FEN and solution errors
exclude that complete row. `Terminated` covers invalid headers, malformed CSV
framing, oversized records, reader failure, cancellation and row-number overflow.
After termination, further calls return `None`. Callers must retain each error
and its continuation status; EOF is not an aggregate import-success result.
This layer never commits valid rows or declares a file partially successful.
Collection-level duplicate/conflicting PuzzleIds and confirmed preview/apply
remain with their designated parent owners.

## Historical child handoff

The 16 source contract tests were authored and exported before production source
creation to the requested absolute path:
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-csv-import-tests.patch`.
The patch was subsequently refreshed with formatting and a precise multiline
cancellation trigger. Fixtures assert independently fixed FENs after black setup
and the full solution, white setup with nonzero counters, underpromotions,
old/current/reordered headers, unknown UTF-8 fields with commas, escaped quotes
and mixed multiline endings, absent versus explicit-zero metadata, malformed
headers/widths/quotes/UTF-8, invalid identity/integer/FEN/setup/late moves,
exact/overbound encoded multiline rows, continuation policy, injected read
failure, early/mid-row cancellation and no next-record read before a result.

Only the installed standalone formatter at
`/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt` was used, matching the repository's
1.99.0 toolchain pin. Owned Rust files passed its edition-2024 formatting check.
Static patch whitespace and source-content checks were performed. No Cargo,
build, test, Clippy, engine verification, CPU diagnostics or runtime benchmarks
ran in this slice, as the parent reserved the build lane. There are 16 authored
tests and zero executed tests here. A missing new API was never reported as a
semantic red result. Compiler, lint and behavior acceptance remain pending the
parent's single registration and verification pass.

## Parent integration and review

The parent integrated this slice after saved-workspace checkpoint
`8c53e89a039b29c826e7677dc3e5e060032b37bc` and registered the source module plus
the public domain suite once. The initial 16 tests passed. A source review found
that the atomic-flag API could not consume the existing executor cancellation
token, which also observes shared capacity revocation. The callback API now
borrows that real token directly.

Review also identified a preview/persistence discrepancy. A real legal knight
cycle with 4,097 solution plies was accepted by CSV, although persistence refused
that definition. The parent observed the regression fail before repair. The
existing limits now live on `PuzzleDefinition`, with typed tree-limit errors,
and CSV and persistence reuse that owner. The regression now excludes the whole
row with `NextRow` and preserves the following valid row. A 4,096-ply positive
case enters the actual persisted-run constructor while retaining complete source
move metadata.

All 143 selected puzzle domain tests passed after the callback/limit repair and
positive boundary case. The two additional real executor cases then passed with
the complete 20-case CSV selection. One cancels the admitted job; one revokes its
shared capacity phase while the reader is held at a controlled multiline scan
boundary. Both return typed terminal cancellation, observe worker join and
executor shutdown, and restore all capacity before final assertions. The fixture
uses an awaited notification instead of polling. No new runtime or cancellation
watcher was introduced. The initial focused Clippy selection passed before the
two executor cases; final canonical verification remains pending.

A second review found the same preview/persistence discrepancy in identity
length. The 1,024-byte multibyte identifier positive case already entered the
real persisted run, but a 1,025-byte identifier incorrectly entered valid preview,
and direct identity construction accepted oversized source/version/puzzle
components. The parent observed one passing and two failing regressions before
repair. `PuzzleDefinitionIdentity` now owns the existing 1,024-byte token policy
and typed field/length errors. Persistence reuses that owner and keeps its
historical budget-error result. No persistence schema or accepted stored format
changed. Source identity, version and PuzzleId are measured in UTF-8 bytes.

After that repair, the complete all-feature library/domain/storage selection
passed 242, 321 and 52 cases: 615 passed, zero failures, ignored cases or filtered
cases. It includes all 22 CSV cases and the direct identity-boundary regression.
The parser audit found no additional definition/persistence acceptance mismatch
within this slice. Final canonical verification remains pending.

## Accepted canonical verification

Final `make verify` exited successfully. All three Clippy selections passed, and
all 1,228 tests passed with zero failures, ignored cases or filtered cases.
The all-target optimized build passed in 4 minutes 49 seconds; the separate
optimized native-harness build passed in 49.48 seconds. These are gate timings,
not parser throughput or memory measurements. The native harness was built and
linted without executing GPU/OS acceptance. The existing macOS debug linker
`__eh_frame` size warning remains visible and unsuppressed.

Every Cargo invocation used the root shared target directory, default detected
parallelism and sequential gates. No dependency, manifest, lockfile, toolchain,
vendor or checker change was needed. T263 and T269 are accepted. Source
acquisition, shared preparation selection, catalogs, installation and production
Start retain their separate tasks.

Exact commands, raw logs and verified hashes are retained in
`.git/parallel-patches/puzzle-csv-integration-20261017/`.

| Log | SHA-256 |
| --- | --- |
| `puzzle-csv-line-bound-red.log` | `2b8c9edaa255e2c1333fc50897ccc2ea3d64ff986a35e866f3515d20c531a23a` |
| `puzzle-csv-identity-bound-red.log` | `58afb6c3efe3a2c75854d3c879b5de966387c9c2e71e35fb0eb864d94d7aad22` |
| `puzzle-csv-combined-tests.log` | `91aeaa41066023b0e68149922641f97f89ee3b80e45560ae2acd2aabb69086e4` |
| `puzzle-csv-make-verify.log` | `d96792b22093da7ea44d3d9c70a67698d69bc4c0e1fcd96607baee72ad11d78d` |
