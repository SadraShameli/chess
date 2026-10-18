# Game content save supervision

## Scope and verification status

This is an authorized source-only handoff from worktree `t3-d870f748`, based on
commit `647147d9ff95cb745ede46637048ef312e3070af`. It implements the game-content
and UTC integration across the existing supervisor, registry, prepared lifecycle
port and typed played-library repository. It follows US1, US3 and US5 in the
[specification](../spec.md), the [native plan](../plan.md), the
[message contract](../contracts/desktop-ipc.md), the
[study contract](../contracts/study-tools.md), the
[game model](../data-model.md#gamesession-and-gamemovenode), and the selected
[clock](../decisions.md#clocks-starts-claims-and-races) and
[storage](../decisions.md#storage-imports-and-backup-recovery) policies.
The constitution, catalog and all seven canonical rules were read.

No Cargo command, compilation, test execution, Clippy, build, CPU measurement,
application execution or runtime diagnosis was performed. The parent controls
serialized Cargo verification. Formatter parsing and patch checks do not prove
compilation, behavior, concurrency correctness or native acceptance. No new
semantic red is claimed. Missing APIs and missing registrations are integration
prerequisites, never behavioral reds.

There were no agents, commits, pushes or additional worktrees. Changes outside
the owned supervisor/registry sources and permitted test files are limited to
this evidence document. `game_transition.rs` required no change: the supervisor's
content owner intercepts production safety and pending-result operations before
calling its existing ordinary-command dispatcher.

## Exact dependency baseline

All four supplied patches passed SHA-256 verification and were applied, in this
order, using `git apply --3way --index` from the clean starting index:

| Dependency | SHA-256 |
| --- | --- |
| `native-library-adoption-recovery.patch` | `61fcfd0f7e03c5c50b077c346dec6bf0e64bfef3f4d8bf4a40d3880a2aacbe61` |
| `engine-premove-checkpoint-parent-fix.patch` | `43b51a3e13d8ed13cde16de064713944cbdecc5ad77585a5bebc14ead7c8d145` |
| `game-lifecycle-preparation.patch` | `e434ee77b9b217ff527b01366a3090e1fadc47e18d66c1d50033171771b4f44a` |
| `library-played-save.patch` | `0c6427f6530025beb30570b823d8b263f9d8b16bd8a64035545a3d25546b86c4` |

The frozen dependency tree is
`d3be71208f582bf962c4357f212b77553e170b22`. The export compares the final staged
index with this tree, rather than with HEAD. Dependency-only source, tests,
evidence and codec visibility hunks are therefore excluded. Overlapping owned
files use their dependency-applied blobs as the incremental baseline:

| Path | Dependency baseline blob |
| --- | --- |
| `src/desktop/game_supervisor.rs` | `7aa349e4fde03193621cb0d1427d929a8d6baeb7` |
| `src/desktop/game_registry.rs` | `84e3232e594e26176406a1629c6ddadb8920830d` |
| `tests/desktop/game_save_cancellation.rs` | `46366889f7d7d7964c8e8fbf1e1c2342ba033a1e` |
| `tests/desktop/native/game_service.rs` | `2f9ac6730cbedd3bc39d6db33786fe5c6e466cac` |

The new content source, new tests and this document were absent at that tree.
The artifact directory next to the patch contains the full dependency tree/blob
manifest, dependency patch snapshot, original owned-file blobs, tests-first
snapshot and final hashes. Its JSON manifest records every exported path's
baseline and final blob, the final staged tree and the incremental patch SHA-256.

## Tests before implementation

The initial `tests/desktop/game_content.rs` was authored before production edits
and frozen as Git blob `666d995d73b35fbe943a4e28586aade4d2351596`, SHA-256
`94352ba1b2a30d8650a5476abf33c29aa32f72922355ca528749555fad9a495d`.
That initial file contains 17 unexecuted tests. Eight further cases and genuine
fixture/API compatibility adjustments were added during integration. The final
file contains 25 asynchronous tests, all unexecuted.

The fixtures own real `GameSession`, `DesktopGameSupervisor`, capacity executor,
serialized storage actor and temporary SQLite database. Controlled monotonic and
UTC providers expose read counts and explicit failure controls. SQL assertions
join the real document and projection revisions. Projection-update triggers
exercise actual atomic write failure; no fake successful receipt substitutes for
SQLite. Finite future polls check the pending actor without sleeps. Timeouts are
bounded failure guards, not timing assertions or observations of application
performance.

Authored coverage includes:

- Fresh Created acceptance, matching typed document/projection UTC and refusal
  before owner exposure when the initial UTC preflight fails.
- Accepted move UTC surviving pause, failed SQLite save, failure interruption,
  advanced UTC and newest-snapshot Retry Save.
- An older observed real receipt preserving a newer watermark, including maximum
  observed UTC when a later accepted edit supplies an earlier value.
- Nonterminal safety Clock/Pause using zero additional wall reads when the
  injected wall provider is unavailable.
- Exact-deadline failure retaining a flagged stopped clock, absent outcome,
  Required status and the unforgeable pending token, with no timer/actor spin.
- Explicit retry accepting the retained result at retry UTC without readjudging
  the original position or charging downtime.
- Failed UTC retry retaining the original token/cause and stale pending context
  refusing before wall sampling.
- SQLite failure after deferred acceptance preserving its accepted UTC through a
  later retry with an unavailable wall clock.
- Separate retained storage and wall failures with a combined typed diagnostic.
- Further Pause, EngineFailure, Quit and Resave preserving pending-result guards;
  actual Quit admission returns SaveRequired while the actor remains available.
- Observed engine shutdown while pending content keeps shutdown incomplete.
- Every active safety entry, including direct retained shutdown, using the same
  terminal UTC/defer protocol.
- Stale action rejection before UTC and ordinary preflight failure preserving
  in-memory and durable revisions.
- Restored checkpoints, navigation and Unknown modified time preserving their
  stored values, including restoration with an already unavailable wall provider.
- Takeback request and response observing an exact deadline before content
  preflight; consent requests/declines remaining checkpoints and accepted
  takeback changing content.
- Engine checkpoint ordering and queued premove retention on a subsequent UTC
  failure, plus both direct and codec-wrapped cancellation requeue.
- Start, Play and Resume producing actual Result stamps regardless of nominal
  command name; study branches changing content while navigation/checkpoints do
  not.

The parent's original accepted-engine checkpoint regression remains present. Its
assertions still verify the accepted engine node/save, typed continuation,
second monotonic failure, unchanged accepted revision and retained queued intent.
Only ownership/context injection and typed-storage compatibility were adjusted.
The parent's reported old-blob red and seven premove plus 32 compatibility passes
are supplied dependency evidence, not executions in this lane. See
[the parent checkpoint evidence](engine-premove-checkpoint.md).

## Ownership and timestamp policy

One ownership-transfer mutex payload now carries the session, accepted-edit
watermark, optional pending-result token, its original wall failure and retained
storage failure. CPU computations take and return this whole payload. There is
no second mutex carrying an independently transferred watermark or token.

The watermark is recorded infallibly immediately after a domain success, before
snapshot capture, request capture, search preparation or subsequent computation.
It retains the latest actual edit revision/cause and the maximum UTC observed
among still-unacknowledged accepted edits. A real receipt only clears an edit
whose revision it covers, after the domain validates that receipt. An older
receipt cannot clear a newer edit. Cancelled saves retain their exact stamped
request; newest-state retries capture the retained watermark without sampling
UTC again.

Actual outcome delta takes priority over command names. Live-cursor changes
identify accepted moves/takebacks, and accepted study-tree changes carry StudyTree.
The trusted content classifier also recognizes Annotation and Metadata causes.
The current closed desktop command enum has no annotation or metadata mutation
variants, so this patch adds no invented command surface or handlers for them.
Their future command owners must preflight UTC and call the same trusted record
boundary after actual acceptance. Ordinary pause/resume, readiness, consent,
view changes and checkpoint writes preserve modified time unless they actually
accept a new result.

Fresh owner acceptance stamps Created before exposing its initial port/snapshot.
Restored sessions with a real storage revision do not invent a timestamp. Every
production supervisor write now uses `StorageLibraryPlayedRepository`; legacy
unindexed repository behavior is untouched. The supplied repository, canonical
codec and atomic document/projection transaction remain the only storage path.

## Prepared safety and explicit retry

Safety uses the canonical `prepare_lifecycle`, `content_change` and
`commit_lifecycle` contracts. Checkpoint commits have no UTC read. A newly
terminal result samples UTC before Accept and records Result immediately after
acceptance. On UTC failure DeferResult owns the exact stopped clock and token,
with Required status and no accepted outcome. The handled snapshot and typed
failure are published without exiting the actor or restarting an expired timer.

Only explicit RetrySave prepares the retained token, using that request's
original `received_at`. Preparation validates its context before UTC. Successful
retry records the actual deferred acceptance UTC; it is never presented as the
original deadline's UTC. Another UTC failure leaves the token and original cause
owned. Once accepted, SQLite failures and later saves reuse that accepted UTC.
Resave and shutdown never accept or serialize a pending result.

Pause/Stop/active Quit, Clock, engine failure, active save failure and direct
retained shutdown use this policy. Already-pending observations preserve the
original guards and cause. Takeback request/response first prepares PollDeadline
at the original receipt instant; a terminal/interrupted observation stops the
command, while an ongoing checkpoint continues canonical takeback handling.

Direct shutdown retains the original absolute deadline through computation,
quiescence, engine/factory shutdown and save observation. Actual engine cleanup
is observed even when content remains pending. Unresolved content returns
SaveRequired and keeps `is_shutdown_complete()` false. No profile lease or native
lifecycle code is changed. The parent retains that lease while owners remain
incomplete under its existing policy.

The parent engine/premove computation split and urgent-control priority remain.
A legal premove preflights UTC before consuming its intent. A later wall failure
therefore leaves the queued premove and already-enqueued engine checkpoint
owned. The existing contextual Premove notice remains the failure route, without
an immediate automatic retry or context rebase.

## Parent integration contract

Both `DesktopGameSupervisorOptions` and `DesktopGameRegistryOptions` add:

```rust
pub wall_time: Arc<dyn GameStorageWallTimeProvider>;
```

The registry retains and forwards that exact port for fresh and restored owners.
The parent bootstrap supplies checked `DesktopNativeWallTime`; no monotonic-clock
trait, clock generic, runtime or storage-adapter API is broadened.

`DesktopGameSupervisorError::Storage` now owns `StorageLibraryError`. New variants
are WallTime(GameStorageWallTimeError), Capture(GameStorageSaveRequestError),
and ContentSave { storage: Arc<Self>, wall_time: Arc<Self> }. The latter keeps
both typed causes visible. Exhaustive presentation handling belongs to the parent.

Existing permitted fixtures reference exactly
`crate::game_storage_wall_time_fixture::GameStorageWallTimeFixture::shared()`.
The parent must register its shared helper from the library's cfg(test) root and
from the desktop/native integration roots, with `chess::game` available as the
helper's crate-relative game import. This lane did not copy or alter the helper
or any root declarations. Register `storage::library_played` in the storage root.

The owned supervisor includes `tests/desktop/game_content.rs` once as its private
`content_tests` module. Do not also register that file in an integration root.
The parent's focused source selection, after its prerequisite registrations, is:

```text
cargo test --locked --all-features --lib desktop::game_supervisor::content_tests:: -- --nocapture
```

That command is proposed for the parent and was not executed here. The parent
must also compile/run the preserved cancellation/premove and permitted desktop
and native compatibility cases before accepting the source.

The frozen dependency version of `DesktopGameSnapshot::capture` still propagates
PendingResult from `takeback_target()`. Its parent-owned handling must classify
that refusal as no available takeback. The actual parent source was read and
already contains this exact adjustment; it is intentionally absent from this
worktree and export. Other root/bootstrap/presentation changes remain parent-owned.

The played adapter's known pre-reservation encoding/allocation concern is not
repaired or obscured by this integration. No adapter, codec, repository, domain,
schema, import or native lifecycle source was edited beyond applying the exact
dependencies. The owner is authoring its regression tests; repair remains gated
on the parent's actual semantic red and separate authorization.

## Export and source freeze

The incremental staged binary patch is
`.git/parallel-patches/game-content-save-supervision.patch` in the common parent
Git directory. The adjacent `game-content-save-supervision-baseline-d870f748/`
contains the external manifest, patch SHA-256 and baseline blobs. It is generated
with a full-index binary diff against the frozen dependency tree. No dependency-only
hunk, root declaration, copied helper or parent-owned adaptation is exported.

Only standalone pinned Rust formatting, Git whitespace checking, source-content
inspection, ownership-path inspection and incremental forward/reverse patch
applicability are verification in this lane. Their exact final results and
frozen hashes are recorded in the adjacent manifest. Compilation and all runtime
acceptance remain unverified. The exported source is frozen for parent verification.

## Parent canonical integration

The combined source and all relevant regressions were executed by the parent.
[Verified integration](library-runtime-import-integration.md) records the exact
snapshot, 1,666 passing tests, three clean Clippy selections, both optimized
builds, durable log hashes, known linker warning and remaining acceptance gates.
Earlier unrun statements describe this writer's isolated handoff only.
