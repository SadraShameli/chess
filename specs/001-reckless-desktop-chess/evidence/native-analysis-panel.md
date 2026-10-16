# Native analysis panel

T084 implementation lane, based on `f0317cf`, in
`/Users/sadrashameli/.t3/worktrees/chess/t3-f6b9f5f2`.

## Integration contract

`presentation::analysis::view::PresentationAnalysisView::show` mounts the matching
egui controls inside the existing styled host. It returns at most one typed
`PresentationAnalysisControllerIntent` per frame. It owns no engine process,
executor, capacity allocation, game, or replacement chess position.

Construct `PresentationAnalysisControllerSelection` from the authoritative
`EngineSearchContext`, full validated `EngineSearchPosition`, and typed workspace
(`Analysis`, `Review`, or `LivePlay`). Construct the controller with that selection.
Call `synchronize` whenever the game, revision, selected history, or workspace
changes. Its returned old active `EngineSearchTag` identifies a search the
integration owner must cancel. Pending UI requests are invalidated immediately;
`is_intent_current` lets a queued command handler check again before dispatch.

Start intents carry the validated `EngineSearchLimit` and full typed selection.
Stop intents carry the exact accepted `EngineSearchTag`. Both carry the existing
`DesktopRequestId`. The handler owns admission, options, process lifecycle,
timeouts, and eventual cleanup. After accepting Start, call `observe_started`
with its request ID and actual `EngineSearchSessionMetadata` before routing
session events. Command refusals go through `observe_failure` with the same
request ID and existing `DesktopFailure`. Route actual
`EngineSearchSessionEnumEvent` values through `observe_event`; Stop receipts can
pass their actual `EngineSearchSessionTerminal` directly to `observe_terminal`.
No separate analysis snapshot/event schema is introduced.

If acceptance arrives after selection invalidation, `observe_started` rejects it;
the handler must cancel the backend search it already owns. Dropping a pending UI
request never substitutes for the backend's stop/cleanup barrier.

Matching uses the backend's full search ID/process generation/game/revision tag,
validated initial position and typed history, accepted limit, and start instant.
A new Start clears candidates before acceptance. Terminal snapshots are applied
immediately and latch completion, preventing delayed telemetry or command errors
from replacing the terminal result. Natural completion, stop, cancellation,
backend failure, and command refusal remain distinct. Retry is explicit.

## Display and validation

Candidate rows retain each backend line's score, PV, and optional statistics.
The independent latest-update statistics are separately labelled. Missing values
show `unavailable`; reported zero remains zero. Counter and duration labels use
integer formatting, including values beyond floating-point integer precision.
The panel derives SAN by applying the actual `BoardPosition` operations to the
backend's validated `BoardMove` sequence. Unchanged immutable lines reuse cached
notation between visual snapshots.

White values and bound directions come only from `EngineAnalysisScore` methods.
The original reported side/value/bound remains visible. Centipawns display in
pawns, mate retains its signed distance, and inequalities identify bounds. The
evaluation bar follows the selected transform in
[presentation decisions](../decisions.md), with
bound/unavailable hatching and no fabricated numeric accessibility value. It
appears only in Analysis/Review; LivePlay cannot start analysis here.

The exclusive editable draft selects Infinite, Depth, Nodes, MoveTime, or Clock.
Validation delegates ranges and budgets to `EngineSearchLimit`,
`EngineSearchClock`, and `ClockDuration`. Invalid numbers have field-specific
errors and cannot produce intents. Controls use native egui labels, focus, radio
selection, text entry, buttons, progress indicators, scrolling, and existing
theme tokens. Hatching uses the same egui painter and theme stroke.

## Verification

Tests were authored before implementation. Cargo was initially withheld by the
parent's compilation-lane policy, so no pre-implementation failing Cargo run is
claimed. After the explicit lane grant, checks use the shared
`CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target` sequentially.

The mounted/headless cases consume real public `EngineSearchSession` snapshots
and terminal events, produced by the actual backend parser/session with a small
in-memory process transcript. They cover ranks and SAN; White/raw cp, mate and
inverted bounds; missing/zero/large counters; exclusive mode validation; one-shot
Start/Stop across discarded egui passes; immediate natural/stop/failure/cancelled
results; explicit Retry; stale request/game/revision/history/search/generation
rejection; empty replacement candidates; and live-play evaluation exclusion.

- Owned Rust files and the two registrations: `rustfmt --edition 2024 --check
  --config skip_children=true` passed. Only those explicit paths were formatted.
- `CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo clippy --locked
  --lib --test presentation -- -D warnings`: passed without warnings.
- `CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked
  --test presentation`: 76 passed, including all 10 analysis tests; zero failed,
  ignored, or filtered tests. Final test execution took 5.76 seconds. This is a
  deterministic contract run, not an optimized performance measurement.
- `git diff --check`: passed.

The first Clippy attempt identified the included fixture's module path; the next
identified a test initializer style issue. Both were corrected. The first test
run passed 75/76 and caught an incorrect test expectation for Black mate lower
bounds; the implementation correctly used the backend's inverted White upper
bound. The expectation was corrected, then Clippy and all presentation tests
passed. No checker was weakened.

The pre-existing macOS linker message `__eh_frame section too large (max 16MB)`
was emitted for the application's binary and remains unsuppressed. No dependency
or Bevy rebuild was needed. Cargo ownership was released to the parent after
these checks; no engine/native acceptance tests were rerun in this lane.

## Scope

This patch owns `src/presentation/analysis/{mod,configuration,controller,line,view}.rs`,
`tests/presentation/analysis.rs`, `tests/presentation/analysis/fixture.rs`, this
evidence, and minimal registrations in `src/presentation/mod.rs` and
`tests/presentation.rs`.

T083 desktop command handlers and T089 application wiring remain with their
owners. No application integration, native GPU/window acceptance, full capability
or performance gate is claimed. Existing accepted T079 real-engine/session and
real-game evidence is unchanged and is not rerun for these presentation edits.
The parent owns task markers, consolidated verification, commits, publication,
and worktree cleanup. This lane adds no dependencies and performs no release,
commit, push, or new-agent/worktree operation.

Parent integration at `b8c5dae` passed `cargo fmt --all`,
`cargo clippy --locked --lib --test presentation -- -D warnings` and
`cargo test --locked --profile dev --test presentation`. All 76 presentation
tests passed with no failures or ignored cases. T084 is accepted as the tested
standalone presentation component; T083/T089 still own desktop/application wiring.
