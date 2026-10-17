# Report model and scoring foundation

Source-only handoff for the model/scoring portion of T156, T160 and T161.
Base: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
No task checkbox is changed. This is not US7 or full13journey acceptance.

The implementation follows [FR-018 and US7](../spec.md), the selected
[report policy](../decisions.md#report-scoring-and-job-scheduling),
[study contract](../contracts/study-tools.md#full-game-analysis-reports),
[model](../data-model.md#reports-openings-and-puzzles), and
[tasks](../tasks.md). The constitution, catalog and all seven complete
[canonical engineering rules](../../../.specify/rules/README.md) were read.

## Owned changes

- `src/report/mod.rs`: declarations only.
- `src/report/source.rs`: bounded immutable line capture, actual `GameId`,
  `GameRevision`, `GameMoveId`, full `BoardPosition` snapshots, and a typed
  version-1 SHA-256 digest using the canonical package digest owner.
- `src/report/configuration.rs`: a finite budget wrapping `EngineSearchLimit`,
  and actual `EnginePackageSelected`, `EngineIdentity` and `EngineOptionsApplied`
  captured from an initialized, idle, resource-configured engine.
- `src/report/job.rs`: aggregate admission, checked attempts/counts/progress,
  per-position state, actual engine binding, completed results, interruption
  causes, pause/cancel/relaunch transitions, and irreversible outdated marking.
- `src/report/finding.rs`: borrowed adjacent-node findings, original typed engine
  scores/statistics/PVs/search tags, candidate alternatives, and application
  classification. A latest primary score remains available without a PV;
  a latest primary bound does not fall back to an older exact candidate.
- `src/report/scoring.rs`: the single selected versioned application policy,
  typed unscored reasons, separate mate distances/missed-mate status,
  forced/best flags, and per-player accuracy/sample accounting.
- `src/report/replacement.rs`: an immutable proposed job plus a checked prior
  report signature. Explicit confirmation releases the proposed job; the old
  report is never mutated or consumed by the preview.
- `tests/report/job.rs`, `tests/report/scoring.rs`, `tests/report/fixture.rs`:
  26 authored tests, comprising 16 job/source/configuration tests and 10 scoring
  tests. The fixture uses real boards, trees, package selection, bounded channels,
  and real engine/search session owners over an in-memory process boundary.

No shared root, engine, game, notation, schema, storage, desktop, UI, scheduler,
service or task-list file is changed. No synthetic `GameSession`, second engine
score type, option registry or scoring-policy registry is introduced.

## Identity, lifecycle and memory contracts

Capture accepts the existing tree and selected tip. It copies only the selected
root-to-tip positions, never the source tree. The digest includes ordered stable
node IDs, variant, canonical full FEN including counters, and canonical moves.
Equality checks full canonical FEN and variant as well as node/move identity;
the hash is not the final equality authority. Metadata revisions remain recorded
but do not invalidate an unchanged line. A root-only game is representable.

Each report position receives the same retained finite search budget. The
default is exactly 3000 ms; depth and node constructors reuse engine validation.
No infinite or live-clock report budget can be constructed. Engine binding checks
the active actual search tag/generation, game/revision context, full initial and
current FEN, complete history, command, limit, package digest/target/profile,
advertised identity/options and actual applied configuration.

Pausing or cancelling bound work retains its tag until its terminal is observed.
An interrupted position contributes no completed result and restarts with the
entire retained budget. Completed results survive pause, cancel and the pure
relaunch transition. Recovery refuses to discard a still-bound engine search.
Cancelled work does not silently resume. Engine interruption/failure causes
remain typed and visible. Outdated work remains readable and refuses admission.

`ReportJobSourceLimits` requires injected positive position and cumulative memory
charge limits. Checked charges cover line capture, result slots, configuration,
in-flight history/command allowance, retained candidate capacity, and replacement
preview admission. Result admission also checks the overlap with active work.
`memory_charge_bytes` is a deterministic admission charge, not measured allocator
usage or an RSS guarantee. The 128-position/8-MiB fixture limits are test inputs,
not product defaults. Engine/process allocations, outstanding external receipts,
allocator overhead and global concurrent-job admission remain adapter/executor
responsibilities. No runtime memory or scaling measurement was performed.

Capture, source comparison, full-history validation and finding/summary projection
are synchronous CPU operations. The later adapter must run this work on its
injected executor and preserve its single capacity budget. This module creates
no runtime, worker, subprocess, scheduler, background task or CPU diagnostic.

## Scoring fixtures and interpretation

Tests were authored before implementation and before each added contract. Fixed
reference expectations are literals derived independently of implementation:
400 cp to 0 loses `0.2310585786300049`, 800 cp to 400 loses
`0.1497384993478776`, and opposite clamp extremes lose
`0.9999999999722241`. The first value distinguishes the selected natural
exponential from a base-ten transform. Mixed loss/zero-loss White samples produce
`88.44707106849975`, displayed as `88.4`; the single 400-to-0 fixture displays
`76.9`. Black samples verify mover conversion across adjacent side changes.

Boundary fixtures include inclusive .05/.10/.20 thresholds, neighboring integer
cp inputs, both i32 extremes, negative-loss clamping, invalid numeric losses,
both bound directions, missing scores, nonterminal mate zero, exact mate winners,
missed mates for either mover, distance changes, actual checkmate/stalemate boards,
forced moves with and without exact scores, zero samples, terminal draw outcomes,
wrong score perspectives, and partial aggregation. Forced losses are excluded;
missing/bound samples never receive acceptable labels. Best and forced are
separate application flags. Partial results retain partial coverage; a fully
attempted report with unscored eligible moves also exposes partial metric coverage.

## Canonical serializer prerequisite

The actual shakmaty 0.30.2 `Chess::PartialEq` implements repetition equality and
omits move counters. `BoardPosition` inherits that behavior. At the recorded base,
`EngineSearchPosition::new` selects `position startpos` using this equality and
loses nondefault counters for a standard arrangement. Integrating the parent's
canonical `src/engine/search/position.rs` repair is a prerequisite: `startpos`
requires both Standard variant and equality with the standard canonical FEN;
other roots use their actual full FEN. This foundation does not change or export
that engine file. Its custom-counter fixture requires successful full-FEN search
dispatch, and its source/digest/context validation retains full FEN plus variant.

The parent reported an independently observed baseline regression with 0 passed
and 1 failed, showing `startpos` lost halfmove 7. Its log is
`.git/parallel-patches/engine-position-counter-baseline-20261017/semantic-red.log`,
SHA-256 `e6ff470e307d424a456eb0f60e7a8a8ba6a17f88c60c126f661b2cc269edcc8a`.
The parent authored the canonical repair and queued surrounding checks; no green
result was reported when this handoff was frozen. This is parent-provided
evidence, not a test executed in this source-only lane.

## Verification and integration

Executed: standalone `rustfmt 1.10.0`, formatting and `--check`, through
`/tmp/claude-chess-pgo-diagnosis-20261016/with-pinned-rust`; scoped source-content
inspection, staged diff whitespace validation, owned-path inspection, patch
reverse-application checks and SHA-256 export verification.

Executed tests: **0**. No Cargo, rustc compilation, test binary, build, Clippy,
runtime, native acceptance, benchmark or CPU diagnostic was run. There is no
observed red or green result in this lane. Formatter parsing and patch checks do not establish
compilation, lint cleanliness, numerical behavior, runtime cleanup or acceptance.
The parent owns compilation, behavioral checks and integration.

The parent must register `pub mod report;` in `src/lib.rs`, and these two modules
in `tests/domain.rs`; the job module includes its own new fixture:

```rust
#[path = "report/job.rs"]
mod report_job;
#[path = "report/scoring.rs"]
mod report_scoring;
```

T162 durable schemas/repository/relaunch, T163 triggers/line service, T164 FIFO
priority/preemption, and later storage/native/presentation acceptance remain
future work. Pure transitions and confirmation previews do not implement those
adapters or their acceptance scenarios.

The exact owned staged export is under
`/tmp/claude-report-model-scoring-20261017/revision-2/`: `01-tests.patch`,
`02-implementation.patch`, `report-model-scoring.patch`, `owned-paths.txt`,
`BASE_HEAD`, and `manifest.sha256`. Apply the first two in order, or apply the
combined patch, against the recorded base. The external manifest hashes every
owned file, all patches and the export metadata, including this evidence.
`03-counter-support.patch` updates an already-applied earlier frozen foundation
whose combined SHA-256 was
`01980b76b137cbdb3785251f3194871a821cb881b9acc65011e53c33dcc548d3`.
Use that delta only when the earlier foundation is already applied; it is not an
additional step after the revised combined or two-part export.
