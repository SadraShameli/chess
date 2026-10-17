# T162 historical engine metadata gateways

Status: source-only handoff. The parent owns module registration, the visibility
prerequisite, report composition, and every executable verification gate. No
compilation or behavior result is claimed.

## Current owners inspected

Read-only inspection used `/Users/sadrashameli/Personal/chess`, including the
current uncommitted search-session owner with its independent
`primary_evaluation`. This worktree starts at
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`; the parent search-session changes
are intentionally absent from the patch.

The inspected owners were engine capabilities, the option registry, acknowledged
options, resource configuration, analysis, package manifest/selection, search
identity/limits/session, engine session, process provider, board moves/positions,
clock duration, and package digest. Existing lifecycle and package-selection
fixtures supply the process transcript and filesystem-attested package.

Global/project instruction bridges, the constitution, and all seven canonical
rules were read. Their owners remain
[code quality](../../../.specify/rules/code-quality.md),
[verification](../../../.specify/rules/verification.md),
[architecture](../../../.specify/rules/architecture.md),
[structure](../../../.specify/rules/structure.md),
[Rust style](../../../.specify/rules/rust-style.md),
[performance](../../../.specify/rules/performance.md), and
[testing](../../../.specify/rules/testing.md).

## Parent prerequisite

Change only the visibility of
`EngineOptionEnumDescriptor::descriptor(self) -> String` in
`src/engine/enums.rs`, inspected at line 142, from private to `pub(crate)`.
`EngineIdentityRecord::capture` calls this existing formatter on the actual
advertised descriptor. The private spin formatter at line 125 needs no change.

This prerequisite is deliberately absent from the patch. Replacing its call with
`EngineEnumOption::descriptor()` would substitute registry defaults for the
historical Threads maximum. Reimplementing descriptor formatting would create a
second protocol owner.

Register `pub mod persistence;` in each existing owner:

- `src/engine/capabilities.rs`
- `src/engine/options.rs`
- `src/engine/analysis.rs`
- `src/engine/search/identity.rs`
- `src/engine/search/limit.rs`
- `src/engine/search/session.rs`

Register `pub mod identity;` in `src/engine/package/mod.rs`. Add
`#[path = "engine/report_persistence_codecs.rs"] mod engine_report_persistence_codecs;`
to `tests/engine.rs`, alongside the existing `engine_lifecycle` and
`engine_package_selection` modules. The new test module includes its own four
children. Public re-exports, if desired, belong to the parent.

The new search identity and limit child modules can access their canonical
parents' private fields. The options child can use the existing owner methods.
No additional visibility gateway is required by this source.

## API and restoration contracts

- `engine::package::identity::EnginePackageIdentity` has typed `new`, `target`,
  `profile`, `digest`, `parse`, and validated JSON serde. Its only metadata is
  target/profile/digest. The new inherent `EnginePackageSelected::identity()`
  uses existing public getters. No executable path, selected-package constructor,
  selected-package deserializer, or conversion back to executable authority is
  introduced. The parent can retain this identity in `ReportJobConfiguration`.
- `engine::capabilities::persistence::EngineIdentityRecord::capture(&identity)`
  and `restore()` preserve name, author, registry version, and every actual
  advertised descriptor. Capture iterates `EngineEnumOption::ALL`; restore uses
  `EngineCapabilities::discover`, `validate`, and `EngineIdentity::new`.
- `engine::options::persistence::EngineOptionsAppliedRecord::capture(&applied)`
  and `restore(&capabilities)` preserve optional resources, variant, Minimal,
  MultiPV, and Unconfigured/Loaded/Unavailable tablebase status with its exact
  path and loaded piece count. Restoration uses canonical resource configuration,
  option settings, Syzygy path validation, and applied-owner methods. It checks
  resources against the supplied historical capabilities. A loaded status is
  archived acknowledgement metadata; restoration performs no filesystem probe.
- `engine::analysis::persistence::EngineAnalysisLineRecord::capture(&line,
  &position)` and `restore(&position)` preserve each optional statistic, score
  kind/value, bound, perspective, and optional PV independently. Whole-millisecond
  elapsed time is exact; fractional milliseconds fail. Restoration builds bounded
  structured info text and uses `EngineAnalysis::parse` for legality, rank, hash,
  numeric and PV checks. Scalar hash validation during serde uses that same parser.
  The public canonical empty default line has an explicit empty-record round trip.
  Perspective and position-dependent legality are checked when the caller supplies
  the authoritative board. Primary evaluation, latest update, and candidate lines
  require separate records in the later report snapshot.
- `engine::search::identity::persistence::EngineSearchTagRecord::capture(tag)`
  and `restore()` preserve the actual search UUID, process-generation UUID, game
  UUID, and revision. Nil identifiers fail; no new UUID is generated on restore.
- `engine::search::limit::persistence::EngineSearchLimitRecord::capture(limit)`
  and `restore()` preserve Infinite, Depth, Nodes, MoveTime and Clock separately.
  Serde metadata targets the existing canonical limit enum. All restored values
  pass the existing constructors. The later report layer must reject Infinite
  and Clock when a finite report budget is required.
- `engine::search::session::persistence::EngineSearchSessionFailureRecord::capture(&error)`
  produces portable archival metadata. JSON parse/deserialize restores only that
  record. Its getters expose the exhaustive top-level category, original message,
  original source chain, thread/lease counts, engine category, process details,
  handshake phase/state when available, and separate cleanup context/message/source
  chain. Process details preserve OS error codes, pipe, UTF-8 offsets, reader task
  cancellation/panic distinction, and portable exit code where available. No API
  constructs a live I/O error, join error, elapsed error, exit status, search-session
  error, or terminal from this metadata. The runtime retains its actual `Arc`
  failure and terminal.

`EnginePersistenceError` and the shared crate-private bounded JSON/text/list
mechanisms live with the identity capability codec. They are reused by all seven
gateway files rather than implementing seven independent decoding policies.

## Bounds and JSON boundary

Every top-level record has a version-1 `{version, data}` envelope and a public
`MAX_BYTES`. `parse(&[u8])` checks the complete input length before serde. Custom
serde first borrows the complete JSON value as `RawValue`, checks its byte bound,
then decodes a borrowed envelope and rejects unknown/future versions before
allocating its payload. This follows the existing serde_json raw-value boundary;
use string/slice JSON decoding, including when nested in a larger report. An
owned generic serde value or streaming reader does not provide that borrowed
boundary. Report ingestion must also enforce its aggregate report bound.

Package identity and tags allow 1 KiB each, limits 2 KiB, engine identity and
analysis 16 KiB each, applied options 512 KiB, and failure metadata 1 MiB. Fixed
text visitors check lengths before cloning. Count visitors reject excess entries
before deserializing another owned entry. Descriptor count comes from the
canonical registry; PV count comes from `MAX_VARIATION_PLIES`. Failure messages
allow 4,096 bytes each and at most 16 entries in each source chain. Capture first
bounds the source-reference count before formatting source messages. Formatting
uses a bounded writer and fails instead of silently truncating originals.

All record/envelope/object payloads deny unknown fields. Remote serde metadata
describes existing closed domain enums without adding another option registry or
alternate option/search policy. Deserialization validates the owning constructors
or archival structural invariants; contextual analysis and advertised-resource
validation finish at `restore` with their required canonical inputs.

## Authored tests and executed checks

The first four integration-test files were written and exported before production
source as `tests-first.patch`. Source review then extended the final suite with a
focused fixture child and further boundary checks. The final files contain 13
integration tests and one unit test. All 14 remain unrun; no baseline red was
observed in this restricted lane.

Coverage includes complete registry restoration with a non-default advertised
Threads maximum, malformed/future/oversized records, nil tags, numeric and clock
bounds, acknowledged resource/option/tablebase states, missing original packages,
all optional analysis statistics, exact large counters, fractional milliseconds,
bounds/perspectives, legal full-length PVs, promotions, standard and Chess960
castles, finite/infinite/clock distinctions, independent actual terminal line
roles, and portable failures captured from real scripted engine/search sessions.
The focused failure process delegates lifecycle behavior to the existing scripted
process and injects controlled I/O failures at the process port. Tests obtain
actual failed terminals, observe cleanup/capacity release, and retain the original
failure `Arc`; they construct no publications, terminals, or receipts.

Standalone rustfmt and Git whitespace checks were used as source checks. They do
not establish compilation, Clippy conformance, behavior, or release acceptance.
Cargo, tests, builds, Clippy, application runtime, and CPU diagnostics were not
run. No agent, commit, push, existing-source edit, root registration, report edit,
or storage edit was performed.

The exported final `NEWFILES.patch` contains only seven new gateway source files,
five new integration/fixture files, and this evidence file. Separate final tests
and source patches permit parent-controlled integration. `SHA256SUMS` identifies
the exact files and patch artifacts; `PARENT-OWNERS.sha256` records the read-only
canonical engine sources inspected for this handoff.
