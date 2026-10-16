# Desktop IPC Contract

**Status**: Design selected under owner delegation; implementation and release evidence remain unperformed.

Use [the resolved decisions](../decisions.md) for concrete defaults, algorithms
and former draft questions; [the review](../review.md) records owner choices.

The Rust application is authoritative. Vue components send typed intents and
render validated snapshots. This interface is local Tauri IPC, not an HTTP API.

## Schema ownership and envelopes

Rust DTOs own one versioned wire schema. Generate TypeScript declarations and
runtime validators from it; do not maintain handwritten copies. Generate schemas
and types deterministically without authored comment banners. Reject generation
drift in the lint workflow. Serialization adapters validate all external fields
before creating domain types.

Every request carries protocol version, request ID, operation tag, typed payload,
and expected game revision when mutating a game. Responses are success with
typed value/revision, or failure with a discriminated error. IDs and large
counters follow [the data model](../data-model.md).

Do not expose shell execution, arbitrary engine commands, unrestricted filesystem
access, or executable paths to the browser. Native dialogs issue opaque file
selection handles; Rust validates and consumes those handles.

## Commands

| Operation | Payload | Result and behavior |
| --- | --- | --- |
| `app.bootstrap` | Supported protocol version | Application identity, settings, package/capabilities, current snapshot |
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

Every event includes subscription ID, monotonic sequence, protocol version,
event kind, and relevant game/revision/session/search/job identifiers.

Event kinds: game changed, clock snapshot, analysis snapshot, search completed,
engine state changed, workbench progress/completed, settings changed, and failure.

Use Tauri channels. The engine contract owns native queue/telemetry bounds.
Allow at most 64 unacknowledged events per subscription; pause replaceable
snapshots until acknowledged. A control event that cannot be delivered safely
closes the subscription with a resync requirement. The authoritative application
state persists, so reconnecting with a snapshot recovers the terminal state.
Do not rely on the WebView channel itself to supply backpressure.

The frontend checks sequence and generation before applying an event. A gap
triggers resync. Presentation interpolation of clocks uses a Rust timestamp and
remaining duration; browser timers never adjudicate flag fall.

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

## Frontend tooling and privileges

Use Vue 3 SFCs for thin views, typed owning models/controllers for interaction,
and the generated bridge for every native call. Use shadcn-vue components backed
by Reka UI for dialogs, menus, forms, and tooltips; tailor their theme centrally
through Tailwind tokens. No second chess rules implementation in JavaScript.

Bun owns frontend dependency installation and scripts, with a committed text
`bun.lock` and a pinned Bun runtime. ESLint flat configuration covers Vue SFCs,
TypeScript, generated-contract drift checks, and zero-warning validation.
Prettier owns formatting; its Tailwind plugin owns deterministic class ordering.
Keep lint and formatting responsibilities distinct, without disabling findings
to hide implementation errors.

Only the main local WebView receives the minimal Tauri permissions it needs.
Use a restrictive CSP, local bundled assets, no remote executable content, and
Rust-owned process/file access. Test automation capabilities and plugins are
excluded from release artifacts.
