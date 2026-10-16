# Durable clock boundary

Five debug persistence-boundary tests pass as part of the 129-case domain suite.
The initial check failed on the missing snapshot and recovery APIs before their
implementation. The boundary exposes typed snapshot options and validates stage
quotas, completed moves, cyclic credits, earned remaining time, delay allowance
overflow and flag/status consistency before producing a usable snapshot.

Recovery restores the saved elapsed charge and unused delay, preserves both
once-used low-time warning flags, and leaves running snapshots paused. No saved
monotonic origin is treated as valid after process restart, and no downtime is
charged. Flagged snapshots remain flagged. Round trips cover every transition
into finite/cyclic stages and both colors, including a Black-to-move start.

This is the clock boundary needed by T065. Full game-document serialization,
transactional autosaves, failed-save recovery and relaunch acceptance remain
separate integration work. Targeted library/domain/engine/native Clippy passed;
no optimized build or timing benchmark was run.
