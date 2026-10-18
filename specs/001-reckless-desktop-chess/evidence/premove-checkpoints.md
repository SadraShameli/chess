# Premove notification and accepted-move checkpoints

The implementation started from `e5f5dd1`. During this shared-checkout pass,
HEAD advanced to `18f0fb0`, which changes only the New Game evidence and task
markers. Completion marks T065 and T067 after the additional cancellation fix;
reviewer-owned checklist markers are unchanged.

The supervisor captures the engine-move storage checkpoint before accepting a
legal premove, then queues that checkpoint before the final transition. Both
acceptances happen without awaiting database I/O. The existing serialized writer
assigns each request the latest storage revision and acknowledges its exact game
revision. Internal writes without caller receipts no longer clone presentation
snapshots. Persistence failures retain the newest consistent game for Retry Save.

An illegal premove produces one typed snapshot notice containing its intent and
accepted engine revision. The supervisor retains that latest notice across
later computations and save completions. Registry snapshots and the existing
bounded stream carry it without another delivery channel or acknowledgement
command. Presentation remembers the consumed notice per game and displays a
nonblocking status for three seconds from first presentation. Repeated snapshots
do not restart its duration. The player's clock continues running. Notices are
transient application state and are not persisted in the game document.

Five deterministic tests in `tests/desktop/game_premove.rs`, registered as a
library unit-test module through the registry, exercise the production registry
and supervisor with a scripted game-engine provider, controlled clock, temporary
SQLite database and the existing executor/storage boundaries. Coverage includes:

- Legal execution with zero, submillisecond and longer actual elapsed charging;
  Fischer increment, simple delay and Bronstein refund behavior.
- Exact-deadline rejection, the flag-fall result and its separate checkpoint.
- Both move acceptances while SQLite writes are held, followed by ordered audit
  records for the engine and premove revisions with consecutive storage revisions.
- Failure at either checkpoint, preservation of both moves, blocked shutdown,
  explicit Retry Save and retained shutdown completion.
- Illegal clearing with a running human clock, durable save and registry
  forwarding, saturated-stream snapshot replacement, equal-time notice delivery,
  egui status expiry and suppression of replay after expiry.
- Final document restoration with matching board, revision, remaining times,
  completed-move counts and clock stages.

Review found one additional cancellation defect: canceling the intermediate
serialization discarded its original request and retried the final position,
skipping the engine-only checkpoint. A private supervisor regression reproduced
this as final game revision 3 appearing before expected intermediate revision 2.
Save requests now share an immutable restoration snapshot. The storage encoder
borrows it, and cancellation requeues the exact request at the head with its
response and quit intent intact. The next attempt refreshes only the storage
revision. This preserves ordering without another move-tree clone or a
latest-state recapture.

The original four-test regression command was
`cargo test --locked --lib desktop::game_registry::tests:: -- --nocapture`.
Against the original behavior it produced one pass and three failures: the
missing intermediate engine checkpoint in legal and deadline cases, and missing
illegal-premove feedback. After the fixes and added failure coverage, the focused
suite passed all five tests.

`cargo test --locked --lib --test desktop --test domain --test presentation`
passed 478 debug tests: 49 library, 201 desktop, 173 domain and 55 presentation.
`cargo fmt --all --check` and
`cargo clippy --locked --lib --test desktop --test domain --test presentation -- -D warnings`
passed. The final run including cancellation and storage coverage passes 507
debug tests: 50 library, 201 desktop, 173 domain, 55 presentation and 28 storage.
Broader `cargo clippy --locked --tests -- -D warnings` exposed the existing
wildcard match in `tests/desktop/native/bootstrap_games.rs`. That fixture now uses
its equivalent typed conditional; the checker configuration is unchanged.
The final broader test-target Clippy check passes.
The known macOS oversized-unwind linker warning remains.

No release build, all-feature build, real-engine acceptance, native GPU session,
OS assistive-technology session or other-platform acceptance was performed. No
dependencies, checker configuration, storage schemas, repository trait contracts,
branches or worktrees were changed. The atomic persistence and backend premove
requirements in T065 and T067 are complete. Remaining mounted controls, premove
highlights and full real-engine acceptance stay with T055, T070 and T071.
