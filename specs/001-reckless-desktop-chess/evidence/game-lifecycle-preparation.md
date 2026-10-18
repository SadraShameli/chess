# Game lifecycle preparation handoff

This is an authorized source-only implementation from baseline
`647147d9ff95cb745ede46637048ef312e3070af`. It is not compiled or runtime-accepted.
The parent retains the exclusive Cargo lane and shared target directory.

The scope follows [game and library requirements](../spec.md),
[game state and persistence contracts](../data-model.md#gamesession-and-gamemovenode),
[clock and race decisions](../decisions.md#clocks-starts-claims-and-races),
[library modified-time semantics](../decisions.md#book-generation-and-explorer-semantics),
and the current [native plan](../plan.md). The
[canonical rule catalog](../../../.specify/rules/README.md) and all seven linked
rules were loaded, together with the constitution and applicable owner guidance.

## Owned patch

- `src/game/session.rs`
- `src/game/session/lifecycle.rs`
- `src/storage/game_document.rs`, only the remote interruption enum refusal
- `tests/game/session_lifecycle.rs`
- This evidence file

No crate root, test root, supervisor, registry, presentation, dependency, schema,
or other source file is changed. The new test file is already registered once
through the source-private `game::session::lifecycle_tests` module. Do not also
register it in an integration test root. Its imports use `crate::` and it checks
private receipt and state guards within that unit-test module.

The baseline blob IDs are:

| Existing path | Git blob at baseline |
| --- | --- |
| `src/game/session.rs` | `ceb1380fafc3b5af9ae9a671a51a3c384f90c29b` |
| `src/storage/game_document.rs` | `b37bdff36e968332d7a0e428c17c555963915b0c` |

The three new paths are absent from the baseline. The parent document adapter's
`pub(crate) fn from_snapshot` was read directly in
`/Users/sadrashameli/Personal/chess/src/storage/game_document.rs`. That visibility
change belongs to the parent. This lane retains the baseline visibility and its
patch contains no hunk touching that method, so applying this patch preserves
the parent's change.

## Public integration contract

All new public types live in `crate::game::session::lifecycle`:

```rust
pub enum GameSessionLifecycleEnumRequest {
    Pause(GamePauseRequest),
    PollDeadline {
        expected_revision: GameRevision,
        received_at: ClockInstant,
    },
}

pub enum GameSessionLifecycleEnumContentChange {
    Checkpoint,
    Result,
}

pub enum GameSessionLifecycleEnumCommit {
    Accept,
    DeferResult,
}

impl GameSession {
    pub fn prepare_lifecycle<P: GameAdjudicationMateProofProvider + ?Sized>(
        &self,
        request: GameSessionLifecycleEnumRequest,
        proof: &P,
    ) -> Result<GameSessionLifecyclePrepared, GameSessionError>;

    pub fn commit_lifecycle(
        &mut self,
        prepared: GameSessionLifecyclePrepared,
        mode: GameSessionLifecycleEnumCommit,
    ) -> Result<GameSessionLifecycleCommitted, GameSessionError>;

    pub fn prepare_pending_result(
        &self,
        pending: &GameSessionLifecyclePendingResult,
        received_at: ClockInstant,
    ) -> Result<GameSessionLifecyclePrepared, GameSessionError>;
}
```

`GameSessionLifecyclePrepared::content_change(&self)` returns the typed content
classification by comparing the actual old and candidate outcomes. Preparation
does not mutate the session. Guard fields, candidate clock/state/revision/kind,
clock update, snapshot, and receipt instant are private. Preparations and pending
tokens have no public constructor, `Clone`, or serialization implementation.

`GameSessionLifecycleCommitted` exposes `transition()`, `update()`, and
`pending_result()` by reference. Its consuming `into_parts()` returns
`(Option<GameSessionTransition>, ClockStateUpdate,
Option<GameSessionLifecyclePendingResult>)` in that order. The pending token
exposes only `game()`, `revision()`, `live()`, and `observed_at()`. Its retained
`Copy` outcome remains private.

`GameSessionError::ForeignLifecyclePreparation { expected, received }` carries
game IDs. Stale revision, live-state, or clock guards use the existing
`GameSessionError::Revision(GameRevisionError::Stale { .. })`. An intervening poll
can invalidate a preparation without advancing the revision, so this error may
carry equal revision numbers. `LifecycleResultRequired` refuses deferral of a
checkpoint, unresolved adjudication, or already deferred retry preparation.
`PendingResult` refuses serialization and ordinary pending-state commands.

## Preparation and acceptance

Pause and deadline polling now share one prepared flag-fall decision path.
The original public `pause` and `poll_deadline` signatures are preserved and use
the same preparation and Accept implementation. The extracted `accept_state`
method is the infallible state mutation boundary shared with `finish_state`.
Existing transition construction and clock warning accounting remain canonical.

Checked revisions, engine-barrier validation, clock observations, adjudication,
outcome validation, and clock snapshots occur before mutation. Commit validates
game/revision/live/state/clock guards and commit mode before changing any field.
The Pause wrapper obtains its required concrete transition through the same
internal Accept helper before mutation; it has no fallible transition extraction
after state changes. Ordinary running polls retain their revision and return no
transition. An ordinary checkpoint requires no wall-clock input anywhere in the
domain.

Preparation retains fixed-size clock/guard/update/snapshot values, sharing the
time control through existing `ClockState`/snapshot ownership. It does not clone
a `GameSession`, tree, adjudication history, or restoration snapshot. Adjudication
uses the supplied proof provider through the existing history owner. The authored
layout test limits the preparation value to 1024 bytes and checks tree identity,
contents and live cursor preservation. That test is unrun and does not establish
allocation counts, runtime latency, or a measured performance improvement.

## Deferred result and Retry Save policy

1. Prepare at the original monotonic receipt instant. `Checkpoint` can commit
   with Accept immediately, with no wall-clock lookup.
2. For `Result`, the parent obtains and validates UTC before Accept. If that
   lookup fails, it commits DeferResult and retains the returned token with the
   original wall-clock error in its owned mutex.
3. DeferResult is legal only for a newly prepared terminal result. It commits
   the exact candidate stopped or flagged clock, advances the revision once,
   clears premove and takeback consent, enters
   `GameSessionEnumInterruption::PendingResult`, and marks saving Required. Its
   transition kind is Interrupted and exposes no accepted outcome. The token
   guards the new stopped game/revision/live/state/clock and retains the original
   observation instant and precomputed outcome.
4. Explicit Retry Save calls `prepare_pending_result(&token, retry_received_at)`.
   This validates the token and prepares the next revision and result snapshot
   without invoking a proof or re-adjudicating the position. The parent obtains
   valid UTC before committing Accept. A failed UTC lookup can discard that
   preparation and retain the original token for another explicit retry.
5. Accepting the retry records ResultAccepted at the new revision, exposes the
   retained outcome, and preserves the exact stopped clock. The parent supplies
   the actual retry UTC for the content update. Neither the original event time
   nor elapsed downtime is fabricated. Required remains until the newly accepted
   revision receives its own durable save receipt.

Only the parent integrates UTC, content timestamps, persistence effects, the
token/error mutex, and Retry Save. This domain patch imports no concrete storage,
desktop, system clock, or wall-clock API into the lifecycle implementation.

## Pending-state safety

View navigation remains available. `select_view` and `return_to_live` change only
the view cursor, which is intentionally outside preparation/token guards. A
prepared retry can therefore commit after view navigation while preserving the
selected view. Live cursor, result state, revision, and stopped clock are guarded
separately.

Repeated pending deadline observations are checkpoint observations with no
transition. Repeated valid Pause and save-failure observations return an
Interrupted transition at the same revision. They preserve the exact stopped
clock and all token guards, consume no proof, and keep Required. Existing Pause
engine-barrier validation still applies. Empty premove cancellation remains
available and cannot alter the pending context. Ordinary resume, recovery,
takeback, move/result, play-option, and study-content operations refuse with the
typed PendingResult error before their normal save gate.

Older queued save receipts return Older and leave Required intact. An exact
pending-revision receipt is refused because no valid pending snapshot can exist.
After retry acceptance the prior pending revision is older and likewise cannot
clear Required.

`restoration()` and direct `GameSession::restore()` reject PendingResult with
`GameSessionError::PendingResult`. The existing remote interruption enum adds
`#[serde(skip)] PendingResult`, so serialization refuses it and decoding the
`pending_result` marker fails. No token, wall-clock error, pending marker, or new
schema is persisted.

## Takeback handoff

The existing private method remains unchanged:

```rust
fn observed_takeback_clock<P: GameAdjudicationMateProofProvider + ?Sized>(
    &mut self,
    now: ClockInstant,
    proof: &P,
) -> Result<Result<ClockState, GameSessionTransition>, GameSessionError>;
```

Both `request_takeback` and `respond_takeback` call it. Its flag branch currently
accepts an outcome inside the takeback command through `finish_decision`, before
the supervisor can obtain result UTC. The narrow owning API for pre-observation
is the new `prepare_lifecycle(PollDeadline { expected_revision, received_at },
proof)` followed by `commit_lifecycle`.

For an Active game, the parent should perform that preparation at the original
takeback request/response receipt instant, before invoking either takeback
method. A terminal preparation must pass through the same UTC/Accept-or-Defer
decision and stop the takeback command. A checkpoint that interrupts adjudication
also stops the command. An ongoing checkpoint commits without a revision change,
then the existing takeback method can receive that same original instant and
expected revision. Its second observation at the same instant cannot newly cross
the deadline. Ordinary already paused/recovered takebacks retain their existing
path; PendingResult refuses the command. This slice does not rewrite takebacks
or repair an existing takeback defect without a parent-observed failing test.

## Parent-owned integration and verification

The parent owns the exhaustive PendingResult presentation matches in
`src/presentation/app.rs` and `src/presentation/game/play.rs`, plus supervisor and
registry integration. The standalone source slice therefore makes no claim that
the whole baseline compiles before those coordinated edits are integrated.

There are 22 authored deterministic tests with real sessions, fixed monotonic
instants, real clock controls and counted proof providers. The initial 18 were
written before lifecycle source changes; four review cases extend coverage.
They cover checkpoint and terminal preparation, exact stopped clocks, deferral,
unsavable pending state, guarded retry, retained draws, no repeated proof or
downtime, foreign/stale guards, same-revision observations, invalid mode,
receipt gating, missing-token restoration/decode refusal, canonical wrappers,
Unknown proof, revision exhaustion, invalid barriers/reversed observations,
premove/takeback clearing, warning delivery, navigation, abandoned preparation,
live/result state guards, and bounded preparation value state.

Tests executed: zero. Compilation, Cargo, Clippy, builds, CPU/runtime diagnostics,
native execution, and parent integration gates were not run. No red-first
failure is claimed: missing new APIs would be a compilation failure rather than
evidence of an existing defect. No agent, commit, push, or extra worktree was
created. Formatting and static patch checks are recorded below.

## Static verification record

Executed checks passed:

- Standalone rustfmt formatting and `--check` for the four owned Rust files,
  edition 2024, with `skip_children=true` to avoid touching unowned modules.
  The installed formatter is rustfmt 1.10.0 from
  `/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt`. A first `rustup run 1.99.0`
  attempt found no rustup command; no toolchain was installed.
- `git diff --cached --check` for all five staged paths.
- Forward `git apply --cached --check` against a scratch index loaded from the
  exact baseline, with no additional checkout or worktree.
- Reverse `git apply --check --reverse` against the authored working files.
- Owned-path review: exactly the five paths listed above, all staged, with no
  unstaged changes. The document adapter has only its two-line remote enum delta;
  no `from_snapshot` visibility hunk is included.
- Source-content scan found no comments, lint suppressions, lint expectations,
  or em dashes in the authored lifecycle/session/test source. Static test count
  is 22. This scan and rustfmt do not establish compilation or behavior.

The staged binary patch is exported to
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/game-lifecycle-preparation.patch`.
Its final SHA-256 is reported in the handoff response, outside the patch itself.
The formatted source SHA-256 values are:

| Path | SHA-256 |
| --- | --- |
| `src/game/session.rs` | `647ccd84dd580b8b898f28571e6e229b3577b3bd46c812f2001d45eba96bdf98` |
| `src/game/session/lifecycle.rs` | `395bbf6beedefcdb95e67b3855eae35cf9cb8ff20b5bd089ac89c8b6e6fa2446` |
| `src/storage/game_document.rs` | `a5c3bca9f2fab9bff3973517d001269794bf8eb4e94bc3efd353a795a4ca53a7` |
| `tests/game/session_lifecycle.rs` | `648003fdf4112e00ac4dd7b39895f0d9243fb152dc61aecd7ee965030184cfd3` |

## Parent canonical integration

The combined source and all relevant regressions were executed by the parent.
[Verified integration](library-runtime-import-integration.md) records the exact
snapshot, 1,666 passing tests, three clean Clippy selections, both optimized
builds, durable log hashes, known linker warning and remaining acceptance gates.
Earlier unrun statements describe this writer's isolated handoff only.
