# T099 game identity source handoff

This source-only increment starts at `8c53e89` and implements the pure identity
owner required by [T099 and T268](../tasks.md). Parent integration and task
acceptance remain pending. Only `src/notation/game_identity.rs`, its mirrored
contract tests, and this evidence file are owned by this increment.

The design follows the [collision-checked identity decision](../decisions.md#book-generation-and-explorer-semantics),
the [library model](../data-model.md), and the identity fields consumed by
[subset selection v1](../decisions.md#starter-subset-selection-v1).

## Supported API

`NotationGameIdentity::new(&BoardPosition, &[BoardMove], &GameMetadata,
Option<NotationGameIdentityEnumResult>)` returns a validated identity or
`NotationGameIdentityError::Mainline { index, source }`. The index is zero-based
and retains the actual `BoardMoveError`. Every move is played against its real
prior board, including its side to move. No parser, repository, runtime,
filesystem, clock, logging, or source-provenance effect is introduced.

`structural_key()` returns `&NotationGameIdentityStructuralKey`. This value owns
the cached `variant()`, full canonical `initial_fen()`, and ordered
`mainline_uci()`. It supports `Eq`, `Hash`, and `Clone` for candidate buckets.
Candidate lookup must use this structural key, then `compare()` against each
full candidate. Full-metadata hashes alone cannot discover missing-versus-known
metadata matches. Hash collisions require ordinary full-key comparison and
must never authorize a merge.

The identity exposes `white()`, `black()`, `date()`, `result()`, and the cached
`position_identity()`. All fields are private. Canonical values are computed
once at construction, and getters borrow or copy their typed values. The
structural values are owned only by the structural key, with no forwarding
aliases or duplicate canonical strings on the identity.

`position_identity()` reuses `BoardPositionIdentity` for the initial position,
including legal en-passant context and castling rook squares while excluding
counters. Full game identity retains all six canonical start-FEN fields,
including both counters and the board owner's en-passant notation. Thus equal
position identities can belong to different full game identities. Standard and
Chess960 remain distinct even with identical FEN and move strings. FRC and DFRC
both use `BoardEnumVariant::Chess960`; generation provenance cannot affect
identity. UCI uses the actual variant, retaining king-to-rook Chess960 castling
and promotion suffixes.

## Metadata policy

Player normalization is Unicode trim, NFD, default non-Turkic full case folding,
then NFC. Absent names, empty or Unicode-whitespace-only names, and a trimmed
PGN `?` placeholder become missing. Internal whitespace and accents remain
meaningful. This explicit boundary policy applies only to identity: borrowed
validated `GameMetadataText`, known tags, dates, and extension display strings
are preserved unchanged.

Dates reuse `GameMetadataDate`. `None` and a date with all three components
unknown canonicalize to `None`; every known partial year, month, or day is
retained. Result is `Option<NotationGameIdentityEnumResult>` with `Unfinished`
and `Completed(GameEnumResult)`. Its `pgn_token()` owns the mapping to `*`,
`1-0`, `0-1`, and `1/2-1/2`. Missing result remains distinct from asserted `*`.
No shared game or metadata type is changed. Hashing uses existing date getters
and the result token, without adding a serialization format.

Strict `Eq` compares the entire canonical value. `Hash` respects that equality.
`compare()` returns `Different` for structural differences or conflicting
mutually known identity metadata. Compatible pairs are `Exact` only when both
players, every date component, and a result are known on both sides; otherwise
they are `Potential`. Asserted `*` is known. Potential matching is deliberately
nontransitive and is never used as equality. No automatic merge, Skip, or Keep
Copy policy belongs to this owner. Nonidentity metadata, annotations, source
IDs, generation provenance, and clocks are excluded.

T268 owns the separate game/puzzle preparation byte encoder and SHA ordering.
It can read the structural getters, canonical player getters, date component
getters, and result `pgn_token()`. This increment introduces no preparation
tuple serializer, SHA encoder, variant-string alias, or dataset effect.

## Dependency and API evidence

The authorized design pins `icu_casemap = '=2.3.0'` and
`icu_normalizer = '=2.3.0'`, each with `default-features = false` and
`features = ['compiled_data']`. The parent owns manifest/lock edits after the
CSV gate, transitive dependency auditing, and compiler validation. This child
installs and vendors nothing.

Read-only inspection of ICU4X commit
`a14f2dad852be26bad277ae704abf27a15cbfed1` confirmed the public borrowed
constructors and their `compiled_data` feature gates:

- [CaseMapper::new and fold_string](https://github.com/unicode-org/icu4x/blob/a14f2dad852be26bad277ae704abf27a15cbfed1/components/casemap/src/casemapper.rs)
- [DecomposingNormalizer::new_nfd, ComposingNormalizer::new_nfc, and normalize](https://github.com/unicode-org/icu4x/blob/a14f2dad852be26bad277ae704abf27a15cbfed1/components/normalizer/src/lib.rs)

The parent's supplied design evidence reports tag `icu@2.3.0`, Rust MSRV 1.88,
Unicode-3.0 licensing, and no OSV advisories for either selected 2.3.0 crate
with compiled data. Those audit claims were not independently rerun here.
The implementation uses the consistent ICU normalization/casefold release
instead of mixing caseless 0.2.2 Unicode 16 with normalization Unicode 18.

## Contract retention and verification limits

The first 20 independent contract tests were formatted and exported while
`src/notation/game_identity.rs` did not exist:

`/Users/sadrashameli/Personal/chess/.git/parallel-patches/notation-game-identity-tests.patch`

SHA-256: `e66286d46a07babd5b72da185c283135e16a7795901016be682dad2e75a54595`.
Three supplementary tests were added during static review to isolate equal-FEN
variant differences, nonstandard FRC/DFRC provenance with actual moves, and
different legal orders reaching the same final position. There are 23 authored
test functions. All use real board and metadata objects, with no mocked rules.

Fixed contracts cover NFC accents, Unicode whitespace, Straße, sigma,
non-Turkic I classes, placeholders, idempotence, raw display preservation,
canonical FEN syntax and counters, turn/castling/en passant, both variants,
FRC/DFRC, Chess960 castling and promotions, invalid/reordered/late-invalid
mainlines, each metadata component, partial/missing dates, all result tokens,
missing versus asserted `*`, nontransitivity, excluded metadata, forced
constant-hasher collisions, and reachable structural candidate buckets.

Standalone Rust 1.99.0 rustfmt and static source/patch checks were run. No Cargo,
build, compiler execution, test execution, Clippy, CPU diagnostics, dependency
mutation, module/test registration, task-marker edit, commit, push, agent, or
additional worktree was performed. The missing new API was not presented as a
semantic red test. No runtime correctness or passing test count is claimed.
Parent registration, dependency resolution/audit, semantic validation, canonical
gates, and final T099 acceptance remain required.

The complete three-path handoff is exported to:

`/Users/sadrashameli/Personal/chess/.git/parallel-patches/notation-game-identity.patch`

## Parent integration on 2026-10-17

The parent integrated this owner after `f89440f`, registering the notation module
and mirrored contracts through the existing domain test root. The initial
23-case run passed. A further regression demonstrated that individually valid
partial dates `2024.??.31` and `2024.02.??` incorrectly matched Potential.
The frozen implementation failed that assertion before the repair. Matching now
validates the union of known components through `GameMetadataDate::new`.
Symmetric February/31, April/31 and 1900 leap-day conflicts are rejected, while
2000 and 2024 leap-day possibilities remain Potential. All 24 focused identity
contracts then passed, with 321 other domain tests filtered. Targeted
all-feature library/domain Clippy passed without warnings. The read-only final
review found no remaining concrete T099 blocker.

The two ICU 2.3.0 direct dependencies were resolved into the lockfile. Comparing
the prior lockfile with the current one confirms 23 added packages and no removed
or upgraded existing package version. The resolved `icu_provider` is 2.3.1;
the other principal ICU and data packages are 2.3.0. The selected ICU crates
declare Rust 1.88 and use Unicode-3.0 licensing. The remaining additions use
Unicode-3.0, MIT, or MIT/Apache-2.0 alternatives. Their actual APIs compiled on
the project's Rust 1.99.0 pin. Stable source verification used tag `icu@2.3.0`
and commit `a14f2dad852be26bad277ae704abf27a15cbfed1`.

An OSV batch query for all 23 exact added versions returned no advisories at
inspection time. This is a point-in-time dependency check, not a general
security guarantee. Exact versions, checksums, licenses, declared MSRVs and
query endpoint are preserved in `game-identity-dependency-audit.json`.

Captured initial, regression-red, repaired focused-test and targeted-Clippy logs,
plus the dependency audit and verified SHA-256 manifest, are retained under
`.git/parallel-patches/game-identity-integration-20261017/`.

The final command was `env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify`.
It exited zero. Formatting and all three canonical Clippy selections passed;
1,252 tests passed with zero failed, ignored or filtered tests. The all-target
release build passed in 5m57s, followed by the separately named native harness
release build in 52.43s. Every Cargo invocation retained the shared target and
default internal parallelism, with sequential gates and no thread/job caps.
The existing macOS debug-linker `__eh_frame` warning remained visible and
unsuppressed; Clippy reported no warnings. The harness was built and linted,
but GPU/OS acceptance was not executed.

T099 is accepted for the pure identity owner and matching contracts. T093,
T097/T098 interchange, T100 library preview and T268 preparation remain their
own implementation and acceptance work. No native library, automatic merge,
dataset acquisition or performance result is implied by this checkpoint.
