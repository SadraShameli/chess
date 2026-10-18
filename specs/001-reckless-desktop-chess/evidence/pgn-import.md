# PGN importer source handoff

Date: 2026-10-17. Scope: source-only T097 implementation and importer-focused
T093 contracts. No task marker or acceptance gate is changed by this handoff.

## Implementation boundary

`NotationPgnImport<R: BufRead, I: FnMut() -> Result<GameMoveId,
GameIdentityError>>` accepts an injected buffered reader and typed move-identity
callback. `next_game(&cancellation)` returns
`Result<Option<NotationGame>, NotationPgnImportError>`. Cancellation is an
injected `Fn() -> bool`; the component creates no runtime, executor, random
identity, session, file, storage transaction, or clock-state snapshot.

Each successful pull returns one complete validated game. Every SAN in every
branch passes through the existing `BoardPosition::parse_san` and `GameTree`
legality boundary. An invalid branch excludes its entire game. Recovery drains
tokens through a balanced top-level required result marker. Unterminated lexical
constructs, unmatched closing variations, cancellation, limits, or exhaustion
without a proven boundary terminate explicitly. Errors expose the encountered
game ordinal, zero-based byte offset, one-based line and byte column, typed
reason, and `NextGame` or `Terminated` recovery. A terminated importer returns a
typed terminated error on subsequent calls.

`NotationGameParts` supplies `BoardStart`, `GameTree`, explicit mainline tip,
`GameMetadata`, the existing T099 `NotationGameIdentityEnumResult`, separate
optional Result-header presence, optional source TimeControl text, typed
per-node `ClockDuration` values, and typed warnings. `NotationGame` validates
root/start agreement, the tip path, clock identities/uniqueness, Result-header
agreement, and unavailable-rating conflicts. The constructor permits a tip
with children; export selection owns any representability refusal. Clock lookup
uses a node index. No second semantic-result enum is introduced.

Unknown tags retain decoded source values and source order. Known numeric Elo
requires nonempty ASCII decimal text fitting u32. A missing rating tag remains
absent. An unavailable rating retains its exact decoded value in
`NotationGameEnumWarning::UnavailableRating { color, value }`, with known Elo
absent. Source-record bytes/ranges and later dataset preparation remain separate
work; this does not complete T268.

Notes, NAGs, all four marking colors, circles/arrows, root annotations, clock
precision through nine fractional digits including u64::MAX nanoseconds, and
nested variations have source contracts. Repeated continuation occurrences
that the existing tree would coalesce return `UnsupportedOccurrence`; they never
overwrite another occurrence's annotation data. Repeated equivalent clock
directives can share one typed value; conflicting clocks fail explicitly.
Unknown directives remain literal plaintext with typed warnings. A malformed
directive prefix stops at the next opener so it cannot swallow a later valid
directive.

## Evidence-based parser refinement

The plan/research recommended pgn-reader 0.29.1 Reader. Read-only predecessor
evidence and the installed pinned source show Reader success is insufficient
for this contract. `reader.rs::read_tags` skips malformed tag forms;
`read_movetext` ignores failed NAG conversions and failed SAN prefixes, skips
semicolon comments, and reaches successful completion without requiring a
closed RAV. Relevant installed source locations are `reader.rs` lines 263-343,
449-452, 550-563, 610-627, and 630-664. A permissive Reader followed by another
validation pass would give two lexical owners and could lose comments before
validation. The implementation therefore has one strict bounded lexical owner,
uses pgn-reader's valid `Nag::from_ascii` helper, and delegates every chess rule
to the existing domain/shakmaty integration.

A second existing dependency defect was found during static review:
`pgn-reader-0.29.1/src/tag.rs::RawTag::decode` iterates adjacent backslashes as
overlapping escapes. Four raw backslash bytes decode to one literal backslash,
where PGN escape pairs require two. A tests-only direct helper probe was sent
to the parent before changing production behavior. The parent confirmed an
actual semantic red with pinned rustc 1.99 against its built pgn-reader 0.29.1
rlib: exit 101, zero passed, one failed, zero filtered; actual one backslash,
expected two. The historical probe and logs are under
`/tmp/claude-chess-pgo-diagnosis-20261016/upstream-raw-tag-probe.rs`,
`upstream-raw-tag-probe-compile.log`, `upstream-raw-tag-probe-red.log`, with
the parent's exact command JSON alongside.

After that observed failure and explicit parent authorization, tag scanning
was refined to append each validated decoded escape byte directly to one
bounded buffer. It avoids the defective RawTag codec and a second allocation.
Vendor code and checkers remain untouched. The permanent importer regression
covers four raw backslashes and adjacent quote/backslash runs. The standalone
upstream failure remains historical evidence rather than a failing canonical
test. No importer semantic green is claimed here.

## Comment interchange convention

Brace comment backslashes are literal and the first closing brace terminates
the comment. No invented brace-escape grammar is used. Arbitrary brace-bearing
note text is represented by standard semicolon comments. Their payload excludes
the opening semicolon and terminating CR/LF. Consecutive comment occurrences on
the same node join with exactly one newline and no trimming or added spaces;
empty first/final payloads preserve leading/trailing newlines. Recognized clk,
csl, and cal directives can share a comment payload with notes. Removing a
directive preserves every remaining literal byte. This convention was accepted
by the parent and relayed to the disjoint exporter writer.

## Bounds and cancellation

The private typed policy fixes production defaults at 20 GiB actual consumed
input, one million encountered games, 100 million encountered plies across all
branches and excluded games, 100,000 plies per game, 64 variation levels, 1 MiB
decoded fields/assembled notes, and a combined 64 MiB retained decoded and
pending buffer budget. All accounting increments use checked arithmetic.
Whitespace, escape-line content, comments, and error-drain content consume the
actual byte budget. EOF is determined from the reader; `read.take` is not used
to disguise overflow. The importer stops on policy-limit failure.

Pending text and retained notes reserve bounded geometric capacity explicitly
after accounting and before growth. Tag decoding uses the same pending buffer
owner; notes include their canonical separators in field limits. Metadata
transfer and cloned duplicate-detection names are charged conservatively;
NAG, marking, and warning vector capacity growth is charged before reservation.
The decoded budget is not a claim about total process RSS, native board/tree
storage, caller-retained returned games, or allocator overhead. Domain tree
growth is separately bounded by encountered plies. The caller owns its reader's
buffer capacity and scheduling/admission.

Small private policies exercise exact boundaries and overflow without huge
allocations; production defaults are not public or weakened. Cancellation is
checked while reading, skipping ignored input, draining errors, parsing
directives/markings, and finalizing annotation nodes. Timing and throughput
remain unmeasured.

This is in-memory parsing/staging. T100 still owns physical staging, disk-space
estimation/admission, immutable previews and duplicate decisions, cancellation
cleanup, confirmation, atomic commit, and preservation of existing library data.
No persistence-space acceptance is claimed for T097 by this source handoff.

## Tests and verification status

There are 31 authored canonical tests: 23 public-contract cases in
`tests/notation/pgn_import.rs`, four private importer-policy cases, and four
private budget cases. All 31 are unrun in this writer. Initial public contracts
and private policy tests preceded production; focused cases were expanded as
the source contract and static review developed. The existing dependency bug
received the parent's actual red before the boundary mitigation.

Covered fixtures include annotated nested branches, ordered unknown tags,
Result-header absence/agreement, black-to-move starts, both castling variants,
promotion, en passant, escaped tags, literal brace/backslash notes, exact
semicolon assembly, unknown/malformed directive prefixes, root annotations,
nanosecond clock precision, unavailable source ratings, illegal branches,
malformed tags/NAGs/delimiters, repeated occurrences, cancellation, byte-exact
locations, recovery, excluded-game totals, every typed budget boundary,
arithmetic overflow, and rejection before buffer allocation.

The exact standalone formatter is
`/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt` from Rust 1.99.0, version
1.10.0. Formatting/parsing, its check mode, and static staged diff checks are
the only writer verification. Cargo, builds, tests, Clippy, runtime diagnostics,
commits, pushes, agents, and additional worktrees were not run by this writer.
The parent's earlier T099 checks are not imported as evidence for these files.

Parent registration is required: export `notation::game` and
`notation::pgn_import` alongside the existing `notation::game_identity`, and
include `tests/notation/pgn_import.rs` from the appropriate discovered test root.
Private tests are already owned `#[cfg(test)]` modules in the importer and budget
files and become discoverable through source-module registration. No module
root, Cargo dependency/configuration, discovered test root, or task artifact is
included in this patch.

## Parent integration and observed regressions

The parent registered the aggregate/importer and the disjoint exporter/FEN owner
after `19b44cf`. Combined library/domain compilation passed before source
corrections. A subsequently added parent test initially had a fixture-only
lifetime error; it was corrected before the semantic baseline and was not
reported as a semantic red.

Independent review produced three adjacent-marker cases (`1.e4*`, `1.e4$1*`,
and `*1.e4*`) and a failed-recovery case combining an illegal `Ke3` branch with
an unterminated comment. Against frozen source, all four failed: the lexer
consumed self-delimiting markers, while failed drain replaced the original
illegal-move cause and location. The baseline passed eight private importer
cases and 100 selected domain cases, with four domain failures. The broad
`notation` filter also selected existing identity and three game cases; these
counts are not a claim that every selected case was newly authored notation.

After those observed failures, the lexer gained both direct marker dispatch and
the marker delimiter. Failed recovery now preserves typed primary/recovery
causes and both locations, with terminal recovery and nested cancellation still
observable. Three private typed cause checks were added. The unchanged public
regressions then passed: 11 private importer cases and 104 selected domain cases,
with zero failed or ignored. The same run filtered 242 other library and 318
other domain cases. Follow-up read-only review found no remaining concrete
blocker in that repair.

Private staging/traversal grouping and explicit error mappings corrected the
reported Clippy findings without changing checker configuration. The subsequent
all-target, all-feature Clippy selection passed with `-D warnings`. Final
canonical verification and task acceptance remain pending at this entry.

The exact upstream escape probe, compile logs, fixture-only failure, four-red
baseline, repaired tests and Clippy log are preserved with verified checksums
under `.git/parallel-patches/notation-pgn-integration-20261017/`.

## Canonical checkpoint

The final command
`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify`
exited zero. Formatting, all three canonical Clippy selections, 1,403 tests and
both optimized builds passed. Tests had zero failed, ignored or filtered cases.
The all-target release took 4m53s; the separately named native harness release
took 49.15s. Every Cargo command used the shared target and default internal
parallelism with sequential gates and no job/thread caps. The existing macOS
debug-linker unwind warning remained visible and unsuppressed; Clippy was clean.

T093's interchange fixtures are accepted at this checkpoint. T097 remains open
for its complete physical staging/space acceptance with T100; the parser and
in-memory staging owner are implemented and verified. The separate actual native
readback attempt failed and does not become a passing GPU result through these
nonexecuting build checks. No import preview, disk-space admission, atomic
library commit, source acquisition or throughput result is claimed here.

## Exact source ownership

The staged patch owns only the five files below plus this evidence file.
The evidence-file digest and patch digest are supplied in the final parent
handoff to avoid a self-referential digest. SHA-256 values for source and tests:

```text
42676c988c743748255b6a1bbf6dd3fbf117827a13e3987b3d4ec86de840fc78  src/notation/game.rs
e0101805bc589ac48e5d86b15fd92c1501596d8070f85aa2f01abef7381086fb  src/notation/pgn_import.rs
451ca836fe6176e5c35e8e421d44365d245e0164f8c881899a6f44c350483f0f  src/notation/pgn_import/lexer.rs
781cc0572b94d7dc163b02490a33df19f63523d07de63154cf91cbbf66421d32  src/notation/pgn_import/budget.rs
c0e732f1fa457caec192d07e6782625cb367a3ef06861eafb5451d3d0b43500d  tests/notation/pgn_import.rs
```
