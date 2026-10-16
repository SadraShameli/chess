# Live-game domain foundation

## Clocks

T053, T058 and T059 implement validated untimed, sudden-death, Fischer,
finite staged, cyclic, simple-delay and Bronstein controls. Fifteen deterministic
clock cases passed in the debug domain target. The tests were added before the
implementation; the initial debug check failed on the absent APIs.

Coverage includes exact nanosecond deadlines before increment/move acceptance,
per-side stage boundaries, Bronstein display/refund behavior, preserved delay
across repeated pauses, complete takeback snapshots, separate once-used warning
flags, checked overflow and transactional rejection. Initial controls resolve to
the selected 10+5 default. Time comes from injected monotonic values.

Targeted debug Clippy passed for the library and domain target. Native clock
widgets, persistence/relaunch, engine orchestration and end-to-end timing remain
separate tasks. No optimized performance result is claimed here.
