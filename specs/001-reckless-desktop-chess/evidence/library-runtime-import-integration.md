# Played persistence, recovery and import framework integration

The parent integrated the source handoffs and observed the final canonical gate
on 2026-10-17. This supersedes the child-only unrun status for this exact combined
snapshot. It does not accept the unfinished mounted library or import backend.

## Verified snapshot and command

The baseline is `647147d9ff95cb745ede46637048ef312e3070af`. The complete source
snapshot is tree `e9219be3c2255064d9611ac5ae5ac37c8a9fc773`, exported as
`.git/parallel-patches/library-runtime-import-current.patch`, SHA-256
`cee393a24f2a999675a030bdc3199c39d8daf7eda431c5e65954c3a7af40f7a1`.
All 67 exported file blobs were rechecked against the working files after the
gate; none differed. This report and the appended evidence links are subsequent
Markdown changes only.

The exact final command was:

```text
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify
```

It exited 0. Cargo used default detected parallelism, sequential gates and no
worker caps. Root Rust remained frozen during execution. Formatting, all three
Clippy selections with `-D warnings`, default all-feature tests, the optimized
all-target build and the separately selected optimized native-harness build
completed successfully.

| Executed test selection | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Library unit/private contracts | 423 | 0 | 0 |
| Tools | 44 | 0 | 0 |
| Assets | 5 | 0 | 0 |
| Desktop integration | 232 | 0 | 0 |
| Domain integration | 451 | 0 | 0 |
| Engine integration | 235 | 0 | 0 |
| Presentation integration | 103 | 0 | 0 |
| Scene integration | 12 | 0 | 0 |
| Storage integration | 161 | 0 | 0 |
| Total | 1666 | 0 | 0 |

The executable unit selection and library doctests each ran zero tests. The
native GPU harness was compiled and linted, not executed. The existing macOS
linker warning, `__eh_frame section too large`, was emitted by debug linking and
remains unsuppressed. Clippy emitted no warnings. This is a successful canonical
command with that known linker warning, not a zero-warning platform acceptance.

The first canonical attempt failed to compile the engine test root because its
shared UTC fixture had not been registered. The parent registered the same
fixture in the library test module and desktop/engine integration roots, removed
an unnecessary native-root registration, and then ran the complete command
above. Compilation corrections are not semantic regression reds.

## Behavior established before the canonical gate

Played saves retain typed accepted edit causes and original UTC separately from
monotonic clock checkpoints. The canonical library adapter updates document and
projection atomically. Preparation reserves receiving-client bytes before CPU
admission; DTO and output growth remain charged and the permit transfers to
prepared ownership. A real exhausted-budget regression first produced 13 passes
and one failure. After repair, all 18 played cases and two private writer-memory
cases passed.

Prepared terminal lifecycle transitions retain an unforgeable pending result
when UTC sampling fails. Clocks stop without exposing an accepted outcome.
Explicit Retry Save accepts that same pending outcome at actual retry UTC;
navigation and safety checkpoints do not invent content timestamps. All 22
lifecycle cases, 26 supervisor content cases and 15 play-panel cases passed.
The original study-continuation review run had 24 passes and two failures: one
actual-effect timestamp regression and one invalid fixture clock origin. The
parent corrected the fixture separately and made study selection of an existing
continuation preserve modification time when neither tree nor study tip changes.

The accepted engine move now yields its checkpoint before a separate premove
computation. The actual injected second-clock-read regression failed before the
repair. All seven focused premove cases and the full later desktop/domain suites
passed. The earlier accepted move and its save ownership survive a later premove
time failure.

Native startup adopts canonical played and notation documents without rewriting
their original bodies or Unknown timestamps. Only actual unfinished played
sessions enter the runtime registry. Individual corrupt/future records retain
bounded recovery findings while valid sessions continue. Both inventory cases,
nine startup cases and all 232 desktop cases passed, including the two originally
observed non-played recovery failures. Production resumed saves use the indexed
played adapter.

The staged import framework retains its primary failure and cleanup owner
across cancellation and response-cache eviction. Unix orphan cleanup verifies
descriptor identity before unlink. Annotation comparison anchors against the
selected mainline and retains exact classes without treating partial matching
as transitive. Typed commit retrieval preserves the committed frame proof.
All 74 import framework/review cases passed after their observed regressions.

Measured deterministic stage reads for 32 and 128 complete conflicting records
fell from 4,104 and 53,768 to 744 and 2,952. Identical-record reads are 164, 316
and 620 at 8, 16 and 32 records. These are controlled operation counts, not
optimized throughput, RSS or latency measurements. Compatible partial-identity
classes can still require quadratic comparisons.

## Durable evidence and remaining work

Nineteen raw gate/focused/red logs and `SHA256SUMS` are retained under
`.git/parallel-patches/library-runtime-import-integration-20261017/`.
The final canonical log is `library-runtime-import-make-verify-2.log`, SHA-256
`58f7a46a27401d528463079f84c450b7edaf08e19550f5c54cba0c7f5c1af801`.
Existing baseline bundles retain the tests-only patches and original source
blobs for the engine checkpoint, native recovery, import and study-effect reds.

The real atomic SQLite import repository, receiving-budget candidate bodies,
typed candidate keysets, actor-owned adoption certification, schema-derived
space bound and real late-frame/cancellation/SQLITE_FULL rollback are subsequent
integration work. The framework's current receiving repository remains a port.
T097 and T100 remain pending until physical staging/backend acceptance.

T104 still needs correlated pause/save/close prerequisites before approved Trash
plans can account for their own lifecycle revision changes. T106 and the mounted
library, import/export, folder/Trash and annotation flows remain pending. No full
US3, native GPU, accessibility, other-platform, installed-package, large-corpus,
whole-process memory or release acceptance follows from this checkpoint. Task
markers remain 94 complete and 201 pending out of 295.
