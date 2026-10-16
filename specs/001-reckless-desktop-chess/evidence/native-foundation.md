# Native foundation evidence

Recorded 2026-10-16. Foundation tasks remain independently tracked in
[the task list](../tasks.md). Component-level results do not establish
production-host or cross-platform acceptance.

## T040: Tagged Reckless search lifecycle

The engine session now serializes one exclusive infinite, depth, node, move-time
or clock limit. Clock limits include both players' time and increment plus
optional moves-to-go. Bounds follow Reckless 0.9's signed 32-bit numeric parser
and depth loop. Initial position and the complete legal UCI move history travel
together, including Chess960 castling encoding.

Search events retain game/revision, search and process-generation identities.
An active or stopping search blocks position/configuration changes until its
final bestmove. readyok remains a readiness event. Stop deadlines originate at
application receipt and remain absolute through queue delays and cancellation;
an event becoming ready at the deadline cannot succeed. An interrupted partial
stop write permanently prevents further protocol writes and forces observed
termination rather than reusing ambiguous state.

The isolated engine worktree passed all 23 engine integration cases, including
13 new lifecycle cases, and strict all-target/all-feature Clippy without
warnings. Tests exposed the absent lifecycle API, a simultaneous
bestmove/deadline defect and three canceled partial-stop-write defects before
their fixes. The reviewed test/source binary patches were then applied to the
feature branch with three-way patch integration and explicit module registration.

These deterministic fixtures use controlled process ports and paused Tokio time.
They do not claim complete engine-option UI, production scheduling or live-game
integration, which remain later tasks.
