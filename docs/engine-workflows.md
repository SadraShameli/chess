# Reckless developer workflows

These diagnostics implement the DEV-06, DEV-07 and DEV-08 workflows in the
[pinned capability inventory](../specs/001-reckless-desktop-chess/contracts/reckless-capabilities.md).
They do not publish an engine, replace the packaged executable, change production
compiler profiles or establish native application acceptance. Current execution
status is recorded in [the feature evidence](../specs/001-reckless-desktop-chess/evidence/engine-developer-workflows.md).

## Inputs and output ownership

The tool accepts an existing Reckless checkout at the exact commit owned by
`EnginePackageManifest`, an existing verified NNUE file, a new output directory
whose parent already exists, and an explicit `llvm-profdata` executable for PGO.
Use the existing model cache directly; a second model copy is unnecessary.
An output path that already exists is refused even when empty. Build targets,
profiles, binaries and JSON receipts remain beneath the newly claimed directory,
outside the source tree. Use a fresh directory for every attempt.

The source must be the repository root with matching HEAD and a clean tracked
and untracked status. Source and model identity are checked before compilation;
the source status and Cargo.lock digest are checked again after the workflow.
Upstream's normal build script generates its ignored Fathom bindings; the tool
does not edit upstream Rust, dependency declarations or its lockfile.

Builds use the upstream locked dependencies offline, retain Syzygy, and derive
portable encoded Rust flags from the application's target manifest. The injected
capacity budget determines Cargo's worker allocation and is reserved for each
build. Inherited Rust flags and compiler wrappers are cleared. Paths containing
spaces remain a single encoded flag argument. Control characters are rejected.
The upstream Makefile's host-native flags and output move are not used.

## Queued process arguments: DEV-06

Run the registered command:

```sh
cargo run --locked --bin chess-tools -- engine-queued-check /tmp/chess-queued-new
```

The packaged engine is selected and checksum-verified through the existing
package selector. Its argv queue contains complete protocol commands, for example
`["position startpos moves e2e4", "d", "eval"]`. These are three arguments passed
directly to the process API. Splitting each command into individual argv tokens
does not implement Reckless's queue protocol.

The first child must replay the domain position and exit after the final
queued command while its stdin remains open. A second child receives queued
`compiler` and `uci` commands. The test waits for `uciok`, then sends the position,
inspection and quit commands over stdin. Each observed FEN is parsed through
`BoardPosition` for the declared variant and compared by position identity and
halfmove count. This establishes the transition from queued CLI commands to interactive
UCI, followed by a reaped process. It does not use `isready` to fence a search.

The pinned engine has a confirmed fullmove-counter defect: after `e2e4` it prints
fullmove 2, while the domain correctly records 1. Its
[counter owner](https://github.com/codedeliveryservice/Reckless/blob/0e92358f5acd66e5ac77b1bf558202e47c515435/src/board.rs#L202-L206)
increments when Black is to move, and the
[caller](https://github.com/codedeliveryservice/Reckless/blob/0e92358f5acd66e5ac77b1bf558202e47c515435/src/uci.rs#L277-L282)
invokes it after applying the move. The fixture retains `e2e4`. A typed
`UpstreamAfterWhite` discrepancy preserves expected and observed fullmove values
and the exact source locations. The final disposition explicitly reports this
counter defect; it does not claim exact FEN equality or general PRO-05 acceptance.
Other identity, halfmove or counter mismatches fail the diagnostic.

## Controlled PGO: DEV-07

The command interface is:

```text
chess-tools engine-pgo <source> <model> <new-output-directory> <llvm-profdata> [--counts-only]
```

Pass an installed stable profile tool explicitly. A successful version query
alone does not prove compatibility with rustc's LLVM. The instrumented training
must produce nonempty raw profiles, and the supplied tool must actually merge
those profiles successfully before the optimized build is attempted. No fallback
to a different LLVM tool or compiler occurs.

`Full` is the default instrumentation mode. The optional `--counts-only` flag
explicitly selects `CountsOnly` through the typed Inputs builder. The selected
mode is serialized in provenance. Counts-only mode appends
`-C llvm-args=--disable-vp` to the instrumented and profile-use builds; baseline,
SPSA and production portable flags are unchanged. The pinned compiler's actual
LLVM help advertises this option as "Disable Value Profiling". The old
`enable-value-profiling` flag is absent and is not used. Counts-only data omits
value profiles and must be assessed as that distinct profiling mode.

The first current-host Full-mode attempt built baseline and instrumented engines
and completed the training bench's output, then terminated abnormally during
process exit. The matching macOS crash report records SIGSEGV, with profile-write
frames; the root cause is unproven. It did not reach profile merge, optimized
rebuild or comparison. The original failure report is preserved. Counts-only is
an explicitly requested controlled mode, with a fresh output directory and the
same detected worker budget and canonical depth. There is no automatic retry or
fallback, and no vendor, toolchain or checker modification. Its complete pipeline
must pass independently before any completion claim. Neither mode changes the
packaged engine.

The controlled sequence is:

1. Verify source, lockfile and model identity, and record the diagnostic executable,
   application commit/status, rustc verbose version, Cargo verbose version, Clang
   version and profile-tool version.
2. Build a portable release baseline in `baseline/` and verify its advertised
   options through the existing typed capability registry. Exercise DEV-06.
3. Build the same pinned source with `profile-generate` in `instrumented/`.
   Run its built-in 45-position corpus with the canonical benchmark depth/hash
   defaults and the detected shared CPU budget. Raw profile names include the
   process and module identities.
4. Merge the generated profiles into `merged.profdata`, record their hashes, and
   rebuild with `profile-use` in `optimized/`. Both instrument and use paths are
   carried through `CARGO_ENCODED_RUSTFLAGS`.
5. Check optimized identity/options and actual inspection/static-evaluation output
   against baseline. Warm up both binaries, then collect five alternating
   baseline/optimized samples at one worker and the detected maximum. A one-worker
   host runs the distinct worker configuration once. Record node totals, reported
   NPS and process elapsed microseconds for every sample. Deterministic one-worker
   node totals must agree across both binaries and all samples. The queued
   observations retain the known upstream fullmove discrepancy for both builds;
   agreement between builds does not repair or conceal that defect.
6. Verify the source/lockfile are unchanged and write the final report.

The report includes exact executable, profile and model SHA-256 identities,
target, arguments, explicit environment overrides, exit outcomes, capacity,
stdout/stderr and all sample observations. Exit outcomes preserve the platform's
`ExitStatus::to_string()` alongside success and exit code, without guessing the
cause of an abnormal termination. Multithreaded search node counts may
vary and are retained as observations. This fixed corpus is a diagnostic behavior
comparison; it does not establish playing strength, broad workload superiority,
minimum-hardware support or native performance acceptance. Review measurements
under [the canonical performance rule](../.specify/rules/performance.md) before
selecting any production profile.

## Optional SPSA compile: DEV-08

```text
chess-tools engine-spsa-check <source> <model> <new-output-directory>
```

This first builds the same portable Syzygy baseline to establish functioning
prerequisites. It then attempts the isolated release build with `syzygy,spsa`.
The pinned source calls `crate::parameters::print_options` and
`crate::parameters::set_parameter` from `src/uci.rs`; `src/parameters.rs` only
defines the macro that would generate those functions. There is no macro
invocation in the pinned source.

Only the controlled compile's two matching E0425 missing-function diagnostics
qualify as `spsa_unavailable_in_pinned_upstream`. Missing compilers, libclang,
dependencies, cancellation, deadline or other errors fail the diagnostic. An
unexpected successful build records compile success with runtime tuning still
unverified. The tool does not provide a tuner or repair the vendor source.

## Process lifetime and reports

The composition root supplies one shared `CapacityBudget` and `CapacityExecutor`
through `ToolsEnginePgoResources`; diagnostics create neither another budget nor
another worker pool. Each command reserves its declared workers. There is one
owned child at a time, concurrent bounded stdout/stderr draining, a cancellation
token, and an absolute stage deadline. The journal retains its process owner;
the child, lease, pipes, buffered output, handshake state, stdin write offset and
original deadline survive a dropped sub-wait. `shutdown_retained` observes the
owned termination helper and child before releasing capacity. Reobserving a
pending command does not reset its deadline. Each pipe is limited to half the existing engine
diagnostic byte budget. Build stages allow 30 minutes, training/samples 10 minutes,
and inspection/merge/queued checks 30 seconds. Cancellation and deadlines terminate
the owned process group on Unix or process tree on Windows and await reaping,
with the existing engine shutdown bound. These are diagnostic bounds, not changes
to live-game stop or shutdown policy.

Callers request cancellation through `ToolsEnginePgoCancellation::cancel` and
continue awaiting the same top-level operation before shutting down the executor.
The composition root owns Ctrl-C signalling and retains that pinned run until its
result is observed. A caller that abandons an inner wait keeps its process owner
and awaits `shutdown_retained`. Do not drop the owner while cleanup is incomplete.

Each completed command writes a new `command-NNN.json` receipt. `report.json`
records the final disposition or failure and refers to those receipts. A failed
attempt leaves its owned output available for inspection and requires a new
directory for retry. DEV-06 also writes the verified `package.json` receipt.
No missing prerequisite is converted into successful PGO or engine acceptance.

## Verified C profiling prerequisite

Reckless's Fathom C compilation must use a Clang whose profile-data ABI is
compatible with Rust's LLVM instrumentation and linked profiling runtime.
The locked `cc` 1.2.22 implementation translates Rust profile generation/use
flags into C compiler flags in `src/flags.rs:279-285`. Selecting a compatible
`llvm-profdata` alone does not establish compatibility for that C object.

The initial Homebrew Full, explicit CountsOnly and isolated official Rust 1.99.0
attempts remain recorded in
[the integration evidence](../specs/001-reckless-desktop-chess/evidence/tool-integration-20261016.md).
They used Apple Clang 21 for Fathom. A safe mixed Rust/C control emitted
`Runtime and instrumentation version mismatch : expected 11, but get 10`;
the corresponding Clang 23.1.3 control passed. This establishes the C profiling
prerequisite without a general compiler-fault conclusion.

A later Full-mode run selected the already installed LLVM Clang 23.1.3 through a
command-scoped PATH prefix, with Homebrew Rust 1.99.0/LLVM 23.1.2 and profile tool
23.1.3. It kept the source, model, portable flags, 16 MiB Hash, depth 12 and 14
detected workers. The complete pipeline passed, including actual merge and
profile-use rebuilding, followed by five samples per build at one and 14 workers.
All ten one-worker samples had 3,588,649 nodes. The typed fullmove discrepancy
remained visible. The optimized build emitted eight missing-profile warnings for
cold standard-library formatting paths; they were retained without suppressions.
This controlled result does not select a production profile or establish a
speedup, playing strength, minimum-platform support or native application
acceptance. The initial failures remain preserved; CountsOnly remains an
explicit experimental mode, with no automatic selection or fallback.

## Final ownership and failure handling

The shutdown deadline bounds the initial cleanup attempt. If it fails, the
consuming workflow retains its journal, process, helper and capacity lease while
forcing and observing helper/tree/child termination. Actual OS reaping can exceed
that graceful bound. The driver does not return or publish the report until its
owned helper and child have been observed reaped. Failed OS observations retain
ownership and use delayed observation instead of releasing capacity or spinning.
The initial operation and cleanup errors remain typed after the final drain.
These developer-tool semantics do not change live-game stop/shutdown policy.

The current source rechecks HEAD, Cargo.lock, model byte length and model SHA-256
before and after each build, and again at final validation. It reuses the model
file without making a per-build copy. A changed input fails verification. Failed
builds and failed revalidation preserve both causes. If report publication also
fails, the returned typed error preserves the original operation/cleanup error
and the publication error; an existing report is not overwritten.
