# Resolved Implementation Decisions

**Date**: 2026-10-16

The owner delegated remaining choices to the assistant: "for every question from
now, pick ur own recommendation and best pick". This document resolves the
previous questionnaires and design placeholders. Explicit earlier owner choices
remain authoritative. These are selected requirements and implementation
strategies, not results of runtime tests. Engineering rules remain in
[the canonical catalog](../../.specify/rules/README.md).

Later direct owner instructions take precedence over earlier selections. These
delegated decisions fill details within the approved specification; they cannot
remove its scope. The plan and contracts implement those decisions. Historical
task checkmarks describe their recorded work, never authority to override the
current requirements or evidence of native acceptance. Resolve a conflict at
its authoritative owner before reconciling dependent tasks.

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
Native installation, GPU/window compatibility, dead-position adjudication, private
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
These are upper selection targets, not promised available counts. Selection
uses the following versioned preparation policy, chosen under owner delegation.

### Starter subset selection v1

ASCII whitespace in this policy is exactly space, tab, LF, form feed and CR
(bytes 20, 09, 0A, 0C and 0D in hexadecimal), matching Rust `trim_ascii` and
`split_ascii_whitespace`. Do not trim or split additional Unicode characters.

Validate and normalize candidates before selection. Deduplicate games using the
approved full game identity, retaining every source identity and its metadata.
If copies differ in nonidentity metadata, use the copy with lexicographically
smallest original UTF-8 source-record bytes for classification; retain the other
copies as provenance. Puzzle identity is the source's PuzzleId. Conflicting
records with the same PuzzleId fail preparation rather than choose a solution
silently; byte-identical copies count once.

For broad games, form one flat stratum per observed pair of rating band and time
class. Both normalized WhiteElo and BlackElo must be available unsigned 32-bit
integers to assign a known rating band. Compare their checked 64-bit sum with
2400, 3200, 4000 and 4800: the resulting exact arithmetic mean belongs to
<1200, [1200,1600), [1600,2000), [2000,2400) or >=2400 respectively. Do not round
the mean before comparison. A missing or invalid rating on either side assigns
the typed Unknown band, with original metadata and the quality reason retained.
This preparation rule does not change explorer rating averages or filters.

Time-class parsing trims outer ASCII whitespace and consumes the entire tag.
Recognize `1 day per move` or `N days per move` for unsigned decimal N >=2
within u32 as Correspondence. Otherwise accept ASCII decimal `B` or `B+I`,
with u32 base/increment seconds, no internal whitespace and at least one positive
operand; plain B means I=0. Compute checked u64 E=B+40*I. Assign Bullet for
E<180, Blitz for 180<=E<480, Rapid for 480<=E<1500 and Classical for E>=1500;
UltraBullet is included in Bullet. Missing/blank/?/- tags are Unknown unavailable.
Malformed, overflowing, multi-stage, slash or hourglass tags are Unknown with
their raw value and reason retained. Do not infer Correspondence from Event,
missing clocks or an unusually long real-time control. These are application
preparation classes, not a claim to reproduce every Lichess internal speed enum.

For puzzles, a known unsigned 32-bit rating belongs to band k=floor(Rating/250),
covering [250*k,min(250*k+249,u32::MAX)] with endpoints calculated in checked u64;
missing/invalid optional ratings use Unknown.
Split Themes on ASCII whitespace, deduplicate exact case-sensitive tokens and
choose the smallest UTF-8 bytewise token as the preparation-only primary theme.
An empty theme list uses typed Unknown, distinct from any literal theme token.
Retain all original themes for filtering; the selected primary theme conveys no
chess importance. Each observed (rating band, primary theme) pair is one stratum.

Order game strata by the five known rating bands ascending, then Unknown; within
each band order Bullet, Blitz, Rapid, Classical, Correspondence, Unknown. Order
puzzle strata by known numeric band ascending, then Unknown; within each band
order known theme tokens bytewise, then Unknown. Allocate equally across these
flat observed pairs, without a second nested quota. Within each stratum order
candidates by ascending SHA-256 digest bytes of the selection identity defined
below, then those identity bytes to break a digest collision. Build one selection
sequence by repeatedly taking the next candidate from each nonexhausted stratum
in that fixed order. Skip exhausted strata and stop at the target or exhaustion;
a partial final cycle gives remaining slots to earlier keys. The master subset
uses its title filter and one unstratified sequence in the same hash order.
Every smaller requested count takes a prefix of its subset's sequence. Source
iteration order, parallel completion order and a random seed cannot change it.

Selection identity bytes are separate from retained provenance/source IDs.
For games, serialize the approved normalized full game identity as the compact
UTF-8 JSON array `[variant, initial_fen, mainline_uci, white, black, date, result]`
using Serde JSON escaping and no whitespace outside strings. Variant and result
use their existing canonical external strings; initial_fen and ordered mainline
move strings use canonical notation. Player strings use the existing NFC,
trimmed, case-folded identity normalization; absent values use JSON null. Date
is `[year, month, day]` with null for each unknown component. For puzzles,
selection identity is the exact nonempty UTF-8 PuzzleId string, without JSON
quotes. Hash each candidate independently; never concatenate unframed identities.
One preparation identity encoder owns both branches: typed tuple JSON through
`serde_json::to_vec` for games and raw PuzzleId UTF-8 bytes for puzzles. It uses
no maps, floating-point values or formatting.
Fixture bytes and digests pin this encoding as part of policy version 1.

Record policy version 1, source hashes, available/unknown/rejected counts,
ordered stratum counts, selected prefix lengths and selection-sequence digests
in the preparation manifest. Retain selected identities/strata in the dataset
provenance so selection can be independently reconstructed from frozen inputs.
The selection-sequence digest hashes an unsigned 64-bit big-endian record count,
followed by each selected identity's unsigned 64-bit big-endian byte length and
identity bytes in selection order. Length/count conversion is checked.

The hard installed starter-data cap is 2,000,000,000 bytes including indexes,
source-game storage, book, puzzle data, manifests, and labels. Component ceilings:
800 MB master, 800 MB broad, 300 MB puzzles, 50 MB book, 50 MB labels/manifests.
Use decimal MB. Measure actual installed file lengths, including every index
and provenance file, assigning each file to exactly one component. Inspect
overflows in master, broad, puzzles, book, labels/manifests order. On the first
reducible overflow, shorten that subset's prefix from n to floor(9*n/10) and
rebuild affected artifacts before rechecking. Book overflow shortens the master
prefix and rebuilds both master data and its derived book. A labels/manifests
overflow, unclassified file, empty required subset or fixed overhead that cannot
fit fails preparation explicitly; do not discard required provenance or silently
ship an empty collection. This bounded prefix reduction need not maximize record
count. Record each reduction and final component/total bytes; every ceiling and
the total must pass. Do not pad collections or count compressed
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
pause, review, and session transition transactionally. While active, request a
remaining-time checkpoint every 250 ms, keeping at most one pending commit and
coalescing newer countdown snapshots; crash recovery restores the last durable
checkpoint paused, with at most one second of unsaved countdown lost. A failed
puzzle checkpoint pauses the run and exposes Retry Save. Bound durable lag,
not merely the checkpoint scheduling interval: active countdown cannot advance
more than one second beyond the last committed checkpoint. If the next commit
is still pending at that boundary, freeze the run in a saving/interrupted state;
failure exposes Retry Save and recovery still requires explicit Resume.
Per-puzzle timeout
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

### Native engine capacity partition

The default application budget remains all detected usable hardware threads C.
Reservations describe runnable CPU work, not every allocated sleeping I/O or
notification thread. Bevy pools must be configured before their global creation;
stock pool defaults plus independent full-size engines/Rayon are not selected.

Choose a **phase-based capacity policy**, retaining the four-thread baseline.
The normal multithreaded Bevy topology needs at least one worker in each IO,
async-compute and compute pool; setting all pool minima to zero is unsafe.
Stock hierarchy propagation assumes a nonzero compute worker and unscoped
asset/pipeline tasks need workers that actually poll. Disable pipelined rendering
in the first native baseline to avoid its extra render thread; rendering remains
on the native host thread. Reconsider only with measured shared-budget evidence.

During preparation/recovery, stop and acknowledge current engine searches and
drain conflicting Rayon leases before admitting heavy Bevy asset/pipeline work.
Preload all piece/font/audio assets and approved quality-tier pipelines before
live search admission. Re-enable searches only after bounded preparation jobs
complete and the measured/render resource state is ready. No detached asset
preparation or hot-reload activity may begin outside this capacity phase.

During steady live play reserve the host/render work and one compute worker;
IO/async-compute pools are quiescent after prewarm rather than counted as
permanently busy CPUs. Their allocated threads remain owned and observable.
The remaining runnable budget is shared by engines and admitted Rayon work
through the coordinator. Divide active engine shares equally within that budget,
not C independently of app work. A newly required heavyweight asset/pipeline job
first enters the acknowledged preparation transition; it cannot independently
consume engine-reserved capacity. Small control/I/O work is bounded separately;
CPU-intensive storage/import/decoding goes through admission.

Use explicit TaskPoolOptions policies: IO one native worker, async-compute one,
and compute one for the initial steady rendering topology. These are supported
framework minima/dependency reservations, not a cap on all app computation.
Parallel imports/reports/proofs use the existing executor and all admitted spare
capacity. During asset preparation the framework's fixed worker topology may
limit its own scaling; measure this instead of claiming all-core asset decode.
Never lend a slot to an engine before conflicting work is quiescent, or start
conflicting work before the old engine allocation is stopped/reaped as required.

The first native migration gate must prove quiescence/admission, bounded wakeup,
minimum C=4 and maximum-capacity behavior, frame/stop latency and actual pool
counts. If uncontrolled upstream work prevents the contract, leave the gate
failed and repair scheduling; do not report the budget satisfied or raise the
minimum platform silently. This policy is a concrete integration strategy,
not a measured scheduler implementation or permission to oversubscribe.

## Local profile ownership and activation

Select one writable owner for the existing `nl.sadra.chess` local data. This is
startup coordination, not a profile picker or multi-user feature. Resolve the
existing data directory explicitly, canonicalize aliases to the same directory
identity, and keep a permanent `control/owner.lock` outside replaceable database
and settings files. User backups/restore do not copy or replace the control
directory, its destination-local profile identity or activation records. Do not
silently relocate the profile through directories-crate naming defaults.

Bootstrap opens the stable lock file without truncation and calls
`std::fs::File::try_lock`. A root-owned, un-cloned `DesktopProfileLease` retains
the exclusive handle. Never unlink/recreate that file, explicitly unlock early
or pass the handle to an engine/installer child. The control directory is
user-private and validated before use; unsupported locking, unsafe permissions
or ambiguous directory identity refuse startup with a typed recovery status.
All application/helper writers cooperate with this OS-backed exclusion.

On contention, the new process reads only bounded activation metadata and
attempts activation. It creates no storage actor, migrations, restore/backup
jobs, games, engine leases or Bevy worker pools. An unavailable endpoint or
response timeout means Busy, never permission to steal the lock. After a prior
owner exits, exclusive acquisition permits normal stale-record cleanup and
saved-state recovery, leaving unfinished games and puzzle runs paused. A
diagnostic PID is not an ownership credential or process-identity proof.

Before ordinary initialization, each successful lock holder checks the durable
installation intent. Pending or unresolved installation enters the
[installer handoff policy](contracts/packaging.md#profile-ownership-during-installation)
instead of opening data. Otherwise the owner creates a fresh instance generation,
binds the activation endpoint and atomically publishes a versioned owner record.
Only then open storage, run migrations/restore-journal recovery and construct
application, engine and presentation services. Accept activation while starting
and coalesce it into one request to show the main window when ready.

Use interprocess 2.4.5 with `tokio` on the single owned runtime. Unix uses its
generic local-socket API and an explicit filesystem socket in a mode-0700
user-owned directory, with peer
effective-user validation on both ends and mode 0600 where supported. macOS
must not fail solely because the optional pre-bind socket-mode operation is
Unsupported. Keep the socket path within native length limits using a short
user-private runtime directory when necessary; its owner record still binds
the canonical profile/generation. Windows uses a local named pipe with an
explicit current-user/SYSTEM DACL and remote clients disabled. Name reclamation
is disabled on Unix; only the guard holder cleans its own stale endpoint. Reuse
sysinfo 0.39.6 for the actual Unix effective user. Windows-only winsafe 0.0.29
(`advapi`) obtains the current token SID through safe fallible APIs; widestring
1.0.2 supplies the security-descriptor input. Inability to establish private
authenticated transport produces a typed startup failure,
not an unauthenticated fallback. Same-user administrator/malicious-process
isolation is not claimed by a desktop activation endpoint.
Authenticate the Windows server reciprocally: query the connected stream's
kernel peer PID, open that process with limited-query access and compare its
token User SID with the current SID through Winsafe. Fail closed if this cannot
be established. The kernel peer PID locates a token, not profile ownership;
this closes an owner-exit/pipe-replacement race after lock contention.

The Windows adapter uses concrete `PipeListenerOptions` with an explicit
descriptor, `accept_remote(false)`, `inheritable(false)` and five instances:
four accepted clients plus one pending listener. Acquire a handler-capacity
permit before calling/polling accept, counting connected-but-not-yet-dispatched
streams; release it on every completion/failure/cancellation. The library creates
the replacement pending instance before returning an accepted stream. Use
`DuplexPipeStream<Bytes>::connect_by_path_with_wait_mode` with zero timeout;
only the application schedules bounded async retry. Do not use the generic
Windows local-socket connect path. An owned wrapper suppresses dirty-stream
linger with safe `assume_flushed()` before every drop, including cancellation,
and never starts an underlying pipe flush. The version-2 exchange adds a
ReceiptAcknowledged frame: the owner reads the client's matching receipt before
acknowledging it, then retains the stream until the client reads that acknowledgment
and closes. This proves consumption of both final frames before either endpoint
discards bytes, without a pipe write half-close or linger worker. All four steps
share the original absolute deadlines; version 1 is rejected before activation.
Timeout discards incomplete bytes without spawning linger workers.

The [activation contract](contracts/desktop-ipc.md#local-activation-interface)
owns versioned framing and closed operations. Select a 4 KiB maximum frame,
one activation/response/receipt/acknowledgment exchange per connection, at most four in-flight
clients and one coalesced pending window request. The second-launch attempt has a 2-second total deadline,
including at most four metadata/connect trials while the owner starts; each
accepted connection has a 1-second read/write deadline. Reject oversized,
malformed, wrong-profile, stale-generation or unsupported-version requests.
No command, path, credential or arbitrary CLI argument is forwarded. There is
no polling heartbeat when idle and no automatic reconnect loop.

Activation only requests the existing window. The native-thread adapter makes
it visible, unminimizes/focuses where supported, and requests informational
attention through winit 0.30.13 on Wayland, using the same resolved winit instance
as Bevy and its public native-window access. Report Focused only after
observed native focus; an OS-denied/unconfirmed request explains how to select
the existing window. Activation never resumes paused games/puzzles or dismisses
confirmation/recovery state. A canceled quit or failed save retains the lease.
Normal shutdown stops admission, drains/observes jobs, reaps children, closes
storage and shuts down owned runtimes before final lease release at process
teardown. This guard must not be an independently dropped Bevy resource.

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
Crash recovery loses zero acknowledged committed revisions and restores game
state and clocks from the same latest durable revision, paused and without
downtime charges. Accepted but uncommitted changes may be lost; never label
them Saved. Migration and restore preserve either the complete last valid
snapshot or the complete validated replacement, with no mixed data/settings
versions. A backup guarantees its recorded consistent revision, not later edits.

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
use a recoverable same-volume rename journal. Retain the
[profile lease](#local-profile-ownership-and-activation) throughout staging,
replacement and recovery; replace database/settings files without replacing
their control-directory anchor. Keep the current database until
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
safety deadline, 3-second shutdown and p95 stop target of 1 second. Native-message mutation
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

Initial window/reset size is 1440x900 native logical points, clamped to the usable display
and the selected 1280x800 minimum; tell the user if a display cannot fit it.
Navigation widths are 208 expanded/64 collapsed; side panel defaults to 360,
range 300..560; reserve at least 560 native logical points for the board workspace.
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
visible orientation compass disclose this. Drag threshold 5 native logical points; cancel
on Escape, pointer loss, off-board release, phase/revision change, or dialog.
An accepted move cancels any obsolete animation without delaying the next input.

High uses 2048 shadow map, antialiasing, one shadowed key light plus fill, DPR
cap 2; Medium uses 1024 shadows and DPR cap 1.5; Low disables shadows and
extra effects, DPR cap 1.25. No expensive postprocessing by default. Start Auto
at Medium. Sample active frame times over 2 seconds; degrade after 3 consecutive
windows above 20 ms p95, upgrade after 10 windows below 13 ms, with a 30-second
cooldown. Reduce shadows/effects through tiers first, then render scale in 0.1
steps down to 0.75; recover resolution before effects. Ignore idle/background
and loading windows. Manual tiers disable adaptation. GPU baseline: hardware rendering through the selected native backend, max
texture dimension at least 4096, working instancing and depth targets. Native
Metal/macOS and supported Vulkan/Direct3D on other targets are engine-selected;
there is no forced Vulkan portability path. Validate integrated Intel UHD 620-class Windows/Linux hardware
and Apple M1-class macOS rather than inferring speed from feature support.
Minimum 8 GiB system RAM, 4 logical CPU threads and 6 GB free installation disk,
plus staging/backup/import space; these are selected validation baselines.

Context loss, initialization failure, shader failure and unrecoverable asset
load errors produce a native interruption of all renderer-dependent live games
and an explicit Retry screen. Retry has a 10-second initialization deadline;
no automatic game resume. Failed initial launch shows diagnostic/setup guidance,
not an empty playable board. Hidden windows stop scene animation; rendering
failure cannot convert clocks into presentation-owned state.

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

## Acceptance measurement protocol

This section defines SC-003, SC-004, SC-005 and SC-009 measurements. These are
acceptance requirements, not recorded benchmark results. Use the declared
Apple M1-class baseline on macOS ARM64 and Intel UHD 620-class x86-64 baselines
on Windows and Linux; test both X11 and Wayland on Linux. Record exact CPU/GPU, usable thread count,
RAM, OS, driver, display refresh/scale, power mode, compiler, lockfile, asset
and fixture hashes, quality settings, engine configuration and capacity leases.
Use optimized packaged builds with no test-only renderer or prelaunch helper
prewarming. Record disk-cache state; a fresh process does not imply a cold OS
cache. If required timing instrumentation is unavailable, report unverified.

Run each workload five times and report every run. For accepted-move feedback,
rejected-input feedback and engine stop, collect at least 200 samples per
applicable workload per run. Sort samples and use the nearest-rank p95 at index
ceil(0.95 * sample_count), counting from one. Timeouts and slow samples remain
failures or samples, never omitted successes. Do not average separate runs or
platforms into a passing percentile. Reference fixtures include both themes,
supported display scales/orientations, camera bounds, legal/illegal input and
promotion, plus one maximum-capacity engine search, multiple admitted engine
games, and search with spare-capacity indexing. Live-game workloads pause reports
as selected; a separate no-live-game workload covers report/interactive-analysis
scheduling. Exercise all applicable fixtures within the admitted budget.

Move feedback starts at native receipt of the final click, square activation,
drag release or promotion choice that submits the move, excluding the player's
promotion deliberation. It ends at the first actually presented frame showing
the accepted authoritative move or the beginning of its configured animation.
A drag preview, selection highlight or input acknowledgment alone does not
satisfy this criterion. Measure explicit rejected-input feedback separately;
both p95 limits are 100 ms. Disk-save and animation completion are separate
measurements. Use correlated native presentation timestamps or equivalent
display instrumentation, not CPU submission time as proof that feedback
appeared. A local game's legal moves can supply input samples while another
admitted engine game consumes search capacity.

For frame pacing, use a nominal 60 Hz display and Automatic quality. Allow at
most 30 seconds of reported stabilization, then capture at least 60 seconds of
continuous prescribed board/camera/move activity per run. Measure actual
presentation-to-presentation intervals; include stalls and missed refreshes
while work requires frames. Each run's p95 must be at most 16.7 ms. Exclude only
intentional idle/background intervals, not slow active frames. Record quality
changes, render scale and GPU/CPU timing separately; CPU frame work alone does
not establish presentation pacing. Manual tiers are reviewed separately and
cannot substitute for Automatic acceptance.

For offline launch, the harness records monotonic time immediately before
spawning the installed executable against a new local-data fixture with bundled
starter indexes already installed. Submit default New Game/Start immediately
when enabled, without human reaction time. Stop at the first presented,
interactive game board with correct clocks, loaded assets and required engine
readiness/admission. Ownership, schema initialization, resource/pipeline
preparation and necessary local-data readiness all count. Each of five fresh
process launches must finish within three seconds. Do not exclude a first-run
index build or perform mandatory startup work before the start timestamp.
Record supported existing-data upgrade timing separately; OS installer prompts,
explicit recovery and user-requested migrations are outside this clean-launch
fixture, never silently discarded samples from it.

Engine-stop timing starts at application-port receipt of Stop, before queue
admission, and ends
after consuming the matching final bestmove/completion and reaching idle with
that search invalidated. A request acknowledgment or readyok is not completion.
The normal p95 target is one second; the separate two-second safety deadline
starts termination/reaping on noncompletion and records a failed/interrupted
search. Never call forced termination a successful normal stop. The independent
1,000 controlled stop/change/restart cycles require zero stale mutations even
when timing fails; no new lease is granted before the old work releases it.

For idle CPU, use an unchanged foreground workspace with no live games,
searches, reports, imports, countdowns, animations, transfers or due backups;
settle for 10 seconds, then take five independent 60-second intervals. Include
the main process and every owned helper/child, all worker threads and render
activity. CPU percent is 100 * summed user-and-kernel CPU seconds / elapsed
monotonic seconds, using one logical core as denominator without dividing by
machine capacity. Every interval must be below 1%; report wakeups and rendered
frames too. Record compositor/GPU activity separately rather than hiding app
CPU work in a presentation-only sample.

Compare one assigned engine/CPU-work worker with the maximum admitted
allocation on identical fixed inputs; native host/framework reservations remain
in force in both cases. Report throughput, latency, peak application/child/hash
memory and GPU allocations separately. Correctness, preserved state and budget
admission must match in both configurations; more threads are not proof of
better scaling.

## Supporting libraries and interface completion

**Native revision**: Select Bevy 0.20.0 + bevy_egui 0.43.1 under owner delegation
after the owner requested help choosing and could not select. It replaces the
previous eframe proposal, preserving egui and all product behavior. Native API
selection remains automatic through Bevy/wgpu, including Metal on macOS and
Vulkan/Direct3D where supported. No forced Vulkan or Glow/three-d path remains.
[Renderer research](research.md#native-egui-renderer-and-integration) owns exact
compatibility, alternatives, source findings and native acceptance limits.

Retain Rust 1.99.0 and existing domain/clock/storage/engine/capacity components.
Share native Rust command/event types between UI and application. Serde and
schemars remain for external/persisted files, without generated TypeScript or
Ajv on an in-process UI boundary. Replace Tauri runtime and service ownership
with explicit root-owned runtime/OS adapters. Current native dependency options,
source/API evidence and test limits live in [research](research.md#final-resolution-research).
Profile locking uses the pinned standard library; local activation and native
security adapters follow [ownership research](research.md#local-profile-ownership-and-activation).

Use bounded application actors, the existing shared CPU executor, reqwest private
streaming and keyring credential adapters. Linux without a credential service
shows setup status and permits explicit session-only credentials, never silent
plaintext persistence. Secrets stay outside general UI snapshots/logs/exports/
backups. Native logical points preserve the selected window/layout sizes.

Matching egui_kittest 0.36.0 semantic/input tests, Bevy adapter tests, native
Bevy screenshots, OS lifecycle and real screen-reader checks replace browser
tests after migration. Enable plugin AccessKit and Linux platform support;
explicitly bridge native accessibility actions into egui. A native migration
acceptance slice must prove lifecycle, GPU-init/recovery and shared task-pool
integration before those facilities are accepted as implemented. Existing Makefile commands still execute the former implementation;
change their definitions only during verified migration.

Extend typed desktop operation families for library organization/imports,
editor drafts, notes/variations/markings, reports, corpora/books, puzzles,
backups, rendering interruption/retry and update credential/check/download/install
state. Mutations carry expected entity revisions. Preview/apply pairs retain
immutable preview identity and digest. [Desktop messages](contracts/desktop-ipc.md)
own complete envelopes, delivery bounds and command semantics.

## Updater completion and zero-cost CI

Use the native updater composition in [packaging](contracts/packaging.md#desktop-ota-and-github-actions):
cargo-packager 0.11.8 installers, reqwest 0.13.5 async transport, keyring 4.2.0,
minisign-verify 0.3.0 signed-manifest verification and minisign 0.10.0 release
tooling. No Tauri updater runtime remains in the final app.
Authenticated GitHub API discovery maps manifests and target assets to verified
release asset IDs; reject arbitrary manifest origins and never forward tokens
to a different origin on redirect. Follow signed asset redirects without the
GitHub Authorization header. Check version, target, length limit, digest and
signed-manifest payload binding. HTTPS only, no downgrade, no prerelease channel. Release tags
must match Cargo/package/signed-manifest versions. Download to a bounded private staging file,
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
| FR-004 | Study tools, storage and profile ownership; library/interchange/Trash, data continuity, exclusive startup and activation without competing writers |
| FR-005, FR-006 | Capability inventory, workbench and packaging; every pinned usable operation/profile and disclosed unsupported facilities |
| FR-007, FR-008, FR-009, FR-010 | Engine protocol, desktop messages, capacity and report scheduler; scores, limits, correlation, bounded cancellation and shared CPU/hash |
| FR-011, FR-012 | UI and presentation decisions; premium 3D, motion/camera/quality, keyboard/accessibility and actual frame/input measurements |
| FR-013, FR-014, FR-015, FR-016 | Packaging, storage and lifecycle; target installers, offline use, durable recovery, notices/identity and no account/telemetry requirement |
| FR-017 | Editor and board model; structural validation, invalid drafts, controls, FEN and explicit start |
| FR-018 | Study tools and report decisions; versioned scoring, exact source correlation, jobs, partial/outdated states and review |
| FR-019, FR-020 | Explorer/book and dataset decisions; exact statistics, filtering/deduplication, source identity, legal Polyglot and study-only behavior |
| FR-021, FR-022 | Puzzle/study and provenance decisions; CSV/setup move, listed solutions, hints, timers, persistence, full offline extras and no silent scope reduction |
| FR-023, FR-024 | Native UI/messages/presentation; settings apply/import/reset, palette, remapping, audio lifecycle and safe resource controls |
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
| Desktop | Minimum OS/GPU/window compatibility, accessibility, context recovery, sleep, precise clocks, native installer and manual OS approval on actual targets |
| Profile ownership | Simultaneous launch exclusion, authenticated bounded activation, unresponsive owner refusal, stable lock through restore/quit and gap-free installer intent; actual Windows/macOS/X11/Wayland focus behavior |
| Updater | Pinned-source tests of private discovery/redirects/token handling, stable signatures, cache/timing/network-cost states, .deb handoff, interrupted migration/backup/install recovery |
| CI and budget | Actual free allowance and hard-stop controls, native ARM Mac runner availability within allowance, storage/asset-limit checks; block workflow activation if not established |
| Engine licensing | Preserve AGPL/Fathom notices and model provenance; no external conveyance without resolving the separate NNUE model-license issue |

Bevy + egui resolves the scene-engine choice under owner delegation. Gates in
this table remain implementation/release obligations, not unresolved preferences.
Failure of a gate cannot silently remove an approved feature or justify a claim
that the application has been tested. The reconciled [implementation tasks](tasks.md)
allocate this native design and its evidence gates. Partial implementation exists;
no native migration or release readiness is claimed.
