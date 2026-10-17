# T096 local two-player composition

Status: T096 accepted after the parent canonical checkpoint: 1,407 tests,
all three Clippy selections and both optimized builds passed. This is component
composition acceptance; native GPU/platform and full US3 library acceptance
remain separate.

## Original source handoff

Status: four component composition tests authored against commit
`19b44cf57cb1dacb57c6aefc9a25a42eb9b7245c`, unregistered and unrun in this lane.
Compilation and behavior have not been established. The parent owns registration,
the Cargo lane, semantic red evaluation, task acceptance and final gates.

## Scope and existing owners

[The tests](../../../tests/desktop/local_play.rs) compose `PresentationApp`, the
mounted New Game form and game controls, `DesktopApplication`,
`DesktopCoordinator::with_games`, `DesktopGameRegistry`, `DesktopGameSupervisor`,
the real game session, legality, move tree, clocks and SQLite game-document
repository. No additional production local-play or local-control owner is needed
to assemble this composition.

The fixture reuses the public crate-visible clock, proof and runtime in
[the registry fixture](../../../tests/desktop/game_registry_fixture.rs).
Application admission, coordinator timestamps, registry and supervisor share the
same `Arc<DesktopGameRegistryFixtureClock>`. The registry runtime owns the single
capacity budget, CPU executor and temporary document store; the coordinator uses
that executor. The bootstrap deliberately reports the engine unavailable.

Mounted controls receive real egui AccessKit Click actions at 1280 by 800 logical
points. Board moves receive the public semantic-square actions. A headless
`PresentationBoardScene` supplies the board's ready scene boundary with an
unrendered texture identifier and fitted camera. No native window, GPU, engine
process, network or owner data is used. This checks component composition through
the mounted presentation, rather than native OS integration.

## Authored coverage

1. `mounted_local_game_routes_both_humans_and_persists_real_clocks_without_engine_work`
   opens New Game, selects Local two-player and starts an active Human/Human game
   while the engine is unavailable. Mounted White e4 at 3 seconds and Black e5 at
   7 seconds each advance the game revision exactly once. With the actual default
   10+5 clock, balances become White 602 seconds and Black 601 seconds. SAN,
   durable positions, revisions, history and paused recovery clock values are
   inspected through the real repository. Illegal movement, a move from the
   wrong side and a stale revision receive application failures without changing
   the persisted game.
2. `mounted_consent_accept_restores_one_ply_clocks_and_keeps_stable_variations`
   requests Black's takeback and accepts it using White's mounted control.
   White's clock continues from 602 to 594 seconds while consent is pending.
   Acceptance returns to White's preceding node, restores Black's 596-second
   pre-move clock, reverses Black's increment and compares the complete clock
   snapshot with the stored pre-move snapshot. A c5 alternative, another
   accepted takeback and replayed e5 preserve both children in stable order and
   reuse e5's original identity without growing a duplicate node.
3. `mounted_decline_cancel_and_another_move_preserve_history_and_running_clocks`
   exercises the opposite player's Decline and the requester's Cancel Request
   controls, checks revision increments and uninterrupted elapsed charging,
   then accepts another human move while consent is pending. The pending request
   clears, its stale acceptance fails, and a fresh request against the new
   revision can be declined. Both played moves remain persisted.
4. `mounted_new_game_can_disable_local_consent_and_undo_immediately`
   changes the actual New Game confirmation checkbox, then verifies an immediate
   one-ply takeback with no pending consent. It restores White's pre-move
   597-second balance and keeps the removed e4 continuation in the stored tree.

Every journey asserts one call to
`DesktopGameRegistryFactoryProvider::factory(game)`. That wrapper is expected even
for local games. Separate atomic counters require zero calls to
`DesktopGameSupervisorEngineFactory::create`, `GameEngineProvider::start`, engine
events, stop, quiescence, resource refresh and engine shutdown. Those forbidden
effect methods return dedicated typed errors, so accidental engine work cannot
silently look successful. Empty factory cleanup is permitted.

UI receipts are admitted and retained by `PresentationApp` until their actual
coordinator responses resolve. Direct rejection checks and Quit retain the real
receipts returned by public application admission. No receipt constructors or
synthetic successful game responses are used. Wake notifications are armed before
checking completion; waits have bounded deadlines and propagate timeout errors.
The journey result is held while the fixture requests Quit, observes registry
shutdown, joins both retained actors and closes the store and executor. A failure
is not represented as successful cleanup. If graceful shutdown fails, remaining
actor handles are explicitly canceled and joined before storage cleanup; the
original error, cancellation results and storage-cleanup failure are retained in
the returned cleanup error.

## Parent integration and verification

Register `tests/desktop/local_play.rs` from `tests/desktop.rs` as a sibling of
`desktop_game_registry_fixture`. No production registration or shared-helper
extraction is requested. Only the new test file and this evidence file belong to
the staged binary patch at
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/local-two-player-tests.patch`.

Standalone rustfmt from the installed pinned Rust 1.99.0 toolchain and static
scope, whitespace and source-policy checks are the only verification permitted
in this lane. No Cargo, compilation, test execution, Clippy, native diagnostics,
commits or pushes were performed. Four authored tests is a source count, not a
passing test count. Any observed failure must first be evaluated by the parent
against the baseline before authorizing a production fix.

This evidence does not establish native GPU behavior, OS accessibility, supported
platform acceptance, full US3 library/interchange acceptance or T096 completion.
Pending-consent expiry and terminal-result races remain covered by the existing
domain takeback suite, rather than additional mounted cases in this handoff.

Requirements: [US3 scenario 1 and FR-001/FR-003](../spec.md),
[T096](../tasks.md), [board interaction](../contracts/ui-experience.md#board-interaction),
[game operations and runtime ownership](../contracts/desktop-ipc.md), and
[clock and race decisions](../decisions.md#clocks-starts-claims-and-races).
Engineering policy: [canonical rule catalog](../../../.specify/rules/README.md).

## Parent validation

The parent applied the two-path handoff after `edf2711` and registered its new
suite through the existing desktop root. Initial compilation passed without
production changes. All four mounted composition cases passed on the existing
implementation, with 215 unrelated desktop cases filtered. The negative move
controls were tightened to use the actual live node, then all four passed again.
No production semantic red or repair is claimed for this increment.

Targeted Clippy found the fixture's 17,328-byte cleanup future. The fixture owner
now returns that future boxed, retaining the same shutdown/error observation.
One intermediate error-coercion compile issue was corrected; targeted Clippy
then passed with `-D warnings`. No checker or test expectation was relaxed.

The final command was
`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify`.
It exited zero: formatting, all three Clippy selections, 1,407 tests and both
optimized builds passed, with zero failed, ignored or filtered tests. All-target
release took 52.05s; the separately named native harness release took 0.52s.
Every Cargo command used the shared target, sequential gates and default
internal parallelism without job/thread caps. The existing macOS debug-linker
unwind warning remained visible and unsuppressed; Clippy was clean.

The combined fixtures prove actual White/Black routing, persisted real clock
values, consent Accept/Decline/Cancel, immediate undo when consent is disabled,
stable retained variations and zero engine creation/search. The one lazy
registry wrapper remains distinct from an actual engine. Existing shared owners
therefore satisfy T096 without duplicate production files.

Exact commands/logs and their verified SHA-256 manifest are retained in
`.git/parallel-patches/local-two-player-integration-20261017/`.
This command built the native harness but did not run GPU/OS acceptance.
