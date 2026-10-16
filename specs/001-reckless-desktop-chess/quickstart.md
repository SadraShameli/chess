# Validation Guide: Reckless Desktop Chess

**Status**: Design selected; this prospective validation guide awaits
implementation. See [resolved decisions](decisions.md) and
[release evidence](decisions.md#release-evidence-register).
This guide does not claim that a desktop application currently exists.

## Current repository

The application is still the Rust starter binary. The existing `make run` launches
it, and `make verify` owns the current Rust checks. The Vue frontend, bundled
Reckless engine, and native installation artifacts have not been implemented.
Read the actual [Makefile](../../Makefile) for available commands.

## Confirmed validation targets

- Windows 11 x86-64.
- macOS 14+ on Apple Silicon, excluding Intel Macs.
- Ubuntu 22.04+ on x86-64 only, with a compatible updated WebView runtime.
- Private personal use, offline engine play, analysis, and local two-player.
- Owner-selected Tauri 2/Vue/Tailwind/shadcn/Bun/ESLint/Prettier stack.
- All eight application feature groups, including both opening explorer and book.
- Live clocks continue on focus loss; computer sleep pauses timed play until
  explicit Resume. Both modes support takebacks, returning to the previous human
  turn against Reckless or undoing one local move. Restore clock snapshots from before undone moves, reversing increments. Local opponent confirmation is configurable
  per game and required by default.
- Initial adjustable new-game time control: 10 minutes plus 5 seconds per move.
  Untimed engine play defaults to a configurable 3 seconds per move. Include
  untimed, sudden death, Fischer increment, staged/cyclic, simple delay, and
  Bronstein delay. Detailed delay accounting and single-movetime engine allocation
  follow the selected clock policy.
- Retain removed takeback moves as alternative variations. Local consent requests
  keep clocks running; acceptance restores snapshots.
- Manual Pause in both modes stops clocks/engine search until explicit Resume.
  Engine faults preserve the game and pause clocks; restart only on Retry, then
  require explicit Resume after recovery.
- Live history browsing leaves game/clocks/search running and offers Return to Live.
- Bundled starter datasets plus personal-file imports, with up to 2 GB installed
  starter data excluding application and engines.
- SQLite indexed storage and PGN/FEN interchange; strict TypeScript with Vite.
- Dark-first premium presentation with richer effects/animation, graphite surfaces,
  emerald accents, green/cream board, and a warm light theme.
- Real-time 3D board/pieces with camera rotation and zoom, ivory/ebony satin
  piece materials, and a 1280x800 CSS-pixel minimum window. Renderer/backend,
  models/assets, camera limits, and exact retry/quality policies remain
  under review. The board is 3D-only. Default to a lightly tilted readable view;
  drag pieces normally, right-drag to draw, and rotate/zoom with visible controls.
  Remember the camera view across launches and provide Reset View. Default quality
  is Automatic with Low/Medium/High overrides. Graphics failures preserve game
  state, pause clocks, and offer Retry; recovery requires explicit game resume.
- Bundled modern sans-serif UI/notation with tabular clock digits; families pending.
- Configurable 180 ms piece moves/160 ms panel transitions and subtle elevation,
  respecting reduced motion and responsiveness.
- Chess.com-inspired collapsible navigation, large board/player bars/clocks,
  and resizable right-side tabs.
- Configurable legal-move dots, last-move/check highlights, captured pieces, and
  user-drawn arrows/circles. Dots appear on selection; highlights/captured pieces
  are initially enabled. Drawing behavior remains under review.
- Command palette, keyboard shortcuts, and optional move/check/result sounds.
  Sounds default to enabled during active play and silent in background.
  Exact mappings/assets/audio controls remain under review.
- Opening book for study only, never supplying opponent moves.
- PGN opening games/studies, Polyglot .bin books, and puzzle CSV imports.
- Puzzle theme/difficulty filters, hints, retries, solution review, saved progress,
  and both whole-session and per-puzzle countdown modes, defaulting to configurable
  5-minute sessions and 60-second puzzles. Each timed run selects one mode.
  Manual pause and app focus loss hide the board/solution and pause the countdown.
  Session expiry ends the run; per-puzzle expiry records a timeout and advances.
  Hints progress through source/destination/next-move reveal and mark assistance.
  Record failures before practice retries on the continuing countdown. Accept
  listed solutions; label other legal moves unlisted without engine verification.
  Shuffle filtered puzzles without repeats until exhausted. Save local attempt
  history, outcomes, assistance, elapsed time, and session summaries. Preserve
  paused runs and remaining time across sleep/relaunch; require explicit Resume.
  After solving, show solution review and wait for Next without charging session
  time during review. Active hints keep the countdown running.
- Automatic reports after games played in the app complete: played main line, configurable
  3 seconds per position by default, depth/node alternatives, and variation analysis
  separately on demand. Reports pause during live games and resume afterward.
  Imported completed games require explicit report requests.
- Report move labels plus an app-calculated accuracy percentage; formulas pending.
  Clickable findings navigate to the position, show analyzed alternatives, and
  offer a separate analysis branch.
- Evaluation bar in analysis/review only; preserve the selected board position
  across move-list, opening explorer, and study-only book tabs.
- White-positive displayed search evaluations, with engine-native raw diagnostics.
- A dedicated Settings tab with common options and advanced engine/resources.
  Appearance applies immediately; other settings require Save, with engine
  changes deferred until a safe point when necessary.

Later live-game validation must distinguish focus loss from computer sleep,
verify explicit resume after sleep, and exercise history browsing while clocks
and engine searches continue without mutating the live position. Verify the
mode-specific takeback target, both restored clock snapshots, reversed increments,
restored move/stage counters, and local consent enabled by default/configurable
per game. Verify the initial 10+5 preset, distinct untimed 3-second move budget,
and retained takeback variations. Validate serialized result/consent races,
cancel-on-result and stable matching-variation reuse under the selected policy. Verify clocks run during pending local
consent and restore only on acceptance; manual Pause freezes clocks/search in
both modes. Inject engine exit/protocol failure/stop timeout, verify preserved
state and paused clocks, confirm no automatic restarts, then exercise Retry and
explicit Resume separately. Validate each clock family and delay/search
allocation after the detailed stage/delay contracts are selected; do not claim
engine-native delay support.

Later report validation must exercise pause/resume across a new live game, preserve
completed findings, and confirm that variations are not automatically traversed.
Click findings and explore alternatives without changing the original game.
Verify that importing completed games does not automatically queue reports;
request their reports explicitly. Verify bar visibility by workspace and selected-position continuity across study
tabs, including missing-data and incomplete-analysis states.

Later UI validation must cover navigation collapse, panel resizing, configurable
board aids/drawings, command discovery, keyboard conflicts, and optional sound
behavior against the detailed decisions once approved. Validate sculpted-piece
readability at display scales, offline bundled-font loading, tabular clock
stability, configured transition durations, and reduced-motion behavior. Actual
frame pacing and renderer resource usage require implementation measurements.
Verify square selection/drawing and keyboard access through supported camera
views and zoom ranges, minimum-window layout, and offline model/texture loading.
Verify that piece dragging, right-drag drawing, and camera controls do not
interfere, and that no 2D view is offered as a fallback.
Verify camera persistence/Reset View and quality selection. Simulate graphics
failure during a live game, verify preserved state and paused clocks, recover
through Retry, and require explicit resume. GPU baseline, preset criteria, and
exact fault-detection/retry details remain under review.

Later settings validation must verify persistence across restart, rejection of
invalid changes, immediate appearance application, Save-controlled changes, and
agreement between displayed and effective runtime values when engine application
is deferred.
Both puzzle timer modes need deterministic clock checks for pause/time
preservation, hidden paused content, session expiry, and per-puzzle timeout/advance.
Verify progressive hints, assistance tracking, preserved failure records,
practice retries without timer reset, and accepted/unlisted/illegal distinctions.
Unlisted moves must not be reported as losing or trigger alternative engine
verification. Verify no repeats before filtered-collection exhaustion, detailed
local progress, and restoration after focus loss/sleep/relaunch with unchanged
remaining time, hidden board/solution, and explicit Resume. Crash/checkpoint and
retention details remain under review. Verify post-solve review waits for Next
and excludes review time from the session countdown while active hint usage
continues that countdown. Report label/accuracy
validation requires the approved formulas.

Later validation must prove starter data works offline, personal imports work,
and book entries never choose an engine opponent move.

## Before this becomes a runnable guide

The data, behavior, presentation and tooling decisions are selected in
[the resolution document](decisions.md). During implementation add actual
setup/run commands and fixed fixtures to this guide, then record measured
results. The current Hello World executable is not a chess implementation.

Implementation validation will exercise the approved game flows, every applicable
[engine capability](contracts/reckless-capabilities.md), ordered IPC and process
recovery, the selected visual/accessibility criteria, and native execution on
each confirmed target. Library/import/editor/report/opening/puzzle acceptance is
required; use [the study-tools contract](contracts/study-tools.md) and selected
resolution policies to generate concrete deterministic fixtures.

No native installation, engine execution, GUI testing, or performance measurement
has been performed by this planning workflow.


Validate Windows NSIS .exe and macOS Apple Silicon .dmg packaging. The new
no-payment constraint selects unsigned Windows installers and locally ad-hoc-
signed macOS builds without Developer ID/notarization. Validate Windows warnings
and manual macOS approval, keeping mandatory OTA signature checks. No paid
services or setup are authorized. Verify the bundled offline Evergreen WebView2 installer.
Linux is x86-64 only and may install missing dependencies using internet access;
document offline preparation. Verify both .deb and AppImage; validate AppImage
OTA separately. Verify .deb downloads and signature validation, confirmed
pause/save/backup, opening the system installer, and OS approval where required.
Before CI activation verify included standard-runner allowance and controls
preventing paid overages; exhausted allowance must stop builds. The offline
OTA signing-key backup uses passphrase-encrypted removable storage kept
disconnected, with the exact procedure separately approved. These requirements
do not establish tested platform behavior.

OTA validation is now required under [the packaging contract](contracts/packaging.md#desktop-ota-and-github-actions).
Its runnable scenarios depend on remaining recovery policies and platform support.
Private binaries, version-tag-triggered publication, startup/manual checks,
repository-scoped Contents-read token storage in the OS credential store, and
automatic downloads with explicit confirmed installation are owner approved.
Verify that installation pauses and successfully saves every live game, aborts
on save failure, and restores games paused. Verify a fresh safety backup at the
configured destination includes user data/settings and excludes datasets and
credentials; backup failure must block installation and offer Retry. Verify
vMAJOR.MINOR.PATCH tags match the application version and only stable releases
are offered. Verify an OS-detected metered connection requires download approval;
unknown or unavailable status also requires approval. Platform detection remains
unvalidated. With controlled time and transport, verify the 15-second total
check deadline across discovery/metadata, excluding binary download. Verify
download timeout at 60 seconds without received bytes or 30 minutes total,
whichever occurs first, and visible manual Retry without automatic retry.
Verify Cancel stops transfer, discards partial bytes, and waits for explicit
Retry. A completed download must survive relaunch and be revalidated before
installation. Verify every metered/unknown attempt, including manual Retry,
requires approval. Verify no Remove Download action exists and completed
downloads remain until installation/replacement. Discovery of a newer stable
release must keep the cached version and offer explicit Download Newer Version
under the same approval rules. Inject a detected connection change to metered
or unknown: stop, discard partial bytes, and ask before a fresh attempt.
Review that published private releases/assets have no automatic cleanup and OTA
signing secrets stay in GitHub Actions encrypted secrets with an owner-controlled
offline backup. No secrets are created or inspected during this planning task.
Verify missing/expired/rejected
tokens produce a persistent Settings notice and credential controls without
blocking offline play. Failed downloads must stop with visible Retry and no
automatic retries. Verify automatic relaunch after success where supported and
manual-reopening explanations where needed; games remain paused.
Verify pre-update retention keeps the latest five successful snapshots and
protects any needed for unresolved recovery without affecting daily/manual
retention. Inject migration failures: normal startup must stop, data must be
preserved, and backup recovery must require preview and confirmation. Verify
compatible-version guidance without claiming automatic binary rollback. Inject
installation failures and verify explicit Retry only where safe, otherwise
manual-recovery guidance, with no automatic retries.
Verify signed update provenance, invalid
signature rejection, supported target matching, offline behavior, and user-data
preservation on installed application versions. No updater implementation or
GitHub workflow has been run by this planning task.

## Selected starter-data direction

Follow [starter provenance/selection](decisions.md#starter-datasets-and-provenance),
[puzzle interchange](decisions.md#puzzle-interchange-and-lifecycle), and
[book generation](decisions.md#book-generation-and-explorer-semantics). Standard
starters, two selectable CC0 online collections including a titled-master subset,
broad puzzle difficulty, manual updates and the 2 GB installed cap are selected.
Exact file checksums and measured collection sizes are preparation outputs,
not unanswered preferences or already acquired data.
