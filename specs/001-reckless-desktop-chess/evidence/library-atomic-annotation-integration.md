# Atomic library import and annotation integration

This increment follows the verified `465837a95c0de9129460171ea57a5edb2d054aac`
library/editor checkpoint. The checkpoint's acceptance does not cover the source
changes described here. Task and native acceptance remain open until their
complete requirements have observed evidence.

## Integrated ownership

The SQLite actor owns adoption certification, bounded candidate pages and one
Immediate import transaction. Certification is reset on reopen and revoked by
successful generic writes or findings. Import body/projection proofs bind the
staged bytes to their validated record. Cancellation and rollback remain owned
until observed. Folder and tag metadata reads use catalog-bound bounded pages.

Notation annotations retain one canonical record, original application receipts,
accepted edit UTC, variation proofs and computation/save/publication futures.
Published snapshots share a receiving-byte permit through their final clone.
The owner and its retained futures own an executor Arc; they can move into a
service task without borrowing a sibling executor field. Save completion returns
the actual SQLite content receipt and its covered game revision.

Played annotations enter the existing supervisor's ordinary queue and use its
canonical session take/replace ownership. They preserve real live position,
outcome and clock state. Each editor receipt retains its own actual repository
acknowledgment independently of a later source publication. Publication pressure
retains the accepted edit, original UTC and application receipt.

The executor input carries the request identity, receipt time and cancellation
token captured from the admitted application receipt. The supervisor retains the
full receipt outside the CPU closure. No unwind-safety assertion, duplicate
session or alternative storage codec is introduced.

## Observed regressions

The snapshot-memory regression failed before repair after a valid 256 KiB note
was saved. Retaining only a published result clone incorrectly allowed the full
receiving budget to be reserved. Its baseline is preserved under
`.git/parallel-patches/annotation-snapshot-memory-baseline-20261017/`.
All 38 annotation command/owner tests subsequently passed. The two later owner
contracts for task ownership and actual SQL acknowledgment also passed in the
private desktop run described below.

The checkpointed unrelated-overflow regression failed before the space repair.
Its identical tiny-import controls observed 65,478 and 65,466 additional named
bytes. A 45,909,042-byte allowance was incorrectly refused with a 50,113,532-byte
requirement. The exact red and source are preserved under
`.git/parallel-patches/import-space-false-refusal-baseline-20261017/`.
The repair accounts for additional DB/WAL/SHM growth, bounds statement-journal
bytes and enforces a scoped SQLite page ceiling. Restoration follows observed
commit or rollback; unresolved cleanup preserves causes and retires the actor.
The original uncheckpointed-backlog refusal remains covered.

The queued-editor clock regression failed in the first compiled private desktop
run and again under its exact filter. The exact run had zero passes, one failure
and 548 filtered tests. Red log SHA-256:
`cdfe5e5cbfd085f8cc1a00dcb7c9265f3a037adadc3c222e18c2e3e4069d4dab`.
Frozen supervisor blob: `a53de5accd7c1aa968524db8c9bab08257b025b3`.
The durable baseline is
`.git/parallel-patches/played-editor-clock-baseline-20261017/`.
The scheduler now checks due clocks before dispatching ordinary work and reuses
the same adjudication operation as its timer branch. Already-received engine
moves and their premove continuations retain canonical timestamp processing
before that ordinary-work boundary. An initial broader ordering timed out the
existing premove-at-deadline fixture; narrowing the boundary restored its real
accepted-move and flag-fall behavior. All 26 played-editor tests then passed in
the 283-case private desktop selection.

The shutdown-publication regression failed after real receiving-memory pressure
accepted a note edit. First, second and final shutdown returned `SaveRequired`;
retrying the original application receipt returned `Closed`. Source preservation,
original UTC, memory/capacity release and storage/executor cleanup checks all ran
before the final semantic assertion. The exact run had zero passes, one failure
and 550 filtered tests. Red SHA-256:
`b98ec84daef793b48170d493bba315c8870d742393409c1f55952867a630ceac`.
Frozen supervisor blob: `0a2b445f28e781523f09983fed393abef038be7f`.
The baseline is preserved under
`.git/parallel-patches/played-editor-shutdown-baseline-20261017/`.

The repaired supervisor retains the original publication envelope. Explicit
shutdown retries that same publication through the owner even after ordinary
admission closes, then observes its real save and final projection. It does not
replay the edit or reread UTC. The unchanged regression passed, including actual
receipt correlation, persisted notes/revision/timestamp and observed cleanup.

## Verification scope

Commands used the pinned Rust 1.99.0 toolchain, Cargo's default parallelism and
`CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target`. Cargo commands were
sequential and root Rust source was frozen during each command.

Logs currently reside in `/tmp/claude-chess-pgo-diagnosis-20261016/`.

| Log | Observed result | Source scope |
| --- | --- | --- |
| `library-atomic-metadata-combined-tests-2.log` | 473 private, 234 desktop and 181 storage passed | Before subsequent editor/publication/space repairs |
| `library-study-public-compatibility-2.log` | 234 desktop, 468 domain and 103 presentation passed | Study effect prerequisite, before played forwarding |
| `annotation-snapshot-memory-repaired-tests.log` | 38 passed, 474 filtered | Publication-memory repair |
| `library-import-space-repaired-tests-2.log` | 40 passed, 485 filtered | Space repair including the preserved red and 1,000-record admission |
| `library-annotation-played-private-tests-2.log` | 280 passed, one queued-clock failure, 268 filtered | Includes the two new owner contracts |
| `played-editor-clock-repaired-tests.log` | 24 passed, 525 filtered | Clock repair with real played-editor contracts |
| `library-annotation-played-clippy-3.log` | Exit 0 | All-targets/all-features Clippy before shutdown repair |
| `library-annotation-shutdown-repaired-tests.log` | 283 passed, 268 filtered | Clock compatibility, both review cases and retained shutdown repair |
| `library-atomic-annotation-make-verify.log` | Exit 0, 1,833 passed, zero failed or ignored | Final formatting, all three Clippy selections, default hermetic suites and both optimized builds |

The final suite totals were 551 private library, 44 asset-tool, five CLI,
234 desktop, 468 domain, 235 engine, 103 presentation, 12 scene and 181 storage
tests. The executable and doctest selections each contained zero tests. The
optimized all-targets build finished in 6m 11s, followed by the separately
selected native-harness build in 52.16s. These are gate elapsed times, not
application performance measurements. The native harness was compiled, not run.

Logs and the exact staged integration patch are preserved under
`.git/parallel-patches/library-atomic-annotation-integration-20261017/`.

The first space-repair compilation exposed unsupported `u64` SQLite row reads.
They were replaced with signed reads and checked conversion. The first played
compilation exposed a full application-result graph crossing the executor's
unwind boundary. Capturing only the required request inputs resolved it. Neither
compile failure is reported as a semantic regression.

## Remaining acceptance

The reviewed import estimator still inventories existing overflow payloads using
`dbstat` during freeze and commit. A structural regression with canonical-query
VM counters is source-ready and remains unrun at this checkpoint. Bounded
planning work and cancellation at large installed scale require their own
observed regression and repair. Anonymous
statement-journal temp-volume admission, physical high-water, filesystem
copy-on-write demand and million-game performance remain unverified.

The admitted library service, absent saved-game opening, source-bound export, native file-handle
flows, close/Trash certification and mounted library/annotation controls remain
integration work. Locked-desktop GPU acceptance was not retried. Existing macOS
`__eh_frame section too large` linker warnings in prior compatibility commands
remain distinct from their passing tests and are not suppressed.
