# Opening corpus, query and statistics source handoff

Baseline: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68` in
`/Users/sadrashameli/.t3/worktrees/chess/t3-daa5ec4a`. The checkout was clean at
admission. This is a source-only T165/T167/T168 foundation, not acceptance of
those entire tasks or FR-019.

The contract is [FR-019](../spec.md#functional-requirements), the
[opening data model](../data-model.md#reports-openings-and-puzzles),
[explorer decisions](../decisions.md#book-generation-and-explorer-semantics), and
[study-tools explorer contract](../contracts/study-tools.md#opening-explorer).
The constitution, all seven canonical engineering rules, applicable instruction
bridges and user-level rule scopes were read before implementation.

## Exact ownership

All nine owned paths are new:

- `src/opening/corpus.rs`
- `src/opening/corpus/limits.rs`
- `src/opening/query.rs`
- `src/opening/statistics.rs`
- `tests/opening/corpus.rs`
- `tests/opening/query.rs`
- `tests/opening/statistics.rs`
- `tests/opening/fixtures.rs`
- `specs/001-reckless-desktop-chess/evidence/opening-corpus-query-statistics.md`

No crate/module registration, test root, existing opening module, storage module,
schema, manifest, lockfile, task marker or existing source was edited. Parent
opening/Polyglot source was inspected read-only.

## API and retained ownership

`OpeningCorpusRecord::new` accepts `OpeningCorpusRecordInput`, explicit
`OpeningCorpusLimits`, and an injected cancellation callback. It borrows the
actual immutable start, `BoardMove` mainline, `GameMetadata`, and optional raw
PGN `TimeControl`. It retains the canonical `NotationGameIdentity` validated
against those exact boards and moves. Missing result and explicit unfinished
result remain distinct identity inputs. No new notation parser, chess rule,
normalizer, or identity serialization is introduced.

`OpeningCorpus::new` binds a validated
`StorageDatasetPackageSourceIdentity` to unique nonzero source game IDs, declared
metadata support and validated records. Empty and legal-mainline corpus states
are explicit. Failed record/corpus construction returns typed errors rather than
admitting invalid records. The source identity is the existing packaging value
contract, also consumed by the parent's book module; no storage effect is called.

`OpeningQuery::new` binds the selected `BoardPosition` to typed filters.
`OpeningStatistics::query` accepts the selected corpus slice as the dataset
filter, explicit limits and a cancellation callback. The callback can call
`CapacityExecutorCancellation::is_cancelled`; this layer owns no executor or
worker. `query_with_hasher` admits a supplied `BuildHasher` and supports real
collision fixtures. The normal path uses a deterministic standard-library hasher.

Results borrow all source records and source identities for their full lifetime.
Provenance includes each source/version/game ID and access to the original record.
The result retains selected sources even when there are no matching games.
Sample pages borrow the immutable result and allocate no page buffer; each page
contains at most 50 games. Opening a sample remains an application responsibility.

## Query and aggregation semantics

Position matching uses full `BoardPositionIdentity` equality. Counters are
excluded by that owner; variant, placement, turn, castling and legal en-passant
semantics remain there. Traversal stops at the first matching occurrence in each
source record, including an ending occurrence. Continuations are actual typed
source moves and are checked by the selected board's canonical `play` operation.

Deduplication buckets on the actual `NotationGameIdentityStructuralKey` in a
full-key-equality map and then calls canonical `compare`. Only `Exact` merges.
`Potential` pairs stay separate and their count is exposed for a preview warning.
Potential matching is never transitive equality. Every exact alias in the
selected corpora retains its provenance, including aliases excluded by an active
metadata filter. A group contributes only if at least one actual source record
passes every active filter. Missing metadata from different aliases is not
combined to make a record pass Both Players or another active filter.

Source identity then source game ID orders aliases before processing. The first
eligible alias supplies the representative sample record. Each side's rating
uses its first available supported value among eligible aliases, once per unique
game. Differing available ratings use that same deterministic order, and all
original values remain accessible through provenance. Result metadata likewise
uses an available supported value from eligible exact aliases.

Player filters call `NotationGameIdentity::canonical_player` and match canonical
names exactly for White, Black or Either. Ratings are inclusive unsigned ranges,
with Both Players as the criterion's default; White, Black and selected-position
Side to Move are explicit alternatives. Required missing/unsupported metadata
excludes the source record while a filter is active. Raw time controls use exact
source-tag equality; this layer does not reinterpret them as live clock controls
or reuse preparation-only time classes.

Date intervals require complete validated endpoints. A partial source date
matches only if some complete calendar date consistent with its known components
overlaps the interval. Every candidate date is validated by `GameMetadataDate`,
including leap days and noncontiguous partial dates. There is no separate
calendar-validity implementation. Completely unknown dates are unavailable.
Active-filter partial-date matches have their own count and sample disclosure.

Outcome shares are exact numerator/nonzero-denominator values over known
completed results. Unknown and unfinished results have a separate count; a
zero-known-result denominator yields unavailable shares. Move frequency uses
matching unique games with a recorded next move. Games ending at the first
occurrence have a separate count. Available ratings expose exact sums and
nonzero per-side sample counts; absent ratings are not zero. Zero and `u32::MAX`
source ratings remain valid available values.

Continuation order is descending game count, then canonical variant-aware UCI.
Samples order by newest known date components, then canonical identity fields,
then source/game identity for a total deterministic order. Unknown date
components sort after known components. Pagination offsets apply to one retained
immutable result, so they cannot silently rebase to a replacement snapshot.

## Bounds and cancellation

The caller injects nonzero maximum source records, plies per record, reserved
bytes and work steps. Admission checks those limits explicitly. Query traversal,
metadata-date search, duplicate comparisons and bottom-up merge sorting check
cancellation and consume work steps. Pathological incomplete-date or duplicate
buckets fail with a typed work-limit error instead of returning partial data.
No wall-clock, sleep, filesystem, SQLite, index builder, engine, UI or service is
used by this implementation.

Vector growth is geometric, fallible and charged before reservation. Hash-table
reservation and canonical text/move storage use conservative explicit charges.
Charges accumulate across temporary allocations; released scratch space does not
refund admission in the same operation. Borrowed input buffers and caller-owned
filter/source values are outside this operation's allocation budget. This is an
allocation-admission model, not a measured allocator/RSS ceiling. No peak-memory,
latency or throughput claim has been established.

The existing canonical `NotationGameIdentity::new` has no cancellation callback.
It is called once per admitted bounded record, with checks before and after.
The injected ply and canonical-text reservation limits bound that unit, but do
not establish a 50 ms interruption guarantee. If finer interruption inside
identity validation is required, the narrow prerequisite is a cancellation-aware
constructor on the canonical notation owner. No replacement normalization or
identity validation was added here.

## Authored checks and evidence limits

The initial 17-case tests-first binary patch was exported before production
source as
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/opening-corpus-query-statistics-tests.patch`.
Its SHA-256 is
`62b2fabe34ecfb803ebd746541d92ab1211c6087eb54ca391bf8a9d9f8320656`.
The final suite contains 26 authored cases: six corpus, six query and fourteen
statistics cases. Later cases and final API refinements are in the final patch;
the initial tests-first artifact is preserved unchanged.

Fixtures use real boards, moves, metadata and source identities. Independent
expected values include five unique matching games, four next-move games, one
ending game, three known results, two unknown results, e4 frequency 3/4 and
per-side rating sums/sample counts. Other cases cover repeated positions, exact
cross-source aliases and every source game ID, forced hash collisions, potential
duplicates, partial/leap dates, missing/unsupported metadata, normalized player
names, rating criteria and boundaries, same-date identity ties, 50/1 sample pages,
source-version conflicts, counter/variant identity, mid-traversal cancellation
and injected limits.

All 26 tests are unrun. No failing executable baseline was observed. No Cargo,
compiler, build, test, Clippy, runtime, CPU diagnostic or agent was invoked.
Standalone rustfmt 1.10.0 from the locally installed Rust 1.99.1 package was used
only for source formatting; the project pins Rust 1.99.0, whose executable gates
remain the parent's responsibility. Formatting and Git whitespace/patch checks
do not establish compilation, behavior, performance or acceptance.

## Parent integration and remaining scope

The parent must declare `corpus`, `query` and `statistics` in its existing
`src/opening/mod.rs`, include the three focused test files from a discovered test
root, and execute the authorized pinned-toolchain gates. Its existing book
registration remains parent-owned. No new dependency is required by this slice.

T166 indexing, durable local game handles, repository/service admission, PGN
import adapters, label-definition/deepest-label lookup, background indexing,
dataset replacement, desktop commands and explorer UI remain outside this source
foundation. Full-scale indexed latency/memory evidence, 50 ms CPU chunks, platform
behavior and all executable T165/T167/T168 acceptance remain unverified. None of
the corresponding task markers was changed.

The final binary patch and JSON manifest are exported under the shared
`.git/parallel-patches/` directory with prefix
`opening-corpus-query-statistics`. The manifest records the full baseline, exact
nine paths, each file SHA-256 and both patch hashes. No commit or push was made.
