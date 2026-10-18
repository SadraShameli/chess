# Engine developer workflow evidence

T087 is verified on the current host. DEV-06 retains its source-backed fullmove
discrepancy, the Full PGO workflow completed all 58 receipts, and the controlled
SPSA build established its precise upstream unavailability. The current source
passed canonical verification with 1,029 tests and all three Clippy selections.
[Final integration evidence](tool-integration-20261016.md) records exact driver
and engine identities, preserved warnings, cancellation and platform limits.

The sections below preserve the earlier source-handoff and execution chronology.
Their pending checks describe those earlier revisions; the integration record
owns current acceptance. No production PGO or performance acceptance is claimed.

## Source identity and inspection

On 2026-10-16 the implementation agent fetched the existing approved engine tag
into `/tmp/chess-t087-reckless-source`. `git rev-parse HEAD` returned
`0e92358f5acd66e5ac77b1bf558202e47c515435`, matching
`EnginePackageManifest::COMMIT`. The cached
`/tmp/chess-reckless-uci-0e92358.rs` compared byte-for-byte equal to this checkout's
`src/uci.rs`. No engine source or dependency file was edited.

The pinned `src/main.rs` collects each process argument into the command queue.
`src/uci.rs::message_loop` consumes complete strings, exits after the last CLI
command, and changes to interactive mode after `uci`. The fixed current-host
execution below confirms both queue modes, with the counter limitation retained.

The pinned `src/parameters.rs` defines an optional SPSA macro. A source search
found no `define!` invocation; `src/uci.rs` calls the absent generated
`print_options` and `set_parameter` functions under `spsa`. The controlled compile
below confirms this source finding; no tuning implementation is supplied.

## Installed tool observations

Read-only version queries observed:

| Tool | Actual observation |
| --- | --- |
| rustc | 1.99.0, commit b940084d7eb6a299eb4bfeb8e34901bc051e7ac4, Homebrew standalone binary, aarch64-apple-darwin, LLVM 23.1.2 |
| Cargo | 1.99.0, commit 5f94df4789f005f9a352888e8355ffc645b7ed0e |
| Clang | Apple clang 21.0.0, clang-2100.3.34.2, arm64-apple-darwin27.0.0 |
| Explicit profile tool | `/opt/homebrew/opt/llvm/bin/llvm-profdata`, Homebrew LLVM 23.1.3 |
| Apple profile tool | `/Library/Developer/CommandLineTools/usr/bin/llvm-profdata`, Apple LLVM 21.0.0 |
| PATH profile tool | `llvm-profdata` was not present on PATH |

LLVM 23.1.3 profile compatibility with rustc's LLVM 23.1.2 has not been tested.
Apple LLVM 21 is not selected as a fallback. No compiler, component, dependency
or other tool was installed. The existing model at
`/Users/sadrashameli/Personal/chess/resources/.cache/networks/v54-5478683c.nnue`
was supplied by the parent. The parent's controlled workflow verified its
63,266,880 bytes and canonical SHA-256 before compilation. It was not copied or
independently rehashed by the implementation agent during the source-only lane.

## Implementation and verification status

The owned source provides the three commands described in
[the workflow guide](../../../docs/engine-workflows.md), registered in the real
CLI by the parent. The existing manifest supplies source/model pins and
portable flags. The capability registry supplies option validation and benchmark
defaults. A single injected budget/executor is reused; no production binary or
compiler profile is published or replaced.

The initial test-only source was exported before production code at
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/engine-developer-workflows-tests.patch`.
Its initial seven contracts cover encoded paths, invalid separators, source-backed SPSA
classification, output ownership, output-bound termination, cancellation and
deadline reaping. The parent ran these plus the existing build fixture with the
drop regression excluded: eight passed in
`/tmp/chess-developer-baseline-contracts-20261016.log`. That is baseline execution,
not a gate for the subsequent retention/counter changes.

The parent observed the dropped-wait regression fail before the retention fix:
`/tmp/chess-developer-retention-red-20261016.log` records one failed test and eight
filtered tests. After polling a real `/bin/cat` child to pending and dropping its
wait, capacity incorrectly returned to one; the assertion required zero until
reaping. The final test now calls `shutdown_retained` and checks that capacity
returns only afterward. The process and termination helper are fields of the
journal-owned process owner. Pipes, buffers, handshake state, write offset and
absolute deadline are retained across dropped sub-waits. An additional controlled
clock test checks that reobserving the wait preserves its original deadline.

The parent's real DEV-06 attempt in
`/tmp/chess-developer-validation.JUujXB/queued/command-003.json` exited successfully
after its queued commands, with stdin held open. The workflow itself failed
`Comparison`, recorded in its `report.json`, before the interactive-transition
stage ran. Actual output after `e2e4` was
`rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 2`, followed by
static evaluation `-16`. The fullmove counter should be 1. This is a real upstream
defect: pinned `src/board.rs:202-206` increments when Black is to move, and
`src/uci.rs:277-282` calls that method after applying the move.

The actual-output regression was exported before adding the canonical parser and
typed discrepancy handling. Its newly introduced helper API did not exist in the
baseline, so no compile error is called a semantic red. The actual failed CLI
comparison above is the observed behavior failure. The revised check uses the
existing `BoardPosition` parser/identity and verifies halfmove equality while
preserving expected and observed fullmove values in `UpstreamAfterWhite`. It
retains the `e2e4` fixture and a distinct discrepancy disposition. It claims
neither exact FEN equality nor general PRO-05 success. No vendor code was changed.

The parent subsequently ran the retained-owner/counter snapshot successfully:
`/tmp/chess-tools-contracts-20261016.log` records 24 tools-binary tests and 235
engine integration tests, all passing. The tools total includes the ten current
developer-workflow contracts, the existing build fixture, and thirteen coverage
command cases. The dropped-wait, resumed-deadline and actual-FEN tests explicitly
passed. These results precede the mechanical typed process-request refactor made
in response to four owned Clippy findings. The parent then reported canonical
`make lint` passing both all-target/all-feature and separately selected native
checks. The later exact-exit-status and explicit profiling-mode additions still
require their own parent checks.

The real revised DEV-06 command succeeded, logged in
`/tmp/chess-developer-queued-fixed-20261016.log`. Its retained directory is
`/tmp/chess-developer-validation.JUujXB/queued-fixed/`. The implementation agent
read and checked `report.json` and `package.json`: the disposition is
`queued_replay_verified_with_upstream_fullmove_discrepancy`, failure is null, and
both auto-exit and interactive observations preserve expected fullmove 1 and
observed fullmove 2, with the exact source references. Five command receipts
cover application commit/status, engine identity, auto-exit and the interactive
transition. This is scoped DEV-06 success with a disclosed upstream counter
defect, not exact FEN or general PRO-05 acceptance.

| Verified DEV-06 identity | Observed value |
| --- | --- |
| Engine target | aarch64-apple-darwin |
| Engine bytes | 65,383,664 |
| Engine SHA-256 | ab4128cd33149b14713475e049db8ddaff702ee79bfb412ecfbde91655273689 |
| Diagnostic application commit | 5e9f55fd94056050e3ba00d7480c7c87f78ac00c, with recorded uncommitted source status |
| Diagnostic executable SHA-256 | d16226a31427c6287e0c0c2dbe0ca42e974838a9330f8310eda99a15774f0fad |

## Controlled SPSA result

The parent completed the workflow in
`/tmp/chess-developer-validation.JUujXB/spsa/`, logged in
`/tmp/chess-developer-spsa-20261016.log`. The implementation agent read the actual
report and command receipts. The portable Syzygy release baseline in
`command-008.json` exited 0. The separate `syzygy,spsa` build in
`command-009.json` exited 101 with exactly these two errors:

```text
error[E0425]: cannot find function `print_options` in module `crate::parameters`
   --> src/uci.rs:158:24
error[E0425]: cannot find function `set_parameter` in module `crate::parameters`
   --> src/uci.rs:323:32
```

Upstream also emitted one unused `define` macro warning. The final disposition
is `spsa_unavailable_in_pinned_upstream`, with null workflow failure. This means
the diagnostic established the unavailable feature; it does not mean SPSA
compiled or tuning ran. The baseline prerequisite succeeded, and the final
source-status receipt was clean. No dependency, source or checker was repaired.

The report records aarch64-apple-darwin, detected capacity 14, exact source/model
identity, and upstream Cargo.lock SHA-256
`233762ff8fcb5d2c7a27437876d08c8e8b8dda7841a0728bddd59e0b492689c0`.
The controlled baseline SHA-256 is
`0364e20a21932f259e542113abbcc40ab4615a3df6105be400aa3e60413af8cc`.

## PGO Full attempt and explicit counts-only mode

The parent's original Full-mode workflow is preserved at
`/tmp/chess-developer-validation.JUujXB/pgo/`, with log
`/tmp/chess-developer-pgo-20261016.log`. Both baseline and instrumented release
builds succeeded. The instrumented executable has 65,589,184 bytes and SHA-256
`ac879056042325eb09360e56000ee8eef20097b067a6b1e92a1e6a81aa031fcd`.
Training in `command-014.json` used the complete queued argument
`bench 16 14 12` and the explicit module/process raw-profile filename pattern.
It printed `Bench: 53249249 nodes 3877105 nps`, then exited unsuccessfully after
14,260,228 microseconds. This single pre-crash training observation is not a
passing performance sample or PGO comparison.

That original receipt predates the exact-status enhancement and records a null
exit code. The matching macOS report
`/Users/sadrashameli/Library/Logs/DiagnosticReports/reckless-2026-10-16-231409.ips`
identifies training PID 46039 and records SIGSEGV/code 11 with exit frames
`initializeValueProfRuntimeRecord`, `writeOneValueProfData`, and
`lprofWriteDataImpl`. The implementation agent inspected that report read-only.
It also found the 69,240-byte raw profile for PID 46039. The root cause remains
unproven. No merge, optimized rebuild or comparison followed this failed process.
LLVM 23.1.3 merge compatibility was not established by this attempt.

The parent explicitly authorized a separate `CountsOnly` mode after querying the
pinned compiler's own LLVM help. `/tmp/chess-profile-options-20261016.log:856`
advertises `--disable-vp` as "Disable Value Profiling"; the obsolete
`enable-value-profiling` option is absent. The typed Inputs builder retains Full
as default and selects CountsOnly only when requested. Provenance serializes the
mode. CountsOnly appends `-C llvm-args=--disable-vp` to instrumented and profile-use
builds, leaving baseline/SPSA/production flags, detected workers and depth intact.
Its flag/config regression was exported before production mode implementation.
There is no automatic retry, fallback, vendor edit or suppressed validation.
The fresh parent-controlled `pgo-counts` attempt also failed training. Its report
is `/tmp/chess-developer-validation.JUujXB/pgo-counts/report.json`, with log
`/tmp/chess-developer-pgo-counts-20261016.log`. Provenance explicitly records
`counts_only` and capacity 14. The instrumented executable has 65,588,992 bytes
and SHA-256
`53bd8ba20e89c6ffa62664a1f9f9dad16b4df3ec2d88002b51659c4a66b55fa2`.
`command-014.json` again uses `bench 16 14 12`, prints
`Bench: 51904925 nodes 3825343 nps`, and records failure after 14,119,675
microseconds with exact status `signal: 11 (SIGSEGV)`, null exit code and empty
stderr. Its comparison and disposition remain null. The PID 57048 crash report
`reckless-2026-10-16-232243.ips` records the same invalid address 3/code 11; its raw
profile has 69,240 bytes. The implementation agent inspected these results
read-only. CountsOnly is experimental and is not a demonstrated workaround.

Parent-supplied matching LLVM 23.1.2 source inspection establishes limited facts:
`/tmp/chess-llvm-pgo-instrumentation.cpp:380-390` includes `DisableValueProfiling`
in the instrumentation guard, and the code around 1044 returns before inserting
value profiling; use-side annotation returns early around 1893.
`/tmp/chess-llvm-instr-profiling.cpp:2110-2117` still creates the profile data's
value-site array. `/tmp/chess-llvm-profile-value.c:310-337` reads that array and
traverses linked value-profile nodes when the recorded count is nonzero. These
source facts do not establish a mismatch in the linked runtime or a root cause.
No further flag, vendor, compiler, dependency or checker change was made.

Both controlled attempts stop on the abnormal child result. Neither reached
merge, profile-use rebuilding, deterministic comparison or repeated one/max
samples. Full remains the default. LLVM 23.1.3 profile compatibility and DEV-07
completion remain unverified, and T087 remains open.

| Required result | Status |
| --- | --- |
| DEV-06 real packaged queued auto-exit and post-uci stdin transition | Passed with typed expected1/observed2 upstream fullmove discrepancy preserved |
| DEV-07 baseline/instrument/train/profile merge/optimized rebuild | Full and CountsOnly baseline/instrument builds passed; both training processes crashed before merge; pipeline incomplete |
| DEV-07 deterministic behavior comparison and one/max repeated observations | Not reached; no PGO completion or performance claim |
| DEV-08 controlled baseline and SPSA feature compile | Verified unavailable: baseline passed, SPSA failed with exactly two E0425 errors, source unchanged |
| Eleven current contract tests and strict Clippy | Earlier ten-contract snapshot tools24/24 and engine235/235 passed; canonical lint subsequently passed; exact-status/CountsOnly revision checks pending |
| Standalone source formatting | rustfmt 1.10.0 parsed and formatted all three owned Rust files; no Cargo invocation |
| Windows/Linux process-tree and target workflows | Not run; native target prerequisites outstanding |
| Native application, accessibility and production PGO selection acceptance | Outside this diagnostic evidence; outstanding |

The parent reported a previously completed full verification of 975 tests and
Clippy, plus its optimized all-target build. Those checks preceded this new
source and do not validate T087. The separately selected native optimized build
owned the initial execution lane. The parent continues to own Cargo. No Cargo,
engine diagnostic, training, merge or compile command was executed by this agent.
No task marker, shared root, dependency, vendor file, commit or push was changed.

## Subsequent parent investigation and review correction

The preceding attempt records preserve their historical status. The parent
subsequently documented the isolated official Rust attempt, mixed Rust/C profile
controls, the matched-Clang Full success, and its exact artifacts in
[the integration evidence](tool-integration-20261016.md). That later Full run
completed the actual merge, optimized rebuild and repeated one/max comparison.
It retained eight upstream missing-profile warnings for cold formatting paths.
It did not select a production PGO profile or establish a performance advantage.
This successful diagnostic executable predates the following review correction.

Independent review found three ownership/provenance/publication defects. The
parent observed all four dedicated semantic regressions fail before production
changes: consuming finish returned while a held cleanup helper remained live;
clean changed HEAD retained stale source provenance; a same-length model
replacement retained its original digest; report publication erased the original
operation/cleanup failure. The red log is
`/tmp/claude-chess-pgo-diagnosis-20261016/developer-owner-red.log`, with zero passed,
four failed and 27 filtered tests. The fixtures closed helper stdin and drained
EOF before their assertions; no child remained after the red run.

The correction keeps the consuming journal alive through final forced helper,
tree and child cleanup and actual OS reaping before any return or report I/O.
The initial graceful deadline remains recorded; physical reaping can exceed it
while capacity remains owned. HEAD and model length/digest are checked along
with lockfile/status around every build and at final validation, without copying
the model per build. Typed combined errors preserve build/revalidation and
primary/publication failures and their sources. Unix-only fixture imports are
conditional. The exact-status and explicit Full/CountsOnly APIs are retained.

The parent corrected one test-only concrete error coercion to `error.into()`.
Its subsequent targeted command passed all four regressions, with 29 filtered,
zero failures and zero ignored tests, in
`/tmp/claude-chess-pgo-diagnosis-20261016/developer-owner-green-corrected.log`.
No child remained afterward. This is targeted review evidence; the final
canonical `make verify` and fresh independent review remain parent-owned and
pending. T087 is not marked complete by this handoff.
