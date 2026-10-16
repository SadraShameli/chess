# Implementation Plan: Reckless Desktop Chess

**Branch**: `release` | **Date**: 2026-10-16 | **Spec**: [spec.md](spec.md)

**Feature**: `001-reckless-desktop-chess`

**Status**: Planning decisions resolved; ready for task generation. The owner
explicitly delegated remaining recommendations. No application implementation,
installed dependency compatibility, native testing, or release readiness is claimed.

## Summary

Build the full offline desktop chess application, with Reckless engine play,
local two-player, analysis, library, PGN/FEN interchange, annotations/variations,
position editor, game reports, opening explorer/study book, puzzles, Settings,
and complete usable engine diagnostics. No approved feature is deferred merely
because the scope is large. Use Tauri 2, Vue 3, strict TypeScript, Vite, Tailwind,
shadcn-vue, Bun, ESLint and Prettier. The real-time Three.js/WebGL2 board is
3D-only, with premium graphite/emerald dark and warm light themes, sculpted
ivory/ebony pieces, accessible semantic controls and on-demand rendering.

[Owner decisions](review.md) and [resolved implementation decisions](decisions.md)
select exact behavior, limits, assets, formulas, numerical defaults and recovery
policies. The latter resolves earlier draft questions under explicit delegation;
contracts refer to it rather than reopening questionnaires. Policy remains in
[the canonical rules](../../.specify/rules/README.md).

Keep private personal use, no application license-file change and no payments
for signing, CI, hosting, datasets or assets. No credentials, workflows, releases,
commits or pushes are created by planning. Git remains on release; Spec Kit's
feature identifier is not a request to create another Git branch.

## Technical Context

| Area | Selected design |
| --- | --- |
| Rust | Existing exact 1.99.0 toolchain, edition 2024, one root Cargo package |
| Frontend | Vue SFC views, typed owning controllers/models, Bun scripts, strict TypeScript 6.0.3, Vite |
| Desktop | Tauri 2, minimal capabilities/CSP, native file dialogs and typed Rust-owned IPC |
| Engine | Reckless v0.9.0, commit 0e92358f5acd66e5ac77b1bf558202e47c515435, owned native UCI sidecars |
| Rules/notation | shakmaty 0.30.2 and pgn-reader 0.29.1 behind board/notation boundaries; separate proof adjudication boundary |
| Storage | Bundled SQLite/rusqlite, one storage actor, WAL/FULL transactions, separate immutable starter indexes |
| Schemas | Serde + schemars draft-07, generated json-schema-to-ts types and Ajv validation |
| Concurrency | Tauri's existing async runtime, capacity-coordinated CPU executor and subprocess leases |
| Graphics | Three.js 0.186.1, WebGL2, dedicated scene controller, original GLB meshes and synthesized WAV sounds |
| Native targets | Windows 11 x86-64; macOS 14+ Apple Silicon; Ubuntu 22.04+ x86-64, X11/Wayland |
| Delivery | Unsigned NSIS, ad-hoc-signed Mac DMG, .deb and AppImage, private signed OTA |
| Automation | GitHub Actions version-tag release after all targets pass; activate only with verified no-overage controls |
| Dataset sources | CC0 Lichess standard-game/puzzle exports, titled-master online and broad online collections, CC0 opening labels |
| Starter data | Standard only, deterministic subsets, 2,000,000,000 installed bytes including indexes; manual dataset updates |
| Tests | Rust domain/protocol/persistence/concurrency, Vitest browser/Playwright/Vue Test Utils, WebdriverIO 9 Tauri native tests |

The [stable version inventory](research.md#final-resolution-research) selects
primary pins based on registry/source checks. Resolve actual supporting stable
versions and lockfiles during implementation; validate Bun/tool/API compatibility
before accepting dependencies. No prerelease shortcut or checker weakening.

## Behavioral ownership

Rust owns positions, legality, game transitions, clocks, claims, resource leases,
saving, analysis identity, datasets, report scoring and puzzle progress. Vue
renders snapshots and submits intents. The scene only owns presentation and
picking; browser clocks never adjudicate chess. The composition root injects
clock, random, engine, storage, capacity and native service ports.

Multiple running games share all usable CPU capacity by equal shares with
one-thread admission reservations, safe boundary rebalancing and an overall
hash budget of 10% available RAM at startup capped at 2 GiB. Pause/reap engines
to release caches. Reports pause during running live games; scheduler details,
rounding, memory admission and overload behavior are selected in
[capacity decisions](decisions.md#capacity-and-memory) and
[report scheduling](decisions.md#report-scoring-and-job-scheduling).

Preserve the owner-selected game lifecycle, takebacks, clock snapshots, premoves,
draw claims, paused recovery and autosaves. Custom clock stages, delay engine
allocation, numbering, input races and exactness safeguards are resolved in
[clock decisions](decisions.md#clocks-starts-claims-and-races). Keep the
[complete pinned engine inventory](contracts/reckless-capabilities.md), including
MultiPV, Syzygy, FRC/DFRC, CPU profiles and Linux NUMA diagnostics. Unsupported
upstream commands and broken SPSA remain explicitly described limitations.

SQLite commits correlate moves, clock snapshots, revision and phase. Import
staging, atomic exports, backups, migration journal and recovery preserve current
data on failure. The [storage decisions](decisions.md#storage-imports-and-backup-recovery)
replace the earlier small PGN-file proposal with bounded streaming imports.
All six study/training subsystems retain their complete selected behavior under
[the study contract](contracts/study-tools.md).

## Visual acceptance and performance

Apply the exact design tokens, camera framing/input, quality tiers, adaptation,
keyboard/audio, original asset and accessibility choices in
[presentation decisions](decisions.md#presentation-assets-and-input).
Preserve 1280x800 minimum, 180 ms moves/auto-flip, 160 ms panel motion, reduced
motion, the 20-degree initial camera and visible camera controls. Selected
baselines are 8 GiB RAM, four logical threads, hardware WebGL2 and 6 GB free
installation disk, plus staging/import/backup capacity. Reference hardware is
Apple M1-class Mac and Intel UHD 620-class Windows/Linux, subject to actual tests.

SC-001 through SC-014 are acceptance targets, not measured results. Measure p95
input feedback <=100 ms, active 60 Hz presentation, offline game start within
3 seconds, p95 stop <=1 second with 2-second safety deadline, no stale result
in 1,000 controlled lifecycle cycles, and idle CPU <1% of one logical core.
Record total native/WebView/engine memory separately from hash. Compare identical
optimized engine builds at one and maximum workers; do not assume linear scaling.
No rendering benchmark or minimum-system support has been established yet.

## Packaging and release boundaries

Bundle the offline Evergreen WebView2 installer. Linux installation may use
internet for system dependencies, with offline-preparation instructions. Chess
runs offline after installation. Mac personal-use OS approval and nested ad-hoc
signing need actual tests. Retain baseline and gated optimized x86 engine
profiles, portable Mac ARM and an optional Linux NUMA profile. Sidecar selection
uses verified CPU/OS support and actual checksums, never trial execution.

Private OTA uses a repo-scoped Contents-read token in the OS credential store,
automatic downloads on detected unmetered connections, and approval per
metered/unknown attempt. Explicit installation pauses/saves all games and creates
a fresh successful safety backup; restored games stay paused. Retain every
private release until owner deletion. The owner-selected update/cache/timeout/
recovery policies are in [packaging](contracts/packaging.md) and
[updater completion](decisions.md#updater-completion-and-zero-cost-ci).

Windows and macOS paid signing/notarization are excluded for now. Native updates
still require cryptographic OTA signatures. Keep the signing key/passphrase in
GitHub Actions encrypted secrets and an encrypted disconnected removable backup.
Verify actual free compute/storage allowance and hard billing stops before any
workflow activation; disable automation if zero cost cannot be established.

## Constitution Check

Governed by [constitution v1.1.1](../../.specify/memory/constitution.md) and its
[canonical catalog](../../.specify/rules/README.md). The table records this
feature's decisions and evidence; the referenced files own the obligations.

| Canonical rule | Pre-research evaluation | Post-design evaluation and evidence |
| --- | --- | --- |
| [Code quality](../../.specify/rules/code-quality.md) | PASS: artifacts only; preserve existing Makefile change | PASS: policy stays in canonical files; contracts have single owners; no code comments or source scaffolding added |
| [Verification](../../.specify/rules/verification.md) | PASS after creating the missing feature specification and rerunning setup | PASS for design: document/link/toolkit checks; native gates and performance measurements are explicitly future acceptance work |
| [Architecture](../../.specify/rules/architecture.md) | PASS: game owns state; sidecar/storage are real effect boundaries | PASS: GameSession, EngineSession, StorageDocumentStore and injected ports; one native bootstrap and one UI bootstrap; UI cannot adjudicate games |
| [Structure](../../.specify/rules/structure.md) | PASS: existing Rust package remains at root | PASS: cohesive source components, thin desktop adapter, one separate UI package justified by WebView compilation |
| [Rust style](../../.specify/rules/rust-style.md) | PASS: typed enums and domain validation selected | PASS: component-prefixed model names and discriminated wire contracts; no raw UCI strings enter domain types |
| [Performance](../../.specify/rules/performance.md) | PASS: search stays in optimized native process | PASS: one capacity coordinator; bounded async pipes; ordered lifecycle channel and coalesced visual telemetry; workload budgets defined |
| [Testing](../../.specify/rules/testing.md) | PASS: behavior and boundary acceptance scope identified | PASS: legal fixtures, real pinned-engine tests, cancellation races, import round trips, target package checks, and measurement protocol documented |

No constitutional exceptions are requested. Vendored Reckless/Fathom internals
are external dependencies, not project-authored unsafe code. They are not edited
to satisfy project styling. A protocol limitation is handled in the adapter or
reported explicitly, never patched silently in vendor code.

Constitutional design review passes for the selected plan. Implementation, installation, accessibility,
performance, and redistribution gates are not represented as passed.

## Project Structure

### Documentation (this feature)

```text
specs/001-reckless-desktop-chess/
├── spec.md
├── plan.md
├── research.md
├── review.md
├── decisions.md
├── data-model.md
├── quickstart.md
├── checklists/
│   └── requirements.md
└── contracts/
    ├── desktop-ipc.md
    ├── engine-protocol.md
    ├── reckless-capabilities.md
    ├── ui-experience.md
    ├── packaging.md
    └── study-tools.md
```

Task generation is a separate phase; `tasks.md` is intentionally not generated.

### Source Code (repository root)

The following tree is the proposed implementation layout, not existing code.

```text
Cargo.toml
Cargo.lock
build.rs
tauri.conf.json
capabilities/
src/
├── main.rs
├── lib.rs
├── desktop/
├── game/
├── board/
├── clock/
├── engine/
│   ├── uci/
│   ├── package/
│   └── tablebase/
├── capacity/
├── storage/
├── notation/
├── report/
├── opening/
├── puzzle/
└── workbench/
ui/
├── package.json
├── bun.lock
├── index.html
├── vite.config.ts
├── tsconfig.json
├── eslint.config.ts
├── .prettierrc.json
├── components.json
├── src/
│   ├── main.ts
│   ├── App.vue
│   ├── components/
│   │   └── ui/
│   ├── desktop/
│   │   └── generated/
│   ├── board/
│   ├── game/
│   ├── analysis/
│   ├── library/
│   ├── editor/
│   ├── report/
│   ├── opening/
│   ├── puzzle/
│   ├── settings/
│   ├── workbench/
│   └── theme/
└── tests/
tests/
├── domain.rs
├── engine.rs
├── storage.rs
├── desktop.rs
├── board/
├── game/
├── engine/
├── storage/
└── fixtures/
benches/
resources/
├── engine/
└── pieces/
tools/
└── engine/
```

**Structure Decision**: Retain the root Rust package. Tauri configuration points
to the root Cargo package and the built UI assets. Frontend dependencies and
lockfile live in `ui/`; expose their commands through the root Makefile during
implementation. `make run` will launch the desktop application using the pinned
Tauri CLI in release mode, forwarding arguments only when supplied. Preserve the
current separator fix.

Domain dependencies flow from desktop orchestration to game/board/clock and
consumer-owned engine/storage ports. Concrete process and SQLite adapters are
wired by bootstrap. The UI receives validated snapshots and dispatches intents.
Neither CSS components nor browser state can choose an engine executable.

## Phase 0: Research

[Research](research.md) records pinned engine facts, framework choices, registry
checks, section-specific dataset licenses, puzzle CSV semantics and updater/CI
feasibility. [Decisions](decisions.md) selects the remaining product and technical
policies under delegation. No unanswered owner preference blocks task generation.

Research does not establish runtime correctness. Freeze downloaded dataset hashes
at preparation, validate dependency lockfiles when adding packages, and satisfy
[the release evidence register](decisions.md#release-evidence-register). General
FIDE dead-position/flag-fall proof coverage, private updater integration, actual
minimum-platform performance and verified zero-cost CI are substantial release
gates. Preserve private personal use until the separate NNUE redistribution-license
gate is resolved for any proposed external conveyance.

## Phase 1: Design and Contracts

- [Data model](data-model.md): typed entities, state, revision and persistence ownership.
- [Desktop IPC](contracts/desktop-ipc.md): typed operations, correlation and bounded streams.
- [Engine protocol](contracts/engine-protocol.md): UCI sequencing and child lifecycle.
- [Capability coverage](contracts/reckless-capabilities.md): complete pinned inventory.
- [UI experience](contracts/ui-experience.md): visual and interaction acceptance.
- [Study tools](contracts/study-tools.md): library, editor, reports, openings and puzzles.
- [Packaging](contracts/packaging.md): targets, provenance, installers and updates.
- [Resolved decisions](decisions.md): concrete completion of former design placeholders.
- [Quickstart](quickstart.md): future validation guide and current unimplemented state.

FR-001 through FR-026 and SC-001 through SC-014 retain their complete scope.
Use the requirement allocation table in the resolution document to generate
ordered implementation tasks. No tasks.md is created by this planning workflow.

## Complexity Tracking

No constitutional exceptions. A binary-only engine requires the subprocess
boundary; a WebView UI requires its own frontend package. Isolated workbench
children protect live sessions from noninterruptible diagnostics. Original meshes,
proof adjudication, offline indexed datasets and authenticated updater delivery
are real implementation work, not permission to silently reduce the feature set.
