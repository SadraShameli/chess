# Puzzle selection core handoff

This slice implements the selection portion of T185 and focused persisted-shuffle
fixtures for T179. It does not accept either complete task or User Story 8.
The owning requirements are [FR-021](../spec.md),
[the puzzle contract](../contracts/study-tools.md#puzzles),
[the lifecycle decisions](../decisions.md#puzzle-interchange-and-lifecycle), and
[the puzzle data model](../data-model.md#reports-openings-and-puzzles).

## Source boundary

`src/puzzle/selection.rs` consumes the existing validated `PuzzleDefinition`,
identity, difficulty and theme types. `PuzzleSelectionDataset` copies the exact
source/version from a validated identity. Collection admission rejects mixed
source/version identities and duplicate identities before filtering. Original
source IDs, including case and surrounding whitespace, remain unchanged.

Filters are frozen by ownership for the lifetime of a selection. Difficulty is
an inclusive validated range. Selected themes use exact token matching with any
selected theme sufficient; duplicate selected themes are rejected. An active
filter excludes a puzzle missing its required metadata. Unfiltered selection
retains unavailable ratings/themes without inventing values. Changing a caller's
next-run filter cannot change the current collection, including on reshuffle.

The selection stores one permutation and a cursor into its frozen collection.
Visited identities are the consumed prefix; the current identity is at the
cursor and is not yet in that prefix. `advance` consumes the current puzzle and
returns `Ready`, `Exhausted` or `NoMatches`. Repeated advancement at exhaustion
does not draw randomness or restart the collection. Only explicit
`reshuffle_and_continue` permits repetition, after nonempty exhaustion. Empty
matches remain a distinct state. Fisher-Yates shuffling uses the injected
`PuzzleRandomProvider`; zero/one-element collections need no shuffle draws.

## Snapshot and random contract

`PuzzleSelectionSnapshotOptions` is a typed persistence boundary containing the
dataset pin, frozen filters, collection identities, permutation, visited
identities, cursor, current shuffle's draw offset and bounded random decisions.
`PuzzleSelectionSnapshot::from_persisted` checks source/version pins, unique
collection membership, an exact permutation, cursor bounds, the visited prefix,
and agreement between the current permutation and its recorded shuffle draws.
Restoration validates the available filtered definitions against that saved
membership and reconstructs the saved collection order even if the supplied
definitions arrive in another order. Missing, added or replaced matching
identities fail restoration. Dataset contents are expected to remain immutable
under their source/version identity, as required by the contract.

`PuzzleSelectionRandom<R>` records each valid bounded draw from the existing
provider. Restoration takes a fresh provider initialized by the caller and
replays all recorded bounds, rejecting the first differing choice. Successful
replay leaves that provider after the recorded draws, preserving future draws
and reshuffles. The provider API does not export a seed, algorithm identity or
internal state. This slice therefore does not claim to serialize those values:
the eventual persistence/composition owner must retain a reproducible provider
initialization contract. No RNG implementation or new dependency is introduced.

The recorder implements the existing provider trait and is available through
`selection.random_mut()` for `PuzzleSession::play_opponent_reply`. Existing
`PuzzleEnumEvent::OpponentReply` remains the owner of the chosen movement, node
and branch index. The fixture checks that this event agrees with the recorded
draw; selection does not duplicate attempt events or implement attempt recovery.
Those records must eventually be saved together with the selection snapshot.

`PuzzleSelectionRandom::transaction` requires `R: Clone`, copies only the
underlying provider and constant-size failure state, and remembers transcript
length. On an error it restores those values and truncates new draws, without
copying prior draws, puzzle definitions or the order. This supports candidate
session replies. `reshuffle_and_continue` uses that same transaction, with its
`R: Clone` bound restricted to this method. Failed shuffles leave the exhausted
selection and its valid snapshot unchanged. Invalid draws outside a transaction
cause snapshot creation to fail explicitly.

## Fixtures and verification

`tests/puzzle/selection.rs` contains 15 deterministic public-contract tests,
authored before the implementation and regression corrections. They use real
validated board/solution/metadata objects and the existing seeded provider. The
fixtures cover original IDs, no repeats, explicit exhaustion, repeated cycles,
seed reproducibility, inclusive filter edges, missing metadata, exact theme
tokens, empty/single matches, duplicate/mixed identity rejection, restore after
source reordering, membership/version changes, invalid snapshot boundaries,
invalid provider/replay decisions, rejected-draw rollback and existing listed
reply events sharing the recorded stream. The transactional reshuffle regression
was authored before the rollback correction.

The installed Rust 1.99.0 toolchain's formatter was used directly on the two
owned Rust files, followed by its scoped check and scoped `git diff --check`.
No Cargo command, test execution, Clippy, build, red-test observation or measured
performance result is claimed. The explicit delegation forbids Cargo and gives
the parent ownership of module/test-root registration and final gates.

The fixtures and lifecycle integration require the parent to add `Clone` to
the existing `PuzzleRandomProviderSeeded`; this handoff does not edit that shared
file. The patch contains only selection source, selection fixtures and this
evidence file, with no dependency setup, module registration or task markers.

## Remaining integration

The lifecycle owner must coordinate selection advancement and explicit
reshuffle with the existing timer's `exhaust_collection` and `arm_next`, carrying
remaining session time forward and waiting for readiness. Selection performs no
clock I/O and does not replace `PuzzleSessionConfig` or `PuzzleTimerSnapshot`.
Attempt/timer/selection snapshots still need joint validated, versioned durable
storage and paused recovery through T186/T187. History deletion and summary
coverage belonging to the rest of T179/T189, native commands/UI and the complete
T193 acceptance remain with their respective owners. No live I/O, UI or engine
search is added by this slice.

Deterministic shuffling and replay have a sequential dependency on the injected
random stream. Collection validation uses typed hash maps/sets; definitions and
permutation indexes use contiguous storage. Snapshot construction copies the
persisted identity/order/draw payload at the explicit boundary. No throughput,
latency, checkpoint cost or scaling measurements were run here.

## Parent verification

Registered the pure component and its tests. Actual pinned Rand 0.10.3 source
shows StdRng deliberately lacks Clone. The puzzle provider now uses the
already-available, explicitly named Xoshiro256PlusPlus generator and derives
Clone, allowing bounded transaction rollback without replaying or copying
the collection/transcript. No dependency or live-game generator changed.

Formatter and targeted library/domain Clippy passed. All 256 domain tests
passed, including all 15 selection cases and all prior attempt/countdown/summary
fixtures. Read-only review found no correctness blocker. Snapshot work scales
with the frozen collection and restore with cumulative recorded draws; no
large-corpus/checkpoint performance is claimed. Joint durable persistence of
provider initialization, actual reply events, selection and timer remains in
T187 and the run integration. T185/T179 remain open for their full acceptance.
The existing macOS linker warning remains unsuppressed.
