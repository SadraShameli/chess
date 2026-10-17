# Dataset selection foundation

Date: 2026-10-17. Status: source-only handoff, awaiting parent registration and
execution. This implements the shared foundation portion of T268 and authors
the corresponding T262 storage contracts. It does not complete either task's
cross-owner requirements, the master/broad adapter, or starter preparation.

The policy owner is
[dataset_selection](../../../src/storage/dataset_selection.rs), with focused
identity, classification, sequence and cap files. Its requirements come from
[starter subset selection v1](../decisions.md#starter-subset-selection-v1),
[the preparation contract](../contracts/study-tools.md#starter-preparation-reproducibility),
[the data model](../data-model.md#selected-starter-data-direction), and
[the canonical rule catalog](../../../.specify/rules/README.md). All seven
canonical rules and the constitution were loaded for this implementation.

The encoder consumes the current parent `NotationGameIdentity` getters, including
its date-intersection repair, and `PuzzleDefinitionIdentity`. The existing
`BoardEnumVariant` serialization and identity result tokens remain authoritative.
Game tuples encode absent dates as three null components and distinguish an
absent result from `*`. Puzzle identifiers use exact UTF-8 bytes without source
or version. Candidate digests are cached once. The narrow `PackageDigest`
extension provides slice hashing, byte access, ordering and crate-local digest
construction; its existing reader behavior remains in place.

Classification reports both per-player rating values or typed Missing/Invalid
reasons, time-class Unavailable/Malformed/Overflow/MultiStage/Hourglass/Slash
reasons, and missing/empty primary-theme reasons. Known puzzle bands carry
validated numeric indexes and checked endpoints. Typed Unknown themes sort after
every known token, including a literal `Unknown` token. Choosing the minimum
token is insensitive to exact duplicate tokens without allocating a copied
theme set. Callers retain every raw rating, TimeControl, title, theme and source
record independently of these preparation labels.

Candidates contain generic opaque record handles, encoded identities and typed
strata. They do not own game trees or source corpora. The adapter resolves
duplicates through the shared strict-identity comparator before constructing a
sequence. For games, Replace selects the smaller original UTF-8 record; Keep
retains the existing representative; Identical admits one copy. A differing
record for the same PuzzleId returns a typed conflict. Distinct incomplete
identities remain distinct even when the notation owner would report a potential
duplicate. The adapter retains all provenance and classifies the chosen game
representative. The sequence refuses unresolved duplicate identities.

The sequence sorts each observed flat pair by digest bytes and then identity
bytes, takes equal turns across ordered nonexhausted strata, and truncates at
the subset's upper target. Master uses one unstratified key. Smaller selections
borrow prefixes, with checked count/length framing and incremental SHA-256.
Available stratum counts remain ordered. Source adapters own executor admission
and any parallel candidate preparation; the pure core imports no filesystem,
acquisition, executor, dataset-package or GameSession owner.

Installed artifact identities and actual/fixed byte lengths are supplied by the
builder. Inventory validation refuses duplicate and unclassified assignments,
invalid fixed lengths and checked-sum overflow. The builder must enumerate the
complete installed output inventory, including indexes and provenance, and
provide one canonical identity per file. The core cannot discover omitted files
or aliases without an effect boundary. Component inspection follows the policy
order. A reduction records before/after prefixes and affected rebuilds; both
master and book reductions rebuild master plus book. Required-empty subsets,
labels overflow and fixed overhead exceeding a component ceiling are explicit
errors. Fixed overhead equal to the ceiling permits a checked rebuild. The
builder must rebuild and supply a fresh measured footprint after each reduction.
The core performs no build or I/O.

[The storage suite](../../../tests/storage/dataset_selection.rs) contains exactly
31 authored `#[test]` functions. The initial 26 contracts were authored before
the first core source edit. Five additional contracts were authored during
static review; the theme-reason extension was also specified in tests before
its implementation. Zero tests were executed and zero semantic failures were
observed in this checkout. Missing module registration or a missing dependency
API is not reported as a semantic red. No pre-existing behavior repair is
included.

The fixtures cover literal identity encodings, independent SHA values, canonical
names/escaping/variants/moves, exact result/date absence, rating and time edges,
u32 maximum inputs, ASCII versus Unicode whitespace, title/broadcast eligibility,
theme ordering, strict duplicate representatives and puzzle conflicts, digest
ties, flat allocation, exhaustion/remainders, simulated worker-completion
permutations, master/broad ordering, prefix stability, framed digests, inventory
accounting, ordered reductions, book coupling and refusal paths. Literal SHA
values were independently derived with OpenSSL from fixed bytes, including
explicit binary framing. No selector implementation is copied into fixtures.

Executed verification is limited to standalone rustfmt 1.10.0 from the installed
Rust 1.99.0 toolchain, formatter checks, static source/diff inspection and staged
whitespace checks. No Cargo command, build, test, Clippy run, CPU diagnostic,
agent, extra checkout, commit or push was performed. Compilation, test execution,
full task acceptance and all performance/installed-size evidence remain with
the parent. No source acquisition or prepared dataset counts are claimed.

Parent integration must register `pub mod dataset_selection` in the existing
storage module and include `tests/storage/dataset_selection.rs` through the
Cargo-discovered storage test root. The parent owns the current notation
dependency, manifest integration, adapters and canonical gates. Shared roots,
Cargo files, tasks, schema, dataset-package code and tools are excluded from
this patch.

## Parent integration and manifest boundary

The parent applied the source-only foundation after `74a8c31`, registered its
public storage module and mirrored suite, and observed all 31 focused contracts
passing. Targeted all-feature library/storage Clippy also passed. An independent
read-only review found no concrete blocker in encoding, classification,
duplicate decisions, round-robin prefixes or ordered cap transitions.

The preparation-manifest consumer then required validated input for the
canonical policy keys. The owning selection types now deserialize their closed
subset/component/stratum vocabulary directly. Nested stratum fields refuse
unknown fields. Puzzle band indexes must correspond to a possible u32 rating;
primary themes must be nonempty individual tokens under the policy's ASCII
whitespace rule. Unicode whitespace within an otherwise individual token is
preserved. Observed stratum counts route through the owning constructor and
reject zero, negative, overflowing and unknown-field inputs. Prefixes,
footprints, reductions and identities gained no unchecked deserializer.

Four independent external-JSON contract cases preceded these new APIs. All 35
focused selection cases passed afterward, with 52 unrelated storage tests
filtered; targeted all-feature library/storage Clippy passed again. Missing
new constructor or deserializer APIs were not claimed as semantic reds.

The final canonical command was
`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify`.
It exited zero: formatting, all three Clippy selections, 1,287 tests and both
optimized builds passed. Tests had zero failed, ignored or filtered cases.
All-target release took 5m07s; the separately named native harness release took
49.33s. The existing macOS debug-linker unwind warning remained visible and
unsuppressed; Clippy was clean. Exact command records, logs and a verified
SHA-256 manifest are retained in
`.git/parallel-patches/dataset-selection-integration-20261017/`.

T266 schema and manifest integration, T268's game adapter, T270's puzzle adapter
and T272's actual bounded builds remain separate. No full task is accepted by
this foundation. It does not acquire a corpus, enumerate real installed files,
measure performance, prove installed starter sizes or execute native acceptance.
