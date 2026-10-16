# US1 production game composition

This increment accepts T068 backend dispatch and adds a real-engine composition
slice of T071. T062 and T071 remain open. The starting HEAD was
`72cedf035e3e23b4c63546dd30c91dd27b30bf1f`, with a clean working tree and
61/295 tasks checked. Accepting T068 brings the count to 62/295, leaving 233
tasks pending.

The explicit harness mode is registered in [tests/native.rs](../../../tests/native.rs)
and implemented in
[tests/native/game_composition.rs](../../../tests/native/game_composition.rs).
It calls the public `DesktopNativeBootstrap::run_with_host` with the shipping
`--profile-directory` argument and a temporary profile. The callback replaces
only the native window host. Production resource discovery, package selection,
`EngineNativeProcessProvider`, native game factories/service, shared CPU/memory
resources, game actors, proof adjudication and SQLite remain the real graph.
It never constructs `EngineLiveGame` directly or submits Play to obtain an
engine move. The only Play submissions are deliberate post-result rejection
checks.

No shipping Rust source, CLI, manifest, dependency, lockfile or profile setting
changed. The native target retains `harness = false` and `test = false`.
Existing compatibility, host-lifecycle, direct-engine and asset-comparison modes
remain registered and compile in the named native target.

## Real-engine cases

Each fixture is validated through `BoardPosition` before bootstrap: parse the
expected UCI move, play it independently and require checkmate. Both games use
custom Standard starts, a Reckless opponent, the specified human seat, explicit
30-second sudden-death clocks and initial game options.

- Human Black, engine White: `7k/8/5KQ1/8/8/8/8/8 w - - 0 1`, expected `g6g7`.
- Human White, engine Black: `8/8/8/8/8/5kq1/8/7K b - - 0 1`, expected `g3g2`.

The callback sends FrameworkReady for the production preparation generation and
waits for Steady through native wake notifications. Bootstrap must report a
ready engine. StreamSubscribe then precedes game creation. Commands retain one
request across Busy admission retries, carry the operation's revision for game
mutations, and observe their receipts through native wake notifications.
Stream consumption requires matching subscriptions and continuous sequences and
acknowledges each consumed event. Lifecycle Failure and any game interruption,
including Unknown adjudication, fail acceptance. Queued Setup snapshots from
before automatic Start are allowed only at an older revision than Create's
active response.

Both production games passed these assertions:

- Exactly one accepted engine move advances the created revision once and
  produces the independently validated final FEN and SAN.
- Phase is Completed, reason is Checkmate, the winner is the human's opposite
  color, outcome origin is the live node, and live turn is the human's color.
- No interruption or save-required state is accepted. The human has zero
  completed moves and its full 30 seconds; the engine has one completed move
  and positive remaining time no greater than its starting allowance.
- Clock views at captured_at and captured_at plus ten seconds are identical.
  This advances a supplied observation instant without sleeping.
- A further revision-checked Play rejects. A successful ReturnToLive query
  retains the same revision, root/live nodes, terminal outcome and clock state.
  Subsequent durable restoration also verifies that no extra node was added.

Missing or unavailable real-engine resources produce failure. This mode has no
skip path and injects no fake engine or alternative adjudicator.

## Persistence, restart and cleanup

The callback captures its Result and always returns through production bootstrap
for owned cleanup. Exercise and cleanup failures are combined without discarding
either cause. Outside the callback, only the independent budget clone and
immutable observations survive; no capacity-phase Arc or application-service
handle escapes.

After each of two production bootstraps, the harness observed **14/14 CPU slots
available**, successfully acquired and dropped a full-worker lease, found zero
direct child processes using sysinfo, and reacquired the isolated profile through
`DesktopNativeStartup::admit` as Owned. Process inventory must include the
harness itself before an empty child set is accepted.

Under a reacquired profile lease, the harness opens `library.sqlite3` through the
existing `StorageDocumentStore`. It reads each document by `GameId::as_uuid`,
copies its body/revision, releases document transport reservations and closes
the store. `StorageGameDocument::restore` then runs synchronously outside the
async runtime. Both restorations preserve game/root/live/view identities, exact
game revisions, a two-node one-move tree, the actual typed move, initial/final
positions, completed outcomes and origin, and frozen clock balances. Storage
revisions must be positive and remain unchanged on relaunch; they are distinct
from game revisions because saving on Quit can advance storage without a new
game transition.

A second production bootstrap readies the native services and subscribes again.
It receives `Games([])`: completed library games intentionally do not reopen as
live tabs. The production service restores only Setup, Active and Interrupted
games. A second read verifies byte-identical completed document bodies and equal
storage revisions, followed by the same typed restoration assertions. The
temporary directory is explicitly closed and failures are preserved.

Command, preparation, event and document-read waits have five-second timeouts;
each callback has a 30-second outer deadline. Wake loops use notifications and
cooperative Busy retries. Production startup/shutdown keeps its existing owned
deadline and observed-cleanup policy. No wall sleeps, internet access or owner
profile data is used.

## Engine and host identity

Observed on macOS 27.0.1, build 26A434, arm64, with Rust/Cargo 1.99.0 matching
the project pin. Production UCI reported:

```text
Reckless 0.9.0 by Arseniy Surkov, Shahin M. Shahin, and Styx
```

The real production resource directory was revalidated with
`EnginePackageSelector` after bootstrap cleanup:

- Target/profile: `aarch64-apple-darwin`, portable.
- Binary: `resources/engine/bin/reckless-aarch64-apple-darwin`, 65,383,664 bytes.
- Binary SHA-256: `ab4128cd33149b14713475e049db8ddaff702ee79bfb412ecfbde91655273689`.
- Source: v0.9.0, commit `0e92358f5acd66e5ac77b1bf558202e47c515435`.
- Manifest-pinned embedded model: `v54-5478683c.nnue`, 63,266,880 bytes,
  SHA-256 `5478683cb1bababde29ae8f29468a99846726548fc6a0ed54cac40ab6d38efbf`.

The binary digest and UCI identity were checked in this increment. Model identity
comes from the validated pinned manifest and the existing
[engine build evidence](engine-build.md), not a fresh extraction of the embedded
network. No engine rebuild or download was performed.

## Executed checks and red-to-green history

Final related Rust edits were formatted and verified sequentially:

```sh
cargo fmt --all
cargo clippy --locked --test native -- -D warnings
cargo test --locked --profile dev --test desktop desktop_game_
cargo test --locked --profile dev --lib desktop::native::bootstrap::tests
cargo test --locked --profile dev --test native -- --game-composition
```

- Named native Clippy: passed with warnings denied; every registered native
  mode is compiled. Its initial run caught three nonexhaustive enum fallback
  arms; shared response extraction now handles all result variants explicitly.
- Desktop game checks: **32 passed**, zero failed/ignored, 169 filtered out.
  These comprise 12 transition, 10 supervisor, five registry and five snapshot
  cases.
- Bootstrap checks: **6 passed**, zero failed/ignored, 44 filtered out, including
  the local game's production creation/play/save/recovered-paused round trip.
- New explicit native mode: **2 real-engine games passed**, two post-result
  Play rejections, both durable checks and the production relaunch. This is a
  standalone `harness = false` executable, not two additional libtest cases.

The same targeted native command was red during harness development: initially
two compilation errors (unused import and SAN API argument); then a timeout
constructed before entering Tokio; then rejection of a queued pre-Start Setup
snapshot; then an incorrect equality between storage and game revisions.
Those were harness mistakes, corrected against the actual public APIs and
observed production lifecycle. They did not establish a production graph bug.
No shipping-source fix or hermetic source regression was needed. The final
assertions still require exact game revision preservation and unchanged storage
revisions on relaunch.

The prior [590-test debug checkpoint](native-controls.md) is historical evidence
at the starting HEAD. This increment reran the listed **38 focused hermetic
tests** and the new production composition mode, not the complete 590-test
selection. It adds no new hermetic libtest cases.

The known macOS linker warning remains: `__eh_frame section too large (max
16MB) to encode dwarf unwind offsets in compact unwind table`. It appeared in
debug executable linking and is not a source/Clippy diagnostic or a warning-free
full-build claim. No checker suppression was introduced.

## T068 eligibility and remaining gates

T068 covers typed backend dispatch and composition. The existing transition
owner verifies start/play, navigation without altering the live clock, all four
promotions, illegal input and engine-seat refusal, stale revisions, retained
analysis branches, resignation/agreement/claims, pause/resume and mode-specific
takebacks. Registry/supervisor cases verify routing, autosave/save failure,
retained cleanup, bounded ordinary/control admission and stale queued requests.
Those 32 checks pass again. The six bootstrap cases and new real-engine mode
establish native composition for local and actual engine games, with real
revisions, autonomous engine turns, terminal results and persistence. This
supports accepting T068 without claiming all native UI or story acceptance.
Its task row now names the actual native bootstrap and transition owners.

T071 remains open: these are two deliberately bounded windowless production
games, not full story 1 scenarios 1 through 8, the complete adversarial corpus,
GPU/input/accessibility interaction, or fresh offline installed-package play.
The real engine is local and no network is used, but the host network was not
physically disabled. Other-platform support and optimized latency/performance
were not evaluated.

T062 remains open because practical dead-position and side-specific possible-mate
coverage remains unresolved; see [mate-proof limits](mate-proof.md). These two
proven mate fixtures do not establish practical general-position completeness.
The remaining foundation, installed-package, full US1 and release gates retain
their existing status. No release, all-target or all-feature rebuild, full
`make verify`, Git mutation, new checkout/branch/agent or publication was performed.
The extension configuration was absent at both pre- and post-implementation
checks, so no extension hooks were dispatched.
