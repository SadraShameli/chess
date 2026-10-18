# Played Open recovery through the application boundary

This handoff adds one source-only regression and its focused fixture. It does
not implement the production repair. Tests authored: **1**. Tests executed:
**0**. Compilation, semantic red, repaired green, native runtime, and release
acceptance have not been established.

## Verified source baseline

The worktree started clean at
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`. The following absolute patches
were SHA-256 checked and applied to the index in order:

1. `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-red-baseline-20261017/source.patch`
   SHA-256 `80194a403857ca571fc3063cdeb5f108e3288c50601f942199fd4fe2e8aed18f`.
   Verified tree `67da2aed144a86d9cdaa7822ae42e71eb6c3854c`.
2. `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-review-repair-20261017/source.patch`
   SHA-256 `ead7e40d540fc3af8eaf67af5afe5f65720ae9313c300660b8e3ba799bcaaf31`.
   Verified tree `b11bd8b2c14558b2b6fcfba52d30384bbef4687b`.
3. `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-application-routing-20261017/source.patch`
   SHA-256 `b29d70fa5f86b7dbf190b054732ab1ef05dfd33d99a0442c5c0e796483194dea`.

The resulting combined dependency tree is
`d16ca056088911865c61ced490b2f4d22440261f`. The exported patch is incremental
against that tree. Dependency changes and parent-owned exhaustive-match changes
are excluded. No parent compile-match edits were needed or mirrored for this
source-only task.

## Gap identified in the supplied source

The service owner's
[Open limitation](library-service-review-repair.md) remains present after the
routing dependency is applied. `DesktopLibraryServiceError::continuation`
obtains `expected` from the retained operation's `editor_context()`. An original
`Open` has no editor context, even when its real lower editor receipt is
`Publishing` with an exact context. `DesktopLibraryServicePlayed::check_retry`
also reads that original operation context, so a fresh `Editor::RetrySave`
currently encounters `ForeignReceipt`.

Resubmitting the original service request is a supported continuation. It is a
different boundary from resubmitting the original application request. The
application receipt stores its terminal result in a `OnceLock`; repeating its
`DesktopRequest` returns that cached `LibraryFailure` without another provider
dispatch. The regression explicitly exercises this control before recovery and
again after cleanup.

These are source observations and the service owner's documented limitation,
not an executed failure from this handoff.

## Regression construction and required behavior

`failed_played_open_exposes_original_context_for_fresh_application_retry_without_replaying_open`
uses the real SQLite store, library repository, registry, library service,
application, and coordinator. It reuses the existing service fixture's resource
free engine factory, fixed clocks, seeded random provider, and positive memory
pressure reservation. It does not use an oversized publication.

The current source is read back from SQLite and installed with the registry's
actual restoration API before pressure is applied. This targets the first editor
Open of an existing restored supervisor. No editor has opened in this current
owner before the actual application Open request. The snapshot publication,
lower receipt, and epoch all come from that real operation. This setup isolates
receiving publication pressure from repository loading admission.

A bounded observer provider forwards the actual `Port::try_submit` result and
awaits the real receipt. It exposes receipt clones and a dispatch count. It does
not manufacture source results, application receipts, editor publications, or
save acknowledgements.

The foreign context comes from a normal successful opening of the same game in
an older real owner. That owner is fully closed first. Its real context matches
the new source's game, revision, selected node, storage revision, and generation,
while its epoch differs. A fresh application retry with this old context must be
refused without replacing the retained original owner, changing its lower
Publishing context, or consuming its original service watcher.

The test records the initial typed continuation, including `expected: None`.
Only a context actually certified by that continuation can construct the fresh
UI retry. The lower receipt's observed context is used for assertions; it is
never substituted into a missing UI continuation or reconstructed from board
data.

After pressure is released, a certified context enables a new actual application
request containing `Editor { expected, action: RetrySave }`. The required
contract is:

- The service binds this retry to the original Open and submits
  `DesktopAnnotationCommands::new(original_application_receipt, exact_context,
  RetrySave)` to that retained lower operation. It resumes the original
  publication rather than opening another editor.
- The original lower receipt completes before fallback cleanup. Both the
  original and new service receipts expose `Opened` matching its actual
  publication. The new application receipt receives recovered `Opened` with its
  result revision and a distinct application identity.
- The first application receipt retains the identical typed failure. Repeating
  its original request cannot dispatch additional provider work.
- No save acknowledgement is invented. The original lower receipt has no
  storage receipt, SQLite document body and revision remain identical, content
  revision and modified UTC remain unchanged, UTC read counts remain zero, and
  registry factory count remains one.
- Source revision, analysis phase, selected and live nodes, and clock view stay
  unchanged through refusal, retry, and cleanup. The tested source uses the
  existing fixture's paused untimed analysis session; it does not establish a
  timed live-game acceptance result.

The lower submission detail above is the requested production contract for the
parent. The regression checks its observable ownership and recovery behavior;
it does not instrument private registry method call counts.

## Cleanup before the regression assertions

If the continuation has no context, the test records that observation and still
finishes ownership cleanup. It releases memory pressure, then resubmits the
original operation directly through the original service port with the same
actual application receipt. It never retries the original application request
as a cleanup mechanism and never constructs a fake binding.

The same supported service cleanup is used if a certified fresh retry leaves
the original publication pending. Fresh retry outcomes and original service
watcher results are captured before this fallback, so cleanup cannot turn a
failed fresh application retry into a passing observation.

The test drops all external original, foreign, retry, and publication handles;
completes coordinator Quit; observes service and registry retained shutdown;
closes SQLite; and shuts down the shared executor. Full publication memory and
the one-worker fixture capacity are checked after closure. Only then do the
final behavioral assertions require `Some(exact_original_context)` and fresh
application recovery. Missing continuation is not a skip or successful early
return. All waits are driven by owned futures and bounded deadlines, with no
sleeps.

On the supplied baseline, the expected semantic red is the final assertion that
the certified continuation equals `Some(actual_context)`. This is a prediction
until the parent registers and executes the test. A production repair must wait
for that observed red.

## Parent registration and verification

The parent owns `tests/desktop/library_service.rs`. Register the private child:

```rust
#[path = "library_service/open_application_recovery_review.rs"]
mod open_application_recovery_review;
```

The new child owns its fixture registration. No existing test root was edited.
No shared compile-match changes, production repair, Cargo command, build,
Clippy, tests, runtime probe, CPU diagnostic, agent, extra worktree, commit, or
push was performed.

Static verification uses
`/tmp/claude-chess-pgo-diagnosis-20261016/with-pinned-rust rustfmt`
with `--edition 2024 --config skip_children=true` on the two authored Rust
files, followed by its `--check` mode. Patch verification includes staged
whitespace, exact incremental path ownership, reverse application, and isolated
index replay against the combined dependency tree. These checks do not prove
compilation or behavior. The bundle manifest records their final results and
the patch SHA-256.

Guidance loaded in full includes the global and project instruction bridges,
the constitution, the rule catalog, and all seven canonical rules:
[architecture](../../../.specify/rules/architecture.md),
[code quality](../../../.specify/rules/code-quality.md),
[performance](../../../.specify/rules/performance.md),
[Rust style](../../../.specify/rules/rust-style.md),
[structure](../../../.specify/rules/structure.md),
[testing](../../../.specify/rules/testing.md), and
[verification](../../../.specify/rules/verification.md).
