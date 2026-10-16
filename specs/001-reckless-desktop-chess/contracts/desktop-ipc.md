# Native Desktop Message Contract

**Status**: Native-egui design revision. Existing browser bridge implementation
is preserved as migration input; this contract defines its Rust replacement.
The filename is retained to preserve feature-document references.

Use [resolved decisions](../decisions.md), [the data model](../data-model.md) and
[the current plan](../plan.md). The application is authoritative. egui submits
typed intents through injected ports and renders immutable snapshots. This is
an in-process Rust interface, without Tauri IPC, HTTP or UI JSON serialization.
Operation labels below document closed Rust enum variants, not dynamic dispatch
by string name. External files and UCI remain separately validated boundaries.

## Local activation interface

Repeated-launch activation is a separate OS-local interface, not access to the
in-process command table below. The
[profile ownership decision](../decisions.md#local-profile-ownership-and-activation)
owns exclusion, authentication, endpoint selection and all connection limits.
No listener is an HTTP server, public API or generic command executor.

Use a version-2, length-prefixed UTF-8 JSON frame: unsigned 32-bit big-endian
byte length, then a bounded validated object. External decoding uses existing
Serde boundary types; it introduces no UI serialization. Reject unknown fields,
unknown operations/versions, invalid lengths and extra frames. One connection
has one request, response, consumption receipt and receipt acknowledgment under the selected
deadlines, without subscription or arbitrary launch-argument replay.

| Message | Typed fields and semantics |
| --- | --- |
| Request | Protocol version, destination-local profile ID, current owner generation, request ID, closed Activate variant |
| Response | Matching version/profile/generation/request ID and closed result: OwnerStarting, Focused, ActivationRequested, Busy or Rejected |
| Receipt | Matching request/profile/generation identity confirming the response was consumed; no second operation or ownership grant |
| ReceiptAcknowledged | Matching request/profile/generation identity confirming the owner consumed the receipt; no second operation or ownership grant |
| Native delivery | At most one coalesced pending window-activation notification, followed by a native event-loop wake; no application mutation command |

Authenticate the local peer before accepting the request, and authenticate the
server before sending a client request or trusting its response. Unix verifies
both effective users; the Windows client verifies the kernel pipe peer's token
SID as specified by the ownership decision. Verify profile and
generation against the record bound by the exclusive owner. Focused requires
actual native focus acknowledgment; calling a focus/attention method alone can
only produce ActivationRequested. OwnerStarting retains one activation request
for the forthcoming main window. Busy/Rejected and expired connections convey
no ownership grant. Do not expose a Resume, migration, backup, update-install,
file-open, shell or credential operation through this protocol.

The second launch exits after the matching ReceiptAcknowledged frame, showing manual window-selection
guidance if activation is unconfirmed. On unreachable-owner failure it shows
Busy/recovery guidance without another writable session. A later explicit
launch retries arbitration; neither timeout nor stale endpoint permits takeover.
Application lifecycle and install barriers remain in effect while activating.
On Windows the concrete pipe adapter's owned stream suppresses drop-time linger;
the receipt confirms response consumption before server closure. The owner
consumes and validates Receipt, writes ReceiptAcknowledged, and keeps the stream
open until the client closes; the client validates that acknowledgment before
closing. This preserves both final frames without an underlying pipe flush,
write half-close, or drop-time linger. Every step retains the same absolute
exchange deadline. A missing, mismatched, unexpected or late acknowledgment
cannot confirm activation. Version 1 is rejected before requesting window
activation. Missing receipt/acknowledgment or timeout permits closure and a later
explicit activation attempt, without replaying an app mutation or claiming
focus/ownership. Unix and Windows share this wire format, not necessarily the
same transport constructor.

## Type ownership and envelopes

Desktop owns `DesktopCommand`, `DesktopResponse`, `DesktopEvent` and their
component-prefixed payloads, identifiers and errors. Both presentation and the
application compile against these types. Revisions and counters retain native
checked integer/newtype values. Serde/schemars belong only to persisted or
external exchange formats; no generated TypeScript or runtime browser validators
are required for in-process messages.

Every request carries an operation/request identity, typed payload and expected
entity revision for mutation. Responses contain a typed value/revision or an
owned error. A request ID is created once per input action and retained through
confirmation/retry; repeated egui passes never mint or resubmit actions.
Mutations are serialized by the authoritative actor. Cancellation and shutdown
are explicit commands with observed completion.

Presentation receives narrow application ports, not direct database/process/file
clients. Native dialogs return opaque selection handles; adapters validate and
consume them. Engine selection is package-owned. Credential-entry buffers go
only to the credential adapter, never general snapshots, logs or saved state.

## Runtime ownership

Profile arbitration precedes application runtime services and Bevy pool creation;
a secondary activation client uses only a bounded short-lived runtime. In the
admitted owner, one root-owned Tokio runtime serves application actors, UCI pipes
and network I/O. One native Bevy App owns the event loop/window and graphics device; egui
integration consumes injected presentation state. ECS entity IDs are private
to the outer scene adapter, never game identifiers.
UI callbacks use bounded nonblocking admission. Full queues return a typed busy
state and retain action identity for explicit retry; do not drop a command or
spawn an unbounded waiter. Background state changes send a coalesced native event-loop wake notification.
The adapter drains bounded control/snapshot work before egui presentation;
widget systems never block_on a future. Bevy update scheduling and egui repaint
requests are coordinated rather than assuming repaint alone wakes every state.
Snapshot delivery cannot delay engine stop, clock adjudication or durable saves.

The ordinary application command queue holds at most 256 admitted requests;
an entity's presentation owner keeps at most one unapplied mutation for that
entity. Acquire admission before allocating waiting tasks. When that queue is
full, reserved urgent-control slots still accept one coalesced Stop/cancel and
Pause intent per admitted entity, plus one global shutdown intent. Repeated
urgent requests share the pending outcome; they do not create extra waiters.
At the application-port receipt boundary, attach the absolute stop deadline so
queue waiting cannot reset or escape it. Busy/rejected admission is explicit.

Application-owned CPU batches check cancellation/yield between bounded chunks
of at most 50 ms on the reference workload; framework or engine work that cannot
cooperate uses the selected preparation barrier or isolated process lifecycle.
Cancellation is complete only when owned work has stopped and cannot later
commit staging or mutate state. Keep partial progress/staging separate from
accepted data. A timed-out mutation has an unresolved outcome until its owner
reconciles it; neither timeout nor Cancel permits blind replay or detached writes.

## Commands

| Operation | Payload | Result and behavior |
| --- | --- | --- |
| `app.bootstrap` | Typed startup context | Application identity, settings, package/capabilities, current snapshot |
| `game.create` | Mode, seats, variant/start, time control | Validated game and initial revision |
| `game.start` | Game ID, expected revision | Active game after prerequisites succeed |
| `game.play_move` | Game/revision, from/to, promotion if required | Accepted move and authoritative snapshot, or typed rejection |
| `game.navigate` | Game ID, node ID | Viewed position only; live game/clocks/search continue; offer Return to Live |
| `game.undo` | Game/revision and mode-checked takeback intent; local consent defaults required and is configurable per game | Previous human turn against Reckless, one move locally; stop search before mutation; restore clocks/stage counters from before undone moves, reversing increments; retain removed continuation as a variation; clocks continue while consent is pending; another accepted move cancels the request; no local request expiry timer; engine-game takeback needs no extra confirmation; terminal results cancel pending consent; serialized revision checks resolve races |
| `game.branch` | Game/node IDs | Explicit analysis branch |
| `game.finish` | Game/revision, resignation/agreement/claim plus intended move if relevant | Validated result or rejected claim; resignation requires user confirmation in both modes |
| `game.pause` | Game ID and expected revision | Both modes; freeze native clocks, interrupt active engine search, preserve game; later explicit Resume required |
| `game.resume` | Game/revision, explicit clock decision | Reconstructed session and active state after explicit Resume; fresh full per-move budget for untimed engine search, remaining clocks for timed search; Retry alone never resumes |
| `analysis.start` | Game/revision, tagged search limit, MultiPV request | Search ID and initial state |
| `analysis.stop` | Session/search ID | Stop accepted; completion arrives in ordered stream |
| `engine.configure` | Typed option changes, expected capability version | Validated applied configuration after idle barrier |
| `engine.clear_hash` | Engine session ID | Idle-only maintenance completion |
| `engine.restart` | Failed session ID and explicit user Retry intent | New generation; no automatic restart or game/clock resume |
| `engine.tablebases` | Opaque selected-directory handles | Path preparation, load status/cardinality or explicit limitation |
| `library.import` | Opaque file handle | Stage fully valid games only, skipping games with invalid moves/branches; preview errors/exclusions and duplicates before confirmation; commit confirmed staged games |
| `library.save` | Game/revision | Durable save receipt |
| `library.open` | Saved game ID | Validated paused/review snapshot |
| `library.export` | Game ID, format, full/mainline-only selection, native destination handle | Atomic export receipt; full PGN is default with supported metadata/notes/variations/notation marks/clocks |
| `position.import` | Bounded FEN plus declared variant | Validated analysis game |
| `position.export` | Game/node ID | Canonical FEN |
| `preferences.update` | Typed patch and expected settings revision | Validated settings and revision; safe runtime application follows the owner-selected Settings-tab policy |
| `preferences.export` | Native destination handle | Versioned JSON excluding machine-specific paths |
| `preferences.import_preview` | Native source handle and expected settings revision | Validated included-field merge preview preserving other settings and destination paths; migrate explicitly supported older schemas before validation/preview; reject unsupported older and all newer schemas without changes; newer schemas explain update needed; explicitly list unavailable engine options and preview only supported changes |
| `preferences.import_apply` | Confirmed preview identity and expected settings revision | Apply the previewed merge; engine changes wait for safe search boundaries |
| `workbench.start` | Tagged diagnostic/benchmark operation and validated parameters | Admitted job ID or capacity refusal |
| `workbench.cancel` | Job ID | Cancellation state, then terminal event |
| `stream.subscribe` | Topics, last acknowledged sequence | Typed channel and initial full snapshot |
| `stream.ack` | Subscription ID, last consumed sequence | Flow-control acknowledgement |
| `stream.resync` | Subscription ID | Fresh snapshot and new sequence boundary |

Search options, workbench operations, and supported variants are closed typed
selections driven by the pinned engine contract; user-supplied text cannot add
an operation. Imported PGN text is displayed as text, never interpreted as markup.

The dedicated Settings tab uses the preferences operation above. Saved values
and effective runtime values must be distinguishable if application is deferred.
Appearance applies immediately; other changes are submitted through Save.
Engine changes wait for a safe point between searches when necessary. See
[the Settings-tab contract](ui-experience.md#settings-tab).

Graphics failure during live play must reach the native game owner as a typed
interruption intent, preserving game state and pausing clocks. Retry/recovery
status is distinct from explicit game resume. Renderer interruption and
recovery intents follow the selected operation families; retain game/revision and
stale-result checks rather than allowing scene code to control clocks directly.

## Ordered streams

The approved editor, full-game reports, openings, puzzles, annotations, and library
facilities add interface requirements recorded in
[the study-tools contract](study-tools.md). Their detailed commands remain
selected in [interface completion](../decisions.md#supporting-libraries-and-interface-completion).

Every event includes subscription ID, monotonic sequence and schema identity,
event kind, and relevant game/revision/session/search/job identifiers.

Event kinds: game changed, clock snapshot, analysis snapshot, search completed,
engine state changed, workbench progress/completed, settings changed, and failure.

Use bounded Rust channels with one subscription owner. The engine contract owns
engine queue/telemetry bounds. Allow at most 64 unacknowledged lifecycle events
per subscription; pause replaceable snapshots until acknowledged. Analysis and
clock presentation use a latest-value slot keyed by entity and generation, with
sequence boundaries that distinguish intentional coalescing from control-event loss. A control event that cannot be delivered safely
closes the subscription with a resync requirement. The authoritative application
state persists, so reconnecting with a snapshot recovers the terminal state.
Repaint wakeups are notifications only; the bounded channel and latest-value
slots own backpressure. Admission must bound waiting work as well as queued work.

Presentation checks sequence and generation before applying an event. An
unexplained control-event gap triggers resync. Clock interpolation uses native
monotonic snapshots; the game clock owner alone adjudicates flag fall.

## Typed failures and concurrency

Error categories include invalid input/position/move, promotion required, stale
revision, invalid game phase, unsupported capability/protocol, resource refusal,
engine lifecycle/protocol failure, tablebase limitation, file/format/size error,
save conflict, and cancellation. Each includes a stable typed code, safe message,
relevant IDs, and explicit retryability. Raw stack traces and private paths are
not part of normal player-facing messages.

Two concurrent moves with the same expected revision cannot both succeed.
Duplicate request IDs for mutation return the retained result within a bounded
idempotency window; after expiry, clients must resync before retrying. A command
timeout is not proof of failure, so the UI queries state before replaying it.

## Native presentation and lifecycle

Use centrally styled egui widgets and component-owned view models through
bevy_egui. Enable its optional AccessKit feature and native platform support;
bridge native ActionRequest events into egui before presentation. The scene
does not supply accessible square actions automatically. Separate
headless widget/input tests from real native GPU scene tests. AccessKit semantics,
keyboard navigation and actual VoiceOver/NVDA/Orca acceptance accompany the 3D
board; a rendered mesh supplies no accessibility by itself.

The `native` production-host test target uses `harness = false` and
`test = false`; default Cargo test execution remains hermetic. Explicit named
target selection provides its separate lint, nonexecuting build and required
native execution checks under the
[test-selection decision](../research.md#native-test-target-selection).
No release application command, activation request or packaged listener can
enable harness controls.

Disable Bevy automatic main-window close. Close/menu/quit/update intents enter
the same application shutdown state machine while confirmation/save is pending.
Pause/save/reap asynchronously; only a successful completion permits exit.
Resolve notes, editor and Settings Save/Discard/Stay prompts through their
existing policies before beginning irreversible shutdown. Stay, canceled quit
and failed saves retain all current edits, games and ownership.
Save failure leaves the window and newest in-memory state available. An update
handoff follows the same save barrier plus its fresh-backup requirement. The
composition root observes all task outcomes and shuts down its runtime after
the Bevy runner exits, with an idempotent bounded cleanup fallback for OS
termination paths and a tested native quit adapter. Bevy AppExit is final
shutdown, not the trigger for beginning an unawaited save. No widget creates a
nested runtime.
After required saves succeed, use one shared three-second graceful worker/child
shutdown window; do not restart it for each child or drain stage. Stop new
admission and cancel noncritical work, then terminate/reap remaining children
at expiry. Observe termination before releasing their leases. An unresolved
critical save or unconfirmed owned-worker exit keeps shutdown incomplete, the
application available and the profile lease held, with actionable pending/error
status. OS-blocked writes cannot be claimed canceled merely because a timer
elapsed; the storage owner reconciles completion before any retry or exit.
The root retains its profile lease until mutable owners, children and runtimes
have stopped, including nonreturning native-exit cleanup. Quit cancellation,
failed saves and graphics recovery never drop it. Installer handoff uses the
[durable installation barrier](packaging.md#profile-ownership-during-installation).
