# Owner Decision Review

**Status**: Native Bevy + egui architecture selected under owner delegation on
2026-10-16; existing implementation and task history preserved. Native migration
and full runtime/release acceptance remain unperformed.

The owner requested native egui, then an engine selecting native GPU APIs, and
could not choose among the researched alternatives. The assistant selects Bevy
+ bevy_egui under the owner's earlier instruction to pick the best recommendation.
This replaces eframe as host; it is recorded as a delegated selection rather than
an explicit quoted Bevy answer. The latest backend preference permits automatic
Metal/Vulkan/Direct3D selection, without forcing Vulkan. All product behavior is
preserved. Former CSS dimensions map to native logical points under OS scaling.

## Confirmed decisions

| Topic | Owner decision |
| --- | --- |
| Product | Modern, visually polished cross-platform chess application |
| Engine | Reckless, with complete usable feature coverage |
| First-release modes | Offline engine play, analysis, local two-player |
| Application extras | All eight feature groups, including both opening explorer and opening book |
| Stack | Bevy + bevy_egui in native Rust, selected under delegation; replaces eframe and former Tauri/Vue tooling |
| Windows minimum | Windows 11, x86-64 |
| macOS minimum | macOS 14+, Apple Silicon only |
| Linux minimum | Ubuntu 22.04+, x86-64 only; Linux ARM64 removed by owner |
| Intended use | Private personal use |
| Application license | Private personal use; retain owner rights and existing notices, add no application license file; external conveyance needs separate review |
| DATA-01: Offline data | Bundle starter datasets and allow importing personal files |
| DATA-02: Starter-data budget | Up to 2 GB installed, excluding application and engines; a research budget, not a promised collection size |
| UI-01: Visual direction | Dark-first premium style with richer effects and animation, plus a light theme |
| BOOK-01: Opening book | Opening study only; never supplies opponent moves |
| STORE-01: Storage | SQLite for indexed data, with PGN/FEN import/export |
| TOOL-01: Frontend tooling | Superseded by native Rust/Cargo UI; no TypeScript/Vite app pipeline in final architecture |
| DATA-03: Import formats | PGN opening games/studies, Polyglot .bin opening books, puzzle CSV |
| PUZ-01: Puzzle experience | Theme/difficulty filters, hints, retries, solution review, saved progress, and timed sessions |
| REPORT-01: Report initiation | Automatically after each game played in the app completes; imported completed games require explicit request under REPORT-05 |
| REPORT-02: Report budget | Configurable 3 seconds per position by default; depth/node limits also available |
| REPORT-03: Report coverage | Automatic reports analyze the played main line; variations analyzed separately on demand |
| SCHED-01: Report scheduling | Pause reports during live games, then resume |
| REPORT-04: Report interpretation | App-calculated move labels, including inaccuracy/mistake/blunder, plus an accuracy percentage; versioned formula and thresholds selected in decisions.md under owner delegation |
| SCORE-01: Displayed scores | Always White-positive; raw diagnostics retain their documented engine perspective |
| PUZ-02: Timed puzzle modes | Both whole-session and per-puzzle countdowns |
| SETTINGS-01: Settings access | A dedicated Settings tab for adjusting commonly used options |
| SETTINGS-02: Settings groups | Appearance/board, game defaults, analysis/reports, puzzles, local datasets, plus advanced engine/resources |
| SETTINGS-03: Applying settings | Appearance applies immediately; other settings use Save, with engine changes deferred to a safe point if necessary |
| PUZ-03: Timer defaults | Configurable 5-minute session and 60-second per-puzzle countdown defaults |
| PUZ-04: Puzzle pause | Manual pause and automatic pause on app focus loss; hide board/solution until resumed; separate from live chess clocks |
| PUZ-05: Puzzle timeout | End an expired session; mark an expired puzzle as timed out and advance |
| PUZ-06: Puzzle timer combination | Select exactly one countdown mode per timed run |
| UI-02: Workspace layout | Chess.com-inspired collapsible left navigation, large board/player bars/clocks, resizable right-side tabs, premium dark/light styling |
| UI-03: Board aids | Configurable legal-move dots, last-move/check highlights, captured pieces, and user-drawn arrows/circles |
| UI-04: Convenience | Command palette, keyboard shortcuts, and optional move/check/result sounds |
| UI-05: Palette | Graphite surfaces, emerald accents, green/cream board, warm light theme; exact tokens selected in decisions.md; contrast/visual acceptance remains |
| UI-06: Initial board aids | Legal-move dots on piece selection, last-move/check highlights and captured pieces enabled; drawings available on demand |
| AUDIO-01: Default sounds | Move/check/result sounds enabled during active play; silent while app is backgrounded; configurable |
| UI-07: Piece style | Sculpted 3D-style pieces; original generated meshes selected in decisions.md; visual acceptance remains |
| UI-08: Typography | Bundled Inter UI/notation with tabular clock digits; Inter 4.1 and original inline SVG figurines selected; packaged-font validation remains |
| UI-09: Animation defaults | Configurable 180 ms piece moves, 160 ms panel transitions, subtle elevation; respect reduced motion and responsiveness |
| UI-10: Rendering | Real-time 3D board and pieces with camera rotation and zoom |
| UI-11: Piece materials | Ivory and ebony with subtle satin highlights |
| UI-12: Minimum window | 1280x800 native logical points, preserving apparent size; panel reflow/collapse retained |
| UI-13: Default camera | Lightly tilted readable perspective; exact initial tilt and framing confirmed in CAMERA-02 through CAMERA-04 |
| UI-14: Pointer controls | Normal piece dragging; right-drag draws; visible controls rotate/zoom the camera |
| UI-15: View scope | 3D only; no selectable or fallback 2D board |
| GFX-01: Graphics recovery | Preserve game state, pause clocks, offer Retry, and require explicit resume after recovery |
| CAMERA-01: Camera state | Remember the last view across launches and provide Reset View |
| GFX-02: Quality | Automatic quality by default with manual Low/Medium/High overrides; presets and adaptation thresholds selected in decisions.md; benchmarks remain |
| UI-16: Evaluation bar | Show in analysis and game review only, not live play |
| UI-17: Review findings | Click to navigate to the position, show analyzed alternatives, and offer an analysis branch |
| UI-18: Study-tab position | Preserve the selected board position across move list, opening explorer, and study-only book tabs |
| PUZ-07: Hints | Progressive source-square, destination-square, then next-move reveal; mark hint use as assisted |
| PUZ-08: Retries | Record the failed attempt, then allow practice retries while the existing countdown continues |
| PUZ-09: Solutions | Accept dataset-listed solution lines; label other legal moves unlisted without claiming they lose; no alternative engine verification |
| PUZ-10: Selection order | Shuffle matching puzzles without repeats until the filtered collection is exhausted |
| PUZ-11: Local progress | Per-puzzle attempt history, outcomes, hint assistance, elapsed time, and session summaries; no changing player rating selected |
| PUZ-12: Run recovery | Preserve paused run/remaining time across focus loss, sleep, and relaunch; always require explicit Resume; hide board/solution while paused |
| PUZ-13: Post-solve transition | Show solution review and wait for Next; do not charge session countdown during between-puzzle review |
| PUZ-14: Hint timing | Existing countdown keeps running while hints are shown; assistance remains recorded |
| REPORT-05: Imported reports | Analyze imported completed games only on explicit request |
| PLAY-01: Live clocks | Continue on app focus loss; pause on computer sleep and require explicit Resume |
| PLAY-02: Takebacks | Allow in both modes; return to previous human turn against Reckless, undo one move locally; restore clocks under PLAY-04 and confirm under PLAY-05 |
| PLAY-03: Live history | Keep live game, clocks, and engine search running while browsing; clearly offer Return to Live |
| PLAY-04: Takeback clocks | Restore both clock snapshots before undone moves, including reversing their increments |
| PLAY-05: Local takeback consent | Configurable per game; opponent confirmation required by default |
| PLAY-06: Initial time control | 10 minutes per side with 5-second Fischer increment; adjustable |
| PLAY-07: Untimed engine budget | Configurable default of 3 seconds per move, separate from report budget |
| PLAY-08: Undone continuations | Retain removed takeback moves as an alternative variation |
| PLAY-09: Clock formats | Untimed, sudden death, Fischer increment, staged/cyclic, simple delay, and Bronstein delay |
| PLAY-10: Pending local consent | Keep live clocks running until the takeback is accepted; accepted takebacks restore selected snapshots |
| PLAY-11: Manual pause | Both engine and local modes; pause clocks/engine search and require explicit Resume |
| ENGINE-01: Engine recovery | Preserve game/pause clocks; restart only after explicit Retry; require explicit Resume after recovery |
| RULES-01: Human draw claims | Require explicit Claim Draw for claimable threefold repetition and fifty-move draws; fivefold/seventy-five-move draws remain automatic |
| PLAY-12: Initial human side | White against Reckless; adjustable in New Game |
| PLAY-13: Initial variant | Standard chess; Chess960/FRC and DFRC remain available |
| RULES-02: Reckless draw claims | App always claims an available valid threefold/fifty-move draw for the Reckless seat |
| PLAY-14: Chess960 starts | Random by default, with numbered-position selection also available |
| PLAY-15: DFRC starts | Independently randomize both back ranks by default, with numbered-position selectors for each side also available |
| PLAY-16: Concurrent games | Multiple live games may run simultaneously, sharing engine resources |
| ENGINE-02: Untimed search resume | Start a fresh search with the full configured per-move budget after pause |
| PLAY-17: Pending takeback invalidation | Cancel a pending local takeback when another move is accepted; allow a new request against the updated game |
| RESOURCE-01: Concurrent CPU | Equal shares among active live-game searches within the shared CPU budget |
| RESOURCE-02: Hash memory | One total hash-memory budget automatically divided among engines |
| UI-19: Concurrent-game UI | Game tabs plus an overview showing every live game's clocks and turn |
| RESOURCE-03: Admission | Limit simultaneously running engine games so each has at least one CPU thread reserved, including during human turns |
| RESOURCE-04: Rebalance | Apply new thread shares at each engine's next between-search boundary; temporary unequal shares are acceptable within the total CPU budget |
| RESOURCE-05: Paused hash | Release paused-game hash allocations for running engines; recreate caches on Resume |
| RESOURCE-06: Initial hash budget | Automatic sizing; formula approved in RESOURCE-07 |
| LIFE-01: Quit with live games | Ask for confirmation, then pause and save all games before quitting |
| LIFE-02: Relaunch | Restore all unfinished game tabs paused; explicit Resume required |
| RESOURCE-07: Automatic hash formula | 10% of OS-reported available RAM at app startup, capped at 2 GiB; Settings override available |
| SAVE-01: Autosave | After every accepted move and pause/resume/result transition, including game state and clock snapshots |
| LIFE-03: Running tab close | Ask for confirmation, then pause, save, and close that tab; unfinished game remains reopenable from library |
| SAVE-02: Save failure | Retain current state in memory, clearly mark unsaved, pause affected game/engine, and offer Retry Save |
| SAVE-03: Crash clocks | Restore last successfully saved clock snapshot paused, charge no downtime, and require Resume |
| RESOURCE-08: Insufficient memory | Keep requested game paused and explain which budget or running games must change |
| PLAY-18: Engine takeback | Apply immediately on selection without an extra confirmation dialog |
| PLAY-19: Local request expiry | No expiry timer; await response, cancellation, or another accepted move |
| PLAY-20: Resignation | Always require confirmation in both game modes |
| SETTINGS-04: Unsaved edits | When leaving unsaved settings that require Save, offer Save, Discard, and Stay |
| SETTINGS-05: Reset | Per-section and all-settings reset, both requiring confirmation; retain games, puzzle history, and datasets |
| SETTINGS-06: Portability | Include validated settings import and export, with details in SETTINGS-07 through SETTINGS-09 |
| SETTINGS-07: File format | Versioned JSON |
| SETTINGS-08: Paths | Exclude machine-specific paths from exports; preserve destination locations |
| SETTINGS-09: Import application | Validate and preview changes before confirmation; merge only included fields and preserve others; defer engine application to safe boundaries |
| SETTINGS-10: Older schemas | Migrate explicitly supported older versions, then validate/preview; reject versions without a migration |
| SETTINGS-11: Newer schemas | Reject without changes and explain that an app update is needed |
| SETTINGS-12: Unavailable options | Preview supported changes and explicitly list unavailable options; apply supported fields only after confirmation |
| PUZ-15: Opponent replies | Randomly choose among dataset-listed opponent replies, with no engine verification |
| PUZ-16: Exhaustion | Show progress and offer explicit Reshuffle and Continue; never reshuffle automatically |
| PUZ-17: Filter changes | Apply to the next run; preserve the current run's collection |
| PUZ-18: Exhaustion timer | Stop session countdown on progress screen; carry remaining duration forward only after explicit Reshuffle and Continue |
| PUZ-19: Retention | Keep puzzle attempt history and session summaries indefinitely until explicit deletion |
| PUZ-20: Summaries | Counts/rates for unassisted solves, assisted solves, failures, timeouts, and time spent; no points system |
| PUZ-21: Primary outcomes | A failed puzzle stays failed after practice completion; show practice completion and hint use separately; primary totals do not overlap |
| PUZ-22: Hint stages | Restart source/destination/reveal progression for each new player move; any hint marks the attempt assisted |
| PUZ-23: Deletion | Confirm deletion of individual attempt, session with its attempts, or all puzzle history; preserve datasets |
| PUZ-24: Countdown start | Start when the puzzle board is ready, visible, and interactive, including after Next |
| PUZ-25: Skip | Include Skip with a separate skipped outcome; advance without revealing solution or resetting session time |
| PUZ-26: Illegal input | Reject without failed-attempt record; keep countdown running |
| PUZ-27: Solve rates | Denominator is all finalized puzzles, including failures, timeouts, and skips; show unassisted and assisted rates separately; exclude active unfinished puzzles |
| PUZ-28: Time spent | Active countdown time, including automatic opponent replies, animations, hints, and retries; exclude pauses and between-puzzle review |
| PUZ-29: Session expiry | Otherwise unresolved current puzzle becomes timed out; an already-failed puzzle stays failed with separate expiry event; end session |
| LIB-01: Organization | Search/filter/sort plus favorites, tags, and folders |
| LIB-02: Duplicates | Preview detected duplicate games; default to skipping, allow explicitly keeping copies, never silently overwrite annotations |
| LIB-03: Trash | Move deleted library games to Trash; allow Restore and confirmed permanent deletion; no automatic expiry |
| LIB-04: Search fields | Players, date, result, variant, opponent mode, favorites, tags, folders, opening name/code, and game length; no annotation-text search in first release |
| LIB-05: Duplicate identity | Same variant, initial position, played mainline, players, date, and result; preview annotation/variation differences |
| LIB-06: Folders | Nested folders; each game occupies one folder or Unfiled; tags provide multiple groupings |
| LIB-07: Open-game Trash | Confirm, then pause/save/close tab and move unfinished game to recoverable Trash |
| LIB-08: Folder removal | Confirm removal of folder and descendants; move all contained games to Unfiled, preserving games |
| LIB-09: Initial sort | Most recently updated first; user may change sort |
| PGN-01: Invalid games | Preview errors/exclusions before confirmation; import only fully valid games and skip any game containing invalid moves or branches |
| PGN-02: Export defaults | Full supported metadata, notes, variations, notation marks, and available clocks by default; mainline-only option available |
| EDITOR-01: Draft persistence | Save/reopen explicitly invalid drafts separately from playable positions; prohibit play/engine analysis until corrected |
| BACKUP-01: Initiation | Backup Now plus daily automatic backups while app is open; schedule overdue backup on next launch |
| BACKUP-02: Contents | User data/settings including games/Trash, annotations, reports, editor drafts, and puzzle history; exclude dataset files |
| BACKUP-03: Location | Default app-data backup folder with configurable destination |
| BACKUP-04: Retention | Keep latest 30 automatic backups and all manual backups |
| BACKUP-05: Restore | Full replacement after creating a safety backup and requiring confirmation; reopen games paused; datasets stay external |
| BACKUP-06: Encryption | Unencrypted local backups in first release |
| BACKUP-07: Failure | If game autosave works, continue play; persistent backup warning with Retry Backup and destination controls |
| BACKUP-08: Portability | Restore across all supported Windows/macOS/Linux targets with explicit machine-path and unavailable-option review; datasets stay external |
| BACKUP-09: Schedule | Automatic backup due 24 hours after last successful automatic backup; run while open or schedule overdue backup on next launch |
| EDITOR-02: Starting layout | New drafts copy selected board position, or Standard setup if none; saved drafts reopen their own position |
| EDITOR-03: Reset controls | Include Clear Board and Standard Setup, both requiring confirmation before replacement |
| EDITOR-04: Undo/Redo | Include editor-local Undo/Redo for draft piece and position-field edits, separate from game takebacks |
| EDITOR-05: Saving | Explicit Save; leaving unsaved edits offers Save, Discard, and Stay, including invalid drafts |
| EDITOR-06: Undo lifetime | Undo/Redo exists only in the current editor session; saved position persists |
| EDITOR-07: Piece input | Piece selection/click placement, palette and square-to-square drag/drop, plus keyboard placement/removal; shortcuts selected in decisions.md |
| EDITOR-08: Play from position | Open New Game with validated position prefilled and require explicit Start |
| EDITOR-09: Draft access | Positions section in Library and draft list in editor; distinguish valid/invalid drafts from games |
| EDITOR-10: FEN into editor | Create new draft, preserving prior draft and its unsaved-edit prompt; reject invalid FEN with explanation |
| OPEN-01: Explorer statistics | Game count, move frequency, White/draw/Black results, available average player ratings, and openable sample games; mark missing data unavailable |
| OPEN-02: Explorer filters | Dataset, year/date range, player rating range, time control, player names, and result, where source metadata exists |
| BOOK-02: Weights | Raw weights and normalized percentage shares within selected book position; relative preferences, not evaluations or winning probabilities |
| OPEN-03: Result percentages | Use known-result games only for White/draw/Black percentages; show unknown-result count separately |
| OPEN-04: Position occurrences | Count only the first occurrence of a position per source game |
| OPEN-05: Dataset scope | Select one dataset or combine selected local datasets; identify combined sources |
| OPEN-06: Combined duplicates | Count matching games once using approved variant/start/mainline/player/date/result identity; retain all source identities |
| OPEN-07: Move frequencies | Denominator is matching games with a recorded next move; show games ending here separately |
| OPEN-08: Rating criteria | Both Players, White, Black, or Side to Move; default Both Players; exclude missing required ratings while filter is active |
| BOOK-03: Ordering | Highest legal book weight first initially |
| BOOK-04: Zero total | Keep legal moves/raw zero weights visible and mark normalized shares unavailable |
| STUDY-01: Continuations | In analysis/study, navigate existing matching variation or add one as needed; preserve main line and never submit a live move |
| ANNOT-01: Notes | Plain text for editing and PGN interchange |
| ANNOT-02: Drawings | Temporary during live play; explicit Save Markings in analysis/review; saved marks belong to specific game-tree position |
| ANNOT-03: Promotion | Confirm saved-study variation promotion; retain prior mainline as alternative; cannot change running game history |
| ANNOT-04: Note saving | Explicit Save; Save/Discard/Stay before leaving unsaved note edits |
| ANNOT-05: Temporary marks | Clear unsaved temporary arrows/circles when selected board position changes; saved marks remain attached |
| ANNOT-06: Branch deletion | Confirm deletion explicitly identifying the entire analysis branch and descendants; cannot remove running-game played history |
| UI-20: Drawing gestures | Right-click toggles circle; right-drag toggles arrow; repeating same marking removes it; camera remains on visible controls |
| UI-21: Drawing colors | Green/red/blue/yellow, initially green; exact shades/modifiers selected in decisions.md; native input validation remains |
| PLAY-21: Premoves | One queued premove against Reckless; configurable and initially disabled; revalidate after engine move |
| PLAY-22: Premove clock | Charge actual elapsed time with no added fixed minimum; normal increment/delay still applies |
| PLAY-23: Premove promotion | Choose promotion piece before queueing, supporting all four pieces |
| PLAY-24: Premove replacement | New premove replaces old; Escape and visible Cancel Premove both cancel; right-click draws |
| PLAY-25: Invalid premove | Clear without execution; brief nonblocking no-longer-legal notice; turn clock continues normally |
| PLAY-26: Interrupted premove | Clear on any pause, takeback, fault, or relaunch; require new premove |
| UI-22: Premove display | Distinct source/destination highlights and Queued Premove label with Cancel |
| UI-23: Clock precision | Show tenths below 10 seconds, otherwise whole seconds; native clock accounting stays precise |
| AUDIO-02: Low time | Visual warning plus one sound per player per game at first crossing below 10 seconds; respect mute/background silence; increments do not retrigger |
| PLAY-27: Local orientation | Fixed initially with per-game Auto-flip option; auto-flip rotates 3D board after accepted moves |
| PLAY-28: Presets | Untimed, 1+0, 2+1, 3+0, 3+2, 5+0, 5+3, 10+0, 10+5, 15+10, 30+0; custom controls retained; initial 10+5 |
| UI-24: Coordinates | Initially always visible around board, following orientation; configurable |
| UI-25: Layout persistence | Remember window size/position, navigation collapse, panel widths, selected side-panel tab; Reset Layout; fit current displays/minimum size |
| GFX-03: Renderer | Bevy scene/PBR/assets with bevy_egui panels, selected under delegation; preserved backend and GLB assets |
| GFX-04: Backend | Automatic native Bevy/wgpu selection: Metal/macOS, supported Vulkan/Direct3D elsewhere; no forced Vulkan |
| GFX-05: Automatic quality | Adapt during use to sustained frame-time changes, preventing rapid quality switching; thresholds/presets selected in decisions.md; measurements remain |
| GFX-06: Frame target | Initially target 60 fps during active rendering; measured platform acceptance remains outstanding |
| GFX-07: Idle rendering | On-demand 3D rendering; stop continuous scene frames while unchanged; clocks update separately |
| GFX-08: Quality priority | Reduce shadows/effects before internal resolution, preserving crisp board rendering as long as possible |
| CAMERA-02: Tilt | Initial camera 20 degrees from top-down, centered with all squares visible |
| CAMERA-03: Reset | Restore default pose/framing/tilt while retaining White/Black orientation |
| CAMERA-04: Zoom | Keep entire board visible even at maximum manual zoom-in |
| CAMERA-05: Bounds | Full 360-degree yaw and tilt 0..45 degrees from top-down; preserve full-board framing |
| CAMERA-06: Steps | Per-click yaw 15 degrees, tilt 5 degrees, zoom 10%, constrained by framing/bounds |
| CAMERA-07: Auto-flip | 180-degree side change over 180 ms; retain tilt/zoom, respect reduced motion, leave native clocks running |
| UI-26: Font | Inter for bundled UI/notation; exact binaries, notices, glyph coverage, tabular behavior pending validation |
| LANG-01: Language | English-only interface in first release |
| NOTATION-01: Display | Offer standard letters and figurines; initially letters; PGN retains standard letters |
| AUDIO-03: Routing | Selected-game move/check sounds; result/low-time alerts from all live games while foregrounded; respect mute/background silence |
| AUDIO-04: Priority | For one move play result over check over ordinary move; do not layer those events |
| AUDIO-05: Controls | Master volume plus independent move/check/result/low-time volume and enable controls; initial values selected in decisions.md |
| AUDIO-06: Levels | Initially 70% master and 100% each category, combined multiplicatively; configurable |
| AUDIO-07: Background | Suppress sounds whenever no app-owned window is focused, including minimized/hidden; live games/clocks continue |
| AUDIO-08: Suppression | Discard muted/background sound events; never replay on focus return or unmute |
| KEY-01: Customization | Remappable shortcuts with conflict detection and Reset Shortcuts; reserved OS bindings need platform validation |
| KEY-02: Initial bindings | Cmd/Ctrl+K palette, Cmd/Ctrl+N New Game, Cmd/Ctrl+, Settings, F Flip Board outside text inputs; Cmd on macOS, Ctrl elsewhere |
| KEY-03: Palette | All applicable user actions across play/analysis/study/library/puzzles/Settings; retain confirmations and unavailable-state explanations |
| UPDATE-01: OTA scope | Include signed desktop OTA through GitHub Actions; private delivery with version-tag publication and startup/manual checks; repo-scoped token, automatic downloads, and explicit confirmed installation after pause/save |
| UPDATE-02: Privacy | Keep update binaries private; approved token design remains subject to source-pinned integration validation |
| UPDATE-03: Trigger | Pushing a version tag builds and publishes its release automatically |
| UPDATE-04: Checks | Nonblocking startup check plus Check for Updates in Settings; offline chess remains usable |
| UPDATE-05: Authentication | User-provided fine-grained GitHub token restricted to this repository with Contents read permission, stored in the OS credential store |
| UPDATE-06: Downloads | Automatically download available updates; wait for explicit installation |
| UPDATE-07: Installation | Explicit Install action with confirmation; pause and successfully save all live games before proceeding; restore games paused |
| UPDATE-08: Credential failures | Persistent update-status notice in Settings with add/replace-token controls; no blocking prompt; offline chess remains usable |
| UPDATE-09: Download failures | Stop the failed download and offer a visible Retry action; no automatic download retries |
| UPDATE-10: Restart | Relaunch automatically after successful installation where supported; explain platforms requiring manual reopening; games remain paused |
| UPDATE-11: Version tags | vMAJOR.MINOR.PATCH, matching the application version |
| UPDATE-12: Channels | Stable releases only for the first release |
| UPDATE-13: Safety backup | Require a fresh successful user-data/settings backup at the configured destination before installation; exclude dataset files and credentials; block installation and offer Retry if backup fails |
| UPDATE-14: Safety-backup retention | Keep the latest five successful pre-update backups, plus any backup needed for unresolved recovery; separate from daily/manual retention |
| UPDATE-15: Migration failure | Stop normal startup, preserve data, and offer pre-update backup recovery with preview and explicit confirmation; compatible app reinstall may be needed |
| UPDATE-16: Installation failure | Report failure; explicit Retry where safe, manual-recovery guidance otherwise; no automatic retries |
| UPDATE-17: Release retention | Keep all published private releases and assets until explicit owner deletion |
| UPDATE-18: OTA signing custody | GitHub Actions encrypted secrets hold the OTA signing key and passphrase; owner controls an offline backup; no key creation during planning |
| UPDATE-19: Metered downloads | Ask before downloading on OS-detected metered connections; ask also when status is unknown; platform detection remains unvalidated |
| UPDATE-20: Unknown connection status | Ask before downloading when metered status is unknown or unavailable |
| UPDATE-21: Check timeout | 15 seconds total per update-check attempt across discovery and metadata, excluding binary download |
| UPDATE-22: Download timeout | Stop after 60 seconds without received bytes or 30 minutes total, whichever occurs first; manual Retry |
| UPDATE-23: Cancel download | Visible Cancel stops downloading, discards partial bytes, and waits for explicit Retry |
| UPDATE-24: Completed cache | Keep completed update downloads across relaunch; revalidate before installation |
| UPDATE-25: Download approval | Each metered/unknown-connection download attempt, including manual Retry, needs approval |
| UPDATE-26: Cache removal | No Remove Download action; retain the completed download until installation or replacement |
| UPDATE-27: Supersession | Keep the downloaded version and offer explicit Download Newer Version; replacement follows download approval rules |
| UPDATE-28: Connection changes | On detected change to metered/unknown, stop, discard partial bytes, and ask before a fresh download attempt |
| PKG-01: Windows signing | Unsigned personal-use Windows installers for now; document OS warnings; mandatory OTA signatures remain |
| PKG-02: macOS signing | Locally ad-hoc-signed personal macOS builds without Developer ID/notarization; document and validate manual OS approval |
| PKG-03: Installers | Windows NSIS .exe installer; macOS Apple Silicon .dmg containing the app; matching OTA artifacts require validation |
| COST-01: Budget | No payments for anything for now; signing, CI, hosting, assets, and data must fit this constraint; no paid setup authorized |
| PKG-05: Windows runtime | Former WebView2 requirement superseded by native app; validate native graphics driver/runtime prerequisites |
| PKG-06: Linux prerequisites | Permit internet-assisted installation of missing system dependencies; document offline preparation; app remains offline after installation |
| PKG-04: Linux formats | Both .deb and AppImage, x86-64 only; validate AppImage OTA and define a separate supported .deb update path |
| PKG-07: .deb updates | Download and verify the .deb; after confirmed pause/save/backup, open the system installer and request OS approval where required; validate integration |
| CI-01: Zero-cost hosted CI | Included standard hosted-runner allowance only; verify controls preventing paid overages before enabling workflows; stop builds when allowance is exhausted |
| KEY-04: Offline signing backup | Passphrase-encrypted removable storage kept disconnected; exact backup procedure requires separate approval |
| DATA-04: Corpus emphasis | Separate master/tournament and broad online-game collections, selectable individually or together |
| DATA-05: Data licensing | Prefer free public-domain or permissively licensed data allowing redistribution with required attribution/notices; review actual source terms before approval |
| DATA-06: Dataset updates | Explicit manual dataset updates and imports only for the first release; separate from app OTA |
| DATA-07: Starter variants | Standard chess starter opening/puzzle datasets; clearly indicate missing matching Chess960/DFRC data |
| DATA-08: Starter puzzle difficulty | Broad available difficulty range with difficulty/theme filters; no player-rating system |
| DATA-09: Starter book | Generate a study-only Polyglot book from the approved opening corpus; master frequency weighting and 40-ply depth selected |
| GENBOOK-01: Source | Generate the bundled book from the approved master/tournament collection only; both collections remain available in the explorer |
| GENBOOK-02: Weights | Observed move frequency in selected source games; integer scaling resolved by the latest owner answer |
| GENBOOK-03: Depth | First 20 full moves, up to 40 plies from the standard starting position |
| GENBOOK-04: Scaling | Preserve fitting counts; otherwise scale per position to max 65535, round nearest, positive counts minimum 1 |
| GENBOOK-05: Rare moves | Include every observed legal move, including a single source-game occurrence |
| GENBOOK-06: Custom starts | Assistant selects standard-initial-position source games only |
| Review process | Owner delegated all remaining choices to the assistant; resolve them without further questionnaires and distinguish selection from verification |

The platform confirmation concerns OS and CPU architecture. It does not approve
the draft's installer formats, signing costs, RAM requirements,
or performance targets. It does not claim those platforms have been tested.

## Owner-approved application features

The owner answered "i want all 8". Every item below is included in the first
release. Sources, default behaviors, and detailed policies remain separate choices.

| ID | Feature | Meaning | Decision |
| --- | --- | --- | --- |
| APP-01 | Saved-game library | Keep an organized local collection of games to find, reopen, and replay | Include |
| APP-02 | PGN import/export | Read/write the standard text format for complete games, moves, player details, and optional notes/variations | Include |
| APP-03 | FEN import/export | Copy/paste the standard text representation of a single board position and its move-state fields; not a complete game history | Include |
| APP-04 | Annotations and variation trees | Add chess notes/marks and explore alternative move branches without replacing the main game | Include |
| APP-05 | Custom position editor | Place/remove pieces and set turn/castling state to analyze a constructed position, with validation | Include |
| APP-06 | Game-wide analysis reports | Analyze every move and summarize evaluation changes, costly moves, and stronger alternatives | Include |
| APP-07a | Opening explorer | Browse opening continuations using a local game database and see available frequencies/results | Include |
| APP-07b | Opening book | Inspect prepared opening moves for study; never supply opponent moves | Include |
| APP-08 | Puzzles | Solve tactical positions with defined solutions and feedback | Include |

Opening explorer, opening book, and puzzles need approved data sources and
offline data packaging. Reckless does not supply them. Adding notes
to a chess game is separate from the owner's prohibition on authored code comments.

## Current questions awaiting answers

None. After the owner could not choose, select Bevy + egui under the existing
recommendation delegation. Engine comparison is resolved in research.md; native
execution and release gates remain evidence work rather than preference questions.

## Final delegated resolutions

The latest direct answers select unchanged counts when Polyglot weights fit,
proportional per-position overflow scaling with nearest rounding/minimum 1, and
inclusion of every observed legal move. The assistant selects standard-initial-
position games only for the generated starter book.

[Resolved implementation decisions](decisions.md) now specify dataset sources,
licenses and size selection; puzzle CSV and lifecycle; book generation; report
formulas; clock/delay/race rules; exact-adjudication safeguards; resource
allocation; SQLite and backup recovery; import/IPC limits; assets, design tokens,
camera/graphics quality; dependencies and testing; and private OTA/zero-cost CI.
The direct owner decisions above remain authoritative. In particular, no paid
services, Linux ARM64, public distribution, implementation or publication are
selected.

The selected master corpus is a clearly labelled titled-master online subset
of CC0 Lichess standard games. Lichess broadcasts are excluded because their
section uses CC BY-SA rather than the preferred permissive terms. Broad games
remain separately selectable, with combined-query deduplication. This source
selection is an assistant decision under delegation, not an earlier owner answer.

## Readiness and remaining evidence

The subsequent clarification selected one writable owner per existing local
profile, with second-launch activation and no takeover of an unreachable live
owner. It was chosen under the same prior delegation, not a new owner response.
Story 9.8, FR-004 and SC-006 now map to
[the ownership design](decisions.md#local-profile-ownership-and-activation),
[activation contract](contracts/desktop-ipc.md#local-activation-interface) and
[helper handoff](contracts/packaging.md#profile-ownership-during-installation).
The plan acquires OS exclusion before mutable initialization and retains it
through restore/shutdown; durable installation intent closes the helper gap.
Actual OS locking, authenticated activation and focus/installer outcomes remain
native acceptance work. Existing implementation, index and task status are
preserved by this planning revision.

Planning decisions are resolved and task generation can proceed. The
[release evidence register](decisions.md#release-evidence-register) separates
unperformed implementation/native/performance work from design choices. General
dead-position/flag-fall adjudication, private updater integration, minimum-system
compatibility and verified no-overage CI remain substantial release gates. No
implementation or release is claimed complete. External model conveyance also
requires resolving the NNUE licensing gate before sharing with other people.
