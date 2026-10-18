# Live-game engine orchestration

T054 and T064 reuse the existing process/session and UCI capability registry.
The game owns the async engine port, native clock capture, full initial position
and played history. Standard uses startpos; Chess960 and DFRC always use FEN.
Each request selects exactly one search-limit kind. Both clock sides, increments
and the active stage's moves-to-go survive conversion; both delay formats use
the approved movetime formula from the decision record.

The debug engine target passed all 51 tests, including 13 new live-turn cases.
The live cases cover correlated legal bestmoves, terminal and invalid history
rejection, stale revisions, exact-deadline discard, illegal-output cleanup,
observed stop barriers, cancellation during stop, and the positive submillisecond
delay boundary. The latter was observed failing with UnsafeBudget before the
extra rejection was removed.

Untimed searches have an absolute watchdog at the configured move budget plus
the existing stop safety grace. Telemetry cannot postpone it. Expiry invalidates
delivery before awaiting cleanup, retires the old adapter, observes final stop
and child cleanup, and returns a typed fault. Retry needs a fresh engine.

The explicit debug native mode passed against the actual installed Reckless
binary for six cases: Standard repetition history, Chess960, DFRC, king-to-rook
castling history, simple delay and Bronstein delay. Every case verifies package
identity/capabilities, a legal correlated move, cancellation/quiescence,
replacement only after completion, and observed child shutdown. It uses one
explicit engine capacity lease and the existing runtime policy.

Targeted debug Clippy passed for the library, domain, engine and native targets.
The macOS linker still reports the existing oversized __eh_frame warning; this
is not a warning-free optimized or cross-platform acceptance claim. No release
build or performance benchmark was run for this increment. Application commands,
durable autosaving and playable native controls remain separate integration work.
