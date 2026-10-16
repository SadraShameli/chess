# Visual and Interaction Contract

**Status**: Design selected under owner delegation; implementation and release evidence remain unperformed.

Use [the resolved decisions](../decisions.md) for concrete defaults, algorithms
and former draft questions; [the review](../review.md) records owner choices.

Implements FR-003, FR-007, FR-011, FR-012, FR-023, and FR-024 from [the specification](../spec.md).
Stack: Tauri 2, Vue 3, strict TypeScript, Tailwind CSS, and shadcn-vue/Reka UI.

## Visual direction

The owner selected a dark-first premium chess application with richer effects
and animation, plus a full light theme. The board remains the focal point.

The overall layout is approved: collapsible left navigation, a large board with
player bars/clocks, and resizable right-side move/analysis/opening-study tabs.
The palette direction is approved: graphite surfaces, emerald accents, a
green/cream board, and a warm light theme. The owner selected sculpted 3D-style
pieces, bundled Inter UI/notation with tabular clock digits, and
configurable 180 ms piece moves/160 ms panel transitions with subtle elevation.
The board and pieces render in real time with camera rotation and zoom. Pieces
use ivory/ebony materials with subtle satin highlights. The minimum usable window
is 1280x800 CSS pixels. The first-release interface is English. Offer letter and
figurine move display, defaulting to letters; PGN retains standard letters.
Exact tokens, original assets, Inter 4.1/SVG figurines, dimensions, renderer pin,
camera framing, input and quality follow
[presentation decisions](../decisions.md#presentation-assets-and-input). Direct Three.js with a dedicated scene controller and WebGL2 is
selected for the first release.

| Element | Direction | Acceptance |
| --- | --- | --- |
| Surfaces | Approved graphite dark surfaces and warm light theme; exact tokens/elevation selected in decisions.md | Both themes feel deliberately designed; no pure-black glare or washed-out labels |
| Accent | Approved emerald direction; exact tokens and non-color cues selected in decisions.md | Selection/check/result remain distinguishable without color alone |
| Board | Approved real-time 3D board with green/cream squares; coordinates and marker styling selected in decisions.md | Equal square dimensions and clear piece contrast at every supported scale |
| Pieces | Approved real-time sculpted 3D with ivory/ebony satin materials; original generated GLB meshes selected | All six kinds distinguishable at the smallest supported square size |
| Typography | Bundled Inter UI/notation with tabular clock digits; English interface; letters/figurines with letters default; Inter 4.1 with original SVG figurines selected | No layout shift as clocks/statistics change; readable notation |
| Layout | Approved left navigation, large board/player bars/clocks, resizable right tabs | Approved minimum 1280x800 CSS pixels; 1440x900 default with selected panel sizes/reflow |
| Motion | Approved configurable 180 ms moves, 160 ms panels, subtle elevation; respect reduced motion | No animation delays authoritative moves or prevents the next action |

Tailwind theme tokens are the shared design source. Use shadcn-vue primitives
for keyboard/focus-aware controls and customize them through that token system.
Do not scatter arbitrary color, spacing, or duration values through components.
Tailwind v4 uses its official Vite integration; do not add Sass as a parallel
styling pipeline. Small custom CSS belongs to its owning component and follows
applicable owner rules, with linting appropriate to Tailwind syntax.

## Real-time 3D rendering and camera

The owner selected real-time rendering for both board and sculpted pieces, with
camera rotation and zoom. Ivory/ebony satin materials apply to the pieces; the
board retains its approved green/cream square palette. Use direct Three.js inside
Vue, owned by a dedicated scene controller, with WebGL2 for the first release.
Three.js 0.186.1, original GLB models, lighting/shadows, quality presets and
framing margins follow the selected presentation decisions; validate actual
GPU/WebView compatibility. Initially
center the camera at 20 degrees from top-down with all squares visible. Keep the
entire board visible at maximum manual zoom-in. Drag pieces normally,
right-drag to draw, and use visible camera controls to rotate/zoom. Allow full
360-degree yaw and tilt 0..45 degrees while keeping the full board visible.
Per-click steps are 15-degree yaw, 5-degree tilt, and 10% zoom, bounded by these
limits and full-board framing. Right-click toggles circles; right-
drag toggles arrows; repeating a marking removes it.
Remember the last camera rotation/zoom across launches and provide Reset View
to restore default pose/framing/tilt while retaining the current White/Black
orientation. Auto-flip changes side by 180 degrees over 180 ms, retaining tilt/
zoom, respecting reduced motion, and leaving native clocks running. Use 35-degree
vertical FOV, 8% fit margin, distance multiplier [1,1.6] initially 1.1, and
multiplicative 1.1 zoom steps under the selected corner-fitting algorithm.

Use Automatic quality by default and allow Low/Medium/High manual overrides.
Adapt during use to sustained frame-time changes with safeguards against rapid
quality switching. Initially target 60 fps during active rendering. Render the
3D scene on demand; stop continuous scene frames while unchanged, with clocks
updating separately. Reduce shadows/effects before internal resolution to retain
crisp board rendering as long as possible. Distinguish intentional idle periods
from active-frame performance samples. Use the selected tiers, p95 thresholds,
2-second sample windows, cooldown and resolution limits in presentation decisions.
Hardware WebGL2 features and reference-system performance require actual tests;
no measured claim is established by naming presets.

Camera movement changes presentation only. Picking and drawing must map back to
validated board squares under every permitted camera view. Preserve semantic
keyboard/screen-reader controls alongside the 3D scene; the renderer does not
become another chess-rules authority. Synchronization of semantic-grid focus
follows visual White/Black orientation independently of free camera yaw, with
explicit labels and orientation compass.

The application is 3D-only: no selectable or fallback 2D board. On graphics
failure during a live game, preserve authoritative game state, pause clocks,
and offer Retry. Successful graphics recovery does not resume gameplay or clocks
until the user explicitly resumes. Record the interruption through the native
game owner; the scene cannot adjudicate clocks or accept stale pending moves.
Context/initialization/shader/asset failures interrupt native state. Initial
startup shows Retry/setup guidance; Retry has a 10-second initialization deadline. Validate recovery in each supported Tauri WebView.
Measure GPU/CPU
usage, frame pacing, resource loading, and board input while Reckless searches.
Rendering performance has not been measured. Animation durations are presentation
defaults, not measured frame pacing or permission to delay input.

## Workspaces

- Play: player bars/clocks, board, move list, new-game actions, result state, and
  a clearly optional analysis panel. No unsolicited engine chatter.
- Analysis: board, navigable variation tree, candidate lines, evaluation/bounds,
  expandable complete statistics, and visible search mode/stop control.
- Library: searchable/filterable/sortable saved games with favorites, tags,
  folders, duplicate import preview defaulting to skip with explicit keep-copy,
  and Trash with Restore/confirmed permanent deletion and no automatic expiry.
  Search/filter players, date, result, variant, opponent mode, favorites, tags,
  folders, opening name/code, and game length; no annotation-text search in this
  release. Nested folders give each game one folder or Unfiled. Duplicate matches
  use variant, initial position, mainline, players, date, and result; preview notes/
  variation differences. Initially sort by most recently updated first. Confirm
  folder/subfolder removal and move contained games to Unfiled without deleting
  them. Confirm moving an open unfinished game to Trash, then pause/save/close
  its tab and move it to recoverable Trash. Include import/export actions, metadata, and actionable
  empty/corrupt-file states.
- Settings: owner-approved dedicated tab for commonly used preferences. The
  editable groups and apply behavior are reviewed in the settings section below.
- Workbench: position inspector, raw evaluation, perft, compiler diagnostics,
  benchmark jobs, cancellation, and exportable results. Keep technical controls
  here rather than in ordinary move selection.

The owner-approved feature expansion also requires position editing, full-game
report review with evaluation graph, opening explorer/book inspection, and a
puzzle-solving experience. Use Library/Positions/Puzzles navigation and board-side
review/report tabs under the selected layout. See [the study-tools contract](study-tools.md).

New game includes side, standard/FRC/DFRC setup, time control, and full-strength
engine search budget where relevant. Do not offer a fictional Elo slider.

## Selected UI/UX behavior

The owner approved a Chess.com-inspired board layout using the project's premium
dark/light styling. Use collapsible left navigation, a large central board with
player bars/clocks, and resizable right-side tabs for moves, analysis, and opening
study. The minimum usable window is 1280x800 CSS pixels. Exact panel dimensions,
reflow details follow presentation decisions. Remember window size/position, navigation
collapse, panel widths, and selected side-panel tab across launches. Include
Reset Layout. Fit restored windows to current displays and respect the approved
minimum window size; initial/reset size is 1440x900, clamped to usable displays. Opening-study tabs share
the selected board position as specified below.

Approved configurable board aids are legal-move dots, last-move/check highlights,
captured pieces, and user-drawn arrows/circles. Initially show legal-move dots
on piece selection and enable last-move/check highlights and captured-piece
display. Coordinates are initially always visible around the board, follow its
orientation, and are configurable. Right-drag draws markings on demand; camera rotation/zoom use visible
controls. Live drawings remain temporary. Analysis/review offers explicit Save
Markings attached to the specific game-tree position. Right-click toggles circles
and right-drag toggles arrows; repeating a marking removes it. Offer green, red,
blue, and yellow with green initially selected. Exact shades, modifiers, and a
Clear Temporary Markings action follow presentation decisions. Clear temporary drawings when the selected
position changes; saved markings remain attached. These user annotations do not approve live engine hints. Captured-piece
display counts actual captures since the recorded initial position, rather than
inferring missing starting material.

The command palette, keyboard shortcuts, and optional move/check/result sounds
are approved. Move/check/result sounds are enabled during active play and silent
while the app is backgrounded by default, with user configuration available.
At first crossing below 10 seconds, show a visual warning and play one low-time
sound per player per game, respecting mute/background silence. Increments do not
retrigger this sound. Display tenths below 10 seconds and whole seconds otherwise,
without reducing native clock precision. Warning lifecycle across recovery and
takebacks preserves the persisted once-used alert flag; no replay after recovery.

While foregrounded, route move/check sounds only from the selected game and
result/low-time alerts from all live games, respecting mute/background silence.
For one accepted move, play only the highest-priority event: result, then check,
then ordinary move. Include master volume and per-category volume/enable controls
for moves, checks, results, and low-time alerts in Settings. Initially use 70%
master volume and 100% per category, multiplying category and master levels.
Suppress sounds whenever no app-owned window is focused, including minimized/
hidden states, without pausing live games or clocks. Discard suppressed muted/
background events; never replay on focus return or unmute. Cross-game overlap,
original synthesized sounds and context-specific bindings/conflict rules follow
presentation decisions. Serialize cross-game sounds by priority and drop overlap.

Shortcuts are remappable in Settings with conflict detection and Reset Shortcuts.
Initially use Cmd/Ctrl+K for the palette, Cmd/Ctrl+N for New Game, Cmd/Ctrl+, for
Settings, and F for Flip Board outside text inputs; Cmd on macOS, Ctrl elsewhere.
The palette exposes all applicable play/analysis/study/library/puzzle/Settings
actions, preserving required confirmations and explaining unavailable actions.
Validate platform-reserved chords; reject collisions in overlapping contexts
and suppress text-field/modal-inappropriate shortcuts under the selected policy.

The owner approved these analysis/review interactions:

- Show the White-positive evaluation bar in analysis and game review only; do
  not show it during live play. The evaluation graph, ranked candidate lines,
  and expandable statistics retain their existing contracts. Exact bar scaling
  and mate/bound/unavailable-score presentation follow the selected E(cp), signed
mate, inequality/hatching and unavailable-neutral policy.
- Clicking a move-quality finding navigates to its position, shows the analyzed
  alternatives, and offers an explicit analysis-branch action. This depends on
  approved scoring and completeness rules. Exploring a branch must not rewrite
  the original game's moves or result.
- Preserve the selected position when switching the move list, opening explorer,
  and study-only opening book tabs. Those tabs consume the same selected board
  position rather than resetting it or creating separate position cursors.
  Switching tabs alone must not play a move or change game history.

## Settings tab

The owner requested a dedicated Settings tab to adjust commonly used options.
The owner approved the following groups. Their individual controls follow the
approved feature scope and the delegated resolved game/puzzle policies:

| Approved group | Controls from existing feature scope |
| --- | --- |
| Appearance and board | Theme, board/piece presentation, board aids, animation/reduced motion, remembered camera/Reset View, Automatic or Low/Medium/High quality |
| Keyboard | Remapping, conflict detection, and Reset Shortcuts; platform-specific initial bindings |
| Audio | Master and per-category volume/enable controls for move/check/result/low-time; selected-game moves/checks and all-live-game results/low-time while foregrounded; 70% master/100% category; suppress and discard while unfocused |
| Game defaults | Default play mode, side, supported time controls, and untimed engine search budget |
| Analysis and reports | Search limit mode/budget, displayed candidate lines, report preferences |
| Puzzles | Theme/difficulty filters, session/per-puzzle durations, hint and retry preferences |
| Data | Active local opening/book/puzzle collections, import access, Backup Now, automatic backup status/destination, persistent failure warning and Retry Backup, cross-platform restore with explicit path/option review |
| Advanced engine and resources | Automatic/manual CPU budget, hash allocation, supported engine options, and local tablebase locations |

Appearance changes apply immediately. Other settings require an explicit Save;
engine changes take effect at a safe point between searches, with pending state
shown until applied. Leaving unsaved settings that require Save offers Save,
Discard, and Stay. Include per-section reset and Reset All Settings, both with
confirmation; preserve games, puzzle history, and datasets. Include validated
settings import and export as versioned JSON. Exclude machine-specific paths
from exports and preserve destination locations. Validate the file, preview
changes, and require confirmation before merging only included fields; preserve
other settings. Engine changes still wait for safe search boundaries. Migrate explicitly supported older schemas before validation/preview; reject
unsupported older schemas. Reject newer schemas without changes and explain that
an app update is needed. For otherwise valid files with unavailable engine
options, preview supported changes and list unavailable options explicitly; apply
supported fields only after confirmation. Exact form boundaries and per-game
overrides follow resolved decisions: defaults apply to new games/runs/jobs,
while existing objects retain snapshotted settings. White-positive score
presentation remains the confirmed policy; this inventory does not add a
perspective switch. Timed puzzle runs select one countdown mode. Manual pause
and app focus loss pause puzzles and hide the board/solution until resumed.
Puzzle runs and remaining time survive sleep/relaunch; always require explicit
Resume, including on focus return. Remaining puzzle policies follow resolved decisions;
live chess clocks retain their separate policy.

Persist accepted preferences and validate input before committing it. Clearly
show saved preferences separately from pending runtime application when those
states differ. Engine changes respect the idle barrier in
[the engine protocol](engine-protocol.md); settings must not bypass search or
clock sequencing. Unsupported engine options remain tied to detected capabilities.
A changed default does not rewrite an existing report's recorded configuration.

Use the existing preferences schema and update operation from
[desktop IPC](desktop-ipc.md) rather than a separate settings transport.
Validate persistence, invalid-input handling, and effective-state presentation
according to SC-014. Verify immediate appearance changes separately from
Save-controlled changes and deferred engine application.

## Board interaction

- Provide 64 semantic focusable squares as a keyboard-managed grid. Sculpted
  piece visuals use the rendering path selected by the owner and retain these
  controls and square labels.
- Support click-to-select/click-to-move and pointer drag. A click alternative
  exists for every drag operation. Right-drag draws; visible camera controls
  handle rotation/zoom. Rejected moves restore the authoritative board.
- Arrow keys move focus in visual orientation; selection/confirmation is explicit.
  Announce square, piece, selection, legal destinations, check, and accepted move.
- Promotion opens a shadcn-vue dialog with all four choices, initial focus,
  keyboard selection, cancellation, and focus return to the originating board.
- Chess960 castling uses explicit legal affordances where a king's final square
  is ambiguous. Display ordinary final piece placement, not UCI rook-target notation.
- Local games initially keep fixed orientation with per-game Auto-flip available;
  when enabled, rotate the 3D board 180 degrees over 180 ms after each accepted
  move. Retain tilt/zoom, respect reduced motion, and leave native clocks running.
  Store remembered yaw offset separately; Auto-flip changes game orientation
  without overwriting that offset.
- Board flip changes presentation only. History browsing visibly distinguishes
  viewed position from live game and running clock. Keep the live game, clocks,
  and engine search running; clearly offer Return to Live. Browsing cannot itself
  change the live position or submit a move from a historical node.
- Allow takebacks in both engine and local modes: return to the previous human
  turn against Reckless or undo one local move. Restore the clock snapshots from before the undone moves, reversing increments. Local opponent confirmation is configurable
  per game and required by default. Keep clocks running while consent is pending;
  acceptance restores the snapshots. Cancel pending consent when another move
  is accepted; allow a new request against the updated game. Local requests have
  no expiry timer; await response, cancellation, or another accepted move.
  Engine-game takebacks apply immediately without an extra confirmation dialog.
  Always confirm resignation in both modes. Terminal results cancel pending
  consent; serialize revisions and reuse matching variations in stable order. Retain removed
  moves as an alternative variation. The initial adjustable new-game
  time control is 10 minutes plus 5 seconds per move; untimed engine play defaults
  to a configurable 3 seconds per move. Include untimed, sudden death, Fischer
  increment, staged/cyclic, simple-delay, and Bronstein-delay controls. Exact
  stage/delay rules and engine time allocation follow the resolved clock policy. App focus loss keeps live
  clocks running; computer sleep
  pauses timed play and requires explicit Resume, independently of puzzle rules.
- New Game initially selects White for the human against Reckless and Standard
  chess. Allow changing the side and selecting Chess960/FRC or DFRC. Chess960
  defaults to random with numbered-position selection also available. DFRC
  defaults to independently random back ranks with numbered selectors for each
  side. Use labelled zero-based Scharnagl 0..959, with 518 the standard start.
- Offer explicit Claim Draw for human claimable threefold/fifty-move draws. Do
  not automatically claim for a human. Automatic fivefold/seventy-five-move draws
  remain automatic. The app always claims an available valid draw for Reckless;
  this does not send an invented engine command.
- Support one queued premove against Reckless, configurable and initially
  disabled. Revalidate after the engine move; queued intent cannot bypass native
  move acceptance or clock authority. Charge actual elapsed time without an
  added fixed minimum, retaining normal increment/delay rules. Select promotion
  before queueing. New premoves replace old ones; Escape and visible Cancel
  Premove cancel, while right-click draws. Clear illegal premoves without
  execution and show a brief nonblocking no-longer-legal notice; turn clocks
  continue normally. Clear pending premoves on any pause, takeback, fault, or
  relaunch. Highlight source/destination and show Queued Premove with Cancel,
  visually distinct from drawings and last-move highlights.
- Offer manual Pause in both game modes. Pause clocks/engine search and require
  explicit Resume. Engine faults preserve the game and pause clocks; show Retry
  and restart only on that action. Recovery alone never resumes gameplay.
  Resuming an untimed search starts fresh with the full configured per-move
  budget; timed searches use remaining clocks.
- Support multiple simultaneously running live games sharing engine resources.
  Use game tabs plus an overview showing every live game's clocks and turn.
  Switching the viewed game does not pause others. Divide CPU capacity equally
  among active live-game searches and automatically divide one total hash budget
  among engines. Limit running engine games so each reserves at least one CPU
  thread, including during human turns. Rebalance at the next between-search
  boundary; temporary unequal shares must fit the total budget. Release paused-
  game hash allocations and recreate caches on Resume. Initial hash budget,
  admission waiting/refusal and tab/overview state follow the selected resource
  and layout policies.
  Initial hash sizing uses 10% of OS-reported available RAM at app startup, capped
  at 2 GiB, with a Settings override. Insufficient engine memory keeps that game
  paused; explain which budget or running games must change.
- When quitting with live games, ask for confirmation, then pause and save all
  games before exit. On relaunch restore all unfinished game tabs paused; require
  explicit Resume. Confirm closing a running game tab, then pause/save/close it;
  retain it in the library for reopening. Autosave state and clock snapshots after
  every accepted move and pause/resume/result transition. On save failure retain
  current state in memory, mark it unsaved, pause the affected game and engine,
  and offer Retry Save. Crash recovery restores the last saved clock snapshot
  paused, charges no downtime, and requires explicit Resume. Checkpoint durability
  and save-retry semantics follow serialized SQLite FULL commits; Retry saves
  the newest consistent state and failed prerequisite saves block exit/update.

## Accessibility and state

Use native semantics and the Reka UI interaction contracts; installing a component
library does not prove accessibility. Test keyboard traps, dialog focus return,
menus, screen-reader labels, and panel reflow in the real system WebViews.
Meet WCAG AA text/control contrast and 24 CSS pixel minimum targets where
applicable; prefer larger move and promotion controls.

Respect OS reduced motion and theme; allow explicit preference overrides.
Live analysis announcements are throttled and player-controlled so a screen
reader is not flooded. Announce moves/results/errors promptly; do not announce
every depth or node count. Keep focus stable when analysis updates.

Loading, engine unavailable, invalid import, stopped search, no legal moves,
empty library, saved/recovered state, and completed game all have designed views.
An engine failure preserves the board and offers a specific recovery action.

## Acceptance evidence

Record screenshots and interaction recordings for play, analysis, promotion,
settings, workbench, command palette, board markings, errors, and completed games
in both themes. Validate at the approved 1280x800 CSS-pixel minimum, 1440x900,
and a large desktop size; include 100%, 150%, and 200% display
scales on the declared target systems. Test current Tailwind browser requirements
against the actual WebView versions before claiming the OS baseline.

Review using keyboard only and VoiceOver on macOS, NVDA on Windows, and Orca on
Linux. Measure frame pacing and input latency while Reckless uses the maximum
assigned capacity. Compare against the budgets in the specification; do not
claim smoothness from screenshots alone. Exercise camera rotation/zoom, square
picking, drawing, occlusion/readability, promotion controls, and keyboard focus
in all permitted views. Test graphics initialization/context recovery after the
remaining fault-detection/retry details are selected. Confirm game/clock pause,
Retry behavior, explicit resume, remembered camera/Reset View, and quality
selection. Verify bundled models/textures load offline.

## Private desktop update controls

Follow [the packaging contract](packaging.md#desktop-ota-and-github-actions).
Settings provides Check for Updates and credential configuration for a user-provided
repository-scoped Contents-read token stored in the OS credential store. Downloads
start automatically when an update is available, except ask before downloading
on OS-detected metered connections and when status is unknown or unavailable.
A check attempt times out after 15 seconds total across discovery/metadata,
excluding binary downloads. Downloads stop after 60 seconds without received
bytes or 30 minutes total, whichever occurs first, and expose manual Retry.
Offer visible Cancel while downloading: stop, discard partial bytes, and wait
for explicit Retry. Keep completed downloads across relaunch and revalidate
before installation. Ask for metered/unknown-connection approval on every attempt,
including Retry. Do not offer Remove Download: retain completed downloads until
installation or replacement. Keep an already-downloaded version when a newer
stable release appears; offer explicit Download Newer Version under the same
approval rules. If connection status changes to metered/unknown, stop, discard
partial bytes, and ask before a fresh attempt. Installation waits for explicit
Install and confirmation, then pauses and successfully saves all live games before
proceeding. After pause/save, require a fresh successful safety backup of user
data/settings at the configured destination, excluding datasets and credentials.
Save or backup failure blocks installation; backup failure offers Retry. Restore
games paused. Only stable updates are offered in the first release.
For missing, expired, or rejected tokens, show a persistent update-status notice
in Settings with add/replace-token controls and no blocking prompt. Offline chess
remains usable. Stop failed downloads and offer visible Retry; do not retry
automatically. Relaunch automatically after successful installation where
supported, explain platforms requiring manual reopening, and keep games paused.
Installation failures show explicit Retry where safe and manual-recovery guidance
otherwise, without automatic retries. Migration failures stop normal startup,
preserve data, and offer pre-update backup recovery with preview and confirmation;
explain any compatible-app reinstall requirement. Retention and recovery details
follow the packaging contract. For .deb updates, show the verified package and open the system installer after
confirmed pause/save/backup, requesting OS approval where required. This path
requires separate integration validation from AppImage OTA. Concrete remaining
mechanics are selected in [the resolved decisions](../decisions.md), with
[release evidence](../decisions.md#release-evidence-register) still unperformed.
