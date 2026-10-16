# Engine Process and UCI Contract

**Status**: Design selected under owner delegation; implementation and release evidence remain unperformed.

Use [the resolved decisions](../decisions.md) for concrete defaults, algorithms
and former draft questions; [the review](../review.md) records owner choices.

Applies to the [pinned capability inventory](reckless-capabilities.md).
The Rust engine actor is the sole owner of the child and all pipe traffic.

## Startup and configuration

1. Resolve an app-owned package by target, checksums, CPU/OS requirements, and
   build capabilities. Launch its exact executable directly, never through a shell.
2. Start bounded stdout and stderr readers before writing commands. Send `uci`;
   require identity, recognized option descriptors, and `uciok` within 5 seconds.
3. Compare package and discovered capabilities; fail visibly on a pin mismatch.
4. Apply validated configuration while idle, send `isready`, and require
   `readyok` within 5 seconds. Configuration operations have one serialized owner.
5. For a new game send `ucinewgame`, apply variant settings, establish readiness,
   then send its validated position/history before search.

Numerical ranges come from discovered options, further constrained by host
capacity and memory. Do not trust upstream numeric parsing or allocation checks.
Use complete typed `setoption` formatting, including button options without value.

## Session transitions

| State | Allowed operation | Completion |
| --- | --- | --- |
| Stopped | Launch | Starting |
| Starting | Handshake/configure | Idle or Failed |
| Idle | Configure/new game/set position | Idle after required readiness barrier |
| Idle | Start validated search | Searching |
| Searching | Consume telemetry, request stop, quit | Searching, Stopping, or ShuttingDown |
| Searching | Natural bestmove | Idle; deliver result only to matching game revision |
| Stopping | Await final bestmove | Idle; discard result if cancellation invalidated it |
| Stopping | Deadline exceeded | Failed; terminate/reap and preserve game |
| Failed | Explicit restart | Starting with new process generation |
| Any live state | Shutdown | ShuttingDown, then Stopped after process reaped |

Reckless discards commands other than `isready`, `stop`, and `quit` while
searching. `readyok` during search is immediate and is not a completion barrier.
Queue configuration or position changes until final `bestmove`; never overlap
searches in one process. Multiple live games may run simultaneously under the
shared resource budget. Active live-game searches receive equal CPU shares;
engines share one automatically divided total hash-memory budget. Reserve at
least one CPU thread per running engine game, including human turns, and limit
admission accordingly. Rebalance threads at each engine's next between-search
boundary; temporary unequal shares are acceptable within the total budget. Do
not allocate capacity still held by an existing search to a newly admitted one.
Release paused-game hash allocations by stopping, quitting and reaping the
child, then recreate it on Resume. Hash is never set to zero.
Initial total hash sizing uses 10% of OS-reported available RAM at app startup,
capped at 2 GiB, with a Settings override. Insufficient engine memory keeps the
requested game paused with actionable budget/running-game guidance. Integer
rounding, reservations and admission transitions follow
[capacity decisions](../decisions.md#capacity-and-memory). Never send option changes during an active search.
Engine callbacks do not mutate games directly. Resuming an untimed game search
starts a fresh search with the full configured per-move budget. Timed game
searches use remaining chess clocks; UCI has no checkpoint-resume operation.

## Positions and moves

Validate the starting position and every replayed move in the domain. Standard
initial games may use `position startpos moves ...`. Every FRC/DFRC game uses
`position fen ... moves ...`, because the tagged startpos branch does not
propagate the Chess960 setting. Preserve initial FEN plus game history to retain
repetition context; sending only the current FEN loses earlier repetition history.

Convert Chess960 castling to king-to-rook UCI using the authoritative position;
the UI still shows the king/rook's final squares. Validate engine `bestmove` and
each PV continuation before exposing them. A malformed continuation is an engine
protocol failure with bounded diagnostics, not browser executable content.

Adjudicate terminal positions before `go`; the tagged engine assumes nonempty
root moves. Do not rely on safe `bestmove 0000` handling for checkmate/stalemate.

## Search limits and correlation

Serialize exactly one limit variant: `infinite`, `depth`, `nodes`, `movetime`, or
clock parameters. Only the clock variant combines both sides' times/increments
and optional `movestogo`. `infinite` must be its sole argument.

The application also supports simple and Bronstein delay as owner-selected clock
families. Reckless has no native delay token. Do not fabricate delay commands or
encode delay as Fischer increment. The application owns delay/refund/flag-fall
accounting. Use the selected single movetime allocation, safe turn deadline and
stage/delay semantics in [clock decisions](../decisions.md#clocks-starts-claims-and-races).
Validate these with the pinned engine and controlled clock fixtures. Untimed engine play uses the
confirmed configurable default of 3 seconds per move.

Each application search receives a new search ID, process generation, game ID,
and position revision. The actor attaches these to output in pipe order. Stop
before replacing the active search and consume the final old bestmove before
starting the next one. Consumers reject mismatched generations/revisions.

Search cp and mate scores are relative to the searched side to move. Normalize
display with the original side, reversing bound direction when negating. Static
`eval` is raw White-perspective NNUE, a separate result kind.

## Bounded transport and cancellation

- Enforce a maximum 64 KiB engine line. Read incrementally so an unterminated
  line cannot allocate unbounded memory. Oversize/malformed output fails the session.
- Keep at most 256 pending control messages and 1 MiB retained diagnostic text.
  Saturation returns a typed error; never silently drop a move or completion.
- Coalesce replaceable visual analysis to at most 20 snapshots per second,
  retaining the newest consistent candidate data. Parse every protocol message;
  do not coalesce lifecycle/error/bestmove messages. The native queue and browser
  receiver both have explicit in-flight limits and acknowledgement/resync.
- A stop request has a 2-second safety deadline, with a p95 acceptance budget of
  1 second. On deadline terminate and reap the child, mark the game interrupted,
  pause clocks and offer explicit Retry. Never restart automatically. A successful
  user-requested restart still requires explicit game Resume.
- Shutdown sends stop/quit as appropriate, waits at most 3 seconds, then kills
  and reaps. Drain both pipes without deadlock. No orphan engine after window close.
- Diagnostics `perft` and `bench` run synchronously inside the engine; stdin stop
  cannot interrupt them. Run in an isolated child with a job deadline and kill/reap
  on cancellation. Never use the live game's child for these jobs.

## Tablebase paths

The pinned parser accepts only one whitespace-free SyzygyPath value. Quotes do
not escape spaces. Validate directory existence/permissions and the platform path
list delimiter. Provide a managed whitespace-free alias directory for selected
tables, preferably with filesystem links rather than copying huge data.

Select an OS-appropriate writable location whose complete path is whitespace-free;
do not assume usernames or temporary paths lack spaces. On Windows, link/junction
permissions and fallback behavior require a native test. If no safe alias can be
created, report a typed actionable path limitation and keep ordinary play working.
Do not modify vendor parsing, silently truncate paths, or claim tablebases loaded
solely because `readyok` arrived. Check actual load diagnostics/cardinality.

## Failure contract

Failures include incompatible binary, hash mismatch, startup/configuration timeout,
missing capability, invalid option, malformed line/move/PV, unexpected EOF,
nonzero exit, stop deadline, resource refusal, and tablebase load/path errors.
Preserve typed cause and stage with a redacted diagnostic tail. Engine failure
cannot become a legal move, a draw, or successful saved state. Clock handling
follows the interruption policy in [the data model](../data-model.md). Preserve
the game and pause clocks on a live-session engine fault. Restart only when the
user selects Retry; allocate a new process generation and revalidate readiness.
Successful restart does not resume play or clocks without explicit Resume.

Manual game pause is supported in engine and local modes. Freeze native clocks
and stop an active engine search through the existing stop/completion barrier;
results received while interrupted cannot play a move. UCI has no search-pause
checkpoint: resumption starts a fresh search with the full configured untimed
per-move budget, as already selected by the owner.
