# Data Model: Reckless Desktop Chess

**Status**: Native-egui design revision; reusable backend implementation exists.
Native presentation migration and complete feature acceptance remain unperformed.

[Resolved decisions](decisions.md) supplies exact algorithms/defaults and the
release evidence register. [Review](review.md) records direct owner choices.

**Scope**: Phase 1 design for [the specification](spec.md). Names below are
proposed owning types, not generated source or permission to scaffold them all.

## Ownership and dependency map

| Component | Owns | Depends on |
| --- | --- | --- |
| Board | Validated positions, legal moves, notation conversion | Wrapped chess library |
| Game | Seats, move tree, rules, claims, result, live/viewed cursor | Board and clock contracts |
| Clock | Time controls, turn accounting, suspend policy | Injected monotonic clock |
| Engine | UCI protocol, capabilities, search, child lifecycle | Board positions, process port, capacity lease |
| Storage | Versioned documents, atomic writes, import commit | Typed game serialization and file port |
| Notation | Streaming PGN import/export and FEN boundaries | Board and game types |
| Report | Full-game analysis jobs and per-move findings | Game snapshots, engine search, capacity |
| Opening | Explorer corpus/index, statistics, prepared book moves | Board identity, selected storage |
| Puzzle | Puzzle positions, accepted solutions, attempt feedback | Board and approved dataset |
| Workbench | Diagnostics and benchmark jobs | Engine package and capacity coordinator |
| Desktop | User intents and native orchestration | Public APIs and injected component ports |
| Desktop profile | Canonical local identity, exclusive lease, activation and startup admission | Injected filesystem lock, OS identity and local-transport ports |
| Presentation | egui widgets, focus, layout, animation, subscriptions | Typed desktop application ports and scene interface |
| Scene | Bevy asset instances, materials, camera, picking and GPU lifecycle | Validated snapshots and the single Bevy render device |
| Update | Authenticated staging, verification and installation state | HTTP, credentials, signature, backup and OS installer ports |

Main constructs one Bevy App, the object graph and one async runtime, injecting
typed ports/view models into egui presentation and Bevy adapter resources. Plain
Rust application actors retain authoritative state. ECS entities represent
view/asset/camera instances; their IDs never enter domain or persistence types.
Render passes consume snapshots and emit single action identities. They cannot
open files, construct engine clients, await I/O or adjudicate chess.

## Identifiers, messages and external values

- `GameId`, `GameMoveId`, `EngineSessionId` and `EngineSearchId` remain opaque
  validated identifiers. Presentation shares their actual Rust types.
- Revisions, nanosecond durations and cumulative counters use checked native
  integer/newtype values. In-process messages have no JavaScript precision limit.
- Existing persisted canonical strings remain readable. External serialization
  adapters own schema-specific encodings and migrations; the UI change alone
  must not rewrite saved data or backups.
- Concrete owners include `BoardPosition`, `BoardEnumColor`, `GameSession`,
  `ClockTimeControl`, `EngineSearchLimit`, `CapacityLease`, `StorageGameDocument`,
  `WorkbenchJob` and `DesktopCommand`. The canonical naming rule owns policy.
- Persistence, settings, backup and signed-update schemas have separate versions.
  Internal Rust command compatibility is compile-time within one binary; startup
  does not negotiate a browser bridge protocol. Reject unsupported future file
  versions before mutation.

`DesktopRequest` contains an action identity, typed operation/payload and expected
revision. `DesktopEvent` contains subscription/sequence plus relevant game,
revision, session/search/job identities. Native bounded channel subscriptions,
latest-value telemetry and immutable snapshots follow
[the message contract](contracts/desktop-ipc.md). No direct egui/game mutex or
independent presentation clock is an authoritative state owner.

## DesktopProfile and startup admission

The destination-local profile denotes the existing local collection, not an
account or a new profile-selection feature. Its
[ownership decision](decisions.md#local-profile-ownership-and-activation) owns
bootstrap ordering, resource lifetime, authentication and limits.

| Value | Meaning and validation |
| --- | --- |
| `DesktopProfileIdentity` | Canonical existing data-directory identity plus destination-local ID; path aliases resolve to one lock anchor |
| `DesktopProfileLease` | One root-owned un-cloned exclusive OS file-lock handle; never serialized, inherited by children or owned solely by Bevy |
| `DesktopProfileOwnerRecord` | Separate bounded versioned metadata: profile ID, fresh owner generation, endpoint and diagnostic PID; atomically published after bind; no ownership authority |
| `DesktopProfileActivationRequest` | Version, matching profile/generation, request ID and closed Activate operation; no mutable app payload |
| `DesktopProfileEnumPhase` | Starting, Ready, Recovery, ShuttingDown or InstallPending, reachable only while holding the lease |
| `DesktopProfileEnumAdmission` | Owned lease, ExistingOwner activation result, Busy or typed startup failure; only Owned admits application services |
| `UpdateInstallIntent` | Durable profile/owner/helper generation, package digest/version, backup and schema binding; Prepared, Committed and recorded outcome follow the packaging contract |

No owner record or heartbeat can stand in for OS exclusion. The lease is acquired
before any user database or migration/restore recovery. Its control directory
survives restore/replacement and is excluded from backups; restoring another
machine's data retains the destination profile identity. Repeated launches
cannot clone game clocks or capacity coordinators. An owner in Starting coalesces
activation until its window is ready; activation changes no game/run phase.

Owned startup follows `Starting -> Ready` or `Starting -> Recovery` after storage
and presentation checks. A canceled or failed quit returns to its prior owned
phase; successful `ShuttingDown` drains all effects before final process teardown.
An acquired lease with a pending install intent admits only the helper/recovery
path in [packaging](contracts/packaging.md#profile-ownership-during-installation),
never ordinary initialization. A contending/unreachable launch cannot change
these states. After confirmed owner exit, the new owner performs saved-state
recovery with unfinished games/puzzle runs paused.
An installer may outlive its helper; lock acquisition alone cannot clear that
barrier. Prepared intent never authorizes installation, and safe cancellation
or recovery must establish helper/installer quiescence before revoking it.

## Native presentation and scene state

`PresentationApp` owns selected workspace/game, widget focus, transient edits,
confirmation state and received snapshots. Settings, draft and note buffers
preserve their distinct Save/Discard/Stay policies. Clock repaint deadlines are
presentation-only. One input action yields one retained request identity even
when egui performs multiple passes.

`SceneRenderer` owns Bevy asset/entity mappings, the board camera, offscreen
image target, immutable presentation snapshots and their render generation.
Bevy render resources own actual GPU object allocation/destruction. `SceneCamera` owns yaw offset, tilt,
zoom and fit, separate from White/Black orientation. `SceneEnumLifecycle` covers
initializing, ready, failed and retrying states. A failed generation invalidates
pending picks/animations and emits one native interruption. The application
pauses games; retry readiness never resumes them.

GPU objects follow Bevy render scheduling/lifetime, without a second device or
custom GL context. CPU asset preparation is bounded through the capacity
partition. Bevy scene hierarchy/name components identify the 12 color/kind GLB
parents and their descendants; do not select pieces by brittle entity indices.
Logical egui points become framebuffer pixels only at the renderer boundary;
resize, display-scale and viewport clipping changes recreate sized targets.

## BoardPosition and BoardMove

| Value | Meaning and validation |
| --- | --- |
| Piece placement | Validated square/piece/color values; exactly one king per side |
| Turn | `BoardEnumColor` |
| Castling rights | Explicit original rook squares, compatible with selected variant |
| En passant | Validated target and legal-context normalization |
| Halfmove/fullmove counters | Checked bounded integers |
| Variant | Standard or Chess960 castling interpretation |
| Legal moves | Derived from the authoritative position, never presentation declarations |
| Repetition identity | Board, turn, castling, and relevant en passant rights; counters excluded |

`BoardPosition` wraps the selected library behind fallible constructors and typed
operations. Distinguish syntactically parsed FEN from a position accepted for play.
Reject illegal king states, inconsistent rights, invalid move sequences, and
unsupported variant declarations before invoking Reckless.

`BoardMove` includes typed origin, destination, optional promotion, and special
move interpretation. UI intent uses visible destination squares. The owning
position converts standard and Chess960 UCI castling; it never guesses from
coordinate strings alone. Distinguish user cancellation from an illegal move.

FRC starts use a validated Scharnagl index 0..959. DFRC uses one validated index
for each color; its position still uses Chess960 castling semantics. Persist the
generated initial FEN and both indexes so random starts are reproducible.

## GameSession and GameMoveNode

| Field | Type or relationship |
| --- | --- |
| Identity and revision | `GameId`, monotonic revision |
| Mode | Human-versus-engine or local two-player |
| Initial state | Variant, initial `BoardPosition`, optional FRC/DFRC seed/indexes |
| Seats | Human or Reckless player per color; mode validates the combination |
| Move tree | Root and nodes identified by `GameMoveId` |
| Live cursor | Node whose position is authoritative for an active game |
| View cursor | Independently selected historical/variation node |
| Study mainline tip | Persisted export/study leaf, independent of live history and view |
| Clock | `ClockState` owned by the live line |
| Phase | Setup, active, interrupted, completed, or analysis |
| Outcome | Optional typed result plus reason and originating move |
| Metadata | Typed known PGN tags, ordered validated extension tags |

A move node owns its move, parent, ordered variation children, annotations,
plain-text notes, NAGs, saved position markings, and optional post-move clock.
Save Markings is explicit in analysis/review; live drawings are temporary. Confirm
saved-study variation promotion and retain the previous main line as an
alternative; this cannot alter a running game's played history. Notes use explicit
Save and Save/Discard/Stay before leaving unsaved edits. Clear temporary drawings
when the selected position changes while retaining saved markings. Confirm
analysis-branch deletion with explicit subtree scope; never delete live played
history through this operation. A game against Reckless may hold one queued
premove, configurable and initially disabled. Treat it as intent rather than an
accepted move; revalidate against the position after the engine move. Charge
actual elapsed time without a fixed minimum and retain normal increment/delay
rules. Choose promotion before queueing. New premoves replace old ones; Escape
and visible Cancel Premove cancel, while right-click draws. Clear illegal
premoves without execution and show a brief nonblocking notice; turn clocks
continue normally. Clear pending intent on any pause, takeback, fault, or relaunch.
Show distinct source/destination highlights and a Queued Premove label with
Cancel, distinguishable from drawings and last-move highlights.

SAN is derived from the parent
position. A cached SAN or position is tied to the immutable parent node; editing
an ancestor invalidates descendants rather than retaining stale derived values.

Domain state has one mutation owner. Applying a move checks expected revision,
live phase, side to move, legality, and result; then updates the clock and tree as
one transition. UI animation follows acceptance and cannot commit a second move.

Terminal outcome evaluation distinguishes checkmate, stalemate, resignation,
agreement, time forfeit, dead position, automatic repetition/move-count draw, and
accepted draw claim. Repetition hashes accelerate lookup but equality is checked
against full normalized identity to avoid collision-based adjudication.

New Game initially selects a White human seat against Reckless and Standard
chess; both choices are adjustable. Chess960 defaults to random with numbered-
position selection available. DFRC defaults to independently random back ranks
with numbered selectors for each side. Store the resolved initial position so
reopening a game never randomizes its start again. Use zero-based Scharnagl
0..959, uniformly sampled by the injected random provider; 518 is Standard.

Human seats require explicit Claim Draw for threefold/fifty-move draws. The app
always claims an available valid draw for the Reckless seat. This policy belongs
to the game owner; UCI has no claim command.
Threefold and fifty-move outcomes require a valid claim, including a claim based
on an intended legal move. Fivefold and seventy-five-move outcomes are automatic;
checkmate precedence follows the selected rules. The outcome owner covers dead
positions and flag-fall possible-mate conditions beyond the library's basic
insufficient-material helper. This needs dedicated acceptance fixtures and must
not be represented as complete merely because move generation works.

Takebacks are permitted in engine and local two-player modes. In engine play,
return to the previous human turn; locally undo one move. Stop the relevant
search and satisfy its completion barrier before mutation, then reject stale
results using existing revision/generation checks. Restore both clock snapshots
from before the undone moves, reversing increments and restoring associated
move/stage accounting. Local opponent confirmation is configurable per game and
required by default. Retain removed moves as an alternative variation.
Keep clocks running while local consent is pending; restore the snapshots only
if the request is accepted. Another accepted move cancels the pending request;
a new request can target the updated game. Local requests have no expiry timer;
await response, cancellation, or another accepted move. Engine-game takebacks
apply immediately on selection without an extra confirmation dialog. Always
confirm resignation in both modes. Serialize revisions; terminal results cancel
consent and prohibit post-result takeback. Reuse matching branches in stable order.

Manual pause is permitted in both game modes. Freeze clocks and stop active
engine search; require explicit Resume before play/clocks continue. A game-engine
fault preserves the game and pauses clocks. Restart only after explicit Retry;
successful recovery still requires Resume. Search resumption starts a new search
as needed, not an unsupported UCI checkpoint restore. Untimed searches restart
with the full configured per-move budget; timed searches use remaining clocks.

Multiple live games may run simultaneously, sharing engine resources. Each game
owns its state, clocks, revisions, and search correlation independently. Shared
Divide CPU capacity equally among active live-game searches. Automatically
divide one total hash-memory budget among engines. Game tabs and a live clock/
turn overview select the viewed game without pausing other running games.
Limit running engine games so each reserves at least one CPU thread, including
during human turns. Rebalance at each engine's next between-search boundary;
temporary unequal shares must fit the total CPU budget. New searches/admission
cannot consume capacity still assigned to an existing search. Release paused-game
hash allocations for running engines and recreate caches on Resume. The engine
release/recreation mechanism stops/quits/reaps the process, recreating it on
Resume because Hash has a nonzero minimum. Integer rounding, reservations and
CPU rebalancing follow [capacity decisions](decisions.md#capacity-and-memory).
The initial total hash budget is 10% of OS-reported available RAM at app startup,
capped at 2 GiB, with a Settings override. Insufficient engine memory keeps the
requested game paused with actionable budget/running-game guidance; do not
automatically reduce allocations. Byte/MiB rounding, hash admission and non-hash
allowances follow the selected capacity policy and require peak-memory tests.

Quitting with live games requires confirmation, followed by pausing and saving
all games before exit. Relaunch restores all unfinished game tabs paused;
explicit Resume is required. Autosave game state and clock snapshots after every
accepted move and pause/resume/result transition. Closing a running tab requires
confirmation, then pause/save/close; keep the unfinished game in the library.
Autosave failure retains current state in memory, clearly marks it unsaved,
pauses the affected game and engine, and offers Retry Save. Crash recovery restores
the last successfully saved clock snapshot paused, charges no downtime, and
requires explicit Resume. SQLite FULL transactions correlate durable transitions;
Retry saves the newest consistent snapshot and failed prerequisite saves block
exit/update/Trash. See [storage decisions](decisions.md#storage-imports-and-backup-recovery). Persist moves, phase, revision, and associated clock
snapshots atomically so recovery cannot combine different transitions.

### Game transitions

| From | Event | To and effect |
| --- | --- | --- |
| Setup | Start | Active; validate seats, position, clock, and engine readiness |
| Active | Legal live move | Active or completed; increment revision once |
| Active | Browse history | Active; only viewed cursor changes; clocks/game/search continue and UI offers Return to Live |
| Active | Branch from history | Separate analysis branch; live clock remains explicit |
| Active | Manual pause, engine/graphics failure, or OS suspension | Interrupted; freeze native clocks and record snapshot/reason; stop game search; faults offer explicit Retry without automatic restart/resume |
| Interrupted | Confirm resume | Active only after any required Retry restores readiness, with reconstructed engine state and explicit clock decision |
| Active | Accepted result | Completed; stop engine and freeze clock |
| Completed | Analyze | Analysis branch without rewriting recorded result |
| Any saved phase | Reopen | Paused review/interrupted state, never silent timed resume |

## ClockTimeControl and ClockState

Presets are Untimed, 1+0, 2+1, 3+0, 3+2, 5+0, 5+3, 10+0, 10+5, 15+10, and
30+0; retain custom controls and initial 10+5.

Display tenths below 10 seconds and whole seconds otherwise without reducing
native clock precision. At first crossing below 10 seconds, warn visually and
play one sound per player per game, respecting mute/background silence. Increments
do not retrigger the sound. Persist once-used flags independently of restored
clock snapshots so takeback/relaunch do not replay alerts. Local games initially use fixed orientation with a per-game Auto-flip
option after accepted moves; orientation is presentation state, not clock state.

- Initial new-game default: 10 minutes per side with a 5-second Fischer increment;
  adjustable through the approved settings/new-game controls.
- Approved control families: untimed, sudden death, Fischer increment, staged/
  cyclic controls, simple delay, and Bronstein delay. Reject negative or overflowing
  values. Stage transitions, bonus exclusivity, delay/refund/flag-fall semantics
  follow [clock decisions](decisions.md#clocks-starts-claims-and-races).
  Model delay kinds separately from increment, with typed duration and applicable
  per-turn accounting; do not reuse a Fischer-increment field for delay.
- Untimed engine play uses a configurable default of 3 seconds per engine move,
  separate from the confirmed report budget. Infinite analysis is not an
  engine-play default that can wait forever.
- State: remaining duration per side, active side, completed move counts, stage,
  turn-start instant, and paused/running status. Use monotonic elapsed time;
  persisted wall-clock timestamps are descriptive, not elapsed-time authority.
- Increment applies once after a legal accepted move. Engine `MoveOverhead` is
  separate from player clock bookkeeping and is never subtracted twice.
- App focus loss does not pause live clocks or engine play. On computer sleep,
  interrupt all live games and engine searches, preserve remaining time, and
  require explicit Resume.
  This policy is separate from puzzle focus-loss pauses and graphics failures.
- Reckless natively accepts remaining times, Fischer increments, and `movestogo`.
  It has no delay field. Delay accounting belongs to the application; use the
  selected single-movetime allocation with native safe turn deadline and test
  controlled-clock and pinned-engine behavior. The application decides flag fall and
  ignores a late move after time expires.

## EnginePackage and EngineCapabilities

`EnginePackage` records engine tag/commit, target triple, required CPU/OS feature
set, build profile, enabled Cargo features, network hash, binary hash, compiler,
license/source bundle, and optional NUMA runtime requirements.

`EngineCapabilities` combines the package manifest and parsed handshake. All
recognized options have typed identities and values; an unknown advertised option
is retained as a diagnostic descriptor and creates a coverage mismatch, not an
unvalidated generic `setoption` escape. Release verification rejects a changed
capability surface until its owner updates the inventory and scenarios.

See [the pinned capability inventory](contracts/reckless-capabilities.md) for
values and behaviors; do not duplicate its ranges into independently maintained
configuration tables.

## EngineSession, EngineSearchLimit, and EngineAnalysis

Session fields include identity, package, applied configuration, actor lifecycle,
child identity, pipe owners, active search, deadlines, and bounded diagnostics.
Its transitions are defined by [the engine contract](contracts/engine-protocol.md).

Search limits are a tagged union: infinite analysis, depth, nodes, move time, or
clock control. Clock control carries both sides' remaining time and increments,
and optional moves-to-go. This deliberately prevents unsupported combined limits.

A search owns `EngineSearchId`, session/process generation, game ID, position
revision, starting position/history, limit, start instant, cancellation state,
and a capacity lease. Output is attached by the single actor and accepted only
for the matching position revision. UCI itself supplies no correlation ID.

Analysis holds ordered PV lines. Each line includes MultiPV rank, depth, selective
depth, typed cp/mate score, exact/lower/upper bound, legal continuation, and
availability of associated statistics. Nodes, elapsed time, NPS, hash occupancy,
and tablebase hits are optional until reported; missing fields are not zero.
Present search scores consistently from White's perspective while preserving the
reported side-to-move value and bound; invert bound direction when negating.
Raw static `eval` is a distinct White-perspective NNUE integer, not search cp.

## CapacityCoordinator and WorkbenchJob

`CapacityBudget` records detected usable hardware threads, detection source,
memory availability, and explicit owner overrides. Discovery failure is a typed
setup failure requiring a supplied budget. `CapacityLease` assigns threads and
memory to one heavy workload and is released on every completion path.

The primary search takes the available budget up to the engine-advertised limit.
Admitting perft, bench, file-processing CPU work, or a NUMA benchmark requires
stop/rebudget or an explicit exclusive run. No second full-capacity search pool.
Do not count sleeping OS threads as equivalent to runnable CPU work; record total
thread counts separately when profiling.

`WorkbenchJob` is a tagged operation with validated parameters, package/profile,
state, capacity lease, deadline, progress, result, and cancellation. Diagnostics
run in a separate child. Blocking perft/bench cannot be interrupted by stdin
`stop`; cancellation terminates and reaps that child.

## Reports, openings, and puzzles

The owner approved these application facilities. Their exact data sources,
policies and interfaces follow [resolved decisions](decisions.md) and
[the study-tools contract](contracts/study-tools.md).

- `ReportJob`: Source game/line revision, engine package and options, requested
  search budget, per-position progress, results, and completion/cancellation state.
  Reports start automatically after games played in the app complete, over the
  played main line; imported completed games require an explicit report request,
  defaulting to a configurable 3 seconds per position. Variations are analyzed
  separately on demand. Jobs pause during live games and resume afterward;
  persist completed positions and restart interrupted positions with full budget
  under the selected scheduler/recovery policy.
- `ReportMoveFinding`: Position identity, played move, evaluations, candidate
  alternatives, app-calculated move labels, and the selected classification-policy
  version. Reports include the selected expected-score-loss accuracy, thresholds,
  aggregation, forced-move exclusions and incomplete findings under the versioned
  [report policy](decisions.md#report-scoring-and-job-scheduling). Selecting
  a finding navigates to its source position, exposes analyzed alternatives, and
  offers a separate analysis branch while preserving the original game.
- `OpeningCorpus`: Source/version, local games/index, supported metadata, and
  validation state. Position queries return typed legal continuations and available
  outcome counts, game counts/frequencies, available average player ratings, and
  openable sample games. Missing ratings/results are unavailable. Filters cover
  dataset, year/date, player rating, time control, player names, and result where
  source metadata exists. Result percentages use known-result games only; show
  unknown-result count separately. Count the first occurrence of a position per
  source game. Allow one or combined selected datasets with source identity.
  Deduplicate combined games using approved variant/start/mainline/player/date/
  result identity, retaining all source identities. Move-frequency denominator
  includes matching games with a next move; show games ending here separately.
  Rating criteria offer Both Players, White, Black, and Side to Move, default
  Both Players; exclude missing required ratings while active. Other metadata
  semantics, normalization and collision-checked position identity follow the
  selected explorer/identity decisions.
- `OpeningBook`: Source/version and prepared legal moves for study only. It never
  supplies opponent moves. Imports support Polyglot .bin with raw weights and
  normalized shares per selected position, expressing relative preferences rather
  than evaluations or winning probabilities. Initially order legal moves by
  descending weight; all-zero legal weights remain visible with shares unavailable.
  One-decimal shares, UCI lexical ties and Standard-only Polyglot coverage follow
  the selected book policy.
  In analysis/study, explorer/book selection reuses a matching variation or adds
  one, preserving the main line and never submitting a live-game move.
- `PuzzleDefinition`: Starting position and accepted solution tree, with source
  identity. `PuzzleSession` owns attempted moves and progression/completion state;
  saved progress and timed sessions are approved, together with theme/difficulty
  filters, hints, retries, and solution review. Both whole-session and per-puzzle
  countdown modes are approved, defaulting to configurable 5-minute sessions and
  60-second puzzles. Each timed run uses exactly one countdown mode. Manual pause
  and app focus loss pause the puzzle and hide its board/solution until resumed.
  Session expiry ends the run; per-puzzle expiry records a timeout and advances.
  Hints progress through source square, destination square, and next-move reveal;
  record assistance. Record failed attempts before permitting practice retries
  on the continuing countdown. Accept dataset-listed solutions; legal unlisted
  moves are not proven losing and do not trigger engine verification. Attempt
  history records outcomes, assistance, elapsed time, and session summaries locally.
  Randomly choose among listed opponent replies. Shuffle filtered puzzles without
  repeats until exhausted, then show progress and offer explicit Reshuffle and
  Continue. Filter changes apply to the next run, preserving the current run's
  collection. Stop the session countdown on the exhaustion progress screen;
  explicit Reshuffle and Continue carries the remaining duration forward.
  Keep history/summaries indefinitely until explicit deletion. Summaries use
  counts/rates for unassisted/assisted solves, failures, timeouts, and time spent;
  no points system. Persist paused run
  state, order/visited identities, and remaining time across sleep/relaunch;
  always require explicit Resume. Divide unassisted/assisted solve counts by all
  finalized puzzles including failures, timeouts, and skips; exclude active
  unfinished puzzles. Time spent is active countdown time including replies,
  animations, hints, and retries, excluding pauses/review. Session expiry makes
  an otherwise unresolved puzzle timed out and ends the run; an already-failed
  puzzle stays failed with a separate expiry event. Exact schema, remaining
  lifecycle and CSV setup-move/schema details follow selected puzzle decisions. Keep primary outcomes
  non-overlapping: failed stays failed after practice completion; track practice
  completion and assistance separately. Hint stages restart per player move,
  retaining attempt-wide assistance. Start each new puzzle's countdown when ready,
  visible, and interactive, including after Next. Skip advances without solution
  reveal or session-time reset and records a distinct skipped outcome; preserve
  already-recorded failure outcomes. Illegal input does not record a failure and
  does not stop the countdown. Confirm deletion of individual attempts,
  sessions with attempts, or all history; preserve datasets. After solving, enter
  solution review and wait for Next without charging the session countdown;
  hints during an active puzzle keep the existing countdown running. Review is
  distinct from the paused-active-puzzle state that hides the board/solution.
- `DatasetPackage`: Proposed common provenance/integrity descriptor for corpora,
  books, and puzzle sets, with format, checksum, local location, and validation state.

## Shortcut preferences and updater boundary

Shortcut preferences own remapped bindings and conflict validation. Initial
bindings are Cmd/Ctrl+K palette, Cmd/Ctrl+N New Game, Cmd/Ctrl+, Settings, and
F Flip Board outside text inputs; Cmd on macOS, Ctrl elsewhere. Reset Shortcuts
is included. The palette exposes all applicable user actions, preserving required
confirmations and unavailable-state explanations. Exact focus/context and OS
reserved-key rules need validation.

OTA updates use private GitHub release assets, a signed versioned manifest and
native download/signature/installer adapters. Cargo-packager creates installers;
it does not own update policy. `UpdateSession` carries check/download attempt,
release/asset identity, target, verified manifest digest, cache receipt and phase.
Its typed states distinguish awaiting network approval, downloading, downloaded,
install prerequisites, handoff, failed and cancelled. Retries create new attempts
and cannot reuse expired metered-connection approval. See [packaging](contracts/packaging.md). Update binaries are private; version-tag pushes automatically
build/publish releases. Check nonblockingly at startup and expose Check for
Updates in Settings. Use a user-provided fine-grained GitHub token restricted
to this repository with Contents read permission in the OS credential store.
Download automatically, except require approval on OS-detected metered
connections and when status is unknown or unavailable. Offer Cancel during downloads: stop, discard partial bytes, and wait for explicit
Retry. Keep completed downloads across relaunch and revalidate before installation.
Each metered/unknown-connection attempt, including Retry, requires approval.
No Remove Download action: retain completed downloads until installation or
replacement. Keep the cached version when a newer stable release appears and
offer explicit Download Newer Version under the same approval rules. If the
connection changes to metered/unknown, stop, discard partial bytes, and ask
before a fresh attempt.
Check attempts have
a 15-second total deadline across release discovery and metadata, excluding
binary download. Downloads stop after 60 seconds without received bytes or
30 minutes total, whichever occurs first, with manual Retry. These deadlines
need a native owner and deterministic validation. Installation requires
explicit Install and confirmation. Retain all published private releases/assets
until explicit owner deletion. OTA signing-key/passphrase custody belongs to
GitHub Actions encrypted secrets with an owner-controlled offline backup;
those secrets are never part of client application state.
Pause and successfully save all live games before proceeding; restore them
paused. A save failure prevents installation. After pause/save, a fresh successful
user-data/settings safety backup at the configured destination is required;
exclude dataset files and credentials. A backup failure also prevents installation
and exposes Retry. Publish stable releases only, with vMAJOR.MINOR.PATCH tags
matching the application version. Retain the latest five successful pre-update
safety backups plus any needed for unresolved recovery, separately from
daily/manual backups. Failed migration stops normal startup and preserves data;
offer pre-update backup recovery with preview and explicit confirmation. A
compatible app reinstall may be required, without a guaranteed binary rollback.
Installation failures expose explicit Retry where safe, otherwise manual-recovery
guidance; do not retry automatically. Recovery follows the staged migration,
journal and confirmed backup policy in resolved decisions. Keep credential material outside
ordinary preferences, exports, backups and general presentation snapshots. Missing, expired,
or rejected tokens produce a persistent Settings status with add/replace-token
controls and no blocking prompt. Offline chess remains usable. Failed downloads
stop and expose Retry; no automatic download retries. Successful installation
requests automatic relaunch where supported, with an explanation where manual
reopening is needed. Restored games remain paused. Recovery contracts follow
resolved decisions; these policies do not establish a tested native lifecycle.

## Display language and notation

The first-release interface is English. Bundle Inter for UI/notation with
tabular clock digits; verify exact binaries and figurine glyph coverage before
bundling. Display notation is a preference enum with Letters and Figurines,
initially Letters. PGN serialization retains standard letters independently of
the display preference; presentation must not change stored move identity.

## Editor drafts and notation interchange

Persist invalid editor drafts separately from validated positions, with explicit
invalid status and actionable validation findings. Saving/reopening a draft must
not bypass validation; prohibit play/engine analysis until corrected. New drafts
copy the selected position or Standard setup if none; saved drafts restore their
own position. Include confirmed Clear Board and Standard Setup and editor-local
Undo/Redo for piece/position-field edits. Undo belongs to the draft, not the
source game; editing a copied position cannot change the source game. Save
drafts explicitly; leaving unsaved edits offers Save, Discard, and Stay. Persist
the draft position but not editor Undo/Redo history across sessions. Include
select-and-click placement, palette/square drag/drop, and keyboard placement/
removal. Play from Position opens New Game with the validated position prefilled
and requires Start. Library Positions and the editor draft list expose the same
saved drafts, clearly distinguished from games. FEN-to-editor creates a new
draft, preserving the previous draft and its unsaved-edit prompt; reject invalid
FEN with an explanation. Exact shortcuts and structural-position validation
follow selected presentation/clock decisions.

PGN import stages fully validated games and previews all errors/exclusions before
confirmation. Skip any game containing invalid moves or branches; do not recover
legal prefixes as complete games. PGN exports default to supported metadata,
notes, variations, notation marks, and available clock annotations, with an
explicit mainline-only alternative.

## StorageGameDocument and preferences

The owner's ORM requirement changes the internal persistence adapter according
to [the storage ORM plan](plan.md#storage-orm-migration) and
[compatibility contract](contracts/storage-orm.md). It does not change any domain
entity or persisted schema. Typed database mappings retain canonical text IDs and
unsigned revision strings, nullable metadata, existing byte bodies and puzzle
journal proofs. ORM row construction never substitutes for the current domain
validation. One actor-owned connection preserves each existing atomic boundary.

The owner selected SQLite for indexed game-library and opening-statistics data,
with PGN/FEN interchange. Schema, adapter, transaction boundaries, migration,
snapshot implementation, migration recovery and backup restore follow
[storage decisions](decisions.md#storage-imports-and-backup-recovery). Backup Now and daily automatic backups while open are approved;
schedule overdue backup on next launch. Include user data/settings, games/Trash,
annotations, reports, editor drafts, and puzzle history; exclude dataset files.
Default to an app-data backup folder with configurable destination. Keep the
latest 30 automatic backups and all manual backups. Restore by full replacement
after a safety backup and confirmation; reopen games paused and leave dataset
files external. Backups are unencrypted in the first release. Automatic backups
are due 24 hours after the last successful automatic backup. If backups fail while
game autosaving works, continue play with a persistent backup warning, Retry
Backup, and destination controls. Restore across all supported platforms with
explicit machine-path/unavailable-option review. Safety retention, supported
schema migration, first-run/retry scheduling and interrupted restore follow
[storage decisions](decisions.md#storage-imports-and-backup-recovery). The bundled starter datasets
have an installed budget of up to 2 GB, excluding the application and engines;
personal imports use the selected streaming/input budgets; the total user
library has no fixed game-count cap.

`StorageGameDocument` schema 3 distinguishes played sessions from clockless
notation under one typed envelope and shared record codec. Played payloads retain
runtime clocks and an explicit study leaf; notation retains its selected mainline
tip and source clock annotations without inventing seats or runtime state. Actual
legacy schema-1/2 envelopes remain readable without rewriting their stored body,
identity or revision. Played duplicate identity follows actual live moves;
notation identity follows its explicit mainline. Codec acceptance evidence is
[the library content foundation](evidence/library-content-codec.md).

Game records contain schema version, game identity, initial FEN/variant,
validated move tree, result, metadata, and the content kind's available clock data.
Library state includes
favorites, tags, folders, and Trash status. Search/filter covers players, date,
result, variant, opponent mode, favorites, tags, folders, opening name/code, and
game length; annotation-text search is excluded from the first release. Nested
folders give each game one folder or Unfiled; tags allow multiple groupings.
Initially sort by most recently updated first. Confirm folder/subfolder removal
and move contained games to Unfiled without deleting them. Import preview identifies
duplicates and defaults to skipping them, with explicit keep-copy available;
never silently overwrite annotations. Duplicate candidates match variant, initial
position, played mainline, players, date, and result; preview annotation/variation
differences. Normalization and missing-field semantics follow resolved decisions.
Trash supports Restore and confirmed permanent deletion without automatic expiry.
Moving an open unfinished game to Trash requires confirmation, then pause, save,
close its tab, and move it to recoverable Trash. Restoration conflicts and sort
update semantics follow selected identity/library decisions.

Preferences contain
typed application options surfaced through the dedicated Settings tab, with
validated accepted values and an explicit settings revision. Editable groups and
save/apply behavior follow [the UI contract](contracts/ui-experience.md#settings-tab):
appearance applies immediately, while other changes require Save and engine
application may be deferred until idle. Leaving unsaved edits offers Save,
Discard, and Stay. Per-section and all-settings reset require confirmation and
preserve games, puzzle history, and datasets. Validated settings import/export
uses versioned JSON. Exports exclude machine-specific paths; preserve destination
locations. Validate imports, preview changes, and require confirmation before
merging included fields while preserving others. Migrate explicitly supported older schemas before validation/preview; reject
unsupported older schemas. Reject newer schemas without changes and explain that
an app update is needed. For otherwise valid files with unavailable engine
options, preview supported changes and list unavailable options explicitly; apply
supported fields only after confirmation.
Distinguish saved preferences from
pending/effective runtime configuration. No executable path supplied by the
presentation is trusted. Preferences include approved configurable board aids and
optional move/check/result sounds. Remember window size/position, navigation
collapse, panel widths, and selected side-panel tab; Reset Layout is included.
Fit restored windows to current displays/minimum size. Coordinates are initially
always visible around the board, follow orientation, and are configurable.
Initial defaults enable legal-move dots on
selection, last-move/check highlights, and captured-piece display. Sounds default
to enabled during active play and silent in background. Live drawings are
temporary; analysis/review explicitly saves markings on a game-tree position.
Clear temporary drawings when the selected position changes; saved markings
remain attached. Right-click toggles circles, right-drag toggles arrows, and
repeating a marking removes it. Colors are green/red/blue/yellow, initially green;
exact shades/modifiers follow selected presentation decisions. Audio preferences include master volume
and per-category volume/enable controls for moves, checks, results, and low time.
While foregrounded, route selected-game moves/checks and all-live-game result/
low-time alerts, respecting mute/background silence. One accepted move produces
only its highest-priority sound: result, then check, then ordinary move. Initially
use 70% master and 100% category levels, combined multiplicatively. Suppress
sounds when no app-owned window is focused, including minimized/hidden, without
pausing live games/clocks. Discard muted/background sound events and never
replay on focus return/unmute. Serialize cross-game sounds by selected priority
and use original synthesized offline assets. Real-time 3D camera rotation/zoom are presentation
state; they must not mutate positions or game history. The default view is
centered at 20 degrees from top-down with all squares visible; normal piece
dragging, right-drag drawing, and visible
rotation/zoom controls are approved. Persist the last camera view across launches
and provide Reset View. Quality defaults to Automatic with manual Low/Medium/High
overrides. Keep the entire board visible at maximum manual zoom-in. Reset View
restores default pose/framing/tilt while retaining White/Black orientation.
Allow full 360-degree yaw and tilt 0..45 degrees, preserving full-board framing.
Button steps are 15-degree yaw, 5-degree tilt, and 10% zoom. Auto-flip rotates
180 degrees over 180 ms, retaining tilt/zoom, respecting reduced motion, and
leaving native clocks running. Framing margins, quality presets and adaptation
thresholds follow the selected presentation policy. Use a native scene owner with the selected Rust UI/engine integration;
[renderer research](research.md#native-egui-renderer-and-integration) owns its
exact integration and dependency pins. Automatic quality adapts during use
to sustained frame-time changes, with safeguards against rapid switching.
Initially target 60 fps during active rendering; render on demand while unchanged,
with clocks updating separately. Reduce shadows/effects before internal resolution.
Adaptation samples must distinguish active frames from intentional idle periods;
idle frame absence is not a graphics-performance failure. Exact tiers,
thresholds, limits and GPU baseline follow presentation decisions; performance
remains unmeasured.
Graphics failures interrupt
the native game, preserve state, and pause clocks; Retry restores rendering and
explicit resume restores gameplay. The board is 3D-only, without a 2D view/fallback.
The renderer consumes validated snapshots without adjudicating moves. The move
list, opening explorer, and study-only book share the selected-position cursor;
tab changes do not mutate the game. Show the White-positive evaluation bar only
in analysis and game review, not during live play.

Imports are bounded by the plan's limits. PGN comments are user game data, not
authored code comments. Keep ordered unknown tags and annotation text as typed
notation-boundary values; HTML is never rendered from imported comments. Export
escaping and variation order must round-trip through the fixed corpus.

## Selected starter-data direction

The preparation manifest extends the existing dataset provenance owner, with
policy version, frozen source digests, observed typed strata/counts, selected
prefix lengths, selection-sequence digests and ordered byte-cap reductions.
Each selected record retains its canonical source identity, full game identity
where applicable, source metadata and derived preparation stratum. Missing
metadata remains typed unavailable with a reason, not a fabricated rating,
time class or theme. The complete source theme set and raw TimeControl remain
available independently of preparation labels. These are dataset/preparation
values; they neither enter GameSession nor change explorer-filter semantics.
Normalization, exact boundaries, ordering and cap transitions have one owner in
[starter subset selection v1](decisions.md#starter-subset-selection-v1).

Follow [starter provenance/selection](decisions.md#starter-datasets-and-provenance),
[puzzle interchange](decisions.md#puzzle-interchange-and-lifecycle), and
[book generation](decisions.md#book-generation-and-explorer-semantics). Standard
starters, two selectable CC0 online collections including a titled-master subset,
broad puzzle difficulty, manual updates and the 2 GB installed cap are selected.
Exact file checksums and measured collection sizes are preparation outputs,
not unanswered preferences or already acquired data.
