# Approved Study and Training Features

**Status**: All eight feature groups and delegated design choices are selected.
Use [the resolved decisions](../decisions.md) for exact defaults and algorithms;
[the review](../review.md) records owner inputs. Acceptance evidence remains unperformed.

This contract extends the [game and engine model](../data-model.md),
[desktop IPC](desktop-ipc.md), and [UI contract](ui-experience.md).
Reckless supplies position searches, not a game database, opening book, puzzle
collection, or report-classification system.

## Saved games, PGN/FEN, and annotations

Approved outcomes are local save/find/reopen/replay, complete-game PGN
interchange, single-position FEN interchange, chess notes, and alternative lines.
Keep these roles distinct from the internal persistence choice.

Each variation has an owning parent position and an ordered continuation. Notes
and notation marks remain attached to their nodes through editing, save/reopen,
and PGN export. Preview all import errors and exclusions before confirmation;
import only fully valid games, skipping any game containing invalid moves or
branches. Do not truncate an invalid game into an apparently complete one.
Default PGN export includes supported metadata, notes, variation trees, notation
marks, and available clock annotations; also offer mainline-only export.

User-written notes are plain text. Live-play arrows/circles are temporary. In
analysis/review offer explicit Save Markings, attaching saved drawings to the
specific game-tree position. Promoting a saved-study variation requires
confirmation and retains the prior main line as an alternative variation; this
cannot modify a running game's played history. Notes use explicit Save; offer
Save, Discard, and Stay before leaving unsaved note edits. Clear temporary
arrows/circles when the selected position changes; saved markings remain attached
to their positions. Confirm deletion of a study variation, clearly identifying
the entire branch and descendants to be removed; never delete running-game
played history through this action.

Include library search/filter/sort, favorites, tags, and folders. Preview detected
duplicate game imports and default to skipping them; allow explicitly keeping
copies and never silently overwrite annotations. Move deleted library games to
Trash with Restore and confirmed permanent deletion, without automatic expiry.
Autosave live games under [the game model](../data-model.md). Search/filter fields
cover players, date, result, variant, opponent mode, favorites, tags, folders,
opening name/code, and game length; annotation-text search is excluded from the
first release. Folders are nested; each game belongs to one folder or Unfiled,
while tags provide multiple groupings. Duplicate candidates match variant,
initial position, played mainline, players, date, and result; preview annotation/
variation differences. Metadata normalization and missing-field semantics follow
[explorer and identity decisions](../decisions.md#book-generation-and-explorer-semantics). Initially sort by most recently updated first. Confirm
folder removal, remove its descendants, and move contained games to Unfiled
without deleting games. Confirm moving an open unfinished game to Trash, then
pause, save, close its tab, and move it to recoverable Trash. Collection size,
restore conflicts and sort-update semantics follow the resolved decisions; backups follow
[the local-backup contract](#local-backups). The opening explorer's
index uses bundled SQLite with the selected schema/index ownership and measured
query acceptance at the final installed corpus scale.

## Local backups

Provide Backup Now and daily automatic backups while the app is open. Schedule
an overdue backup on next launch. Include user data and settings: games/Trash,
annotations, reports, editor drafts, and puzzle history. Exclude dataset files.
Use an app-data backup folder by default and allow changing the destination.
Keep the latest 30 automatic backups and all manual backups. Restore by complete
replacement after creating a safety backup and requiring confirmation; reopen
games paused and leave dataset files external. Backups are unencrypted in the
first release. Automatic backups are due 24 hours after the last successful
automatic backup. When a backup fails but game autosaving still works, continue
play with a persistent warning, Retry Backup, and destination controls. Support
restoration across all supported Windows/macOS/Linux targets, with explicit
machine-path and unavailable-engine-option review; dataset files remain external.
Safety-backup retention, schema migration, first-run/retry scheduling, path/option
review and interrupted-restore journaling follow
[storage decisions](../decisions.md#storage-imports-and-backup-recovery). A failed safety backup cannot satisfy the selected restore
prerequisite; leave current data intact if that prerequisite fails.

The eventual design must verify consistent SQLite snapshots and capture related
user records without mixing revisions. Dataset exclusions require documenting
missing external dependencies on restore; do not claim a dataset-free backup
contains imported dataset files.

## Custom position editor

Allow piece placement/removal and explicit turn/castling-state editing. Validate
the constructed position independently before engine search or live play. Preserve
en passant/counter fields through FEN interchange when provided and valid.

An editor draft may temporarily be invalid. Keep it separate from BoardPosition,
which represents an accepted position. Show actionable validation failures;
never reinterpret an invalid draft as a legal board or call the engine to decide.
Allow saving/reopening explicitly invalid drafts separately from playable
positions; prohibit live play and engine analysis until validation succeeds.

New drafts copy the currently selected board position, using Standard setup if
none is selected. Reopening a saved draft restores that draft instead. Include
Clear Board and Standard Setup, both requiring confirmation before replacing the
current position. Include editor-local Undo/Redo for piece placement and position-
field edits, separately from game takebacks.

Drafts use explicit Save; leaving unsaved edits offers Save, Discard, and Stay,
including invalid drafts. Keep Undo/Redo only in the current editor session;
saved drafts retain the position, not their edit history. Include selecting a
piece then clicking a square, drag/drop from a piece palette or between squares,
and keyboard placement/removal.

Play from Position opens New Game with a validated position prefilled and
requires explicit Start after mode/side/time-control selection. Saved drafts are
available from a Positions section in Library and an editor draft list; clearly
distinguish valid/invalid drafts from recorded games. FEN brought into the editor
creates a new draft, preserving the prior draft and its unsaved-edit prompt.
Reject invalid FEN with an explanation.

Editor shortcuts and promotion input follow presentation decisions. Accept
structurally legal custom studies without requiring historical reachability proof;
reject invalid king/check/right/counter states through the board validator.

## Full-game analysis reports

A report is a job over a particular game/line and a recorded engine configuration.
Each analyzed position has its own search identity and result. Record the engine
package, options, budget, perspective, position identity, and completion state
so results are reproducible and cannot attach to changed moves.

The approved report shows per-move evaluation changes, available stronger
alternatives, costly moves, and an evaluation graph. The owner approved
app-calculated move labels such as inaccuracy/mistake/blunder and an accuracy
percentage. Display search evaluations consistently as White-positive, retaining
the documented engine perspective in raw diagnostics. Positive scores favor White
regardless of the player or the side to move. The evaluation bar is visible in
analysis and review only. Clicking a reported move-quality finding navigates to
its position, shows analyzed alternatives, and offers an analysis branch that
preserves the original game.

Use [the selected report policy](../decisions.md#report-scoring-and-job-scheduling)
for expected-score transformation, exact loss thresholds, versioned accuracy,
mates/bounds/forced moves, partial reports, invalidation and durable job recovery.
This is an application metric, not an engine-native or cross-site accuracy.

Reports start automatically after each game played in the app completes and cover the played
main line. Analyze variations separately on demand. The default search budget
is 3 seconds per position, configurable by the user; depth and node limits also
remain available as alternative search modes. Roughly 80 positions imply around
4 minutes of allotted search, plus processing and scheduling overhead, not a
guaranteed completion time.

Pause reports during live games and resume them afterward. Preserve completed
findings across the pause. Persist completed positions and restart an interrupted
position with its full budget; restore unfinished jobs paused until their
scheduler admits work. Imported completed games
are analyzed only on explicit request; importing them does not queue reports.
Line changes mark old reports outdated and create a new job only on completion
or explicit request; metadata-only edits and reopening do not queue reports.

Do not represent an application-derived accuracy measure as native Reckless Elo
or an official cross-site standard. Report jobs share the approved CPU budget
with gameplay and diagnostics. Live games pause reports. Interactive analysis uses spare capacity; without
live games it preempts FIFO reports. Diagnostics require exclusive explicit
admission; indexing uses spare capacity, and puzzles perform no engine search.

## Opening explorer

The approved explorer maps a selected position to corpus continuations and their
available game/result statistics. The move list, explorer, and study-only book
tabs share the selected board position; switching tabs preserves it. It works from a local corpus once that data is
present. Display corpus identity and distinguish unavailable results from draws.
Show each move's game count, frequency, White/draw/Black results, available average
player ratings, and sample games that can be opened. Mark missing ratings/results
unavailable. Support dataset, year/date, player-rating, time-control, player-name,
and result filters where source metadata exists. Compute White/draw/Black
percentages from known-result games only; show unknown-result count separately.
Count only the first occurrence of each position within each source game. Allow
selecting one dataset or combining selected local datasets, identifying sources.
Deduplicate combined datasets using variant, initial position, mainline, players,
date, and result; count each matching game once and retain all source identities.
Move-frequency denominator is matching games with a recorded next move; show
games ending here separately. Rating filters offer Both Players, White, Black,
and Side to Move, defaulting to Both Players. Exclude games missing required
ratings while that filter is active. Other active filters exclude records missing
required metadata; normalization and partial dates follow the resolved decisions.

Opening game/study imports use PGN. Bundle separate master/tournament and broad
online-game collections, selectable individually or together. Prefer free
public-domain or permissively licensed redistributable data with required
attribution/notices; review each actual source before approval. Dataset updates
and imports are explicit manual actions only in the first release, separate
from app OTA. Sources, deterministic bounded subsets, full position identity,
normalization, opening labels and layout follow the resolved decisions. Do not count a search evaluation as
historical game frequency or imply that the book and corpus are the same dataset.

Indexing must validate legal move sequences and preserve source provenance.
The selected storage design must support the owner's approved corpus scale;
do not commit to scanning every PGN file for each position query.

## Opening book

The approved book exposes prepared legal moves for a position. Reckless v0.9.0
has no book option; the application owns this facility outside its UCI adapter.

The owner selected opening study only: the book never supplies opponent moves.
Keep prepared study moves distinct from engine search results. Never send a
fabricated book command to the engine.

Book imports support Polyglot .bin. Show raw weights and normalized percentage
shares within the selected book position. These are relative book preferences,
not engine evaluations or winning probabilities. Initially order legal moves by
descending weight. If all legal weights are zero, keep moves/raw zeros visible
and mark normalized shares unavailable. Do not invent equal shares.

In analysis/study, selecting an explorer/book continuation navigates an existing
matching variation or adds a study variation if needed. Preserve the main line;
this action never submits a live-game move.

The generated starter book uses only the approved master/tournament collection,
weighted by observed move frequency, through the first 20 full moves (40 plies)
from the standard starting position. Both corpus collections remain available
in the explorer. Preserve fitting counts; scale overflowing positions to maximum
65535 with nearest rounding and minimum 1. Include every observed legal move,
from standard-initial-position games only. Display one-decimal shares, tie-break
by canonical UCI, and support Standard-only Polyglot imports. Prepared moves are not engine principal variations or calibrated
difficulty controls.

## Puzzles

The approved basic experience loads a valid tactical position, accepts attempted
moves, follows a defined accepted solution tree, and gives progression/completion
feedback. Validate starting positions and every solution continuation before use.

The owner approved theme/difficulty filters, hints, retries, solution review, saved
progress, and both whole-session and per-puzzle countdown modes. Their configurable
defaults are 5 minutes per session and 60 seconds per puzzle. Select exactly one
countdown mode per timed run. Allow manual pause and automatically pause when
the app loses focus; hide the puzzle board and solution while paused. Preserve
the paused run and remaining time across computer sleep and application relaunch.
Always require explicit Resume, including after focus returns; never resume the
countdown merely because the window regained focus. These puzzle pause rules
do not alter live chess clocks. Save each input/transition and active remaining time once per second; a crash
restores the last durable checkpoint paused. Save failure pauses with Retry Save.

After solving a puzzle, show solution review and wait for an explicit Next
action. Do not charge the session countdown during this between-puzzle review.
Review is a separate phase from a manually/focus-paused active puzzle: its
solution remains visible. Start a new puzzle's countdown when its board is ready,
visible, and interactive, including after Next. Per-puzzle timeout advances when the next board is ready; its solution remains
available from history. Session timeout shows the final summary.

End a run when its session countdown expires. Record an otherwise unresolved
current puzzle as timed out; preserve an already-failed primary outcome and
record session expiry separately. When a per-puzzle countdown expires,
record that puzzle as timed out and advance. On collection exhaustion show
progress and offer explicit Reshuffle and Continue; never reshuffle automatically.
Stop the session countdown on this exhaustion progress screen. Only explicit
Reshuffle and Continue carries remaining duration forward; never reset the
session duration. Follow the selected timeout and summary transitions.

Hints progress from highlighting the source square to the destination square,
then revealing the next solution move. Record hint use as assisted and keep the
existing countdown running while hints are shown. Restart hint stages at the
source square for each new player move. Any hint use keeps the whole attempt
marked assisted.

When a submitted move is not accepted by the selected solution policy, record
the failed attempt and allow practice retries while the existing countdown
continues. Preserve the recorded failure if practice later completes: its primary
outcome remains failed. Show practice completion and hint use separately; keep
primary outcome totals non-overlapping. The owner selected counts/rates without
a points system. Divide each unassisted/assisted solve count by all finalized
puzzles, including failures, timeouts, and skips; exclude active unfinished
puzzles. Zero finalized puzzles show an unavailable rate.

Accept dataset-listed solution lines. Label other legal moves as unlisted;
absence from the dataset is not proof that a move loses. Do not launch an engine
search to verify alternative solutions under this selected policy. Distinguish
illegal input from a legal unlisted move. Reject illegal input without recording
a failed attempt and keep the countdown running. Include Skip: advance without
revealing the solution, record a separate skipped outcome, and preserve remaining
session time. Skipping a puzzle already recorded as failed must preserve its
failed primary outcome; show a secondary skip event.
Randomly choose among dataset-listed
opponent replies when multiple accepted branches exist. Persist shuffle order
and chosen reply decisions from the injected seedable random source.

Personal puzzle imports use the Lichess-compatible CSV schema, including its
opponent setup move and optional DailyDate, under
[puzzle decisions](../decisions.md#puzzle-interchange-and-lifecycle). Sources
and size selection follow the deterministic CC0 starter-data policy.

Shuffle puzzles matching the selected theme/difficulty filters without repeats
until that filtered collection is exhausted. Preserve the current ordering,
visited puzzle identities, accepted moves, assistance/failure records, and remaining
time when restoring a paused run so resume cannot restart its shuffle or timer.
Filter changes apply only to the next run; keep the current run's collection
unchanged. At exhaustion show progress and require explicit Reshuffle and
Continue before repeating the collection. The session countdown stops on this
screen and continues with its remaining duration only after explicit continuation.
Dataset versions remain immutable and pinned by active runs; replacement stages
a new version and switches after validation/preview/confirmation.

Save per-puzzle attempt history, outcomes, hint assistance, elapsed time, and
session summaries locally. Keep them indefinitely until explicit deletion.
Summaries show counts/rates for unassisted solves, assisted solves, failures,
timeouts, skips, and time spent; no points system. Time spent is active countdown
time, including automatic replies, move animations, hints, and practice retries;
exclude pauses and between-puzzle review. Persist typed events/snapshots under
the resolved lifecycle and storage policies. Include
confirmed deletion of an individual attempt, a session with its attempts, or all
puzzle history; preserve puzzle datasets. Recompute affected summaries from
retained records so deleted attempts cannot remain in aggregate totals. This
does not add a changing player rating.
Difficulty filtering does not approve a changing player rating, achievements,
or a network service.

Puzzle attempts use their own session identity and do not rewrite a saved game.
Do not label an unfamiliar but legal alternative as losing based solely on a
missing engine search; the selected solution/validation policy owns that decision.

## Data provisioning and integrity

Bundle starter datasets for opening exploration, opening-book study, and puzzles,
and support importing personal files. The owner approved an installed starter-data
budget of up to 2 GB, excluding the application and engines. This is a research
budget, not a promised collection size. Approved personal imports are PGN opening
games/studies, Polyglot .bin books, and puzzle CSV. Providers, exact datasets,
CSV schema and exact package composition are selected in
[dataset decisions](../decisions.md#starter-datasets-and-provenance); freeze
actual source hashes and measured counts during preparation. Starter opening/puzzle datasets cover Standard chess only; show
clearly when Chess960/DFRC have no matching data. Starter puzzles cover a broad
available difficulty range with difficulty/theme filters, without a player-rating
system. Generate the bundled study-only Polyglot book from the approved opening
master/tournament collection, with observed move-frequency weights, through the
first 20 full moves (40 plies) from the standard starting position. Integer
scaling, rare-move inclusion and custom-start handling follow the selected book
policy (unchanged fitting counts, proportional overflow, all observed legal moves,
standard-initial-position games only). It never
chooses Reckless opponent moves. Use separate master/tournament and broad online-game collections,
selectable individually or together. Prefer free public-domain or permissively
licensed redistribution with required attribution/notices, subject to actual
source review. Dataset updates/imports are explicit manual actions only; no
automatic dataset update or first-launch download is approved.

A DatasetPackage records source, version, applicable terms, format,
checksum, local path/index, and validation status. Failed acquisition/import must
not replace a working collection. Dataset updates and large indexing jobs need
bounded processing, cancellation, and capacity accounting in the eventual design.

Once provisioned, the approved study and training features work offline. Missing
optional data gets a truthful setup state, not fabricated statistics or puzzles.

## Selected interface extension points

Extend the generated Rust-owned IPC schema with the selected operation families:
editor draft/validate/open, annotation
and variation edits, report start/cancel/read, opening corpus import/query,
book inspect/configure, and puzzle collection/session/attempt.

Concrete operation families and ID/revision/preview contracts are selected in
[interface completion](../decisions.md#supporting-libraries-and-interface-completion).
Generate schemas from Rust-owned DTOs during implementation. Requests and events retain the existing ID/revision/cancellation model.

## Acceptance evidence

- Library/notation: reopen and round-trip annotated games with variation trees.
- Editor: valid position/FEN round trips and specific invalid-position rejection.
- Reports: source-position/configuration correlation, correct per-move search
  association, cancellation, and an explicitly incomplete state for missing work.
- Explorer: exact continuation/result counts on a fixed known corpus.
- Book: only legal prepared moves and clear origin, and no book-supplied opponent moves.
- Puzzles: known accepted/unlisted/illegal attempts, assisted hints in the
  approved sequence, preserved failure records through timed practice retries,
  completion, and malformed-data rejection. Do not claim unlisted moves lose or
  verify them with engine searches. Exercise each timer mode independently; manual and focus-loss pauses
  hide the board/solution and preserve remaining time. Session expiry ends the
  run; per-puzzle expiry records a timeout and advances.
- Data: approved offline provisioning path, provenance, failed update recovery,
  and indexing measurements at the owner's selected corpus size.
