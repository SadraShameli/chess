# Workbench diagnostic export implementation

T086 adds the version 1 JSON encoder and caller-owned write boundary in
`src/workbench/export.rs`, with the source-ready results adapter in
`src/presentation/workbench/results.rs`.

## Data and reproducibility

`WorkbenchExport::capture` accepts the terminal backend `WorkbenchJob` and its
captured `WorkbenchExportContext`. Queued, admitted, running and cancelling jobs
return a typed refusal. The export reads the final canonical diagnostic tail
from the job and never accepts the presentation snapshot's rows or log slice.

The concrete serializable content contains the selected executable SHA-256,
target and profile, the pinned engine version/source repository/tag/commit and
embedded model name/length/SHA-256. The source/model constants are the same
invariants validated by `EnginePackageManifest` before package selection; no
executable path, host identity, clock reading or network lookup is introduced.

`WorkbenchExportContext::observed(job, identity, applied)` copies the actual
isolated child's validated identity and acknowledged options into an immutable
snapshot bound to the job ID and executable digest. Resources retain threads,
Hash in MiB and MoveOverhead in milliseconds. Variant, Minimal, MultiPV and
Syzygy load state/cardinality/engine path are preserved. `unobserved(job)` exports
an explicit unavailable observation when the child never reached that boundary.
A different job ID or package digest is rejected. An observation must never be
constructed using another engine session's settings or a UI draft.

Parameters preserve initial/current FEN, variant and complete ordered UCI
history. Results preserve the inspector's legal moves, distinct raw
White-perspective NNUE integer, compiler fields, every ordered perft root and
exact totals, or all benchmark counters. Benchmark parameters record their
resolved values, including canonical upstream defaults when selected. All
unsigned 64-bit counters, durations, byte counts and supplied timestamps use
decimal JSON strings; elapsed time uses exact nanoseconds. No float conversion
or display rounding occurs.

Failed outcomes retain a typed failure category, message and source chain.
Cancellation retains user/deadline reason, elapsed cancellation request time,
and any late validated result, failure or termination. A late result remains
inside a cancelled outcome. Ordered stdout/stderr text is JSON-escaped without
execution or markup interpretation. Existing line/byte limits and exact eviction
counts disclose the bounded tail; the encoder imposes no additional truncation.

`encode_content()` excludes per-export identity and timestamp. The versioned
`encode(metadata)` envelope separates the backend job ID and optional explicitly
supplied export UUID, Unix milliseconds and application version. Default
metadata contains explicit absence. Repeated encoding is byte-identical for the
same content and supplied metadata.

## Write and preview boundary

`prepare(metadata)` returns `WorkbenchExportEncoded`, which owns the exact bytes,
length and SHA-256 available for preview. `PresentationWorkbenchResults::preview`
constructs that artifact directly from the completed backend job and exposes
retained/evicted diagnostic counts. It never reconstructs a result from display
rows. Writing the preview writes those same bytes.

`WorkbenchExportPort` is defined by the lower-level workbench component. Its
caller owns the approved destination and staging lifecycle. `write` must stage
all bytes or return its concrete source error. `commit` must publish through the
existing native atomic-file boundary, checking the caller's cancellation state
at its serialization point. `abort` discards staging and is called for every
encoding, write, cancellation or commit error. A receipt is returned only after
`commit` succeeds. Native I/O belongs on the handler's existing blocking worker;
the encoder does not schedule detached work or own a runtime.

The in-memory failure adapter and real `StorageAtomicFileStage` adapter exercise
partial writes, cancellation before/after staging, stale destinations, discarded
temporary files and source-error propagation. The existing atomic-file owner
remains responsible for publication and synchronization semantics, including
indeterminate failures after native publication. The export layer does not
claim rollback after an operating-system publication failure.

## Integration handoff

The parent merges `pub mod export` and the integration-test registration. After
merging T085, add `pub mod results` to its existing workbench presentation module;
this lane deliberately supplies no replacement UI module or edits to its view.
T083 selects/resolves the opaque native destination, constructs the port around
`StorageAtomicFileStage`, and retains cancellation authority through commit.

T082 provenance supplies `WorkbenchExecutor::observation()` after acknowledged
resources, diagnostic variant readiness and successful command delivery, retaining
it through cleanup. Build the observed context using `executor.job()`,
`observation.identity()` and `observation.applied_options()`, then export after
job completion. When no observation exists, use the explicit unobserved context.
The executor and handlers are owned by the parent and other lanes, and are not
modified in this patch.

## Verification

The initial six integration tests were authored before implementation. The
suite was extended to 12 tests for configuration, metadata, larger root counts
and native failure boundaries. They
cover the canonical five-operation registry, deterministic content, exact values
above the IEEE-754 integer limit and at `u64::MAX`, a perft position exceeding
128 display rows, defaults and explicit benchmark parameters, actual acknowledged
configuration copied from a hermetic engine session, both diagnostic eviction
budgets, terminal outcomes, active refusal and native atomic-file behavior.

Executed in this isolated worktree: standalone pinned `rustfmt` on owned Rust
files, its check mode, and `git diff --check`. Cargo execution remains reserved
by the parent, so these tests have not yet been compiled or run here. No red
execution, test-pass count, runtime performance, native handler wiring or complete
T086 acceptance is claimed by this evidence until the parent performs those
checks and completes the listed registrations and routing.

## Parent verification

Integrated with the T082 executor, actual observation API, and existing T085
module. Parent resolved the shared declaration conflict and covered the newer
Session/SessionCleanup failure variants without a wildcard. The first combined
run observed the configuration test failing: its transcript emitted a Minimal
acknowledgment unsupported by the pinned engine contract. Removing that invented
line and asserting the actual updated option state fixed the fixture while
retaining the immutable earlier snapshot assertion.

Formatter and targeted library/domain/engine/presentation Clippy passed. All
193 engine and 87 presentation tests passed, including all 12 export cases
and 21 executor cases. Read-only export/provenance reviews found no concrete
blocker. T086 is accepted for canonical encoding, immutable preview and the
caller-owned atomic-write port. T083/T089 still own actual native routing and
application wiring. The existing macOS linker warning remains unsuppressed.

The pinned native `--workbench resources/engine` acceptance also passed:
five isolated operations, all 45 benchmark positions at depth one, canonical
16/1/12 defaults, standard depth-two perft 400, real depth-twelve cancellation,
live-engine isolation/readiness and exact lease recovery. This native check
exercises executor behavior; native export destination routing remains T083/T089.
