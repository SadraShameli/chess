# Reckless v0.9.0 Capability Contract

**Status**: Design selected under owner delegation; implementation and release evidence remain unperformed.

Use [the resolved decisions](../decisions.md) for concrete defaults, algorithms
and former draft questions; [the review](../review.md) records owner choices.

**Pin**: `v0.9.0`, commit `0e92358f5acd66e5ac77b1bf558202e47c515435`.

This is the complete capability inventory for FR-005/FR-006. A row is accounted
for only when its implementation or documented developer workflow is verified.
The current planning state is designed, not implemented. Source evidence lives
in [research](../research.md); protocol sequencing lives in
[the engine contract](engine-protocol.md).

## Advertised options

Source: [tagged UCI implementation](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/uci.rs).

| ID | Option | Upstream default and bounds | Application surface | Verification |
| --- | --- | --- | --- | --- |
| OPT-01 | `Hash` | 16 MiB; 1..262144 | Advanced memory setting, additionally bounded by memory budget | Range rejection, applied value, memory-pressure test |
| OPT-02 | `Threads` | 1; max `max(512, 4 * available_parallelism)` | Auto-capacity default and explicit validated override | Discover dynamic range; one/max-worker test |
| OPT-03 | `MoveOverhead` | 100 ms; 0..2000 | Advanced time-management setting | Zero/upper bound and clock-boundary fixture |
| OPT-04 | `Minimal` | false | Full live output or final-report output | Both modes complete with valid results |
| OPT-05 | `Clear Hash` | Button, no value | Idle-only maintenance action | Correct wire command and unchanged game |
| OPT-06 | `UCI_Chess960` | false | Derived from selected variant | Standard/FRC/DFRC castling corpus |
| OPT-07 | `MultiPV` | 1; 1..218, capped by legal root moves in search | Candidate count in analysis | Ranks and legal lines at 1/multiple/root limit |
| OPT-08 | `SyzygyPath` | Empty; only with Syzygy feature | Validated local tablebase paths | Loaded cardinality, valid/invalid/no-space alias cases |

Application defaults may override upstream defaults explicitly, especially
Threads. The parser-discovered descriptor and package manifest are authoritative;
unknown options cause a version-coverage discrepancy rather than silent omission.

## Session, search, and output

| ID | Capability | Workflow and verification |
| --- | --- | --- |
| PRO-01 | `uci`, identity, option discovery, `uciok` | Startup identity and option inventory; timeout/mismatch fixtures |
| PRO-02 | `isready` / `readyok` | Startup/configuration readiness, including proof it is not search completion |
| PRO-03 | `ucinewgame` | Reset histories/hash between games; replay position afterward |
| PRO-04 | `position startpos [moves ...]` | Standard-game initial position and full move-history replay |
| PRO-05 | `position fen ... [moves ...]` | Imported/custom positions and every FRC/DFRC game |
| SRC-01 | `go infinite` | Continuous analysis; exact one-token mode; explicit stop |
| SRC-02 | `go depth N` | Positive validated depth, natural completion |
| SRC-03 | `go nodes N` | Positive validated node budget, natural completion; report actual nodes |
| SRC-04 | `go movetime N` | Positive move milliseconds, completion and overrun measurement |
| SRC-05 | Clock `go` | `wtime`, `btime`, `winc`, `binc`, optional `movestogo`; sudden death, increment, staged controls |
| PRO-06 | `stop` | Await final bestmove, reject stale state; timeout terminates/reaps child, preserves game, pauses clocks, awaits explicit Retry and later explicit Resume |
| PRO-07 | `quit` | Bounded graceful shutdown, EOF and process reaping |
| OUT-01 | `bestmove` | Validate legal move against originating search; no unsupported ponder suffix |
| OUT-02 | `depth`, `seldepth`, `multipv`, `pv` | Candidate rows and legal continuation with proper ranks |
| OUT-03 | `score cp`, `score mate` | Typed scores, side/perspective normalization, signed mate values |
| OUT-04 | `lowerbound`, `upperbound` | Display bounds correctly, including inversion with perspective |
| OUT-05 | `nodes`, `time`, `nps` | Search statistics without invented values or JavaScript integer truncation |
| OUT-06 | `hashfull` | Occupancy in per-mille units, display converted deliberately |
| OUT-07 | `tbhits` | Tablebase telemetry; distinguish unavailable from reported zero |
| OUT-08 | Identity, informational text, option/error diagnostics | Bounded diagnostic view and typed recognized statuses; unknown text preserved safely |

Never serialize multiple limit kinds into one `go`. No-position or terminal
searches are rejected by the application before the process is called.

Simple and Bronstein delay are approved application clock features, not native
Reckless options. Their app-side time accounting and mapping to one supported
search limit follow the selected clock policy in [the engine protocol](engine-protocol.md).
This does not expand SRC-05 into a claim of engine-native delay support.

## Chess, evaluation, and optional builds

| ID | Capability | Required coverage |
| --- | --- | --- |
| CHS-01 | Standard chess | Full play, analysis, legal history, special moves |
| CHS-02 | Chess960/FRC | Indexed/random/imported start, FEN, king-to-rook UCI castling |
| CHS-03 | DFRC positions | Independent validated back ranks; same Chess960 option and FEN path |
| EVA-01 | Embedded threat-aware NNUE | Pinned model provenance, offline execution, build identity |
| CPU-01 | Scalar/x86 baseline | Portable compatible package and offline search |
| CPU-02 | AVX2 path | Full x86-64-v3 feature gating and benchmark against baseline |
| CPU-03 | AVX512 path | Full explicit feature/OS-state gating, safe selection, benchmark |
| CPU-04 | AArch64 NEON path | Controlled Apple Silicon build and execution; Linux ARM64 is outside release scope |
| CPU-05 | Parallel workers | Auto-capacity configuration, resource accounting, measured scaling |
| TB-01 | Syzygy/Fathom default feature | Enabled production builds; local path load and cardinality diagnostics |
| TB-02 | Root DTZ and WDL fallback | DTZ fixture, WDL-only fixture, missing-table behavior and rule-50 context |
| NUM-01 | Optional NUMA build | Linux/libnuma package, topology diagnostics, actual NUMA-host test; no claim on other OSes |

Syzygy data is optional, but its engine support is not removed merely to simplify
packaging. NUMA is compiled into a separate Linux profile and selected only when
its runtime requirements are met. No fictional NUMA UCI toggle is added.

## Advanced and developer workflows

| ID | Facility | Surface and verification |
| --- | --- | --- |
| DEV-01 | `d` | Workbench ASCII/FEN inspector; compare to domain position |
| DEV-02 | `eval` | Workbench raw White-perspective static NNUE integer; never label search cp |
| DEV-03 | `compiler` | Build diagnostics: compiler, target, target features |
| DEV-04 | `perft depth` | Validated depth at least 1, root counts/totals/time/NPS; isolated cancellable child |
| DEV-05 | `bench [hash [threads [depth]]]` | Built-in 45-position benchmark; defaults 16 MiB/1 thread/depth 12; explicit shared budget |
| DEV-06 | CLI queued-command mode | Document each process argument as a complete command string; test auto-exit and UCI transition |
| DEV-07 | PGO release workflow | Instrumented build, fixed bench corpus, merged profile, optimized rebuild, provenance and behavior comparison |
| DEV-08 | Optional `spsa` feature | Static source indicates missing generated functions; controlled compile check required, unavailable in this pin unless disproven |

The SPSA row is an upstream limitation, not permission to implement a tuning
framework or edit vendor code. Bullet/OpenBench references acknowledge external
training/testing tools; they are not hidden APIs supplied by this release.

## Unsupported at this pin

The following are explicit absence findings from the tagged parser/options:

- Ponder, `go ponder`, `ponderhit`, and `bestmove ... ponder ...`.
- `go searchmoves`, `go mate`, combined limits, and arbitrary unknown go flags.
- Skill Level, `UCI_LimitStrength`, `UCI_Elo`, or calibrated weaker opponents.
- `UCI_ShowWDL`, WDL probability output, and `UCI_AnalyseMode`.
- Runtime `EvalFile` replacement, opening books, and Polyglot.
- SyzygyProbeDepth, SyzygyProbeLimit, and Syzygy50MoveRule controls.
- PGN/SAN libraries, persistence, GUI/clocks, online play, or public data-generation
  and network-training APIs.

Application features such as clocks and PGN are supplied by this project; their
existence is never attributed to the engine. Do not send unsupported flags because
some fall through to an unintended unbounded search.

## Coverage acceptance

Implementation evidence records each ID, exact binary hash/profile, fixture,
expected behavior, result, target OS, and any platform-conditioned reason.
Production capabilities require real-engine tests, not transcript doubles alone.
Only genuinely unavailable upstream features may be marked unavailable, with
source/build evidence. Missing application work cannot use that designation.
