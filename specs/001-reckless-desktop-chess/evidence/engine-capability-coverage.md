# Engine capability coverage runner

T088 is verified: the canonical 48-row inventory, registry bindings, guarded
schema and concrete command passed 13 base coverage cases and 24 command cases
within the full 1,029-test gate. All three Clippy selections and both optimized
build selections passed. The final release command produced 48 explicitly
outstanding rows and did not claim measured capability success. Current
acceptance and limits are recorded in
[integration evidence](tool-integration-20261016.md). T090, T091 and T092 remain
open, including native Windows confinement and target acceptance.

The source-handoff chronology below retains its original revision boundaries.

T088 source handoff, 2026-10-16. Base application source:
`7ce747f3240d8667afdb541ac4f4268153f063a1`. The runner, evidence fixture and
hermetic tests are authored. Cargo compilation, test execution, command
integration and real-engine acceptance have not been performed by this worker.
T090, T091, T092 and final capability reconciliation remain open. All 13 product
journeys remain intended first-release scope.

## Canonical coverage

[The runner](../../../tools/engine/coverage.rs) reads the
[pinned capability contract](../contracts/reckless-capabilities.md) through
`include_str!`. Its parser validates the source pin and table rows once into
typed category/ordinal identities and entries. There is no separately maintained
full identity list. Report completeness is exact membership in that parsed
inventory, including every option, protocol, search, output, chess, evaluation,
CPU, tablebase, NUMA and developer row.

The current contract contains 48 rows and has SHA-256
`1f7b19acdffd1925b18312f0f797810e3de2e8a4191391892063d9e45b25eb3b`.
Every member of `EngineEnumOption::ALL` and `WorkbenchEnumOperation::ALL` must
bind exactly once to an inventory entry. Option names come from their owning
enum. Workbench command bindings exhaustively match the existing operation enum.
Unknown options, missing registered operations and duplicate bindings fail.
The remaining canonical rows stay in the inventory even when their application
or native workflow has not run.

## Evidence boundary

[The JSON fixture](../../../tests/fixtures/engine-capabilities.json) is an
example of a fully classified, unsatisfied report. All 48 rows are outstanding;
none asserts an engine execution. Its package identity is taken from the
committed engine manifest. This is schema evidence, not confirmation that the
fixture binary was executed or that a current package still has that digest.

Reports carry an exact schema version, actual option-registry version and
inventory digest. Context carries target, typed profile, executable SHA-256,
pinned source commit and model SHA-256. Each row carries the digest of the
entire context. Import requires a caller-supplied expected context. Changed
package, target, profile, model, source or inventory and unsupported versions
fail before the report becomes usable.

Each row has target applicability and one typed outcome:

- Measured: Passed or Failed, expected behavior, actual observation, a hashed
  fixture and hashed engine transcript, workbench export, native measurement or
  controlled-build log.
- Outstanding: a typed prerequisite, concrete missing evidence and next action.
  The entry owns the required prerequisite. A missing native target, authentic
  tablebase corpus, CPU profile, scaling run, NUMA host, PGO execution or SPSA
  compile check cannot be replaced by a generic success.
- Upstream unavailable: a reason plus source and controlled-build artifacts.
  Only the canonical optional SPSA row permits this outcome. Missing application
  implementation never qualifies.

Artifact names are bounded relative paths with SHA-256 identities. Empty
captured artifacts, duplicate artifact paths, traversal, absolute paths,
unbounded text and missing outcome fields fail. Reports reject duplicate,
missing and foreign identities and successful empty coverage. Encoding sorts
rows by typed identity and artifacts by path, has fixed field order, includes no
implicit timestamps or randomness, and enforces a document size bound.

The schema checks evidence structure and binding. The provider is the explicit
effect boundary responsible for authentic execution and for retaining the exact
artifact bytes. A syntactically valid transcript reference does not establish
that a native scenario or benchmark ran.

## Parent integration port

Shared registration remains with the parent. The required integration is:

1. Declare `coverage` in `tools/engine/mod.rs`, register `engine-coverage` in
   `tools/main.rs`, and include `tests/engine/coverage.rs` from `tests/engine.rs`.
2. Create `ToolsEngineCoverageInventory::pinned()` and construct
   `ToolsEngineCoverageContext::new(package.target(), package.profile(),
   package.digest())` from the actual verified `EnginePackageSelected`.
3. Implement the consumer-owned `ToolsEngineCoverageProvider` required async
   `classify(&mut self, &ToolsEngineCoverageEntry, &ToolsEngineCoverageContext)`
   method. It returns `Result<ToolsEngineCoverageObservation,
   ToolsEngineCoverageError>`.
4. Compose existing engine/session and workbench operations in that adapter.
   Entry accessors expose identity, facility, requirement, typed implementation
   binding, applicability and prerequisite. Construct observations through
   `ToolsEngineCoverageObservation::new(entry, context, outcome)` and hash actual
   retained bytes with `ToolsEngineCoverageArtifact::capture`.
5. Await `ToolsEngineCoverageRunner::run(&inventory, &context, &mut provider)`.
   Encode the validated report and retain outstanding results. Use
   `ToolsEngineCoverageReport::parse(bytes, &inventory, &expected_context)` when
   importing evidence. `is_satisfied()` is false for outstanding or failed rows.

The runner invokes each canonical entry exactly once and propagates provider
failure. It creates no runtime, process, worker pool, capacity budget, test
implementation or hidden workflow discovery list. Sequential provider invocation
keeps engine configuration/search/stop barriers and exclusive diagnostic
capacity under their existing owners. An adapter may retain and reuse an actual
workflow result where multiple rows require it, while still classifying each
row against its full requirement. Merely starting a process is insufficient to
pass all protocol, output or chess requirements.

Existing inspector, raw evaluation, compiler, perft and benchmark operations and
their exports are available for composition. T087 owns queued-command, PGO and
controlled SPSA execution. Authentic tablebases, actual NUMA hardware, optimized
x86 profiles and cross-platform runs remain required evidence. The worker did
not execute any of them.

## Stop limitation

The parent reports a source-backed immediate `go`/`stop` race in the pinned
engine: its UCI listener can handle `stop` before the queued `go` starts and
resets `RUNNING`. Normal native Stop waits for actual telemetry; the original
two-second escalation correctly terminates and reaps the child. This worker
did not reproduce those executions. The PRO-06 fixture preserves the precise
limit and required normal/race/escalation evidence. General Stop support is not
classified as upstream unavailable. T091/T092 remain open.

## Verification and ownership

[Thirteen hermetic tests](../../../tests/engine/coverage.rs) cover actual registry
bindings, nonempty complete classification, malformed and drifting inventory,
provider failure, duplicate/missing/foreign rows, future versions, context
mismatches, deterministic encoding, source-plus-build unavailability proof,
measured artifact requirements and concrete prerequisites. Their synthetic
artifact cases explicitly describe schema checks and do not enter the fixture
as native successes.

Executed: standalone pinned `rustfmt` formatting and syntax parsing; independent
UV-managed Python JSON, canonical row-membership and SHA-256 consistency checks;
temporary-index binary patch export and patch whitespace/applicability checks.
The test source has 13 test declarations. Executed Rust tests: zero.

Unrun under the explicit shared-target restriction: Cargo compilation, meaningful
compile-red observation, Cargo test discovery/execution, Clippy, all-target and
native builds, `make verify`, CLI execution, engine/runtime checks and native
acceptance. These must be performed by the parent after shared registration and
adapter integration. Passing formatting or static fixture checks does not accept
T090 or any other runtime task. The parent owns task-marker updates.

Only `tools/engine/coverage.rs`, `tests/fixtures/engine-capabilities.json`,
`tests/engine/coverage.rs` and this evidence file belong to the patch. Shared
roots, Cargo, Makefile, task files, vendor code and T087-owned files are untouched.
No dependencies, suppressions, code comments, commits, pushes, agents or extra
worktrees were introduced. Applicable guidance is the
[constitution](../../../.specify/memory/constitution.md), its complete
[canonical rule catalog](../../../.specify/rules/README.md), the global/project
instruction bridge and the invoked
[implementation skill](../../../.agents/skills/speckit-implement/SKILL.md).
No `.specify/extensions.yml` exists, so no before/after implementation hooks are
registered in this checkout.

## Concrete evidence-import command handoff

The follow-up adds [the command adapter](../../../tools/engine/coverage_command.rs)
for `engine-coverage <package-directory> <new-output-report> [evidence-report...]`.
The original base patch is unchanged. Its owning schema types gain narrow
read-only report/observation/artifact accessors, and the existing base contract
tests exercise those accessors. Persisted schema ownership remains in
`coverage.rs`; the command introduces neither a second schema nor another
capability identity list.

The final root-facing API is:

- `ToolsEngineCoverageCommandConfig::new(package_directory, output_report,
  evidence_reports)` with native `PathBuf` selections.
- `ToolsEngineCoverageCommandResources::new(&executor, cancellation,
  runtime_handle)` with the existing shared-budget `CapacityExecutor`,
  `Arc<AtomicBool>` cancellation and owned
  `tokio::runtime::Handle` supplied by the composition root.
- `ToolsEngineCoverageCommand::run(config, resources).await`, returning a typed
  result containing a `Display` summary. The root owns argument registration,
  stdout, Ctrl+C, runtime and executor shutdown.

The root must observe the operation through its outcome after an interrupt,
setting the injected cancellation flag and then awaiting shared executor
shutdown. The adapter submits one charged blocking job containing preparation,
canonical report composition and publication. The injected existing runtime
handle drives the existing runner with an in-memory provider on that worker;
there is no new runtime, pool, process or engine test implementation. Temporary
directories and their cleanup stay inside the worker even when the caller
cancels or abandons its future. The executor holds its existing work lease
through cleanup; pure runner composition takes no second lease, including with
one available worker. The root's shared budget remains the executor's owner.

The command detects the actual host and streams a stable regular packaged
executable into a private bounded snapshot. The canonical package manifest
parser and `EnginePackageSelector::select` validate the snapshot against the
actual detected host, length, executable permissions and advertised SHA-256.
This bounds the canonical selector's hash read even if the original file changes
concurrently. Package presence and successful checksum verification establish
identity only. They never create a measured capability result.

Every explicitly selected input is a complete coverage report accepted by
`ToolsEngineCoverageReport::parse` against that selected context and canonical
inventory. Imported Outstanding rows supply no measured evidence. Each Measured
or source-backed SPSA-unavailability artifact must exist beneath its selected
report's directory as a regular file with no symbolic-link path components.
The adapter checks nonempty bounded size, complete bytes, stable file identity
and the recorded SHA-256 before using the observation. Missing, changed, unsafe
or oversized evidence fails the command rather than becoming an outstanding
fallback or apparent success. The base schema has no declared artifact-length
field; the command verifies the observed file length and actual complete read,
with the recorded digest binding the exact bytes.

The command limits report count to 32, each report to the schema owner's existing
document bound, each artifact to 64 MiB, and a packaged executable to 256 MiB.
Aggregate input is bounded to 512 MiB, including the selector's snapshot hash
read. Binary copying uses 64 KiB buffers. Artifact bytes are processed one file
at a time, copied to private staging and released; the adapter does not retain
every artifact payload in memory. The global input bound and common destination
transaction make this one sequential filesystem operation under the injected
executor's capacity lease.

Verified artifacts receive deterministic content-addressed paths under
`<output-filename>.artifacts/<artifact-kind>/<sha256>`. Reconstructing through
`ToolsEngineCoverageArtifact::capture` and `ToolsEngineCoverageObservation::new`
preserves the schema owner's validation and canonical ordering. Identical
overlapping observations deduplicate after path normalization; conflicting
observations for any identity fail. All other rows use the entry-owned typed
prerequisite plus the canonical expected behavior and concrete next workflow.

Publication creates a new artifact bundle without replacing an existing one,
checks its directory identity, links the staged files and syncs supported
directories before publishing a completely written and synced report with
`NamedTempFile::persist_noclobber`. Existing reports and bundles are never
overwritten. Cancellation is checked during reads, before publication and before
the final report commit. Precommit failures remove the command-owned bundle
when its identity is intact. Cleanup errors preserve their causes; a directory
sync failure after report publication is explicitly reported as published.
The installed tempfile implementation guarantees no overwrite and documents that
some filesystems may retain an extra temporary hard link. Native filesystem and
power-loss acceptance remain unperformed by this worker.

The summary includes classified, measured-passed, measured-failed, upstream
unavailable and outstanding counts, and explicitly leaves T090 real execution
acceptance open. A report with 48 Outstanding rows is a useful complete coverage
accounting artifact, not a successful engine acceptance run. The parent also
reports a pinned queued-command FEN fullmove-counter discrepancy after `e2e4`:
the engine reports 2 while the domain expects 1. This adapter assigns no automatic
PRO-05 or DEV-06 pass; actual imported failure evidence remains a measured failure.

[Thirteen private command tests](../../../tests/engine/coverage_command.rs)
are included from the command module's `cfg(test)` declaration and are discovered
through the `chess-tools` binary test target after parent module registration.
They cover exact registry-derived outstanding coverage, explicit summaries,
verified artifact relocation, failure retention, deterministic equivalent
overlaps, conflicting overlaps, missing/tampered/oversized artifacts,
metadata/path/package/context mismatch, existing report/bundle refusal,
cancellation, publication races and cleanup after staged-file failure. One test
is Unix-specific symbolic-link behavior; Windows link and native filesystem
acceptance remain separate outstanding evidence. Test package bytes and failure
artifacts are explicitly hermetic adapter fixtures and assert no engine run.

The parent reports that the original 13 base coverage tests passed before this
adapter/accessor delta. This worker executed zero Cargo, Rust test, Clippy,
engine, CLI or native runtime checks under the parent's serialized Cargo-lane
restriction. Standalone formatting/syntax parsing, source restrictions,
independent canonical inventory/fixture/hash/link checks and delta patch checks
are the worker's performed checks. The adapter remains source-ready until the
parent integrates registration and runs the required verification.

The parent's first all-target/all-feature Clippy run rejected four adapter
issues: an unused production import, the private one-worker fixture's budget
error conversion, an artifact-import method above the configured argument limit,
and a nested cleanup condition. The refreshed adapter delta incorporates the
parent's minimal import/conversion corrections, groups import inputs in one
typed request and collapses the cleanup condition. Fixture budget errors pass
through the existing capacity-executor error owner, preserving their cause;
no production error variant exists solely for test setup. No dependency,
persisted schema, capacity reservation or publication behavior changed.
Formatting and delta checks were repeated. Cargo and runtime revalidation stay
with the parent, and no new passing test result is asserted by this worker.

## Review regressions: artifact aliases and checked ancestors

This repair starts from exact current root copies of the three owned files at
root HEAD `5e9f55fd94056050e3ba00d7480c7c87f78ac00c`. The production adapter
baseline blob is `d87282665757f27017687b27fc4866211ffc52e8`, private tests
`b54ba9eb590cec912dedb0cb6351c3e4117f27cb`, and this evidence
`a964102250fc56996e7d4f77caa97492d4c951c3`. No other root files were copied or
changed by this worker.

The tests-only patch was delivered with production still exactly at that
baseline. The parent observed both semantic failures before authorizing the
production repair. The alias regression failed with
`Coverage(Evidence(Developer4))`; the ancestor regression failed its assertion
that replacing a checked ancestor must not permit outside evidence bytes.
Each parent command exited 101 with one failed test and 26 filtered tests. The
parent owns the red logs under its PGO-diagnosis scratch directory. This worker
ran no Cargo, build, tests or diagnostics.

The base schema permits same-kind, same-byte artifacts at distinct source
paths. Every original artifact reference is still opened, bounded and verified.
The adapter now deduplicates their verified canonical destination paths before
constructing the resulting observation. This preserves the original measured
verdict and fields, keeps different evidence kinds distinct and yields the same
encoded output with or without a redundant content alias. It does not remove a
capability, skip a source verification or expand upstream unavailability.

Checked paths now own retained ancestor identities. On Unix they retain the
actual directory descriptors and use the already installed rustix safe
`openat` facility with `NOFOLLOW`; directories additionally use `DIRECTORY`,
and leaf opens use `NONBLOCK` before regular-file validation. The absolute
directory anchor is walked without following symbolic links. Artifact paths
duplicate their selected report's original directory descriptors, and binary
paths duplicate the selected manifest's descriptors. They do not re-resolve a
replacement root. This follows the safe descriptor pattern already used by the
project's profile adapter without widening that profile-scoped API or adding a
dependency. No unsafe code or checker suppression was introduced.

Read/copy checks compare the retained ancestor identities, regular-file handle,
length and modification time. Ancestors are revalidated on successful and failed
operations, including cancellation and output errors. A simultaneous operation
failure and path change preserves both typed causes. The selected report's path
is also revalidated around its entire import, including artifact failures. One
additional deterministic copy-error regression replaces an ancestor during the
writer call and checks that both the original output error and ancestor failure
survive.

The Unix guarantee is descriptor-relative data access to the captured directory
chain, with symbolic-link traversal rejected and detected namespace changes
refusing the observation. Holding a descriptor does not freeze in-place writes;
the existing full-byte digest and stable-file checks remain necessary. The
portable non-Unix fallback retains handles and validates ancestor identities
before and after operations, but uses path-based opens. It does not provide
descriptor confinement and cannot exclude a replacement followed by restoration
between checks, or promise that no outside bytes were temporarily read in that
window. This is an explicit Windows hardening/native-acceptance limit, not a
silent claim that revalidation alone is equivalent to descriptor-relative I/O.
The optional Windows deny-delete/reparse-handle variant is not implemented or
represented as verified here.

Root command APIs and the base coverage schema are unchanged. The private suite
now contains 16 test declarations, including three Unix-specific cases. The two
already applied red-test additions are excluded from the incremental production
repair patch; it carries the production changes, the new error-path regression
and this evidence append. Post-fix execution belongs to the parent and remains
unperformed by this worker. T090 and Windows/native acceptance remain open.

## Publication confinement follow-up, 2026-10-17

The parent reported 16/16 command tests passing after the input-path and alias
repairs. A fresh independent review then found a distinct destination-path gap:
the retained bundle-root identity did not protect a substituted kind directory,
and pathname-based hard links could write outside the selected bundle. The
final report's pathname-based publication had the same parent replacement
window. Those findings are not covered by the earlier input repair.

Only the current root adapter, private tests and this evidence were copied as
the new baseline. Their Git blob identities were respectively
`67025a8580c13e01a6ff584981dd22b54e865a74`,
`78a13ef43e572e072a93a526dd64c912ea851a5a` and
`d92dc9041498f5b238f2e06b0284dd9bcab769dd`.
The new publication test was delivered first while production remained exactly
at that baseline. The parent observed its semantic red: zero passed, one failed,
32 filtered, exit 101, with the assertion that a kind-directory symlink must be
rejected despite an unchanged bundle-root identity. The parent owns
`coverage-publication-red.log` in its diagnosis scratch directory.

The repair retains the selected destination parent's descriptor and the
descriptors of the created bundle and kind directories. Unix creates children
with `mkdirat`, opens them with `DIRECTORY|NOFOLLOW`, and creates artifact and
report links with `linkat` using single-component names beneath the retained
source/destination directory descriptors. Existing names are never replaced.
Directory fsync uses those retained descriptors. The final report is completely
written and synced in private staging before its noclobber hard link is created
in the original selected parent. Namespace checks occur before and after report
linking and again after staging cleanup.

Bundle validation enumerates bounded directory entries through retained Unix
descriptors. It rejects unowned descendants, missing/replaced kind directories,
unexpected leaf entries and changed file identities/metadata. Complete-bundle
checks rehash the bounded artifact bytes against their existing content-addressed
identities before and after report linking. The report's own bytes are also
checked against the serialized document. A moved or modified bundle cannot
produce a successful result when detected at those validation points.

Final-destination cleanup uses tracked file identities, descriptor-relative
unlink and empty-directory removal. It does not recursively traverse an output
bundle or follow a substituted kind-directory symlink. Ambiguous ownership or
remaining foreign entries fails cleanup rather than deleting their contents.
Private staging remains an owned temporary tree, but its Unix cleanup is also
descriptor-relative: pathname-based automatic cleanup is disabled once
publication owns it, and cleanup traverses its original descriptor without
following symbolic links. This prevents a renamed output parent from redirecting
temporary-file deletion into a lookalike directory outside that parent. Cleanup
errors preserve their causes; a valid published report with failed staging
cleanup is reported explicitly as published with cleanup failure.

The original deterministic red test now uses the private publication constructor
and bundle-creation method. Three further Unix tests place the replacement
between descriptor capture and the actual hard-link operation, exercise the
final report parent, and preserve an outside staging lookalike when the parent
changes before commit. They use fixed filesystem steps without watchers,
sleeps or timing races. The private suite now has 20 source test declarations.

The public Config/Resources/run API, coverage schema and dependencies are
unchanged. This is a repair inside the existing owned adapter. Windows continues
to use its documented path-based fallback with identity/reparse observations;
it does not gain Unix descriptor confinement or a guarantee that concurrent
namespace substitution cannot cause an outside write. Native Windows hardening
and acceptance remain outstanding. Neither this change nor a source-only review
accepts T090.

This worker applied scoped source formatting and static patch checks only. It
ran no Cargo, build, test, engine or performance diagnostics. The parent retains
the execution lane for targeted compilation, the new regressions, Clippy and
the full verification sequence.

## Staging lifecycle confinement follow-up, 2026-10-17

The parent reported all 20 publication command tests passing, then independently
identified an earlier staging boundary. Before publication, ordinary TempDir
cleanup and pathname artifact writes could follow a replaced selected parent
into an outside directory with the same staging name. The production baseline
for this repair is blob `6dfb26e4caded95ad0a98f19ff4213e940c8bcfc`.

The two staging regressions were delivered before production changes. The parent
observed deletion of the outside staging lookalike on prepublication drop. The
initial import fixture failed before mutation because the macOS `/var` alias was
not canonicalized; that fixture failure is not an import-defect result. After
the parent applied the one-line canonicalization correction, the import test
reached its semantic assertion and failed because artifact staging wrote into
the replacement-parent lookalike: zero passed, one failed, 39 filtered, exit 101.
The parent owns `coverage-staging-red.log` and
`coverage-staging-import-red.log` in its diagnosis scratch directory.

Private staging now owns the original parent and stage descriptors from initial
creation. Unix creates the stage with `mkdirat`, traverses created descendants
using directory descriptors and `NOFOLLOW`, and creates each file with
`openat` and `CREATE|EXCL|NOFOLLOW`. Package snapshot bytes, imported artifacts
and the serialized report use this owner. File permissions and fsync operate on
the opened files. Parent/stage identities are checked before and after writes,
including failed writes. Renaming or substituting the selected parent cannot
redirect those Unix operations to the replacement pathname.

The single observed executor job explicitly finalizes staging on package
selection, preparation/import, cancellation, Runner, encoding and publication
failures, as well as successful publication. Finalization returns the original
operation error and any cleanup error together. Descriptor-based fallback Drop
cleanup handles unwinding or private-owner abandonment and reports cleanup
failure through the existing logging facility. It never recursively deletes
through a Unix stage pathname. Cleanup removes original stage contents through
retained descriptors, checks the named stage identity before removing the empty
directory, and syncs its original parent. A moved stage whose original name
cannot be authenticated produces a cleanup error instead of deleting a
replacement. There is no automatic TempDir, NamedTempFile or TempPath cleanup in
the production adapter.

The already-applied staging tests remain present, with the corrected canonical
input. The drop case additionally checks that the original stage was removed.
Two further cases exercise Runner cancellation on one worker and simultaneous
operation/cleanup failure when the stage itself is replaced by a symlink. They
check original-content cleanup, outside-data preservation and retained typed
causes. The command suite now contains 24 source test declarations.

This worker ran scoped source formatting and static patch checks only. It ran
no Cargo, build, test, Clippy, engine or performance diagnostics. The parent must
execute the new staging cases, the complete command suite, prior publication
regressions and final gates. Public Config/Resources/run and the base schema are
unchanged; no dependencies or pools were added. The non-Unix fallback has
explicitly observed cleanup and no automatic temporary-path deletion, but its
path-based creation, linking and removal remain vulnerable to concurrent
namespace substitution between checks. Windows descriptor confinement and
native acceptance remain outstanding. T090 remains open.
