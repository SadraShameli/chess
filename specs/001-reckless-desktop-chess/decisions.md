# Resolved Implementation Decisions

**Date**: 2026-10-16

The owner delegated remaining choices to the assistant: "for every question from
now, pick ur own recommendation and best pick". This document resolves the
previous questionnaires and design placeholders. Explicit earlier owner choices
remain authoritative. These are selected requirements and implementation
strategies, not results of runtime tests. Engineering rules remain in
[the canonical catalog](../../.specify/rules/README.md).

## Delivery and evidence

Complete the full first-release scope, then generate implementation tasks in a
separate workflow. This planning task changes documentation only. Keep the
application private with all owner rights retained; add no application license
file and convey no third-party distribution rights. Preserve upstream notices.
No purchases, new accounts, credentials, workflows, releases, commits, or pushes
are authorized by these decisions. No analytics, advertising, cloud sync, or
network requirement for chess. Dataset preparation is a build-time operation,
not an automatic client download.

Build for Windows 11 x86-64, macOS 14+ ARM64, and Ubuntu 22.04+ x86-64, with
X11 and Wayland validation. Retain NSIS, DMG, .deb, and AppImage selections.
Unsigned Windows and ad-hoc-signed macOS packages need tested OS-warning and
manual-approval instructions. OTA signatures remain mandatory on all targets.
Native installation, WebView compatibility, dead-position adjudication, private
updater delivery, and measured performance are release gates, never inferred
from design. See [the release evidence register](#release-evidence-register).

## Starter datasets and provenance

Use the [Lichess database exports](https://database.lichess.org/) for CC0 standard
games and puzzles. Its broadcast section explicitly uses CC BY-SA 4.0; exclude
that collection. The master collection is titled-master **online** games,
labelled accordingly, not misrepresented as over-the-board tournament games.
Require both players' source PGN title tags to be GM, IM, FM, WGM, WIM, or WFM;
exclude missing titles from this collection. Preserve title provenance. The
broad collection has no title requirement. Deduplicate combined queries using
the approved game identity, preserving both collection memberships.

Prepare from the December 2025 standard export and the puzzle export advertised
as updated 2026-10-05. Use the CC0 lichess-org/chess-openings TSV corpus at Git revision a6189a30dc273ccb21fc2536a9a2fefd5592a67a for ECO
codes/names. The data-preparation task must resolve actual source URLs, save
license copies, freeze the openings Git revision, and record downloaded and
derived SHA-256 hashes in a versioned manifest. A changed/missing source fails
preparation explicitly; do not silently fetch a different snapshot. These large
files have not been acquired by planning.

Target up to 100,000 master games, 250,000 broad games, and 200,000 puzzles.
These are upper selection targets, not promised available counts. Choose games
by deterministic ascending SHA-256 of canonical source identity. Stratify broad
games by known rating bands (<1200, 1200..1599, 1600..1999, 2000..2399,
2400+, unknown), then by bullet/blitz/rapid/classical/correspondence/unknown
time class, allocating equally to nonempty strata. Apply the same deterministic
method to puzzles by 250-point rating bands and primary theme, redistributing
unused quota to nonempty strata. Retain source identity for all entries.

The hard installed starter-data cap is 2,000,000,000 bytes including indexes,
source-game storage, book, puzzle data, manifests, and labels. Component ceilings:
800 MB master, 800 MB broad, 300 MB puzzles, 50 MB book, 50 MB labels/manifests.
Use decimal MB. Measure the actual installed read-only databases and reduce
deterministic selection counts until every ceiling and the total cap pass;
report final counts and coverage. Do not pad collections or count compressed
download size as installed size. Personal imports are outside this starter cap.
Starter indexes cover the first 40 plies; retained full games support samples
and deeper positions with an explicit bounded background index extension.

Manual dataset replacement builds a new immutable version in staging, validates
all records, previews counts/errors/source changes, then switches atomically
after confirmation. Keep the old version pinned while a puzzle run, query, or
report references it. Deletion requires confirmation and must not invalidate an
active run. User history retains source IDs and puzzle snapshots after removal.

## Puzzle interchange and lifecycle

Support the documented Lichess CSV header: PuzzleId,FEN,Moves,Rating,
RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags, with optional
DailyDate. Accept the older header without DailyDate. UTF-8 and standard quoted
CSV fields are required. Moves are space-separated UCI. The first move is the
opponent setup move: apply it to the imported FEN before displaying the board;
the second move begins the player's solution. Validate the whole line. A CSV
row supplies a single solution line, never invented branches. Unknown columns
are listed in preview and preserved as source metadata, not executed. Missing
optional ratings/themes are unavailable, not invented. Reject invalid rows and
preview exclusions before committing valid ones. No engine verification of
unlisted moves is introduced.

Use persisted shuffle order and opponent-reply choices from an injected,
seedable random provider. Save each accepted move, hint, failure, skip, timeout,
pause, review, and session transition transactionally. While active, checkpoint
remaining time once per second; crash recovery restores the last durable
checkpoint paused, with at most one second of unsaved countdown lost. A failed
puzzle checkpoint pauses the run and exposes Retry Save. Per-puzzle timeout
records the outcome and advances directly when the next board is ready;
solution review remains accessible in history. Session timeout ends the run
and displays its summary. Skip preserves earlier failed primary outcomes and
adds a secondary skip event. End Run requires confirmation and finalizes an
otherwise unresolved active puzzle as skipped with an ended-run reason.
Zero finalized attempts display an unavailable rate, never division by zero.
History deletion while a run is active is refused with an explanation.

## Book generation and explorer semantics

Generate the book only from deduplicated titled-master games starting at the
standard initial position, through 40 plies. Count the first occurrence of a
position in each game, including every observed legal next move. If the maximum
count M at a position is at most 65535, preserve counts. Otherwise store
max(1, floor(count * 65535 / M + 0.5)), using checked integer arithmetic.
Record the original counts in provenance; Polyglot weights are scaled preferences.
No pruning of one-game moves. Generated Polyglot keys/move encoding must pass
independent reference fixtures. Display shares to one decimal place from legal
stored weights, explain rounding may not total 100%, and sort ties by canonical
UCI move ascending. Imported standard Polyglot books are Standard-only; show
unsupported variant state in FRC/DFRC instead of guessing hash conventions.

Use a collision-checked full position identity containing variant, placement,
turn, castling rook squares, and legally available en-passant rights. Exclude
move counters. Never use a 64-bit hash alone to merge positions. Normalize game
identity with Unicode NFC, trim surrounding tag whitespace, case-fold player
names for matching, preserve original display strings, canonicalize moves/FEN,
and represent incomplete date components and missing tags explicitly. Missing
identity metadata yields a potential duplicate with a preview warning; no
automatic destructive merge. Active filters exclude missing required metadata.
Date intervals use overlap with partial dates and disclose partial-date matches.
Rating averages use available per-side ratings, with sample counts shown.
Opening names use the deepest matched known opening position, with code then
name lexical tie-breaking. Show unavailable when no definition matches.
Sample games sort newest known date first, then canonical game identity; page
50 at a time. Initial library pages are 100 items. Only content/metadata edits,
accepted moves, and organization changes update modified time, not mere viewing.
Trash restore keeps a surviving folder or moves to Unfiled; restore never
overwrites another game ID.

## Report scoring and job scheduling

Use a versioned application metric, explicitly not Chess.com's or Lichess's
accuracy. For exact centipawn evaluations, convert White cp to White expected
score E = 1 / (1 + exp(-cp / 400)); clamp cp to [-10000, 10000] for numerical
stability. For Black decisions use 1-E. For a played move, loss L is
max(0, E_before - E_after) from its mover's perspective. Analyze both adjacent
positions at the same configured budget; results can still be noisy.
Labels: blunder for L >= 0.20, mistake for L >= 0.10, inaccuracy for L >= 0.05,
otherwise acceptable. Mark a matching top candidate as best separately; a
single legal move is forced. Accuracy per player is
100 * (1 - mean(L)) over valid scored moves, clamped to [0,100], displayed to
one decimal. Show sample count and analyzed/total progress. Exclude forced
moves from accuracy. No scored moves means unavailable. Partial reports say
partial and never present their metric as the completed game's accuracy.

Proven mate winners map to E=1 or E=0; terminal draws map to E=0.5. A lost forced
mate receives its own missed-mate flag. Mate distance changes are shown
separately, not converted into centipawns. Bound scores are not exact: show the
bound and leave that move unclassified until exact search results are available.
Missing results remain unscored. Record formula version, engine identity,
configuration and immutable line digest with every report. Completed positions
survive pause/cancel/relaunch; restart an interrupted position with its full
budget. Changed lines produce a new job, preserving the old report as outdated.
Metadata-only edits do not invalidate searches. Reopening never queues a report;
a newly played completion does. User-requested reanalysis previews replacement.

All running live games pause automatic reports. Live searches have first CPU
priority; interactive analysis uses spare capacity and stops for live demand.
With no running live games, selected interactive analysis preempts reports;
otherwise run one FIFO report using all CPU capacity available to it. Workbench
CPU diagnostics require explicit start and no conflicting live search or report;
show resource refusal rather than oversubscribing. Puzzles use no engine search.
Background indexing runs only on spare capacity and yields at bounded chunk
boundaries. Every UI request remains responsive through the async boundary.

## Clocks, starts, claims, and races

Use Scharnagl numbering 0..959, with 518 the standard arrangement; display
zero-based labels explicitly. Draw FRC indexes uniformly; DFRC draws two
independent indexes. Save actual indexes and initial FEN. Validate structural
position legality, rights, checks and counters with shakmaty. Accept constructed
studies without attempting proof of historical reachability; label custom starts.
FEN must declare its variant; use Shredder-FEN rook-file castling rights for
randomized starts. Halfmove range 0..1000000, fullmove 1..1000000.

Each time-control stage has positive move quota or final unlimited quota, base
time, and one bonus kind: none, Fischer, simple delay, or Bronstein. No increment
and delay on the same stage. Stage transitions occur for each side after that
side's quota of accepted moves; add the next stage's time once. Final cyclic
stages repeat their stated quota/time. Delay applies per turn. Simple delay
charges max(0, elapsed-delay). Bronstein also charges max(0, elapsed-delay)
on completed turns, but shows the main clock counting immediately and applies
the refund only when a timely legal move completes; flag at main+delay elapsed,
not at a transient displayed zero. Use a visible delay/refund indicator. Pause
retains already used delay and elapsed turn time; Resume does not grant a second
delay. Accepted takeback restores complete pre-move snapshots. The native actor
timestamps move receipt, validates its revision, checks flag fall, then commits
the move, stage and bonus atomically. A move received at or after the deadline
is late. Chessmate/other terminal effects from an accepted timely move precede
subsequent timer events. Serialize result/takeback races; terminal result cancels
pending consent, and post-result takebacks are unavailable. Reuse a matching
undone variation, preserving stable sibling order.

For delay engine turns use only `go movetime B`, with a native hard turn deadline.
B = max(1 ms, min(remaining+unused_delay-overhead,
max(remaining/30, 1 ms)+unused_delay-overhead)). If the safe deadline has already
passed, do not search. The default overhead remains Reckless's documented
100 ms; the search estimate does not change player clock accounting. Timed
non-delay modes use supported UCI clock parameters and stage movestogo.
Untimed Resume uses a fresh full configured budget as already selected.
Intended-move draw claims validate the intended legal move without committing
it first; invalid claims explain the reason and leave clocks running. The engine
seat claims at its turn before search or before committing a qualifying intended
move. Agreed draw requires both local seats' confirmation; in engine games offer
only valid claims, automatic draws, or resignation, not a fictitious UCI draw offer.

FIDE dead position and flag-fall possibility of mate are existential legal-move
questions, not ordinary engine WDL or a material-value heuristic. Use a typed
adjudicator with proven sound material/blocked-position recognizers and a
transposition-memoized legal reachability proof fallback; draws and side-specific
flag-fall exceptions require proof, not engine evaluation. A bounded proof
search has a 2-second/1,000,000-state/128 MiB initial budget, whichever is
reached first, and may return Unknown: preserve the position, pause clocks/search, and show
an adjudication interruption with Retry or explicit consensual result entry;
never silently award a win/draw. Do not call this exhaustive practical FIDE
coverage. Establish practical completeness and latency on the adversarial rules
corpus before release; unresolved coverage blocks that release gate. Fifty/
seventy-five-move and repetition termination retain their selected claim/automatic
semantics, including checkmate precedence. Custom starts have no implied history.

## Capacity and memory

Detect affinity/quota-aware usable hardware threads at bootstrap; failure is a
typed setup error with manual-budget input. The default CPU budget is all usable
threads. Running engine games reserve at least one thread each. Divide active
search allocations by quotient/remainder, with remainder rotation in stable
game order between searches. Reservations for games on their human turn remain
admission obligations; lend spare cycles to current searches, then reclaim at
their next boundary before another search starts. A queued search waiting for
reclaim shows capacity waiting; its live clock continues. New game Start/Resume
waits paused for its first reservation and memory allocation, without charging
a not-yet-started turn. Temporary unequal allocations never exceed the budget.

Stop, quit, and reap a paused game's engine to release hash and other process
memory; reconstruct its position/history/cache on Resume. This avoids pretending
Hash accepts zero. Resize other caches only at idle boundaries and release old
allocations before granting new ones. Hash budget is floor(min(available_RAM/10,
2 GiB)/MiB) MiB, divided by quotient/remainder in stable engine order, minimum
1 MiB per engine. A zero result refuses admission. Also reserve the pinned NNUE
bytes plus 128 MiB non-hash allowance per process and 512 MiB for the app/OS
headroom when checking current available memory; measure and increase these
admission allowances if reference-system peaks require it. No automatic shrink
of configured hash to admit a game. Treat these as initial conservative budgets,
not guaranteed memory bounds. Recheck on admission and report OS pressure.

## Storage, imports, and backup recovery

Use bundled SQLite via rusqlite on one dedicated async-facing storage actor;
no blocking database work on runtime workers. WAL, foreign_keys=ON,
synchronous=FULL, 5-second busy timeout, and transactional numbered migrations.
Tables own games, move nodes, notes/marks, clock snapshots, folders, tags,
reports/jobs/results, datasets/provenance, puzzle definitions/attempts/runs,
preferences, and backup history. Add foreign-key and query indexes; immutable
starter databases are separate from user data. Store full position keys alongside
hash lookup accelerators. Emit an autosave receipt only after transaction commit.
On failure retain in-memory state and pause the affected game as selected.
Retry Save uses the newest consistent snapshot under serialized ownership;
quit/update/Trash cannot discard unsaved state after a failed prerequisite save.

Use the SQLite online-backup API and a revision-consistent manifest in a
checksummed archive; keep credentials, update installers and datasets out.
Validate archive paths, entry count, decompression size and checksums before
restore. First automatic backup is due 24 hours after first successful user-data
creation if no earlier automatic backup exists. An overdue launch schedules it
after initialization. Failure warns persistently and retries only on explicit
Retry or next launch, without an in-session retry loop. Pre-restore safety
snapshots follow the same latest-five-plus-protected policy as pre-update safety
snapshots, in separate categories. Manual backups remain indefinitely.

Restore previews source/schema and dataset/path/option dependencies; keep this
machine's paths by default, disable unavailable options after preview, and
require explicit confirmation of the resolved plan. Support current schema and
explicitly shipped older migrations; reject newer/unsupported schemas unchanged.
Prepare and migrate a complete staged database, fsync it and its manifest, then
use a recoverable same-volume rename journal. Keep the current database until
the staged restore validates; interrupted startup finishes or rolls back the
journal deterministically. Never mix database/settings versions. Safety-backup
failure prevents replacement. Restore all games and runs paused.

Replace the earlier 16 MiB PGN-file proposal with streaming imports up to 20 GiB
of input, 1,000,000 games, 100,000,000 total plies, 100,000 plies per game and
64 nested variations, checked before commit. Bound a text field at 1 MiB, a CSV
row at 1 MiB, a FEN at 512 bytes and a settings file at 1 MiB. Starter builders
may stream larger raw sources but obey bounded buffers and installed caps.
Use 64 MiB decoded import buffers and at most 128 MiB index batches. Reject
limit violations explicitly, preserving existing data. Stage all imports outside
the live library; present valid/excluded/duplicate counts and commit only after
confirmation. Cancellation deletes staging. User libraries have no artificial
total-game cap; capacity depends on disk space and validated indexes. Estimate
required staging and final space and refuse safely if unavailable.

Retain existing UCI 64 KiB lines, 256 control messages, 1 MiB diagnostic tails,
20 Hz telemetry, 64 unacknowledged UI events, 5-second handshakes, 2-second stop
safety deadline, 3-second shutdown and p95 stop target of 1 second. IPC mutation
idempotency stores the last 1024 request results for 5 minutes per app session;
expiry requires snapshot reconciliation. Each subscription has a 10-second
ack deadline before closing with resync required. File/job progress is coalesced
to 5 Hz. Workbench deadline defaults to 60 seconds, configurable to 10 minutes;
noninterruptible engine jobs cancel by child termination/reaping.

## Presentation, assets, and input

Use original parametric sculpted meshes authored for this project, checked into
resources as GLB with the generator source and provenance. Model distinct knight,
bishop, rook, queen and king silhouettes; use lathed geometry for suitable pieces
and a shaped knight profile. No paid asset or unlicensed downloaded model.
Generate short PCM WAV effects from owned synthesis parameters, bundled offline:
soft move click, distinct check/result cues and restrained low-time pulse.
Visual review must establish that the resulting models meet the premium brief.
Bundle Inter variable roman 4.1 with its OFL notice. Use original monochrome SVG
piece silhouettes for inline figurine notation, with accessible letter labels;
the board remains fully 3D. Avoid reliance on OS chess-glyph coverage.

Initial window/reset size is 1440x900 CSS pixels, clamped to the usable display
and the selected 1280x800 minimum; tell the user if a display cannot fit it.
Navigation widths are 208 expanded/64 collapsed; side panel defaults to 360,
range 300..560; reserve at least 560 CSS pixels for the board workspace.
Collapse navigation first, then make the side panel an explicit drawer at widths
below 1360. Library, Positions, Puzzles, Settings and Workbench are nav entries;
review/report tools are board-side tabs. Retain last selected layout as approved.
Dark tokens: background #11151a, surface #1b222b, elevated #252f3b, text #f3f5f7,
muted #b8c2ce, emerald #34d399. Light: background #f6f2e9, surface #fffdf7,
text #17212b, muted #4b5968, accent #006b50. Board #4b7057/#eee5cf; ivory
#f2e6cb, ebony #202328. Markings green #15803d, red #dc2626, blue #2563eb,
yellow #ca8a04. Contrast tests can require token correction before release.
Use outlined/patterned highlights in addition to color. No essential state
depends only on animation, sound, hover, or color.

Perspective camera vertical FOV 35 degrees; recompute a fit distance from all
board corners for the current aspect/tilt/yaw plus 8% framing margin. Zoom is
a distance multiplier in [1,1.6], initially 1.1; zoom-in divides by 1.1 and
zoom-out multiplies by 1.1, clamped. Thus all legal zooms retain the full board.
Remember tilt/yaw offset/zoom separately from game orientation; auto-flip adds
180 degrees to orientation without rewriting the stored offset. Reset zeroes
offset and restores 20-degree tilt/1.1 zoom. Keyboard board focus follows
visual White/Black orientation, independent of free camera yaw; labels and a
visible orientation compass disclose this. Drag threshold 5 CSS pixels; cancel
on Escape, pointer loss, off-board release, phase/revision change, or dialog.
An accepted move cancels any obsolete animation without delaying the next input.

High uses 2048 shadow map, antialiasing, one shadowed key light plus fill, DPR
cap 2; Medium uses 1024 shadows and DPR cap 1.5; Low disables shadows and
extra effects, DPR cap 1.25. No expensive postprocessing by default. Start Auto
at Medium. Sample active frame times over 2 seconds; degrade after 3 consecutive
windows above 20 ms p95, upgrade after 10 windows below 13 ms, with a 30-second
cooldown. Reduce shadows/effects through tiers first, then render scale in 0.1
steps down to 0.75; recover resolution before effects. Ignore idle/background
and loading windows. Manual tiers disable adaptation. GPU baseline: hardware
WebGL2, max texture dimension at least 4096, working instanced drawing and
depth textures. Validate integrated Intel UHD 620-class Windows/Linux hardware
and Apple M1-class macOS rather than inferring speed from feature support.
Minimum 8 GiB system RAM, 4 logical CPU threads and 6 GB free installation disk,
plus staging/backup/import space; these are selected validation baselines.

Context loss, initialization failure, shader failure and unrecoverable asset
load errors produce a native interruption of all renderer-dependent live games
and an explicit Retry screen. Retry has a 10-second initialization deadline;
no automatic game resume. Failed initial launch shows diagnostic/setup guidance,
not an empty playable board. Hidden windows stop scene animation; rendering
failure cannot convert clocks into browser-owned state.

Shortcuts retain approved Cmd/Ctrl+K, N, comma and F. Add Escape cancel,
Left/Right previous/next and Home/End first/live in focused notation, Space
activate a focused square, P pause/resume with explicit action, H puzzle hint,
Shift+N next puzzle only in review. Disable single-letter actions in text fields
and while modal dialogs own focus. Board-arrow keys move focus, not history.
Reject duplicate active chords in overlapping contexts and platform-reserved
bindings. Provide visible actions for all shortcuts. Editor: arrows focus,
K/Q/R/B/N/P select kind, C toggle placement color, Delete/Backspace remove,
Enter place, Cmd/Ctrl+Z undo and Shift+Cmd/Ctrl+Z redo in editor context.
Drawing uses visible color selection; Shift red, Alt blue, Shift+Alt yellow
where platform input permits, with green default and a Clear Temporary Markings
action. Captured pieces show captures since the recorded initial position,
never missing pieces inferred from custom-start material. Promotion dialog
permits click or Q/R/B/N, with no automatic queen assumption.

Persist low-time-alert-used per side per game independently of takeback clock
snapshots: takebacks/relaunch never replay it. Across games, serialize sounds
with priority result > low-time > check > move, drop lower-priority overlapping
events and coalesce identical category events within 100 ms; never replay muted
or background events. Show the source game for result/low-time notifications.
Evaluation bar uses E(cp) from the report transform for exact scores, labelled
in pawns; mate fills toward the proven winner with signed mate label, bounds
show inequality/hatching, unavailable uses neutral hatching without a numeric
claim. Settings defaults affect new games/runs/reports; per-game overrides are
snapshotted, not retroactively replaced. Safe runtime option changes retain
saved-versus-effective indication. Keyboard/screen-reader semantic board,
VoiceOver/NVDA/Orca, reduced motion, 200% scale and WCAG AA remain acceptance gates.

## Supporting libraries and interface completion

Select the versions in [research](research.md#final-resolution-research) as design
pins, with compatibility and lockfile resolution during implementation. Retain
Rust 1.99.0. Use bundled rusqlite, Serde, schemars draft-07 JSON Schema,
json-schema-to-ts for generated wire types and Ajv strict validation. Generate
from one Rust DTO schema, with comment-free output. Use Tauri's existing Tokio
runtime, one capacity-coordinated Rayon executor for CPU work, native reqwest
for private downloads, and keyring backed by Keychain/Windows Credential Manager/
Linux Secret Service. If no Linux credential service is available, show an
update-auth setup error, with session-only token entry as an explicit action;
never silently persist plaintext. Keep tokens out of WebView payloads after
native entry, logs, exports and backups. Bun is the script/package runtime;
validate Vite/ESLint/Vitest behavior on Bun against the selected pins.

Use Vitest browser tests with Playwright and Vue Test Utils, plus WebdriverIO's
stable Tauri service and test-only native WebDriver plugin on packaged targets.
No beta-only testing dependency or release automation listener is selected.
Preserve canonical make verify ordering; extend it for frontend formatting,
schema drift, strict vue-tsc, zero-warning ESLint, tests, and builds. Pin actual
supporting dependency versions and hashes in lockfiles when adding them; verify
stable tags and compatibility rather than guessing unresearched patch numbers.

Extend desktop IPC with typed ID/revision/preview operations: library query/
organize/trash/restore/delete, editor new/update/validate/save/list/delete,
notes save, variation promote/delete, markings save, report start/cancel/read/
list, corpus preview/apply/query, book preview/apply/query, puzzle import/filter/
start/move/hint/skip/next/pause/resume/end/history/delete, backup create/list/
preview/restore, renderer failed/retry status, and update check/approve-download/
cancel/retry/install/credential-status. Mutation payloads carry expected entity
revision; preview/apply pairs carry immutable preview IDs and digest. Rust owns
file handles and update credentials. Result envelopes and streams retain the
existing correlation/backpressure contract. Do not add raw command strings or
generic filesystem execution to accommodate these operations.

## Updater completion and zero-cost CI

Use stable Tauri updater 2.14.0 after source-pinned integration validation.
Authenticated GitHub API discovery maps manifests and target assets to verified
release asset IDs; reject arbitrary manifest origins and never forward tokens
to a different origin on redirect. Follow signed asset redirects without the
GitHub Authorization header. Check version, target, length limit, digest and
Tauri signature. HTTPS only, no downgrade, no prerelease channel. Release tags
must match Cargo/Tauri/UI versions. Download to a bounded private staging file,
fsync and rename on successful validation; incomplete attempt files are deleted
on failure/cancel/relaunch. Keep at most one completed current version and one
replacement in progress. Keep the completed old version until replacement
validates, then remove the superseded cache. No Remove Download action.
After successful installation remove its consumed cached artifact on next
verified startup. Revalidate persisted bytes before every install.

Use OS network-cost detection, treating unsupported/failed detection as Unknown;
ask for each metered/unknown attempt including Retry. Detect connection changes
at OS notifications and 5-second polling while downloading. Stop/discard on
observed change to metered/unknown and require fresh approval. No silent claim
of detecting an unobservable network transition. Checks occur on startup and
explicit Settings action only, with one check at a time and no automatic retry
loop. Rate-limit/offline/API errors show status and manual Retry; respect
Retry-After before accepting another attempt. Native overall check deadline is
15 seconds; download no-byte deadline 60 seconds, overall 30 minutes. All stages
are cancellable before installer handoff. Limit installer bytes to 6 GiB and
require space for cache, replacement, installed files and safety backup before
starting. Reject oversize packages explicitly.

Installer handoff occurs after explicit confirmation, paused/saved games, and
fresh successful safety backup. User cancellation afterward leaves games paused.
Snapshot edits/settings prompts must resolve before handoff. Keep a durable
install intent linking version, backup and database schema; verify startup identity
before clearing it. Never restore a backup or retry an installer automatically
after failure. Provide native installer/recovery guidance where process control
cannot guarantee relaunch, particularly .deb system-installer handoff.

CI uses only included standard hosted-runner allowances, no larger runners.
Do not enable workflows until actual account budget controls prevent paid
overages, including artifact/cache storage. Use no paid-overage fallback; if
unverifiable, leave release automation disabled with a local-build guide. Cancel
superseded verification runs; release-tag jobs are serialized. Avoid persistent
Actions caches initially; transient artifacts expire after 1 day. Retain final
private GitHub Releases/assets until owner deletion, distinct from transient
Actions artifacts. Verify release-asset limits and free storage feasibility for
actual sizes. Gate publication on every platform/package job and signatures,
create a draft while collecting artifacts, then automatically publish the complete
release. Failed matrix jobs leave it unpublished with an actionable failure.
Use minimum job permissions and immutable action commit pins verified against
stable releases at workflow creation. No credentials in PR jobs, no automatic
signing-key rotation; rotation requires a documented verification-key transition.
Keep the owner-controlled signing backup on encrypted disconnected removable
storage, with key restoration tested before the first release. No backup or
key material is created in this planning task.

## Engine workbench and optional data

Enable the engine's production Syzygy feature and expose local directory setup,
loaded cardinality, root DTZ/WDL and rule-50 context. Bundle no Syzygy table files
and perform no automatic tablebase downloads. Link validated user files into a
managed whitespace-free alias directory; test OS permissions and path-list
semantics. If alias creation is unavailable, return actionable setup failure
without disabling ordinary play or copying huge datasets silently. On Unix use
a private 0700 whitespace-free temporary alias root with regenerated links after
relaunch; on Windows use a validated owner-approved whitespace-free writable
directory with junction/link validation. Do not require administrator rights for
ordinary chess or claim loaded data from readyok alone.

Bundle baseline x86-64, full-feature-gated v3/v4 and portable Apple ARM profiles,
plus a Linux x86-64 NUMA build/profile. Auto-select the highest fully supported
profile after CPU/OS feature checks. NUMA additionally requires library/topology
support; if absent, show the reason and use the standard supported profile.
Advanced overrides can select only compatible app-owned profiles. No arbitrary
engine executable path. Display the active profile, model hash, capabilities,
full search telemetry, raw static eval and reproducible exported diagnostics.
Workbench includes perft/divide, bench, compiler/build diagnostics, full typed
option inspection/configuration and isolated supported developer operations.
Use perft depth 4 initially with range 1..12 under the job deadline; benchmark
parameters retain upstream defaults. Invalid/unsafe operations are rejected,
not encoded as raw text. Keep upstream broken SPSA and unsupported commands
explicitly unavailable with source evidence. Cover registry entries and all
applicable capability rows, including NUMA on actual suitable hardware before
claiming that profile verified.

## Requirement allocation

| Requirements | Owning contracts and validation focus |
| --- | --- |
| FR-001, FR-002, FR-003 | Game/clock model, engine protocol and UI; legal fixtures, clocks, claims, takebacks, premoves, concurrent games and exact-adjudication gate |
| FR-004 | Study tools and storage decisions; library organization, PGN/FEN, notes/variations, staging, round trips and Trash |
| FR-005, FR-006 | Capability inventory, workbench and packaging; every pinned usable operation/profile and disclosed unsupported facilities |
| FR-007, FR-008, FR-009, FR-010 | Engine protocol, IPC, capacity and report scheduler; scores, limits, correlation, bounded cancellation and shared CPU/hash |
| FR-011, FR-012 | UI and presentation decisions; premium 3D, motion/camera/quality, keyboard/accessibility and actual frame/input measurements |
| FR-013, FR-014, FR-015, FR-016 | Packaging, storage and lifecycle; target installers, offline use, durable recovery, notices/identity and no account/telemetry requirement |
| FR-017 | Editor and board model; structural validation, invalid drafts, controls, FEN and explicit start |
| FR-018 | Study tools and report decisions; versioned scoring, exact source correlation, jobs, partial/outdated states and review |
| FR-019, FR-020 | Explorer/book and dataset decisions; exact statistics, filtering/deduplication, source identity, legal Polyglot and study-only behavior |
| FR-021, FR-022 | Puzzle/study and provenance decisions; CSV/setup move, listed solutions, hints, timers, persistence, full offline extras and no silent scope reduction |
| FR-023, FR-024 | UI/IPC/presentation; settings apply/import/reset, palette, remapping, audio lifecycle and safe resource controls |
| FR-025 | Storage/backup decisions; retention, consistent snapshots, path review and interruption-safe confirmed restore |
| FR-026 | Packaging/updater and zero-cost CI; private authenticated signed delivery, cache, approval, safeguards, native handoff and verified billing hard stops |

SC-001..SC-014 retain the specification's selected acceptance thresholds. Record
fixtures/reference systems and actual pass/fail counts in implementation evidence;
this allocation does not mark any unimplemented behavior as tested.

## Release evidence register

| Gate | Required evidence before declaring release support |
| --- | --- |
| Rules correctness | Legal moves, FRC/DFRC castling, delay/stages, draw claims, dead positions and side-specific timeout mate-possibility fixtures; exact-adjudication practical coverage is a substantial risk |
| Library and imports | Fixed reference corpus, deep trees, malformed input, duplicates, streaming bounds, cancellation and query indexes at measured starter scale |
| Data provenance | Actual frozen source manifests/hashes, CC0 section-specific notices, master title filtering, puzzle setup-move tests and total installed bytes below cap |
| Assets | Original GLB/sound provenance, Inter OFL and packaged font identity, premium visual review at all allowed camera views |
| Performance | Selected hardware/OS records, one/max-worker runs, p95 latency/stop/frame times, memory/admission allowances and idle CPU; no timing claims yet |
| Desktop | Minimum OS/WebView compatibility, accessibility, context recovery, sleep, precise clocks, native installer and manual OS approval on actual targets |
| Updater | Pinned-source tests of private discovery/redirects/token handling, stable signatures, cache/timing/network-cost states, .deb handoff, interrupted migration/backup/install recovery |
| CI and budget | Actual free allowance and hard-stop controls, native ARM Mac runner availability within allowance, storage/asset-limit checks; block workflow activation if not established |
| Engine licensing | Preserve AGPL/Fathom notices and model provenance; no external conveyance without resolving the separate NNUE model-license issue |

These are implementation and release validation obligations, not unanswered owner
preferences. Failure of a gate cannot silently remove an approved feature or
justify a claim that the application has been tested. The plan is ready for task
generation; neither application implementation nor release readiness is claimed.
