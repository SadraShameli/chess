# Engine developer and coverage tool integration

This increment follows `5e9f55f` on `001-reckless-desktop-chess`. It adds the
registered developer workflows and capability-report command. Canonical
`make verify` passed: all 1,029 tests, three strict Clippy selections, optimized
all-target/all-feature build, and the separately selected optimized native
harness build. Final gate output is retained as `make-verify.log`.

| Suite | Passed |
| --- | ---: |
| Library | 84 |
| Tools | 44 |
| Assets | 5 |
| Desktop | 213 |
| Domain | 293 |
| Engine | 235 |
| Presentation | 102 |
| Scene | 11 |
| Storage | 42 |

No tests failed, were ignored or were filtered in the full run. The application
binary and library doctests contain no cases. The Clippy selections cover the
default tools binary, all targets/features, and the separately selected native
harness. The existing macOS debug-linker `__eh_frame` size warning remains
unsuppressed in Cargo test output. Source Clippy was warning-free.

## Controlled developer execution

The real packaged queued-command check passed with its explicitly retained
upstream fullmove-counter discrepancy. The controlled SPSA baseline compiled,
while the optional feature failed with exactly the two documented missing
parameter functions. These outcomes are detailed in
[developer workflow evidence](engine-developer-workflows.md).

Three initial PGO attempts stopped during profile writing after training:
Homebrew Rust 1.99.0 Full, the explicit CountsOnly mode, and an isolated official
Rust 1.99.0 Full attempt. None of those attempts reached merge. The official
attempt used command-scoped PATH selection after installing checksum-verified
official Cargo, rustc and standard-library components into
`/tmp/chess-rust199.lpwpio/toolchain`. The project pin and installed default were
unchanged. Its receipts identify Rust commit
`b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`, LLVM 23.1.1, and official Cargo 1.99.0.
Its baseline and instrumented build receipts also retain `rust-objcopy` warnings
about a missing temporary-prefix `libLLVM.dylib`. Those builds were not
warning-free.

The official training receipt records PID 66756, `bench 16 14 12`, 53,832,587
nodes, 4,025,206 reported NPS, 13,924,608 microseconds, and SIGSEGV. Its matching
`reckless-2026-10-16-233208.ips` crash report records address 3 and
`initializeValueProfRuntimeRecord`, `writeOneValueProfData` and
`lprofWriteDataImpl` during exit. Running only `compiler` on the same instrumented
binary also exited with SIGSEGV. Benchmark search is therefore unnecessary to
trigger that binary's failure.

An empty Rust profile control and a safe threaded dynamic-dispatch control
passed. The latter also passed with matching fat LTO, one codegen unit,
panic-abort and portable target flags. Linking a profiled Apple Clang 21 C object
into the safe Rust control emitted the actual diagnostic
`Runtime and instrumentation version mismatch : expected 11, but get 10`.
The same mixed control using Clang 23.1.3 passed without that diagnostic.
These controls do not establish a general Rust compiler defect.

Inspection of upstream's locked `cc` 1.2.22 `src/flags.rs:279-285` establishes
that it translates Rust profile generation/use flags into Clang profile flags.
The Fathom build had selected Apple Clang 21 even though Rust's LLVM was 23.
Its generated C profile records use 64-byte spacing. The linked LLVM 23 runtime
reads the Values field at offset 48, which contains a counter value in that
older record layout. The invalid pointer value 3 is consistent with the
observed failure. Both C and Rust instrumentation must use compatible profile
formats; changing only rustc did not establish that compatibility.

A fresh Full attempt selected the already installed Clang 23.1.3 using only:

```text
PATH=/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin
```

Rust remained Homebrew 1.99.0 with LLVM 23.1.2. The supplied profile tool remained
LLVM 23.1.3. Source, model, portable flags, depth 12, 16 MiB Hash and detected
capacity 14 were unchanged. No vendor source, default compiler or production
package was modified. This attempt completed baseline/instrumented builds,
training, actual merge, profile-use rebuild, identity and behavior checks,
warmups, and five alternating samples per binary at one and 14 workers.

The actual report at
`/tmp/chess-developer-validation.JUujXB/pgo-matched-llvm/report.json` has null
failure and disposition
`controlled_comparison_completed_no_performance_acceptance`. Its 45 receipts
retain every invocation. All ten one-worker samples have exactly 3,588,649
nodes. The original fullmove discrepancy remains represented in both compared
builds. The optimized build emitted eight missing-profile warnings for cold
standard-library formatting paths. Those warnings remain in command 016;
neither the warning flag nor upstream code was changed to suppress them.

| Artifact | SHA-256 |
| --- | --- |
| Diagnostic executable | `123d90fe7a8832fe86332d3f2148a0ca9eed3b5e6eda11e239aa1ee81e95d8fd` |
| Baseline engine | `8be26cdae224a7d57738aa1dac501d0f5321c6a87e289f4992075026e509bd1e` |
| Optimized engine | `c98ef935d7125e29b2067ca7cc78137d4fdfdd54606b86b5e3f68f182e8b7e3f` |
| Embedded model | `5478683cb1bababde29ae8f29468a99846726548fc6a0ed54cac40ab6d38efbf` |

The report proves this controlled workflow on this host. It does not select a
production PGO profile, establish playing strength, support another target, or
justify a speedup claim. Multithreaded node counts vary and remain observations.
The diagnostic executable predates the subsequent review corrections described
below; current source verification is tracked separately.

## Coverage command boundary

The registered coverage command verified the current package and published a
complete report with 48 classified rows, all explicitly outstanding. It
reported zero measured passes, zero measured failures and zero upstream
unavailability. The summary says evidence is unsatisfied and T090 remains open.
Package verification alone creates no measured capability success.

The base schema and concrete artifact-import/publication adapter are described
in [coverage evidence](engine-capability-coverage.md). T090, T091 and T092 still
require their real capability, timing and lifecycle acceptance.

## Independent review regressions

The parent observed both new coverage regressions fail against the reviewed
source before any corresponding production edit:

- Valid distinct-path aliases with equal artifact kind/content failed after
  content-addressed normalization created duplicate observation paths.
- Replacing an already checked intermediate directory with a symlink allowed
  an outside artifact read, violating selected-report confinement.

Each isolated command ran one test, failed it, and filtered 26 others. Logs are
`coverage-alias-red.log` and `coverage-path-red.log` under
`/tmp/claude-chess-pgo-diagnosis-20261016/`.

Four additional private-owner regressions also failed before production fixes:
consuming finish returned with a live cleanup helper, a changed clean HEAD
retained stale provenance, a same-length model replacement retained its old
digest, and report publication erased the original operation/cleanup failure.
That command recorded zero passed, four failed and 27 filtered cases in
`developer-owner-red.log`. Fixtures cleaned their processes before assertions;
no child remained after the failed run. After the ownership/provenance/error
repairs, all four passed with 29 other tests filtered. The first recompile
exposed a test-only boxed-error inference problem; changing the conversion to
the declared error boundary fixed compilation. The subsequent successful log
is `developer-owner-green-corrected.log`. The final forced drain retains the
helper, child and lease through physical reaping even after the graceful
deadline. That OS observation can exceed the graceful bound. No final gate or
task acceptance is asserted by this draft.

The first coverage repair then passed all 16 targeted command cases, including
the original two regressions and a new copy-error revalidation case. Fresh
review found a related output-side gap: an unchanged bundle root could contain
a replaced artifact-kind directory. Its deterministic publication regression
failed before production changes, one failed and 32 filtered cases in
`coverage-publication-red.log`. Publication must also retain directory ownership
through artifact links and the final report commit. Windows descriptor-level
confinement and native filesystem acceptance remain open.

The next developer-tool review found two terminal-receipt paths beyond the
first four regressions. Both new cases failed before their repair: cleanup hid
the stored Cancelled outcome, and a conflicting `command-000.json` hid a failed
child's outcome behind publication I/O. The current source derives the primary
failure before cleanup, publishes a recovered completed receipt after reaping,
and retains one bounded inline receipt when its separate file cannot be
published. That retained receipt blocks further command admission. The combined
six owner regressions then passed, with 29 other tests filtered, in
`developer-terminal-green.log`. Fresh read-only review found no further concrete
blocker in those repaired paths. Full-source final gates remain pending.

The descriptor-bound publication repair subsequently passed all 20 command
cases, including retained kind-directory and final-parent replacement fixtures.
The next review found the earlier preparation interval still used pathname
staging writes and automatic `TempDir` cleanup. Two deterministic regressions
then reproduced outside staging writes and deletion before publication began.
The import fixture first needed its selected input canonicalized on macOS;
that setup error was not counted as a behavior red. The corrected case failed
its outside-write assertion in `coverage-staging-import-red.log`. Stage ownership
must cover construction through every preparation and publication error exit.

The subsequent repair owns descriptor-based staging from initial creation and
explicitly finalizes preparation, runner, encoding and publication outcomes.
All 24 command cases passed, including both observed staging regressions,
runner cancellation and dual cleanup-error preservation. Final read-only Unix
review found no remaining concrete blocker in those paths. Windows stronger
confinement, native filesystem and power-loss acceptance remain open.

The default-feature tools Clippy selection exposed enum names whose common
prefix was hidden by the optional asset-tools variant. The owning variants
were renamed without changing command strings. The corrected targeted selection
passed, and `make lint` now includes that default CLI selection before its
all-feature and named-native selections. No lint was disabled or relaxed.
The current debug driver build passed. A fresh Full PGO attempt using the
reviewed retention/provenance/terminal-receipt source also completed successfully:
`pgo-verified-owner/report.json` records 58 receipts, null failure and the same
explicit no-performance-acceptance disposition. It rechecked actual HEAD,
lockfile and model around all three builds and at final validation. Its ten
one-worker samples all contain 3,588,649 nodes; five samples per build also ran
at 14 workers. Command 026 preserves the eight missing-profile warnings plus
Cargo's aggregate warning line. No profile warning was suppressed.

| Current verified PGO artifact | SHA-256 |
| --- | --- |
| Current diagnostic driver | `721b6bc2b952e44becc7db095a12fd0bf386f5be53a8dd61996956c1bdf99b67` |
| Baseline engine | `8be26cdae224a7d57738aa1dac501d0f5321c6a87e289f4992075026e509bd1e` |
| Optimized engine | `881892f7091e079345051383419361acb50222964669e4b4db1efbc209150bde` |

The baseline is byte-identical to the earlier matched-toolchain baseline. The
optimized artifact differs after a separate multithreaded training run; no
deterministic optimized-binary or performance claim is made. Coverage staging
repairs remain separate from this verified PGO source revision.

The real Ctrl-C probe then observed the admitted child process group, signalled
the current pinned operation, and saw a cancelled report with no surviving
group after return. The original probe used the wrong macOS `ps` query and did
not observe a child, so it was not counted as cleanup proof. Its corrected
record is `current-cli-cancellation-observed.json`. The single observed timing
is not a native Stop percentile or lifecycle acceptance result.

The actual diagnostic JSON reports and command receipts are archived locally
under `.git/parallel-patches/tool-integration-20261016/`, with a SHA-256 inventory.
The measured host is macOS 27.0.1 build 26A434, arm64 Mac16,8, 24 GiB RAM and
14 available logical CPUs. This is not minimum-macOS or cross-target evidence.
