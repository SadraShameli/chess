# Native live-game controls and recovery

## Intended-move draw claims

The native panel now lets a human select an intended legal move and explicitly
claim threefold repetition or the fifty-move draw. The selection carries the
authoritative `BoardMove` from the snapshot, including promotion and Chess960
castling identity. Existing serialized move fields are preserved. Domain code
validates the claim and determines the result; selecting or submitting an intent
does not play the move in presentation state.

The selector requires an active game, live view, a human turn, saved state and no
pending operation. A game, revision, view, phase, save-gate or busy-state change
clears selection. Repeated layout passes preserve request deduplication.

Five UI regressions failed before the controls were applied. The first assembled
run then passed 192 desktop tests and 52 presentation tests, with two repeated-
selection failures. Pinned egui 0.36.2 automatically closes popups on pointer
clicks; accessibility activation also requires explicit closure when a choice is
clicked. Applying that correction to every choice, including clearing or
reselecting a move, produced 54 passing presentation tests.

The eight new cases cover intended repetition and fifty-move claims without a
currently available claim, stale-selection clearing, operation gates, four
promotions, ambiguous Chess960 king/castling destinations, unchanged serialized
fields, and domain rejection without mutation followed by an accepted claim
without adding a move-tree node.

## Shutdown recovery

The delivered shutdown regressions reproduced five failures before source fixes:
retained storage recovery, pending engine-factory settlement, coordinator Quit
progress, runtime reservation publication, and explicit save recovery after a
failed Quit. The sixth added check verifies that canceling a waiter does not
release ownership of cleanup still in progress. The tests use controlled factory
signals, an SQLite failure trigger and a synchronous wake observer.

The integrated fixes bound save retries to explicit recovery attempts, preserve
the save gate until an exact receipt, release a completed shutdown reservation
before notifying observers, and cancel uncommitted factory requests only after
cleanup is observed. Preparation does not restart canceled requests implicitly.

Review identified a further missing case: the first storage error can occur
during Quit. Its regression failed waiting for an updated aggregate snapshot.
The runtime now keeps the existing supervisor-snapshot forwarding path alive
while ordinary registry processing is stopped for recovery. An explicit Retry
Save therefore receives the current revision and publishes its successful
receipt without automatically resuming the game. All 199 desktop tests pass.

## Graphics recovery

Four native bootstrap regressions reproduced terminal cancellation failures and
queued preparation/ready events being processed after teardown. The capacity
owner now arbitrates terminal state against the last preparation generation;
matching retired events cannot reopen admission, and wrong generations still
fail. A preparing resource reservation remains held until the host observes its
completed render frame.

Review also found that normal graphics failure begins in Quiescing rather than
Preparing. A fifth native composition regression reproduced a stale-generation
failure caused by acknowledging an interruption that needed no draining step.
`CapacityPhase::complete_interruption` now owns both paths. Cancellation tests
assert that preparation returns the terminal-cancellation outcome explicitly.

Asset Retry rebuilds scene state and reloads the piece asset. Source review of
pinned Bevy 0.20 established that detached reload can overlap an older load under
the same identity. The host therefore drops the old root handle, waits for
Bevy to retire its managed identity and pending task, then starts a normal load
with a distinct identity. Old completion messages cannot satisfy the fresh
readiness gate. Repeated Retry preserves the retiring identity, and retirement
does not reset the existing readiness budget. Private checks cover identity
rejection, old messages, fresh completion, repaired dependencies and retained
failure causes. Actual repaired glTF loading through the GPU renderer is not
verified.

## Verification boundary

These checks use debug builds and hermetic egui/application tests on the current
macOS host. The combined debug run passes 44 library, 199 desktop, 172 domain and
54 presentation tests. Targeted Clippy passes for the library and those three
integration targets after all graphics corrections. The native command fixture
also preserves its request identity while retrying expected admission contention
within its existing deadline. The existing macOS linker
unwind-section warning remains. No optimized performance, actual assistive-
technology session, renderer interaction or other-platform acceptance is claimed.
This records the implemented portions of T055, T068 and T070; their broader
milestone markers remain governed by the feature task list.
