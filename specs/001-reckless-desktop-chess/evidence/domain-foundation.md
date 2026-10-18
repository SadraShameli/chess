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

## Starts, game sessions and variations

T056, T057, T060, T061 and T063 add typed identities/seats/results, uniformly
sampled reproducible Standard/FRC/DFRC starts, immutable-parent move trees,
independent live/view cursors, repetition history and atomic game/clock/result
transitions. Initial FEN and zero-based indexes round-trip together and reject
conflicting records. All 960 Chess960 starts are tested for legality and unique
placements. Metadata validates dates, ECO codes and ordered extension tags.

The combined debug domain target passed 98 tests after the structural review:
40 existing cases, 15 clock cases, 17 start/adjudication cases, 13 session cases,
seven tree cases and six premove-component cases. Library/domain Clippy passes.
An options object and an inline prepared-node guard fix the two initial Clippy
findings without suppression or a per-move heap box. Accepted moves stage a small
clock candidate and one derived position instead of cloning the entire tree.

Deadline tests cover both late move submission and native deadline polling,
before any increment or move commit. Human draw claims remain explicit; Reckless
claims current or intended valid draws without an invented UCI command. History
identity includes legal en-passant and castling rights. Notes, NAGs, markings and
clock snapshots stay attached to their exact immutable-parent position.

The premove component validates preselected promotions, replacement, explicit
cancellation and legal execution. Its session/UI integration remains T067.
Adjudication uses a proof port and preserves an interrupted state when the proof
is unknown. The dedicated bounded proof implementation and practical coverage
remain T062. These domain checks do not establish complete native play, durable
autosaving, takebacks, engine orchestration or UI acceptance.
