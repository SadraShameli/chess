# Registry close review regressions

Status: tests authored and source reviewed only. None has been compiled or run.
The root must register the suite once and observe actual baseline failures before
repairing production code.

## Exact dependency baseline

Base commit: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.

Apply these patches from `/Users/sadrashameli/Personal/chess/.git/parallel-patches/`
in this order:

1. `library-native-mount-dependency-20261017/source.patch`, SHA-256
   `41954367eb94b0d1200cc79549a254de717139395d2236d7fce327661cea4472`.
2. `registry-game-close-barrier-20261017/source.patch`, SHA-256
   `4679a47c4ae093eb572ec653a6ba1d427681723393463573d7086b44f27994cc`.

The resulting Git tree is `a5b1e3efff497d5a147045be56da26c526099c52`.
The second patch already contains the supervisor acknowledgment prerequisite.
The review patch contains only the new suite, its focused fixture, and this file.

## Cases and expected baseline failures

The suite lives at
[`tests/desktop/game_registry/closing_review.rs`](../../../tests/desktop/game_registry/closing_review.rs).
Its private fixture lives at
[`closing_review/fixture.rs`](../../../tests/desktop/game_registry/closing_review/fixture.rs).

- `quit_completed_before_close_retains_actual_acknowledgment_and_releases_barrier`:
  open a real saved local game, Start it, complete ordinary Quit, observe its
  gated factory cleanup completion, then poll the registry before admitting
  close. Read the committed SQLite revision and request
  close using that current owner and revision. Record the first outcome and one
  explicit retry of the same close operation. After retained shutdown, require an
  actual `Closed` acknowledgment matching the stored game and storage revisions,
  barrier release, and a new paused saved opening. Source inspection predicts
  `Supervisor(Closed)` on both close attempts because the completed actor was
  discarded before close admission. The assertion reports both errors after
  cleanup, without inventing a cancellation certificate.
- `unrelated_saved_open_progresses_while_target_certificate_is_retained`:
  obtain the target's actual close certificate and keep it held while admitting
  and completing a second game's saved open. Check the target remains blocked and
  its committed content revision remains unchanged. Source inspection predicts
  immediate `Busy` for the unrelated game at saved admission.
- `target_close_preserves_an_already_admitted_unrelated_saved_open`:
  queue the second game's opening before target close admission, then require its
  actual editor result while the target certificate is still held. Source
  inspection predicts that close progress drains this unrelated request with
  `Busy`. The recorded failure is asserted after releasing the target and
  observing shutdown.
- `unrelated_saved_constructor_does_not_delay_actual_target_close_certification`:
  complete target Quit's save but hold its real factory cleanup with a semaphore.
  Admit an already recovered unrelated source and retain the fixture's sole
  capacity lease so its real supervisor initialization remains pending. Admit
  target close, release only target cleanup, poll the registry, and poll the
  actual close receipt before releasing the constructor lease. The unrelated
  factory count and absent editor result establish that construction has begun
  and remains pending. Source inspection predicts a pending target certificate
  because close progress waits for any saved constructor. Release the lease,
  observe the original opening, and settle the original close operation before
  asserting the earlier ordering. A real explicit close retry is used only when
  needed to clean up the baseline outcome.

These are source-derived predictions, not observed red results.

## Determinism and ownership

The fixture reuses `DesktopLibraryServiceFixture` for temporary SQLite, real local
sessions, the shared capacity executor, and application-admitted receipts. It
builds a real registry using the existing factory provider boundary. It does not
replace the registry, supervisor, persistence, or close proof with a mock.

Both saved-open receipt layers must share the actual application outcome. The
suite checks paused editor publication and the requested game's storage revision.
Close proofs come exclusively from `DesktopGameRegistryCloseReceipt::wait`.
Only `Closed` with the supervisor's real storage acknowledgment satisfies the
installed-game assertions.

The tests use Tokio's default current-thread test runtime. Factory completion
notification is followed by polling the existing registry run future. For these
local games, the completed Quit leaves no engine, computation, save, or editor
projection for supervisor cleanup to await after factory completion. The registry
can therefore observe the completed actor before the close receipt is sampled.
The constructor test holds capacity only after target save completion, so target
certification needs no executor work that the unrelated constructor could own.

There are no sleeps, timing thresholds, detached tasks, or elapsed-time assertions.
The only wall-clock deadlines are the existing retained-shutdown cleanup bounds.
Certificates are released through actual `CancelConfirmed` observation where
available. The permanent-barrier baseline has no valid certificate to release;
its real failed receipt remains retained through registry shutdown. All expected
regression assertions follow observed registry, service, store, and executor
cleanup, including factory creation/shutdown count verification in the reused
fixture.

The older closing fixture was left untouched. This suite needs a target-specific
gate with a completion notification and shared drivers for its own registry;
reincluding the older fixture would also introduce unrelated unused helpers.

## Root integration and verification boundary

The root can register this suite once inside `src/desktop/game_registry.rs`:

```rust
#[cfg(test)]
#[path = "../../tests/desktop/game_registry/closing_review.rs"]
mod closing_review_tests;
```

That registration is deliberately absent from the tests-only patch. No old test,
production source, visibility, root module, manifest, ORM, or checker setting was
authored or changed.

Loaded global/project guidance, constitution, and all seven canonical rules.
Static review checks scope, relative module resolution, receipt correlation,
retained cleanup, and prohibited source constructs. Patch checks and SHA-256
verification establish artifact integrity only.

Cargo, builds, tests, Clippy, application runtime checks, and CPU diagnostics were
not run. No agents were used. The pinned Rust 1.99.0 formatter is unavailable in
this checkout's tool environment, so formatting is source reviewed and awaits the
root's pinned formatter. Compilation, lint acceptance, and actual regression
failures remain unverified.
