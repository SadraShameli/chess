# Feature Specification: Reckless Desktop Chess

**Feature Branch**: `001-reckless-desktop-chess`

**Feature ID**: `001-reckless-desktop-chess`

**Created**: 2026-10-16

**Status**: Specification updated and reviewed for native desktop delivery on
2026-10-16. Approved first-release scope and requirement IDs are preserved;
acceptance now explicitly covers native installation, display/input consistency,
accessibility, lifecycle recovery and existing-data continuity. Partial
implementation exists; native UI migration and runtime acceptance remain
unperformed.

[Review](review.md) preserves direct owner choices. Remaining choices were
explicitly delegated to the assistant and are recorded in
[resolved decisions](decisions.md). This invocation updates the existing feature
from that agreed description. It does not create another feature or branch.
Technical selections remain in [the plan](plan.md) and its contracts; this
specification defines player outcomes and observable behavior. The native desktop
revision preserves the fully 3D board, accessibility, offline features and update
requirements.

**Input**: A beautiful, modern desktop chess application using the complete
Reckless engine on Windows, Apple Silicon macOS, and Linux. The owner selected
offline engine play, full analysis, and local two-player chess for the first
release, together with all eight agreed library, interchange, study and training
feature groups, configurable Settings, local backups and private desktop updates.
The owner confirmed the proposed OS/architecture minimums and selected private
personal use; retain owner rights and upstream notices, adding no application
license file or external distribution grant.
The approved study tools include both the opening explorer and a separate
study-only opening book. The owner selected a premium dark-first
workspace with a light theme and a real-time, fully three-dimensional board.
Keep personal data local, support the selected desktop platforms and require
zero payments for the first release.
The updated direction is a native desktop application using each target's
supported graphics capabilities without requiring the player to choose a
graphics backend. Preserve the approved presentation, interactions and saved
data through this revision.

## Clarifications

### Session 2026-10-16

The following decision was selected under the owner's earlier instruction to
choose the recommended option for remaining questions; no new owner answer
was requested or inferred.

- Q: What should happen when the app is launched twice against the same local data? → A: Keep one writable application owner per local profile and activate its existing window. If a live owner is unreachable, refuse a competing session with recovery guidance; after confirmed owner exit, normal paused recovery applies.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Play a complete game against Reckless (Priority: P1)

A player chooses their color, variant, and time control and plays a complete legal
game against Reckless without an internet connection.

**Why this priority**: This is the primary purpose of the application.

**Independent Test**: Install a packaged build, disconnect the network, start a
game as either color, exercise special moves, and reach a recognized result.

**Acceptance Scenarios**:

1. **Given** a fresh offline installation, **When** the player starts a game,
   **Then** the board is playable and Reckless replies on its turn.
2. **Given** legal castling, en passant, or promotion, **When** the move is
   selected, **Then** the board and notation update correctly, with all four
   promotion choices available.
3. **Given** checkmate, stalemate, an automatic draw, a valid draw claim,
   resignation, or flag fall, **When** the condition occurs, **Then** its result
   and reason are recorded and further moves are rejected.
4. **Given** a supported Fischer-random start, **When** either side castles,
   **Then** move input, board placement, and notation agree.
5. **Given** a timed game, **When** either side moves, **Then** its clock and
   increment update exactly once and the engine receives the correct allocation.
6. **Given** a claimable human draw, **When** Claim Draw is validated, **Then**
   a valid claim ends the game; an invalid claim explains why and leaves play
   running. Reckless claims its own available valid draws automatically.
7. **Given** a running game, **When** a takeback is accepted, **Then** the required
   move or moves and their clock effects are undone, the removed continuation is
   kept as a variation, and play returns to the specified side's turn.
8. **Given** a queued premove, **When** Reckless replies, **Then** a still-legal
   premove executes with actual elapsed clock charging; an illegal one clears
   with a notice and leaves the player's clock running.

### User Story 2 - Use the complete engine for analysis (Priority: P1)

A player explores games and positions, compares candidate lines, and controls the
full supported Reckless search and configuration surface.

**Why this priority**: Complete engine coverage is an explicit owner requirement.

**Independent Test**: Load a position, run every verified search mode, inspect
multiple variations and statistics, stop, change positions, and restart.

**Acceptance Scenarios**:

1. **Given** a position under analysis, **When** engine results arrive, **Then**
   legal ranked lines, scores, depth, speed, elapsed time, and available statistics
   update without interrupting board interaction.
2. **Given** a running search, **When** the player stops or changes positions,
   **Then** old results cannot move the board or replace new results.
3. **Given** a supported advanced setting, **When** a valid value is applied,
   **Then** its effect is acknowledged; invalid or unavailable settings are
   explained before they reach the engine.
4. **Given** compatible local tablebases, **When** probing is enabled, **Then**
   the engine uses them and exposes its available probe information. Missing
   optional tablebases do not prevent ordinary play.
5. **Given** a diagnostic or build-specific capability, **When** its workbench
   action is selected, **Then** it is accessible with prerequisites, progress,
   cancellation, and results isolated from a live game.

### User Story 3 - Play locally and keep games (Priority: P1)

Two people share the board, or a player revisits saved games and variations.

**Why this priority**: Local two-player play is an explicit first-release choice;
saved games make both play and analysis useful between sessions.

**Independent Test**: Play a local game without engine replies, save and reopen
it, and round-trip its notation, metadata, annotations, and variations.

**Acceptance Scenarios**:

1. **Given** local two-player mode, **When** either side moves, **Then** the same
   legality and clock rules apply without an automatic engine reply.
2. **Given** an annotated game, **When** it is imported and exported, **Then**
   moves, supported metadata, comments, and variations are preserved; unsupported
   syntax is reported explicitly.
3. **Given** a saved game or application restart, **When** it is reopened,
   **Then** the position and history are restored without silently resuming clocks.
4. **Given** historical navigation during play, **When** an earlier position is
   inspected, **Then** live-game and analysis states remain distinct; continuing
   from history requires an explicit analysis branch or confirmed game action.
5. **Given** an existing annotated game and a matching import, **When** the import
   is previewed, **Then** differences are visible, Skip is selected initially,
   and keeping a copy requires an explicit choice without overwriting notes.
6. **Given** an organized library, **When** the player searches, filters, sorts,
   tags, favorites or moves a game between nested folders, **Then** the matching
   games and organization persist after reopening.
7. **Given** a game in Trash, **When** Restore is selected, **Then** it returns
   without overwriting another game; permanent deletion requires confirmation.
8. **Given** a PGN with an invalid variation, **When** import is previewed,
   **Then** that whole game is excluded and other valid games remain selectable.
9. **Given** unsaved notes or a proposed branch deletion or promotion, **When**
   the player leaves or confirms the edit, **Then** the relevant save or branch
   confirmation protects the original notes and main line as specified.

### User Story 4 - Enjoy a polished accessible workspace (Priority: P1)

A player gets a coherent board-first workspace that stays clear on different
displays and works with mouse, keyboard, and assistive technology.

**Why this priority**: Modern visual quality is part of the product requirement.

**Independent Test**: Exercise core flows in both themes at different display
scales, with keyboard-only input, reduced motion, and a screen reader.

**Acceptance Scenarios**:

1. **Given** a resized window, changed display scale or move between displays,
   **When** the workspace updates, **Then** pieces, coordinates, clocks, history,
   and controls remain sharp and operable without overlap; selecting or dragging
   on each of the 64 squares targets that square at every allowed camera pose
   and either side orientation, and panel input never submits a board move.
2. **Given** either theme, **When** selection, check, a move, or an error is
   shown, **Then** its meaning is available without relying only on color.
3. **Given** keyboard-only input, **When** starting a game, selecting a move,
   promoting, navigating history, or stopping analysis, **Then** every action is
   reachable and focus stays visible.
4. **Given** reduced-motion preferences, **When** a piece moves, **Then** its
   final position remains clear without unnecessary animation.
5. **Given** any permitted camera pose, **When** the player rotates, tilts or
   zooms through visible controls, **Then** the entire board remains visible,
   piece input and drawings retain their roles, and Reset View retains side
   orientation while restoring the initial framing.
6. **Given** a customized layout and camera, **When** the app reopens on a
   different display arrangement, **Then** it restores a usable fitted window
   and remembered view; Reset Layout and Reset View remain available.
7. **Given** Automatic quality during sustained slow frames, **When** quality
   adapts, **Then** shadows and effects reduce before board resolution and
   settings do not rapidly alternate; manual quality remains selectable.
8. **Given** a supported screen reader and either board orientation, **When**
   the player navigates the board and activates its controls, **Then** all 64
   squares expose their coordinates, occupants or emptiness and current state;
   selecting moves, choosing promotions and hearing turn, check and result
   feedback work without sight or pointer input, regardless of camera angle.

### User Story 5 - Install and recover on every target (Priority: P1)

A player installs a self-contained application and gets clear engine identity,
capabilities, and actionable failures without command-line setup.

**Why this priority**: Platform support means working packaged applications.

**Independent Test**: Install every target on a clean environment, run offline
play and analysis, and verify recovery from an engine failure.

**Acceptance Scenarios**:

1. **Given** a clean supported target installation, **When** it launches offline,
   **Then** the board, panels, bundled engine, neural network, fonts, sounds and
   starter data work without application-resource downloads or a browser-runtime
   prerequisite; documented operating-system and graphics requirements apply.
2. **Given** CPU or optional-system limitations, **When** an engine package is
   selected, **Then** compatibility is checked before execution and a compatible
   build or specific error is presented.
3. **Given** an engine exit, timeout, or invalid response, **When** it occurs,
   **Then** the game is preserved and recovery cannot accidentally move or resume.
4. **Given** the engine information view, **When** opened, **Then** exact version,
   build capabilities, licenses, and corresponding-source access are available.
5. **Given** an engine or graphics failure, **When** Retry succeeds, **Then**
   the preserved game remains paused until explicit Resume, with no second board
   mode substituted for the selected three-dimensional presentation.
6. **Given** a live game that was saved before an unexpected exit, **When** the
   app reopens, **Then** the last saved position and clocks return paused and no
   downtime is charged.
7. **Given** missing graphics support, corrupt presentation assets or an
   initialization failure, **When** startup cannot present a usable board,
   **Then** an actionable failure and Retry are offered, saved data remains
   intact and no game begins or resumes. A Retry that cannot finish within
   10 seconds returns to the failure state with setup guidance.
8. **Given** compatible local data from an earlier application version, **When**
   the native revision opens it, **Then** games, annotations, reports, drafts,
   puzzle progress and preferences remain available without manual export and
   reimport; unfinished games and puzzle runs remain paused. Unsupported or
   failed migrations preserve the original data and enter explicit recovery.

### User Story 6 - Construct and annotate study positions (Priority: P1)

A player constructs a position in an editor, validates it, analyzes it, and adds
notes or alternative lines without replacing the original game.

**Why this priority**: Custom editing and annotations are owner-approved scope.

**Independent Test**: Build a valid position, round-trip its FEN, and retain its
annotations and variations through save/export/reopen.

**Acceptance Scenarios**:

1. **Given** the editor, **When** pieces, turn, and castling state are changed,
   **Then** the position is validated before it can be searched or played.
2. **Given** an invalid position, **When** validation is requested, **Then** its
   specific problems are shown without sending it to Reckless.
3. **Given** an annotated branch, **When** the game is saved or exported,
   **Then** its notes and alternative moves remain associated with the right node.
4. **Given** an invalid draft, **When** it is saved and reopened, **Then** its
   invalid status and fields remain available for correction while Play and
   engine analysis remain unavailable.
5. **Given** a valid draft, **When** Play from Position is selected, **Then**
   New Game opens with that position and waits for explicit Start.
6. **Given** unsaved draft edits, **When** the player leaves or imports a FEN,
   **Then** Save, Discard and Stay protect the draft; a valid imported FEN opens
   a separate draft rather than silently replacing it.
7. **Given** edits in the current editor session, **When** Undo, Redo, Clear Board
   or Standard Setup is selected, **Then** local history works independently of
   live-game takebacks and destructive setup replacement requires confirmation.

### User Story 7 - Review full games and explore openings (Priority: P1)

After each game played in the app completes, a full-game report starts
automatically. Imported completed games are analyzed only on explicit request.
The player inspects evaluation changes and suggested alternatives, and studies
opening moves with local explorer and book data.

**Why this priority**: Reports, the opening explorer, and the opening book are
all explicitly included by the owner.

**Independent Test**: Analyze a fixed game, compare its per-move report to the
underlying searches, query a known opening corpus, and exercise a book position.

**Acceptance Scenarios**:

1. **Given** a game, **When** a report completes, **Then** each reported move
   links to its position, evaluation, and available stronger alternatives.
2. **Given** report cancellation, **When** the job stops, **Then** completed and
   incomplete work are distinguished and stale results cannot attach to another game.
3. **Given** an opening corpus, **When** a position is selected, **Then** its
   continuations and available frequency/result statistics match that corpus.
4. **Given** an opening book, **When** a book position is inspected, **Then**
   its legal prepared moves are shown distinctly from engine search results.
5. **Given** a move-quality finding, **When** it is clicked, **Then** the board
   navigates to its position, shows analyzed alternatives, and offers a separate
   analysis branch without rewriting the original game.
6. **Given** a selected position, **When** the player switches between move list,
   opening explorer, and study-only book tabs, **Then** that position is preserved.
7. **Given** a queued report, **When** another live game runs, **Then** report
   work pauses and resumes after live play without losing completed positions.
8. **Given** an imported completed game or an edited main line, **When** reports
   are viewed, **Then** import alone starts no report and prior analysis of a
   changed line is clearly outdated rather than attached to new positions.
9. **Given** overlapping opening datasets with unknown results or repeated
   positions, **When** they are queried together, **Then** deduplication and
   denominators follow FR-019 and unavailable metadata remains explicit.
10. **Given** a study-only book move or a zero-weight position, **When** it is
    selected or inspected, **Then** only a study branch changes, the played main
    line is preserved, and undefined percentages are shown as unavailable.

The book is for opening study only and never supplies opponent moves. Report
interpretation follows the selected, versioned application metric rather than
claiming equivalence to another chess service's accuracy score.

### User Story 8 - Solve chess puzzles (Priority: P1)

A player opens a tactical exercise, attempts its solution, and receives feedback
against the selected puzzle's defined solution.

**Why this priority**: Puzzles are explicitly included in this release.

**Independent Test**: Load a fixed puzzle set and exercise a correct solution,
an incorrect move, and a puzzle whose data is invalid.

**Acceptance Scenarios**:

1. **Given** a valid puzzle, **When** the player follows an accepted solution,
   **Then** feedback reflects its progression and recognizes completion.
2. **Given** an incorrect or illegal attempt, **When** it is submitted,
   **Then** feedback follows the approved puzzle policy without corrupting the position.
3. **Given** invalid puzzle data, **When** it is loaded, **Then** validation
   reports the issue before presenting an unsolvable or malformed exercise.

4. **Given** a timed active puzzle, **When** focus is lost, sleep occurs or the
   run is paused, **Then** its board and solution hide, time is preserved, and
   explicit Resume is required, including after relaunch.
5. **Given** a listed solution, **When** the player requests progressive hints,
   makes an unlisted legal move or submits illegal input, **Then** assistance,
   failure and rejection are distinguished according to FR-021 without claiming
   an unlisted move is a losing chess move.
6. **Given** failure followed by a practice solve, **When** the session summary
   is displayed, **Then** the primary outcome remains failed, practice and hint
   use are separate, and finalized outcomes have no overlapping totals.
7. **Given** either timer mode, **When** it expires, **Then** the selected
   timeout policy applies once; paused time and between-puzzle review are not
   charged and active reply/animation time is charged.
8. **Given** an exhausted filtered collection, **When** progress is shown,
   **Then** the session countdown waits for explicit Reshuffle and Continue;
   changed filters affect the next run and no puzzle repeats before exhaustion.
9. **Given** saved puzzle history, **When** a confirmed history deletion occurs
   outside an active run, **Then** only the selected records and their summary
   contribution disappear; the puzzle datasets remain.

### User Story 9 - Manage simultaneous games safely (Priority: P1)

A player keeps several games open, sees every clock and turn, and shares the
machine's available engine resources without losing progress.

**Why this priority**: Multiple simultaneous live games are an explicit owner choice.

**Independent Test**: Start multiple games within a recorded machine budget,
switch tabs, pause and resume them, and exercise save and capacity failures.

**Acceptance Scenarios**:

1. **Given** several running games, **When** the player changes tabs or browses
   history, **Then** each live clock and engine turn continues and the overview
   identifies the current turn and time for every game.
2. **Given** insufficient thread or memory capacity, **When** another engine game
   is started or resumed, **Then** it stays paused with an explanation of what
   must change and no time is charged before admission.
3. **Given** changes in running games, **When** existing searches reach safe
   boundaries, **Then** their equal resource shares are updated without exceeding
   the total budget; pausing a game releases its engine cache allocation.
4. **Given** a failed game autosave, **When** the failure is reported, **Then**
   the affected game pauses with its current unsaved state retained and Retry
   Save available; close, quit and update cannot silently discard it.
5. **Given** running games, **When** closing a tab or quitting is confirmed,
   **Then** the required games pause and save before closing and reopen paused.
6. **Given** running games in selected and unselected tabs, **When** app focus
   is lost or the window is minimized, **Then** live clocks and admitted engine
   searches continue while puzzle and sound behavior follows their separate
   policies. Computer sleep pauses all live games, and wake requires explicit
   Resume without charging sleep time.
7. **Given** running games, **When** quit is requested through the window close
   control, application menu or operating-system shortcut, **Then** the same
   confirmation and successful pause/save are required; cancellation or failed
   saving keeps the application and current in-memory games available.
8. **Given** an existing local profile, **When** the app is launched again while
   its owner is running, **Then** the existing window is activated without a
   second writable session, duplicate games or competing engine allocations. If
   that owner is alive but unreachable, show a busy/recovery explanation without
   taking ownership or starting migration, restore, backup or installation work.
   After confirmed owner exit, the next launch follows normal saved-state/crash
   recovery and leaves unfinished games and puzzle runs paused.

### User Story 10 - Configure a personal workspace (Priority: P1)

A player uses Settings to control appearance, game defaults, training, datasets
and advanced engine resources, with clear saved and effective states.

**Why this priority**: Common and advanced configuration are approved first-release scope.

**Independent Test**: Change each settings group, restart, import/export a settings
file and try invalid or unavailable options without affecting existing games.

**Acceptance Scenarios**:

1. **Given** Settings, **When** appearance changes or other values are saved,
   **Then** appearance applies immediately, other values require Save, and
   engine changes show pending until their safe application point.
2. **Given** unsaved settings, **When** leaving, **Then** Save, Discard and Stay
   are offered; confirmed section or full reset preserves games and datasets.
3. **Given** an import with supported fields and unavailable engine options,
   **When** validation and preview finish, **Then** supported changes require
   confirmation, omitted values and machine paths remain, and unavailable
   options are listed without being applied.
4. **Given** an older or newer settings file, **When** it is imported, **Then**
   only explicitly supported older versions migrate; unsupported versions leave
   current settings unchanged with an actionable explanation.
5. **Given** a user action or shortcut remap, **When** it is requested through
   the palette or Settings, **Then** availability and confirmations are retained,
   conflicting bindings are rejected, and visible controls remain available.
6. **Given** events from several games, **When** focus, mute or volume changes,
   **Then** event selection, priority and background silence follow FR-024 and
   suppressed sounds never replay later.

### User Story 11 - Back up and restore local data (Priority: P1)

A player protects the local collection with manual and daily backups and can
restore a complete snapshot on any supported platform.

**Why this priority**: Local-only personal data needs the approved recovery workflows.

**Independent Test**: Back up a representative collection, restore it on a clean
profile, and exercise backup, compatibility and interrupted-restore failures.

**Acceptance Scenarios**:

1. **Given** saved user data, **When** Backup Now or a due daily backup succeeds,
   **Then** all included user data and settings are recoverable, datasets and
   credentials are excluded, and category-specific retention is respected.
2. **Given** a failed automatic backup with working autosave, **When** the
   player continues playing, **Then** play remains usable with a persistent
   warning, Retry Backup and destination controls.
3. **Given** a compatible backup from another supported platform, **When** restore
   is confirmed after preview and a successful safety backup, **Then** complete
   user data replaces the current snapshot, machine-specific choices are reviewed,
   and restored games and puzzle runs remain paused.
4. **Given** a corrupt, unsupported or interrupted restore, **When** recovery
   runs, **Then** current data is preserved or a complete valid snapshot is
   recovered; mixed or partially replaced user data is never presented as success.

### User Story 12 - Receive private updates without losing a game (Priority: P1)

A player discovers and downloads private stable updates, then explicitly
installs only after every game and a fresh recovery backup are safe.

**Why this priority**: Private, zero-payment desktop updates are approved scope.

**Independent Test**: Exercise available, offline, unauthenticated, failed,
canceled and verified downloaded updates with recoverable live games.

**Acceptance Scenarios**:

1. **Given** startup or Check for Updates, **When** the check runs, **Then**
   chess stays usable; missing or rejected credentials produce a persistent
   Settings notice with credential controls rather than a blocking prompt.
2. **Given** an available update, **When** download starts on a known unmetered
   connection, **Then** it runs automatically; a metered or unknown connection
   requires approval for each attempt, including Retry.
3. **Given** an active download, **When** canceled, failed, timed out or observed
   to change to a metered/unknown connection, **Then** partial bytes are discarded
   and no retry begins without the required explicit action and approval.
4. **Given** a completed download, **When** the app relaunches or a newer stable
   release appears, **Then** the cached version is retained and revalidated;
   Download Newer Version is explicit and no Remove Download action appears.
5. **Given** explicit confirmed Install, **When** any save, safety backup or
   authenticity check fails, **Then** installation is blocked and retained data
   remains recoverable. After success, restored games require Resume.
6. **Given** migration or installation failure, **When** recovery is presented,
   **Then** data remains preserved, restore requires preview and confirmation,
   and only safe explicit Retry or manual guidance is offered.
7. **Given** a matching stable version tag and verified zero-cost automation,
   **When** every platform release check succeeds, **Then** one complete private
   signed release is published; failed checks leave it unpublished and exhausted
   included allowance never starts paid work.
8. **Given** an unavailable operating-system credential service, **When** update
   credentials are configured, **Then** setup guidance and explicit session-only
   use are available, no plaintext credential is saved, and session-only access
   requires reentry after restarting the app.

### User Story 13 - Study offline with bundled and personal data (Priority: P1)

A player starts with opening and puzzle collections and imports personal files
without turning ordinary offline chess into a network-dependent workflow.

**Why this priority**: Starter datasets and personal imports are explicit scope.

**Independent Test**: Install with no network, browse both opening collections
and puzzles, then preview and import valid, duplicate and malformed personal data.

**Acceptance Scenarios**:

1. **Given** an offline installation, **When** opening study or puzzles opens,
   **Then** Standard starters are usable and missing Chess960/DFRC data is clear;
   separate titled-master online and broad-game sources are correctly labelled.
2. **Given** a personal PGN, Polyglot book or supported puzzle CSV, **When** it
   is imported, **Then** validation, exclusions, source identity and counts are
   previewed before confirmation without damaging existing collections.
3. **Given** a puzzle CSV's opponent setup move, **When** a puzzle opens,
   **Then** that move has already been applied and the player's solution starts
   from the resulting position.
4. **Given** a manual dataset replacement or deletion, **When** confirmed,
   **Then** active work retains its referenced version and personal history;
   canceled or failed preparation preserves the previous dataset.
5. **Given** the generated starter book, **When** inspected, **Then** it uses
   only standard-initial-position master games and their first 40 plies, includes
   rare legal moves, and never supplies Reckless's opponent moves.

### Edge Cases

- Pinned en passant, promotion cancellation, all underpromotions, discovered
  check, and Fischer-random castling where king or rook already occupies its
  destination.
- Threefold/fifty-move claims versus automatic fivefold/seventy-five-move draws;
  repetition identity includes turn, castling, and relevant en passant rights.
- Checkmate precedence; dead positions and flag-fall draws require the selected
  chess rules rather than a crude piece-count heuristic.
- Suspend/resume in timed games; historical navigation while live clocks run.
- Stop/change/restart races, terminal positions sent to the engine, late output,
  and closing the application during search.
- Invalid or oversized game files, Unicode paths, tablebase paths with spaces,
  missing/corrupt tablebases, and packages built without optional capabilities.
- Impossible hash/thread requests, exhausted memory, and simultaneous diagnostic
  and search work.
- Full-strength engines without a built-in rating limiter; search budgets cannot
  be advertised as calibrated Elo levels.
- Report jobs racing with game edits, incomplete/canceled reports, and engine
  configuration changes invalidating cached analysis.
- Opening corpora with transpositions, duplicate games, unknown results, or no
  continuation for a position; book entries that do not match legal moves.
- Puzzle solution lines with illegal moves or multiple accepted continuations;
  editor positions that cannot be used for play or engine search.
- Move arrival exactly at a flag-fall deadline, result versus takeback races,
  exhausted delay or stage boundaries, and premoves cleared by interruptions.
- Disk-full autosaves versus failed backups, quit blocked by an unsaved game,
  corrupt archives, cross-platform paths and interrupted replacement restores.
- Empty puzzle filters, no finalized attempts, failed-then-solved practice,
  unavailable difficulty metadata and removed datasets referenced by history.
- Unsupported settings versions, unavailable engine options, reserved keyboard
  bindings, muted or simultaneous alerts from several games.
- Missing update credentials, malicious or corrupt installers, unknown network
  cost, canceled partial downloads, stale cached versions, failed migrations,
  and exhausted free release-building allowance.
- Graphics initialization or device failure, repeated unsuccessful Retry,
  missing presentation assets and late results from an interrupted search.
- Display-scale changes while dragging, camera rotation with either orientation,
  overlapping board/panel input and inaccessible squares in a fully 3D view.
- Clock updates and engine completion while minimized or unfocused; sleep and
  wake across several live games, and quit requests while saving is unavailable.
- Existing supported local data opened by the native revision, including paused
  games and puzzle runs; failed migration must not replace the last valid data.
- Repeated launches while a profile owns live games, migration, restore or an
  update; unreachable live owners versus confirmed exits and crash recovery.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Provide offline human-versus-Reckless and local two-player modes,
  either color, untimed play, and configurable sudden-death, Fischer increment,
  staged/cyclic, simple-delay, and Bronstein-delay time controls. Untimed engine
  play uses a configurable default of 3 seconds per move, separate from reports.
  Stage/delay accounting and engine search-time allocation follow
  [the selected clock policy](decisions.md#clocks-starts-claims-and-races). The adjustable initial new-game
  preset is 10 minutes per side plus 5 seconds per move. Keep clocks running on app
  focus loss; pause all live games on computer sleep and require explicit Resume.
  Offer manual Pause in both game modes, pausing clocks and active engine search
  until explicit Resume. Initially select White for the human against Reckless
  and Standard chess in New Game; retain side and variant selection. Offer
  Untimed, 1+0, 2+1, 3+0, 3+2, 5+0, 5+3, 10+0, 10+5, 15+10, and 30+0 presets,
  retaining custom controls and the initial 10+5 default. Show tenths
  below 10 seconds and whole seconds otherwise, without reducing native clock
  precision. Warn visually and play one low-time sound per player per game on
  first crossing below 10 seconds; respect mute/background silence and do not
  retrigger after increments. Local games initially keep fixed orientation, with
  a per-game Auto-flip option rotating the 3D board after accepted moves. Support
  multiple simultaneously running live games sharing engine resources. Divide
  CPU capacity equally among active live-game searches and automatically divide
  one total hash-memory budget among engines. Limit running engine games so
  each reserves at least one CPU thread, including during human turns. Apply
  thread-share changes at each engine's next between-search boundary, allowing
  temporary unequal shares within the total CPU budget. Release paused-game hash
  allocations for running engines and recreate caches on Resume. Automatically
  size the initial total hash budget to 10% of OS-reported available RAM at app
  startup, capped at 2 GiB, with a Settings override. If memory cannot support
  starting/resuming an engine game within the budget and required engine memory,
  keep that game paused and explain which budget or running games must change.
  Autosave game state and clock snapshots after every
  accepted move and pause/resume/result transition. Closing a running game tab
  requires confirmation, then pause, save, and close it; retain the unfinished
  game in the library for reopening. On autosave failure, retain current state
  in memory, clearly mark it unsaved, pause the affected game and engine, and
  offer Retry Save. After a crash, restore the last successfully saved clock
  snapshot paused, charge no downtime, and require explicit Resume.
  When quitting with live games, ask for confirmation, then pause and save all
  games before quitting. On relaunch restore all unfinished game tabs paused;
  require explicit Resume.
  Provide game tabs and an overview
  showing every live game's clocks and turn. After
  pausing an untimed engine search, Resume starts a fresh search with the full
  configured per-move budget; timed searches use remaining chess clocks.
- **FR-002**: Enforce legal Standard, Chess960/FRC and DFRC chess,
  special moves, game results, draw claims, automatic draws, and clock rules.
  Human players must explicitly use Claim Draw for claimable threefold repetition
  or fifty-move draws. The app always claims an available valid draw for the
  Reckless seat; this is app policy, not an engine protocol command. Fivefold
  repetition and seventy-five-move draws remain automatic. Chess960 starts are
  random by default with a numbered-position selector. DFRC independently
  randomizes both back ranks by default with numbered selectors for each side.
  Label numbering as zero-based 0..959, with 518 the Standard arrangement, and
  retain the actual chosen start. Invalid draw claims explain why and do not
  stop the clocks. Never substitute a piece-count guess or engine evaluation
  for a required dead-position or timeout mating-possibility ruling: if the
  position cannot be adjudicated, preserve it, pause and explain the interruption
  with explicit Retry or consensual result entry. Required correctness evidence
  remains a release gate. Checkmate on an accepted timely move takes precedence
  over a subsequent draw or clock event; a move received at the deadline is late.
- **FR-003**: Provide navigation, takebacks in engine and local two-player modes,
  explicit analysis branches,
  board flip, configurable legal-move dots and check/last-move indicators,
  captured-piece display, user-drawn arrows/circles, and all promotions. Initially
  show legal-move dots on selection and enable last-move/check highlights and
  captured-piece display. Against Reckless, a takeback returns to the previous
  human turn; locally, it undoes one move. Restore the clocks from before the
  undone moves, including reversing increments. Local opponent confirmation is
  configurable per game and required by default. Keep removed continuations as
  alternative variations. Local clocks keep running while consent is pending;
  acceptance restores snapshots. Cancel a pending local request when another
  move is accepted; allow a new request against the updated game.
  Apply engine-game takebacks immediately on selection without an extra
  confirmation. Local requests have no expiry timer; await response, cancellation,
  or another accepted move. Serialize revision checks; terminal results cancel
  pending consent and disallow post-result takebacks. Always confirm resignation in both modes.
  Browsing history keeps the live game, clocks,
  and engine search running and clearly offers Return to Live; viewing a past
  position never changes the live position. Live drawings remain temporary;
  analysis/review offers explicit Save Markings attached to the specific game-
  tree position. Clear temporary arrows/circles when the selected position
  changes; saved markings remain attached. Board coordinates are initially
  always visible around the board, follow its orientation, and are configurable.
  Right-click toggles circles and
  right-drag toggles arrows; repeating a marking removes it. Offer green, red,
  blue, and yellow with green initially selected. Support one queued premove
  against Reckless, configurable and initially disabled; revalidate after the
  engine move. Charge actual elapsed clock time with no fixed minimum and apply
  normal increment/delay rules. Select a promotion piece before queueing. A new
  premove replaces the old; Escape and visible Cancel Premove cancel it, while
  right-click retains its drawing role. Clear illegal premoves without execution
  and show a brief nonblocking no-longer-legal notice; the turn clock continues
  normally. Clear queued premoves on any pause, takeback, fault, or relaunch.
  Show distinct source/destination highlights and a Queued Premove label with
  Cancel, distinguishable from drawings and last-move highlights.
- **FR-004**: Support validated position and game import/export, metadata,
  annotations, variations, local save, and recovery. Preview PGN errors and
  exclusions before confirmation; import only fully valid games, skipping any
  game with invalid moves or branches. Default PGN export includes supported
  metadata, notes, variations, notation marks, and available clocks; offer
  mainline-only export. Provide library search,
  filters, sorting, favorites, tags, and folders. Preview duplicate game imports
  and default to skipping them; allow explicitly keeping copies and never silently
  overwrite annotations. Move deleted games to Trash with Restore and confirmed
  permanent deletion, without automatic expiry. Search/filter players, date,
  result, variant, opponent mode, favorites, tags, folders, opening name/code,
  and game length; exclude annotation-text search from the first release. Use
  nested folders with one folder or Unfiled per game. Duplicate candidates match
  variant, initial position, played mainline, players, date, and result; preview
  annotation/variation differences. Initially sort by most recently updated first.
  Removing a folder requires confirmation, removes its descendants, and moves
  contained games to Unfiled without deleting games. Moving an open unfinished
  game to Trash requires confirmation, then pause/save/close its tab and move
  the game to recoverable Trash. Use plain-text notes. Require confirmation before
  promoting a saved-study variation, keeping the previous main line as an
  alternative variation; do not change a running game's played history. Save notes
  explicitly and offer Save, Discard, and Stay before leaving unsaved edits.
  Confirm analysis-variation deletion, identifying the entire branch and its
  descendants; do not delete running-game played history through this action.
  Preserve supported existing libraries and associated user data when upgrading
  to the native revision, without requiring manual export/reimport. Reopen
  unfinished games and puzzle runs paused; unsuccessful migration follows the
  data-preserving recovery policy in FR-026. Only one application instance
  may own a local profile for writes, live clocks, engine allocations and
  migration/restore/backup/update coordination. A second launch targeting that
  profile activates the existing window without creating a competing session.
  If a live owner cannot be reached, explain the busy/recovery state and refuse
  competing ownership. Recover ownership only after the previous owner has
  exited; apply the normal saved-state/crash policy with unfinished games and
  puzzle runs paused. Do not infer owner exit from a response timeout.
- **FR-005**: Account for every released supported Reckless option, search mode,
  output field, position diagnostic, and maintenance capability in a versioned
  inventory, with an appropriate workflow and verification scenario per entry.
  The [selected capability inventory](contracts/reckless-capabilities.md) bounds
  complete coverage to the pinned engine release rather than future features.
- **FR-006**: Identify build-specific and developer-only capabilities with their
  packaged profile or reproducible workflow. Distinguish upstream unsupported
  or broken behavior from application work; silently omitting features fails.
- **FR-007**: Display ranked legal analysis lines and available engine statistics,
  with White-positive search evaluations, explicit bounds, and mate distance.
  Raw engine diagnostics retain their documented engine perspective. Show an
  evaluation bar in analysis and game review only, not during live play.
- **FR-008**: Support time-, depth- and node-limited searches, continuous
  analysis, stopping, cache clearing/sizing, thread allocation, multiple candidate
  lines, move overhead and optional user-supplied local tablebases wherever the
  packaged engine supports them. Show the exact available controls and valid
  ranges; unavailable engine facilities must not appear to work.
- **FR-009**: Correlate results to game, position, and search; cancel safely and
  recover without stale mutations.
- **FR-010**: Use available processing capacity for engine work while preserving
  interactive controls, respecting engine limits and realistic memory allocations.
  Board feedback, clocks, search cancellation and Settings remain responsive
  during maximum-capacity search and admitted background work. Include graphics
  and preparation work when assessing the application's total resource use.
- **FR-011**: Deliver a board-first visual system with light/dark themes, sharp
  original or licensed sculpted 3D-style pieces, coherent typography/spacing, optional smooth
  animation, and deliberate loading, empty, and failure states. Use the approved
  Chess.com-inspired layout: collapsible left navigation, a large board with
  player bars/clocks, and resizable right-side move/analysis/opening-study tabs.
  Remember window size/position, navigation collapse, panel widths, and selected
  side-panel tab across launches. Include Reset Layout; fit restored windows to
  current displays while respecting the minimum supported size.
  Use graphite surfaces, emerald accents, green/cream squares, and a warm light
  theme. Bundle Inter for UI/notation with tabular clock digits. The first-release
  interface is English. Offer letter and figurine move notation, defaulting to
  standard letters; PGN export retains standard letters. Figurines have
  accessible letter labels and remain available offline.
  Default to configurable 180 ms piece moves and 160 ms panel transitions with
  subtle elevation, respecting reduced motion and responsiveness. Exact
  models/assets, framing and typography follow
  [presentation decisions](decisions.md#presentation-assets-and-input). Render
  the board and sculpted pieces in real time, with camera rotation
  and zoom, ivory/ebony satin piece materials, and a minimum usable window of
  1280x800 logical display units, preserving the approved apparent size at
  scale 1.0 and adapting correctly to OS display scaling. Initially center the camera at 20 degrees from top-down,
  showing all squares. Keep the entire board visible at maximum manual zoom-in.
  Reset View restores default pose/framing/tilt while retaining current White/
  Black orientation. Permit full 360-degree yaw and tilt from 0 to 45 degrees,
  preserving full-board framing. Button steps are 15-degree yaw, 5-degree tilt,
  and 10% zoom, constrained by bounds. Auto-flip changes side by 180 degrees over
  180 ms, retains tilt/zoom, respects reduced motion, and leaves clocks running. Drag
  pieces normally, right-drag to draw, and rotate/zoom through visible camera
  controls. The board is 3D-only, without a 2D mode/fallback. Camera changes must
  preserve game state and legal input. Remember the camera view across launches
  and provide Reset View. Default to Automatic quality with manual Low/Medium/High
  overrides. Automatic quality adapts to sustained frame-time changes with
  safeguards against rapid switching. Initially target 60 fps
  during active rendering. Render on demand and stop continuous 3D frames while
  the scene is unchanged; clocks update separately. Reduce shadows/effects before
  internal resolution, preserving crisp board rendering as long as possible.
  Exact presets, thresholds, resolution limits and baseline follow
  [presentation decisions](decisions.md#presentation-assets-and-input).
  Preserve square selection and drawing accuracy through permitted camera,
  layout and display-scale changes; interactions with panels never submit board
  input. A geometry, scale, camera or orientation change during a pointer gesture
  cancels that gesture without submitting a move or marking; require fresh input
  against the updated board mapping. Hardware rendering uses a supported path selected for the target without
  requiring players to choose a graphics backend.
- **FR-012**: Provide keyboard-complete flows, screen-reader position/move
  feedback, visible focus, sufficient contrast, and reduced-motion support.
  Make all 64 squares individually discoverable and actionable with coordinates,
  piece or empty status, selection and relevant state. Screen-reader actions
  support move entry and promotion, and announce turn, check and result changes.
  Camera pose and orientation must not make board controls inaccessible.
- **FR-013**: Package and validate Windows, Apple Silicon macOS, and Linux builds
  with the correct engine, neural network and all required application resources
  included. The native application has no browser-runtime prerequisite. Document
  actual graphics and operating-system prerequisites and validate the minimum
  supported targets; unsupported hardware produces an actionable failure.
- **FR-014**: Preserve games and preferences through engine failures and restart;
  do not silently resume timed play. Engine faults preserve the game and pause
  clocks; restart only when the user selects Retry, with explicit Resume required
  after recovery. For graphics failure during a live game,
  preserve game state, pause affected clocks and engine searches, offer Retry,
  and require explicit user resume after graphics recovery. A shared graphics
  failure affects every live game dependent on that renderer. Failed graphics
  startup never starts a game or silently discards saved data. Repeated Retry
  failures keep games preserved and paused, with an actionable explanation.
  Bound each graphics Retry to 10 seconds before returning to the failure state
  with setup guidance, including missing or corrupt presentation resources.
  Window close, application-menu quit and operating-system quit shortcuts follow
  the same confirmation and pause/save policy; cancellation or failed saving
  leaves the application open with the current in-memory games retained.
- **FR-015**: Expose engine identity, build capabilities, dependency notices, and
  the exact corresponding-source materials required for distribution.
- **FR-016**: Require no account, telemetry, cloud service, or network connection
  for the first release's core workflows.
- **FR-017**: Provide a custom position editor, explicit validation feedback, and
  transfer of valid constructed positions into play/analysis and FEN interchange.
  Allow saving/reopening clearly marked invalid drafts separately from playable
  positions; block play and engine analysis until corrected and validated. New
  drafts copy the selected board position, or Standard setup if none is selected;
  reopening a saved draft restores it. Include Clear Board and Standard Setup,
  both requiring confirmation, and editor-local Undo/Redo for piece and position-
  field edits, separate from game takebacks. Use explicit draft Save and offer
  Save, Discard, and Stay on leaving unsaved edits. Undo/Redo lasts only for the
  current editor session; saved positions persist. Support piece selection/click
  placement, palette and square-to-square drag/drop, and keyboard placement/
  removal. Play from Position opens New Game with the validated position
  prefilled and requires Start. Provide a Positions section in Library and an
  editor draft list, clearly distinguishing valid/invalid drafts from games. FEN
  brought into the editor creates a new draft, preserving the previous draft and
  its unsaved-edit prompt; reject invalid FEN with an explanation. Exact shortcuts
  and validation follow selected presentation and structural-position policies.
- **FR-018**: Automatically start a full-game report after each game played in
  the app completes; imported completed games require an explicit report request.
  Show per-move evaluation changes and available stronger alternatives correlated
  to the analyzed game and configuration. Analyze the played main line by default,
  with variations analyzed separately on demand. Use a configurable default of
  3 seconds per position; depth/node limits remain available. Pause report work
  during live games and resume afterward. Include app-calculated move labels and
  a per-player accuracy percentage using
  [the selected report metric](decisions.md#report-scoring-and-job-scheduling),
  with the metric version, scored sample count and analyzed/total progress visible.
  Distinguish best/forced moves, acceptable moves, inaccuracies, mistakes,
  blunders and missed mates. Do not present bounds or missing scores as exact
  classifications, or partial accuracy as a completed-game result. Changed
  analyzed lines leave old reports marked outdated; preserve completed work across
  cancellation and relaunch. Clickable findings navigate to the position, show
  analyzed alternatives, and offer a separate analysis branch.
- **FR-019**: Provide an offline-capable opening explorer with continuations and
  available frequency/result statistics from its selected local corpus. Preserve
  the selected board position across move-list, explorer, and study-only book tabs.
  Show game counts, move frequency, White/draw/Black results, available average
  player ratings, and openable sample games; mark missing data unavailable.
  Filter by dataset, year/date range, player rating range, time control, player
  names, and result where source metadata exists. Calculate result percentages
  from known-result games only and show the unknown-result count separately.
  Count only the first occurrence of a position per source game. Allow querying
  one dataset or combining selected local datasets, identifying sources. Count
  duplicate games once across sources using the approved game identity, retaining
  all provenance. Move-frequency denominator is matching games with a recorded
  next move; show games ending here separately. Rating filters offer Both Players,
  White, Black, and Side to Move, defaulting to Both Players; exclude games missing
  required ratings while active. Other missing-metadata filters and identity
  normalization follow
  [the selected explorer policy](decisions.md#book-generation-and-explorer-semantics);
  distinct positions must not be merged because a lookup identifier collides.
- **FR-020**: Support an opening book as a separate application facility. Its
  role is opening study only; it must never supply opponent moves. Display raw
  Polyglot weights and normalized shares within the selected book position as
  relative preferences, not evaluations or winning probabilities. Initially order
  legal moves by highest weight first. For all-zero legal weights, show moves/raw
  zeros and mark shares unavailable. In analysis/study, clicking an explorer/book
  continuation navigates an existing matching variation or adds a study variation
  as needed, preserving the main line and never submitting a live move.
  Display shares to one decimal and break weight ties by canonical coordinate
  move notation; explain that rounded shares may not total exactly 100%.
  Book integer scaling, standard-start source inclusion and rare moves follow
  [book decisions](decisions.md#book-generation-and-explorer-semantics).
- **FR-021**: Provide tactical puzzles with validated positions/solutions and
  feedback for attempted moves and completion. Include theme/difficulty filters,
  hints, retries, solution review, saved progress, and both whole-session and
  per-puzzle countdown modes, with configurable defaults of 5 minutes per session
  and 60 seconds per puzzle. Select one countdown mode per timed run. Support
  manual pause and automatic pause on app focus loss, hiding the board/solution
  until resumed. Minimizing or hiding the puzzle window also pauses the run and
  hides its board and solution, even if another app-owned window retains focus.
  Restoring visibility or focus never resumes the run automatically.
  Session expiry ends the run; per-puzzle expiry records a timeout
  and advances. Hints progress through source square, destination square, and
  next-move reveal, marking attempts assisted. Record failed attempts and allow
  practice retries on the existing continuing countdown. Accept listed solution
  lines; label other legal moves unlisted without claiming they lose or using
  engine verification. Randomly choose among listed opponent replies. Shuffle
  filtered puzzles without repeats until exhausted, then show progress and offer
  explicit Reshuffle and Continue. Filter changes apply to the next run; preserve
  the current run's collection. Stop the session countdown on the exhaustion
  progress screen; carry its remaining duration forward only after explicit
  Reshuffle and Continue.
  Save per-puzzle attempt history, outcomes, hint assistance, elapsed time, and
  session summaries locally. Preserve the paused run and remaining time across
  sleep/relaunch; always require explicit Resume. Keep history and summaries
  indefinitely until explicit deletion. Summarize counts/rates for unassisted
  solves, assisted solves, failures, timeouts, and time spent without a points
  system. A failed puzzle remains failed after practice completion; show practice
  completion and hints separately and keep primary outcome totals non-overlapping.
  Restart hint stages for each new player move; any hint marks the attempt assisted.
  Include confirmed deletion of individual attempts, sessions with their attempts,
  and all puzzle history, preserving datasets. Start each new puzzle's countdown
  when its board is ready, visible, and interactive, including after Next. Include
  Skip with a separate skipped outcome, advancing without revealing the solution
  or resetting session time. Reject illegal input without recording a failed
  attempt and keep the countdown running. Preserve already-recorded failures.
  Divide unassisted and assisted solve counts separately by all finalized
  puzzles, including failures, timeouts, and skips; exclude active unfinished
  puzzles. Time spent is active countdown time, including automatic replies,
  animations, hints, and retries; exclude pauses and between-puzzle review.
  Session expiry makes an otherwise unresolved puzzle timed out and ends the
  run; an already-failed puzzle stays failed with a separate expiry event.
  Lifecycle/checkpoint/CSV policies follow
  [puzzle decisions](decisions.md#puzzle-interchange-and-lifecycle).
  After a solved puzzle, show solution review and wait for Next without charging
  the session countdown during that review. Hints keep the countdown running.
- **FR-022**: Make the approved library, PGN/FEN, annotations/variations, editor,
  reports, explorer/book, and puzzle features usable without a live network once
  their required data is locally available. Bundle starter opening and puzzle
  datasets and support personal-file imports. The starter data has an installed
  budget of up to 2 GB (2,000,000,000 bytes), including its indexes and metadata
  and excluding the application and engines. Support PGN opening games/studies,
  Polyglot .bin books and the
  [selected puzzle CSV format](decisions.md#puzzle-interchange-and-lifecycle).
  Bundle Standard-only starters with broad puzzle difficulty/theme coverage;
  show explicitly when another variant has no matching data. Offer separately
  labelled titled-master online and broad online-game collections, selectable
  individually or together. Generate the study book from master games starting
  in the Standard initial position, include every observed legal move in their
  first 20 full moves, and use observed frequency with the selected proportional
  integer scaling when counts exceed the file format's range. Retain source and
  license notices. Dataset changes require explicit manual import/update and
  previewed confirmation, preserving active work and existing data on failure.
- **FR-023**: Provide a dedicated Settings tab for adjusting commonly used
  application options. Preserve accepted preferences across application restarts,
  validate changes, and show whether a runtime setting has taken effect. Include
  appearance/board, game defaults, analysis/reports, puzzles, local datasets, and
  advanced engine/resource settings. Apply appearance immediately and use Save
  for other settings; defer engine application to a safe point between searches
  when necessary and show the pending state. When leaving unsaved settings that
  require Save, offer Save, Discard, and Stay. Include per-section and all-settings
  resets, both requiring confirmation and preserving games, puzzle history,
  and datasets. Include validated versioned JSON settings import/export.
  Exclude machine-specific paths
  from exports and preserve destination locations. Preview changes and require
  confirmation before merging only included fields; preserve other settings.
  Migrate explicitly supported older schemas before validation/preview and reject
  older versions without a migration. Reject newer schemas without changes and
  explain that an app update is needed. For otherwise valid files containing
  unavailable engine options, preview supported changes and list unavailable
  options explicitly; apply supported fields only after confirmation.
- **FR-024**: Provide a command palette, keyboard shortcuts, and optional
  move/check/result sounds. Enable sounds during active play and silence them
  while the app is backgrounded by default, with configuration available. Exact
  shortcut mappings and audio lifecycle follow
  [the selected presentation policy](decisions.md#presentation-assets-and-input).
  Allow shortcut remapping with conflict
  detection and Reset Shortcuts. Initially bind Cmd/Ctrl+K to palette, Cmd/Ctrl+N
  to New Game, Cmd/Ctrl+, to Settings, and F to Flip Board outside text inputs
  (Cmd on macOS, Ctrl on Windows/Linux). Expose all applicable user actions in
  the palette, retaining confirmations and explanations for unavailable actions.

  While foregrounded, play move/check sounds for the selected game and result/
  low-time alerts for all live games, respecting mute/background silence. For
  one accepted move, play only the highest-priority event: result, then check,
  then ordinary move. Include master and per-category volume/enable controls
  for moves, checks, results, and low time. Initially use 70% master and 100%
  per-category levels, combined multiplicatively. Suppress sounds when no app-
  owned window is focused, including minimized/hidden, without pausing games or
  clocks. Discard muted/background events and never replay on focus/unmute.
  Cross-game sound priority/overlap and original assets follow selected
  presentation policy.
- **FR-025**: Provide Backup Now and daily automatic local backups while the app
  is open; schedule an overdue backup on next launch. Back up user data and
  settings, including games and Trash, annotations, reports, editor drafts, and
  puzzle history; exclude datasets, credentials and cached update installers.
  Default to an app-data backup folder
  with configurable destination. Keep the latest 30 automatic backups and all
  manual backups. Restore by full replacement only after a safety backup and
  confirmation; reopen games paused and keep dataset files external. Backups are
  unencrypted in the first release. Automatic backups are due 24 hours after the
  last successful automatic backup. If backup fails while game autosaving works,
  continue play and show a persistent warning with Retry Backup and destination
  controls. Support restore across all supported platforms with explicit path/
  unavailable-option review. Safety-backup retention, restore schema compatibility,
  first-run/retry scheduling and interruption recovery follow
  [storage decisions](decisions.md#storage-imports-and-backup-recovery).

- **FR-026**: Provide authenticated private stable desktop updates and a
  version-tag-driven release process. A vMAJOR.MINOR.PATCH tag matching the app
  version automatically builds every selected package and publishes one complete
  private release only after all required platform checks pass. Failed checks
  leave the release unpublished. Retain published releases and assets until
  explicit owner deletion. Only included free build/storage allowance may be
  used, with verified controls preventing paid overages before automation is
  enabled; stop work when allowance is exhausted.

  Check nonblockingly at startup and provide Check for Updates in Settings.
  Offline chess remains available. Use a user-provided read-only download
  credential restricted to the private repository. Persist it only in the
  operating system's secure credential store, outside exports, backups and
  diagnostics. When that service is unavailable, show setup guidance and permit
  explicitly selected session-only use; require reentry after app restart and
  never save a plaintext fallback.
  Missing, expired or rejected credentials produce a persistent Settings notice
  with add/replace controls and no blocking prompt. Check attempts have a total
  15-second deadline, excluding the installer download.

  Download automatically on detected unmetered connections. Every metered or
  unknown-connection attempt, including Retry, requires fresh approval. Stop a
  download after 60 seconds without bytes or 30 minutes total, whichever comes
  first. Cancel, failure or a detected change to metered/unknown stops the attempt
  and discards partial bytes; offer explicit Retry without automatic retries.
  Retain completed downloads across relaunch and revalidate before installation.
  Provide no Remove Download action. If a newer stable release appears, keep the
  completed download and offer explicit Download Newer Version under the same
  approval rules; retain the older cache until its replacement validates.

  Reject altered, unauthentic, incompatible or older packages. Installation
  requires explicit Install and confirmation, successful pause/save of all live
  games, resolution of unsaved edits and a fresh successful safety backup at the
  configured destination. Exclude datasets and credentials from that backup;
  block installation if any safeguard fails. Keep the latest five successful
  pre-update backups plus any needed for unresolved recovery, separately from
  daily/manual retention. Relaunch automatically where supported and explain
  required manual reopening elsewhere; games stay paused until Resume.

  Preserve existing data if migration fails, stop normal startup and offer
  recovery with preview and explicit confirmation. Explain when a compatible
  app reinstall is needed; do not promise binary rollback. Installation failure
  offers explicit Retry where safe and manual instructions otherwise, with no
  automatic retry or restore. Linux .deb updates open the system installer only
  after the same verification and save/backup safeguards, requesting operating
  system approval where required.

  Keep zero-payment personal-use packages: unsigned Windows NSIS .exe, ad-hoc-
  signed Apple Silicon macOS .dmg, and Linux x86-64 .deb and AppImage. Document
  Windows warnings and manual macOS approval; paid signing/notarization is
  excluded while cryptographic update signatures remain mandatory. Windows
  setup includes any required redistributable application runtimes for offline
  installation, as established by clean-install validation under FR-013. Linux
  may fetch missing system dependencies, with offline-preparation guidance.
  Protect the signing key with an owner-controlled passphrase-encrypted removable
  backup kept disconnected. Operational delivery details remain in
  [the packaging contract](contracts/packaging.md).

### Key Entities *(include if feature involves data)*

- **Local profile**: The user's local collection of games, history, preferences
  and recovery state, with one writable application owner.
- **Game**: Variant, players, initial position, move tree, clocks, result,
  annotations, metadata, and viewed position.
- **Position**: Pieces, turn, castling, en passant, counters, and validated context.
- **Engine session**: Binary identity, capabilities, configuration, lifecycle,
  active search, and recovery information.
- **Search**: Position identity, limits, candidate lines, statistics, and outcome.
- **Engine package**: Platform, architecture, CPU requirements, build features,
  neural-network identity, integrity information, source and license materials.
- **Preferences**: Validated application options surfaced through the Settings
  tab. Exact editable groups and application timing follow
  [the settings design](contracts/ui-experience.md#settings-tab).
- **Game report**: Analyzed game/positions, engine configuration, job state,
  per-move evaluations, alternatives, and interpretation policy.
- **Opening corpus**: Source identity, local games, position continuations,
  frequencies, and available result counts.
- **Opening book**: Source identity and prepared moves for validated positions,
  independent from the engine's supported capabilities.
- **Position draft**: Saved editor fields, validity status and unsaved edits,
  separate from recorded games and current-session Undo/Redo.
- **Library organization**: Nested folders, tags, favorites and recoverable Trash
  membership associated with saved games.
- **Puzzle**: Source identity, starting position, acceptable solution tree,
  dataset difficulty and themes.
- **Puzzle run and attempt**: Frozen filtered collection, shuffle order, selected
  timer mode, remaining time, player moves, primary outcome, assistance, practice
  events and non-overlapping session summary.
- **Backup**: Snapshot identity, creation time, category, compatibility, included
  user data/settings and recovery protection status.
- **Update package**: Private stable version, target, verification state,
  download state and related installation/recovery backup, excluding credentials.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All specified legality and result fixtures pass for standard and
  supported Fischer-random positions, including the listed special cases,
  clock families, claims and move/timeout/takeback boundary races.
- **SC-002**: Every supported capability inventory entry has a working workflow
  and passing verification; no entry is silently unaccounted for.
- **SC-003**: Visible move feedback is at most 100 ms at the 95th percentile and
  active board presentation meets the 60 Hz target with 95th-percentile frame
  intervals at most 16.7 ms in Automatic quality during full-capacity search on
  recorded reference machines; reduced motion stays fully usable. Board-input
  fixtures at 100%, 150% and 200% display scales, both orientations and camera
  bounds target the intended square with zero unintended panel-to-board actions.
  Rejected-input feedback has the same 100 ms p95 bound. Sampling, actual
  presentation boundaries and workload definitions follow
  [the measurement protocol](decisions.md#acceptance-measurement-protocol).
- **SC-004**: An offline game can start within 3 seconds after launch on those
  machines, excluding OS installation prompts. The
  [measurement protocol](decisions.md#acceptance-measurement-protocol) includes
  clean-profile initialization, required local preparation and engine readiness,
  while excluding human reaction time through immediate automated Start.
- **SC-005**: Search stop completes within 1 second at the 95th percentile on the
  fixed corpus; 1,000 controlled stop/change/restart cycles apply zero stale results.
  Normal completion, forced termination and stale-mutation accounting remain
  separate under [the measurement protocol](decisions.md#acceptance-measurement-protocol).
- **SC-006**: Every supported packaged target passes clean installation, offline
  play, analysis, save/reopen, shutdown and the applicable verified update path.
  All update failure/approval fixtures preserve data and prevent installation
  when verification, save, backup or confirmation is missing; free-allowance
  exhaustion fixtures start zero paid work. Graphics startup/recovery, sleep/
  wake and every supported quit entry point preserve game state, clocks and
  explicit-resume safeguards. Supported upgrade fixtures retain all included
  user data without manual interchange; failed migrations preserve the last
  valid snapshot. Clean-install fixtures require no browser runtime. Repeated-
  launch fixtures create zero competing profile writers, duplicate live games
  or independent engine allocations; unreachable live owners cannot trigger
  takeover, migration, restore, backup or installation from the new launch.
  Confirmed-owner-exit fixtures recover saved games and puzzle runs paused.
- **SC-007**: Game round trips preserve supported moves, variations, annotations,
  and metadata across the fixed import/export corpus. Every compatible backup
  fixture restores all included user data/settings, while corrupt, incompatible
  and interrupted-restore cases preserve a complete recoverable snapshot.
- **SC-008**: Core flows pass keyboard-only review; themes meet WCAG AA contrast
  targets and the visual checklist at 100%, 150%, and 200% display scales.
  VoiceOver on macOS, NVDA on Windows and Orca on Linux can discover all 64
  squares, enter legal moves and promotions, and report turn/check/result state
  through the actual application window in both orientations.
- **SC-009**: Fixed-workload measurements compare one-worker and full-capacity
  search, recording throughput, responsiveness, and memory without assuming
  linear scaling. Concurrent-game fixtures show zero thread/hash-budget
  violations and preserve every game's clock/turn identity. An unchanged idle
  workspace with no active games, jobs, countdowns, animations or transfers uses
  less than 1% of one logical core on the reference machines.
  Use [the measurement protocol](decisions.md#acceptance-measurement-protocol)
  for whole-process/child scope, CPU denominator, sampling and scaling comparisons.
- **SC-010**: Editor/FEN and annotated-game round trips preserve the accepted
  position, notes, and variation structure across the reference corpus.
- **SC-011**: Every completed report entry matches its source game position,
  recorded engine configuration, and underlying search result; incomplete entries
  are never represented as analyzed. All scoring fixtures reproduce the selected
  versioned metric, with zero incorrect completed/partial/outdated labels.
- **SC-012**: Opening continuations, frequencies, and result counts match a known
  reference corpus; every exposed book move is legal for its position. Installed
  starter data, including indexes/metadata, is at most 2,000,000,000 bytes and
  every collection has recorded source/license provenance.
- **SC-013**: Puzzle fixtures produce correct progression/completion feedback,
  and malformed positions or solution lines are rejected before use. All timer,
  hint, retry, skip, expiry, resume and summary fixtures produce the specified
  non-overlapping outcomes; crash recovery loses at most one second of unsaved
  countdown and always resumes paused.
- **SC-014**: Approved Settings-tab changes survive application restart, invalid
  values leave the last accepted configuration intact, and the displayed applied
  state agrees with the effective runtime configuration. All settings import,
  reset, shortcut conflict and audio-focus fixtures preserve their specified
  confirmations, exclusions and no-replay behavior.

## Assumptions

### Selected scope and decision authority

Offline engine play, analysis, local two-player and every approved application
extra are included. Online multiplayer, cloud accounts/sync, mobile, Intel macOS
and Linux ARM64 are excluded. Windows 11 x86-64, macOS 14+ Apple Silicon and
Ubuntu 22.04+ x86-64 are selected; runtime support still needs native evidence.
Complete engine coverage means every usable pinned production and diagnostic
capability, with upstream unsupported/broken facilities disclosed explicitly.
Optional Syzygy table files are supplied by the user, never automatically
acquired; the engine's embedded NNUE remains included.

Private personal use and zero payments are hard constraints. No application
license file or external rights grant is added. External conveyance requires
separate legal/provenance review, particularly the model-license gate.

The owner delegated remaining choices to the assistant. [Resolved decisions](decisions.md)
now supplies exact settings, data sources/CSV, formulas, camera/asset choices,
clock/race behavior, storage/import/backup limits, dependencies and updater/CI
policies. These selections do not assert measured performance or release readiness.
No unanswered questionnaire remains. [Release evidence](decisions.md#release-evidence-register)
identifies native testing, exact-adjudication practical coverage, private OTA and
verified no-overage CI as obligations before release support can be claimed.

### Dependencies and validation conditions

- The description comes from the agreed conversation and existing feature,
  with explicit owner delegation for remaining recommendations. No unrecorded
  product defaults or deferred preference questions are introduced.
- The selected baseline is 8 GiB RAM, four usable logical CPU threads, hardware
  three-dimensional graphics support and 6 GB free installation space, plus
  space for imports, backup and update staging. Reference classes are Apple M1
  on macOS and Intel UHD 620-class graphics on Windows/Linux. Native compatibility
  and performance on these systems remain to be demonstrated.
- Install and test Windows 11 x86-64, macOS 14+ Apple Silicon, and Ubuntu 22.04+
  x86-64 with both selected Linux display environments. A display must fit the
  1280x800 minimum usable window. Other platforms are outside the first release.
- Reckless, starter data and presentation assets are available locally after
  installation. Optional user tablebases and personal imports are not bundled.
  Starter counts depend on validated available sources within the installed
  budget; exact downloaded snapshot hashes and final counts are preparation
  evidence, never invented completeness claims.
- Private update discovery/download and automated release building require
  network access and the selected repository credentials/free allowance. Their
  absence never prevents ordinary offline chess. No paid fallback is allowed.
- Engineering policy is incorporated through
  [the constitution](../../.specify/memory/constitution.md) and
  [its canonical rule catalog](../../.specify/rules/README.md). Concrete design
  choices and operational limits remain in the existing plan and decision
  artifacts, without copying engineering rules into this specification.
