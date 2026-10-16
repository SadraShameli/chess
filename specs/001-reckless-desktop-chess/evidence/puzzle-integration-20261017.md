# Guarded puzzle owner, runtime and presentation integration

This increment follows pushed tooling checkpoint `10169bd`. The parent applied
the three isolated puzzle slices, registered their production modules and kept
their crate-local private tests included once through their owning source.
No fixture collection or new-run factory was added to the shipping composition.

## Combined verification

The merged source passed 142 library cases and 297 domain cases with all
features: 439 passed, zero failed, ignored or filtered. The combined all-feature
library/domain Clippy selection passed with `-D warnings`. The canonical gate
then passed all three Clippy selections and 1,091 tests, with zero failures,
ignored cases or filtered cases. Both optimized build selections passed: all
targets and features in 4 minutes 42 seconds, then the separately selected native
harness in 47.34 seconds. Canonical `make verify` exited successfully. These build
durations describe verification and are not application performance measurements.

The existing macOS debug linker `__eh_frame` size warning remains visible and
unsuppressed. The Clippy result is warning-free. Logs and their SHA-256 manifest
are retained under `.git/parallel-patches/puzzle-integration-20261017/`.

| Log | SHA-256 |
| --- | --- |
| `puzzle-combined-tests.log` | `0b542bbef4b39b2a3a79896ef4384740dabde3cbeef33b010d1a48135cfb922a` |
| `puzzle-combined-clippy.log` | `db98a88f55a438c609591e94f2020b165080b50e3c0e7c3203e4112773b6a78b` |
| `puzzle-make-verify.log` | `4a2164389336b46f2c19fbe794a9690370e55b31108b4b81cc4a57573043617a` |

Every Cargo invocation used the root shared target directory, detected default
parallelism and sequential gates. The native harness was compiled and linted;
this command does not execute native GPU, OS or accessibility acceptance.

## Reviewed behavior and regressions

[The command owner](puzzle-command-owner.md) guards time advancement, retains
one write plus the latest coalesced state, settles completions with fresh time,
and exposes compact immutable snapshots. Initial persistence precedes countdown
start. Save backpressure has its own persisted pause reason; recovery and failed
saves require explicit Resume after the newest paused state becomes durable.

Three exact-deadline owner regressions failed as stale before the dispatch
repair. The owner now lets the requested Pause, Hidden or confirmed End command
arbitrate expiry through the real run rather than pre-polling and discarding
the original context. All 62 selected owner/lower cases and its targeted Clippy
selection then passed in the isolated lane.

[The native runtime](native-puzzle-runtime.md) owns bounded ordinary requests,
independent urgent interruption/shutdown capacity, clock/scheduler injection,
retained receipt observation and explicit summary requests. Four observed
baseline failures covered initial and Snapshot save-admission errors, an old
receipt causing repeated settlement and false Ready health, and direct cleanup
renewing an original queued shutdown deadline. The coalesced-admission case
already passed baseline and was retained as coverage. All 20 actor and two
lifecycle cases passed after the repairs, and a fresh review found no remaining
concrete blocker in those paths.

[The panel](native-puzzle-panel.md) consumes authoritative board, timer and
summary values. It conceals paused/frozen content, preserves ordinary play
during pending saves, retains an urgent concealment slot, acknowledges actual
readiness and exposes explicit listed-reply intent. End confirmation is bound
to the full context. Both foreign-session receipt regressions failed before
moving session validation ahead of request-slot consumption; all 17 panel
cases then passed. Genuine stale same-session receipts still complete without
rolling newer snapshots or summaries back.

Native OS/window signals, root application routing, initial/recovered-run
service ownership, actual three-dimensional mounting and production collection
preparation remain separate integration/acceptance work. The current tests use
real domain objects with controlled storage/clock boundaries and semantic egui
fixtures. They do not establish actual OS sleep or rendered native acceptance.

## Production collection boundary

The current `puzzle_collections` row is keyed by run identity and freezes the
session configuration, filters, seed, every supplied definition, initial order
and draws. It is not an installed dataset catalog. The existing repository can
load a known saved run; no installed collection discovery or production Start
factory exists yet.

The codec's 16 MiB envelope cannot hold the approved 200,000-puzzle starter:
that allows fewer than 84 bytes per definition before order, draws, journal and
checkpoint overhead, while required identity/FEN/solution JSON already exceeds
that allowance. Definitions are serialized before filtering. The 100,000-event
and full-replay bounds also need explicit production lifetime/recovery design.
No size or performance result at production scale is claimed.

The next source work separates bounded saved-run inventory from immutable
versioned definition/index storage and pinned run references. US13 retains
actual acquisition, licensing, preparation, indexes, integrity and version
leases. T187/T190 and remaining native/presentation/history acceptance stay
open. All thirteen journeys remain first-release scope.

## Tooling worktree cleanup

After tooling commit `10169bd` was pushed and remote-verified, both completed
tool worktrees and their local branches were removed. Full worktree and Git
metadata archives, staged/unstaged diffs and verified content manifests remain
under `.git/parallel-patches/completed-tool-worktrees-20261016T224137Z/`.
The two archives were verified against 472 and 473 file/link entries before
removal. The three puzzle worktrees were retained for integration.
