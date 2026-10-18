# Workbench execution provenance

Prepared from pushed `ebed5c00407d909b95c5aa67fd543d69d2d6f472` in
`/Users/sadrashameli/.t3/worktrees/chess/t3-6e686ad6` for T086. The parent owns
integration, verification, task markers, publication and worktree cleanup.

`WorkbenchExecutor::observation()` returns an optional borrowed
`WorkbenchExecutorObservation`. Its private fields expose immutable `job_id()`,
`package()`, `identity()`, `applied_options()` and `generation()` facts. The
package includes the selected executable, target, profile and verified digest.
The identity, options and process generation come from the actual isolated
session. No global settings or reconstructed option defaults supply the snapshot.

The snapshot appears only after Threads, Hash and MoveOverhead acknowledgments
and readiness, followed by the diagnostic's actual variant acknowledgment and
readiness when it has a position, position delivery and diagnostic command
delivery. The crate-private `begin_diagnostic` returns the acknowledged applied
options at that successful boundary. It introduces no await between successful
delivery and snapshot capture. Public `EngineSession::applied_options()` keeps
its existing lifecycle guard; interrupted preparation still cannot expose
ambiguous options. Engine lifecycle transitions remain unchanged.

The executor retains this one observation through success, cancellation, timeout,
operation failure, dropped cleanup waits and retained cleanup retries, including
after releasing the session. Setup failure, dropped setup and cancellation before
setup completion leave it unavailable. Export consumers can pass `job()`,
`observation.identity()` and `observation.applied_options()` to
`WorkbenchExportContext::observed`. Active diagnostics still borrow the canonical
owner, and final cleanup still moves that single tail into the job.

Tests were authored before source changes using the existing executor transcript
fixture. Four new cases cover Chess960, each resource/variant acknowledgment and
resource/variant readiness cancellation boundary, failed setup/delivery, and
pre-setup cancellation. Existing cases additionally check all five operations,
exact package/job identity, an advertised Threads maximum of 1024 rather than the
registry baseline, distinct process generations, explicit two-thread/32 MiB bench
resources, unavailable failed/expired setup, and preserved snapshots after every
cleanup outcome. Existing diagnostic ordering and isolation regressions remain.

The applicable [canonical rules](../../../.specify/rules/README.md) were read in
full. The change keeps the engine boundary crate-private, adds no dependency or
task, allocates one immutable snapshot per execution, and leaves live search,
option configuration and cleanup behavior unchanged.

Scoped `rustfmt --edition 2024 --config skip_children=true` and its check mode,
plus `git diff --check`, passed for the owned files. Cargo is reserved by the
parent and was not run; no red-first execution, compilation, Clippy, test count,
native-engine run or full verification result is claimed.

## Parent verification

Integrated without modifying canonical diagnostic-tail ownership. Formatter
and targeted library/domain/engine/presentation Clippy passed. All 193 engine
tests passed, including the 21 executor cases; all 87 presentation tests passed.
Read-only review verified actual resource and variant readiness, successful
delivery before observation publication, and retained observation after cleanup.
No concrete blocker was found. The export consumer is integrated under T086.
The existing macOS linker warning remains unsuppressed.

The pinned native `--workbench resources/engine` acceptance also passed:
five isolated operations, all 45 benchmark positions at depth one, canonical
16/1/12 defaults, standard depth-two perft 400, real depth-twelve cancellation,
live-engine isolation/readiness and exact lease recovery. This native check
exercises executor behavior; native export destination routing remains T083/T089.
