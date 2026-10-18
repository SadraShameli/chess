# Pause, resume and takeback domain acceptance

T066 passes twelve integration cases plus three private receipt-boundary cases.
Manual pause and sleep preserve charged time and require explicit Resume. Engine,
graphics and save faults require explicit recovery before Resume. A native
deadline reached before pause or takeback takes precedence over the action.
Queued premoves clear at interruption and history changes.

Engine games consume a non-cloneable game/revision-bound quiescence receipt
before pause or undo. Missing and stale receipts leave state unchanged. Engine
takebacks immediately target the previous human turn for either human color;
local takebacks target one ply. Local consent is required initially, clocks keep
running while it is pending, no expiry is added, and a new accepted move cancels
the request. Decline and requester cancellation preserve played history.

Undo restores the exact before-move snapshot while preserving once-used warning
flags. Removed moves and alternatives remain in the tree. Replaying a matching
move reuses its identity and replaces the corresponding pre/post clock pair.
Resuming an untimed engine game produces a fresh request with the full configured
budget and complete retained live history.

The debug domain suite passed 129 tests, the library suite passed 24 and the engine
suite passed 51. Targeted Clippy passed. Separate real-engine acceptance verifies
the provider stop barrier; native action/UI wiring and transactional autosaving
remain T068/T070/T065. T067 remains pending until queued premoves execute through
those application and presentation owners with native clock timestamps.
