# Dataset package and preparation manifest type foundation

Status: T266 types only, source ready for parent integration. T266 remains pending.
No SQL, schema registration, migration runner, task acceptance, dataset acquisition,
prepared catalog implementation, or application integration is included.

The later parent-reviewed API below supersedes the initial per-record metadata
shape: each `SourceRecord` owns its raw fields, and provenance constructors take
an explicit representative index.

The implementation extends [the existing descriptor](../../../src/storage/dataset_package.rs).
Its original `new(format, checksum, absolute_location)` constructor and shared
executor `verify_integrity` API remain available. Byte integrity and semantic
validation are independent states. A semantic receipt records caller supplied
validation counts and must bind the descriptor's exact format and checksum;
creating a receipt does not run a notation, puzzle, or catalog validator.
`PreparedPuzzleCatalogV1` distinguishes a future internal catalog from CSV
interchange. This change does not implement that catalog format.

The source identity binds the exact source identifier, version, and acquisition
manifest digest. A frozen source also retains its acquisition origin, downloaded
checksum, source kind, applicable terms, and checksummed relative license-copy
reference. Source paths are not identity keys. Starter manifests require the
standard games, puzzles, and openings sources with CC0 declarations; acquisition
and review of the actual source URLs, frozen snapshots, license copies, and
checksums remain T267 work. The wire schema version is independent of SQLite
migration numbering.

`RecordProvenance::game` retains the real `NotationGameIdentity` and uses the
selection owner's full canonical identity encoding. Puzzle provenance retains the
real `PuzzleDefinitionIdentity` and its exact PuzzleId bytes without trimming.
Both constructors retain ordered raw tags or columns, including repeated unknown
fields, source-record identities and raw-record digests. Ratings, raw TimeControl,
and the complete raw Themes field remain available; classification and quality
reasons come from the selection owner. Conflicting duplicate values for classified
fields fail explicitly. Game provenance exposes the derived broad-game stratum;
T268 still owns titled-master eligibility and raw record normalization. T270 still
owns its CSV metadata bridge. The per-record serializer retains the canonical
selection bytes, digest, classification, raw fields, and source records.

The manifest stores checksummed provenance artifact references rather than
embedding per-record provenance. `Observation::from_sequence` consumes real
selection sequences and preserves their ordered core stratum counts, actual
prefix length, and framed digest. Counts mean:

- `observed = available + rejected + duplicates`, with checked arithmetic.
- `available` is the unique valid candidate count before target truncation.
- `unknown` counts available records in strata with an Unknown coordinate,
  counting each such record once. Master selection has a single known stratum.
- Rejection and duplicate quantities are supplied by the external preparation
  adapter. They are observations, never selection targets or fabricated sizes.

Unavailable metadata carries source, category, and reason. Duplicate findings
and an unavailable required source are refused in an accepted starter manifest.

All installed artifact descriptors carry a safe relative path, component, role,
unsigned 64-bit installed/fixed byte lengths, digest, and frozen source bindings.
The manifest validates license/provenance references, complete ordered subset
observations, ordered nonzero strata, count arithmetic, source bindings, portable
path collisions, file/directory conflicts, and duplicate references. It recomputes
component and total measurements from every declared artifact, including indexes
and provenance. Targets, component order, ceilings, framing, and prefix reductions
come from `StorageDatasetSelection`; no policy numbers or identity encoder are
duplicated here.

Reduction records are constructed by the core footprint transition. Parsing
replays each decision, validates its claimed component and before/after prefixes,
checks the history chain, and requires final measurements to pass the core cap
transition. All structured external records reject unknown fields. Validated
values deserialize through their constructors; core keys use the parent-owned
validated deserializers. There is no dynamic JSON value boundary.

Parsing establishes structural and reference validity. Call
`Manifest::validate_reproduction(subset, sequence)` against each independently
rebuilt frozen input sequence to verify membership and ordering. Parsing alone
does not read files, prove the acquisition manifest digest, measure filesystem
lengths, verify artifact hashes, or establish inventory completeness. T272/T277
must perform those effects, including counting any outer manifest/envelope without
attempting a self-referential checksum. The `Manifest` artifact role supports
externally checksummed manifest artifacts; no self-checksum construction is
claimed by this foundation.

## Source-only checks

The owned suite contains 26 tests: four existing integrity/executor cases and
22 new type/provenance/manifest cases. The initial 14 contract cases were authored
before production code. Eight further boundary cases were authored during static
review; source additions prompted by those cases followed them. No missing-API
compilation failure is claimed as a semantic red.

The cases cover validated source/license round trips, source/version/manifest
binding, malformed versions/counts/paths/references/digests/strata, unknown fields,
portable path aliases, file/directory conflicts, source unavailability, duplicate
findings, count overflow, prefix/cap refusal, reduction replay and round trip,
selection reproduction and changed membership, raw metadata preservation,
exact PuzzleId bytes, distinct prepared-catalog format, and independent semantic
and integrity states. Small fixtures use real notation/puzzle identities and core
selection values. Fixture byte counts are synthetic artifact measurements, not
claims about acquired or shipped datasets.

Executed: pinned standalone Rust 1.99.0 rustfmt and Git whitespace checks.
Not executed: Cargo, compilation, tests, Clippy, builds, runtime checks, migration
checks, or release gates. All 26 tests remain unrun in this isolated lane. Parent
registration, source integration, the actual next schema slot, migration runner,
focused test execution, Clippy, and canonical verification remain required.

## Parent review and validation

The parent integrated the types after `19b44cf` and observed all 26 initial suite
cases passing. Independent review found two invalid provenance bindings:
retaining an unrelated PuzzleId alongside the canonical one, and claiming
different acquisition-manifest digests for one source/version. Both independent
tests first exercised valid controls, then failed against frozen production:
zero passed, two failed, 109 unrelated storage cases filtered.

The review also identified that one common raw-field list could not preserve
different metadata from duplicate game copies. New contracts preceded the API
repair. Each source record now owns and serializes its raw fields; provenance
stores an explicit representative index and classifies only that copy. Tests
retain distinct Elo/TimeControl values and source attribution, exercise reversed
record order and reject an invalid representative. Every retained puzzle copy
must have the canonical PuzzleId, and the representative must bind the original
puzzle source/version. The shared source-identity owner enforces one digest per
source/version while allowing multiple records in the same frozen source.

An initial incremental apply atomically stopped because the tests' index
snapshot lagged behind the new regressions. The repeat run still used frozen
source and remained red. After synchronizing the owned tests snapshot, the
complete repair applied. All 32 suite cases then passed, with zero failed or
ignored and 83 other storage cases filtered. Follow-up read-only review found
all three findings resolved. All-target, all-feature Clippy passed after the
repair with `-D warnings`.

Final canonical verification remains pending at this entry. Actual acquisition,
filesystem/hash checking, SQLite schema integration and full T266/T268 acceptance
remain open. Exact commands, compile/apply chronology and logs belong to the
parent integration archive rather than the earlier source-only count.

The final canonical command
`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify`
exited zero: formatting, all three Clippy selections, 1,403 tests and both
optimized builds passed. Tests had zero failed, ignored or filtered cases.
All-target release took 4m53s; the separately named native harness release took
49.15s. The existing macOS debug-linker unwind warning remained visible and
unsuppressed; Clippy was clean. Exact commands, logs and verified checksums are
retained in `.git/parallel-patches/dataset-package-integration-20261017/`.
This accepts the type/provenance/manifest foundation only; T266 remains open
until its real schema and integration are implemented.
