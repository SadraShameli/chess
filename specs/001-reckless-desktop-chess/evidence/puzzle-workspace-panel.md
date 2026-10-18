# Saved puzzle workspace panel

Final parent integration and gate results are recorded in
[saved workspace integration](puzzle-saved-workspace.md). The source-only and
filtered-run statements below describe their historical handoff revisions.

## Status and scope

This source-only implementation owns exactly:

- `src/presentation/puzzle/panel.rs`
- `tests/presentation/puzzle_panel.rs`
- `specs/001-reckless-desktop-chess/evidence/puzzle-workspace-panel.md`

The initial 22 deterministic contract tests were authored before the wrapper.
The parent reported all 22 passing, with zero failures/ignored and 175 filtered.
Four review regressions were then appended while production remained frozen.
The parent ran those 26 cases against the frozen production logic and observed
23 passed, three failed, zero ignored and 178 filtered. The three failures were
behavior assertions, not missing-API compilation failures. The matching-Opened
authorization case already passed. The source repair below has not been compiled
or executed here or reported green by the parent yet.

Cargo, builds, tests and CPU diagnostics remain withheld in this worktree. The
parent owns the exclusive execution lane, initially identified as make-verify
session 69526, and subsequently supplied the targeted review result below.
The supplied 1,091-test base and inventory worker results belong to earlier
parent/worker scopes. Neither those counts nor the targeted panel tests establish
native, full-feature, performance or release acceptance.

The global instructions, project bridge, constitution and all seven canonical
rule files were read. The implementation skill's prerequisite discovery
succeeded with the explicit absolute feature directory. The requirements
checklist has 16 checked items and the native checklist has 40 checked items,
with no unchecked checkbox items. No extension hook configuration exists.
Parent-owned task markers and all module/test roots remain unchanged.

The service contract was read directly from
`/Users/sadrashameli/.t3/worktrees/chess/t3-caa86227/src/desktop/puzzle_service.rs`.
The inventory contract was read from the supplied
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-run-inventory.patch`.
Neither dependency was applied or edited here. No other worktree was edited.

## Observed review regressions and source repair

The parent's recorded command was:

```sh
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib presentation::puzzle::panel::tests:: -- --nocapture
```

The log at
`/tmp/claude-chess-pgo-diagnosis-20261016/puzzle-workspace-review-red.log` was read
directly before repair. It records these three failures:

- `matching_opened_receipt_preserves_a_newer_same_session_watch_snapshot`: the
  installed controller had revision 0 instead of the authoritative watch's
  revision 1 after the watch was consumed before the Opened receipt.
- `terminal_failed_open_restores_the_existing_selected_active_run`: opening
  concealment remained active after the replacement request failed terminally.
- `active_run_open_refuses_before_admission_or_concealing_the_running_countdown`:
  public Open returned success for an active run and admitted the replacement.

The fourth case,
`newer_watch_requires_a_matching_opened_receipt_before_installing_a_controller`,
passed in that same run and remains unchanged. All original 22 tests and all four
review cases are preserved byte for byte. The failure-restoration case uses the
existing private submit boundary to model an already accepted Open independently
of the new public active-run guard. Its original blocker and explicit-retry
assertions remain intact.

The frozen production file's SHA256 was
`64b6d0056440d208e9f97daf2d9b2711d02d1e4a4e616a8c63d5853e17c26e3d`.
The tests-applied baseline file's SHA256 is
`e38212eb8ec8fcb0bf4566dbfcbfec3fb71013c59993fb96116d8267fcd74bba`.
The parent added only the required private cfg(test) include to its production
copy. The incremental repair changes no module or test registration and does not
remove or replace that include.

After a valid matching Opened response authorizes controller construction, the
panel now reads the current authoritative watch value and uses the same canonical
observer as normal watch updates. That merge does not depend on a watch change
still being unread. Same-session revision/timestamp guards stay on the existing
controller, and foreign watch/receipt data does not authorize a controller.

Opening concealment now requires the retained request to be pending. A failed
Open remains retained with its actual request and receipt for explicit retry,
while the still-selected original run regains its content and controls.

Public Open and retry of a failed Open first observe matching authoritative state
and apply one shared guard. An unpaused continuing run returns the typed
`PresentationPuzzlePanelError::PauseRequired` before creating a request or
concealing content. Its message requests Pause and saving first. Paused and
terminal runs continue through normal application/service admission; the service
still owns durable replacement eligibility. No pause, Resume or rebasing is
synthesized by the panel.

## Root-facing API

The public component is
`crate::presentation::puzzle::panel::PresentationPuzzlePanel`.
Its connection is:

```rust
pub struct PresentationPuzzlePanelConnection {
    pub owner: DesktopMessagesEntityId,
    pub state: watch::Receiver<DesktopPuzzleServiceState>,
}
```

`PresentationPuzzlePanel::new(connection)` returns `Self`. The read API is
`board() -> Option<&DesktopBoardSnapshot>`,
`controller() -> Option<&PresentationPuzzleController>`,
`has_pending_work() -> bool`, `is_promotion_pending() -> bool`,
`inventory() -> Option<&Arc<StoragePuzzleRunPage>>`,
`summary() -> Option<&PuzzleSummary>` and
`failure() -> Option<&PresentationPuzzlePanelError>`.

The following methods are generic over the existing `ClockTimeProvider` and
`DesktopApplicationWakeProvider`, accept the existing application port and
return `Result<(), PresentationPuzzlePanelError>`:

```rust
panel.synchronize(&port)
panel.show(ui, &port)
panel.board_action(action, &port)
panel.report_readiness(readiness, &port)
panel.first_page(&port)
panel.next_page(&port)
panel.open_saved(id, storage_revision, &port)
panel.request_action(action, &port)
panel.refresh_snapshot(&port)
panel.request_summary(&port)
panel.retry(&port)
```

These methods use the parent-proposed canonical constructor
`DesktopCommand::puzzle(owner, operation)` and receive
`DesktopEnumValue::Puzzle(Box<DesktopPuzzleServiceEnumResult>)`. They do not
calculate an expected revision. The message changes, service module and inventory
dependency are root-owned integration and are excluded from this repair.

The tests also require the parent-owned application completion API:

```rust
DesktopApplication::complete_puzzle_control(
    &self,
    work: &DesktopApplicationWork,
    response: Result<DesktopPuzzleServiceEnumResult, DesktopFailure>,
) -> Result<(), DesktopApplicationError>
```

It is the puzzle equivalent of the existing game/engine control completion API.
The tests use ordinary application admission, actual returned receipts and the
real application's commit/cancellation boundaries. They never construct an
application receipt.

The parent owns `pub mod panel;` in the existing puzzle module root and has added
the test file once as a private child of its panel source, following the existing
controller test pattern:

```rust
#[cfg(test)]
#[path = "../../../tests/presentation/puzzle_panel.rs"]
mod tests;
```

Do not also register it as a public-contract integration module. Its controlled
storage boundary deliberately uses the existing crate-private
`PuzzlePersistenceReceipt::committed` constructor, and it inspects private panel
slots to establish actual request/receipt retention.

## Implemented behavior

The panel retains one current `Arc<StoragePuzzleRunPage>` and replaces it on an
explicit first/next-page result. It does not collect all history, infer recency,
classify recoverability from inventory metadata or label saved runs as installed
collections. Each Open command carries the chosen metadata storage revision.
The lower repository owns page size, cursor semantics and unsigned identifiers.
Loading, an empty page, a missing/corrupt load and explicit retry have visible
states. The actual setup component receives empty installed metadata and a false
start allowance, so Start Run remains disabled. There is no fixture collection,
new-run factory or prepared-collection catalog in production code.

Only an explicitly matching Opened response constructs or replaces a controller.
Watch snapshots update an existing controller for the selected same session,
through its canonical snapshot observer, including the latest matching watch
value immediately after an authorized Opened result. Loading, Replacing, Closing,
shutdown, disconnection, a pending Open and workspace hiding conceal the shared
board. Terminal Open failure ends request-based concealment while retaining the
failed request for retry. Active replacement is refused with a pause-first typed
error before admission or concealment. The active view is also withheld outside
the selected visible workspace, so hiding the shared board does not expose
solution details through that view. There is no selected-game or bootstrap-board
fallback.

Ordinary actions preserve the controller intent's exact context. Pause and Hidden
send Interrupt with that original intent's session ID. The service owns choosing
the authoritative current context within that same session. Confirmed End stays
a Session Command End with the original exact context; it is never rebased.
Confirmed End occupies the ordinary retained panel slot even though application
admission treats the command as a control, leaving the separate Interrupt slot
available to conceal the workspace immediately.

Each retained slot owns the original presentation intent, the application-created
request and the actual admitted receipt. Busy and EntityBusy admission reuse the
same request. Failure and cancellation retain these objects until explicit retry
or another admissible action. Retry creates a fresh application request after a
terminal response and retains the previous attempt in one bounded history slot
per lane. Same-session stale successes finish their own controller slot without
rolling back newer canonical state. Foreign results cannot install a controller
or finish the current controller intent. There are no unbounded panel maps,
spawned tasks, native runtime calls or synthesized application receipts.

Readiness is supplied by the actual host, including while a paused board is
absent. Reporting Ready does not request Resume. A new canonical context must
receive actual readiness again. Only after the controller permits OpponentReply
is one requested for that context; repeated synchronization or readiness reports
do not duplicate it. The panel has no puzzle random source or engine verification.
Ordinary retained writes do not themselves conceal the board or stop its timer;
the canonical checkpoint gate determines that behavior.

Ended Open displays only the returned canonical summary. Controller summaries
are requested explicitly as Service Session Summary and installed through the
existing observer. The panel never computes a summary from a snapshot. Refresh,
Summary, Retry Save and Interrupt have no blanket panel-level Closing refusal;
the parent must provide the stated application admission allowance and the
service still decides whether recovery is permitted. Idle/terminal failure does
not advertise pending work, and the panel sends no per-frame Snapshot/Poll
commands while idle.

## Authored test coverage

The original 22 deterministic cases cover:

- Initial watch state cannot install a controller, and idle advertises no work.
- Actual request/receipt identity, deduplication and one admitted action.
- EntityBusy and global Busy preserve the same request until admission succeeds.
- Hidden concealment and a separate original-session Interrupt behind ordinary
  work, including an ordinary transition that advances the authoritative context.
- Readiness while paused and explicit Resume without visibility-driven resumption.
- Loading/Replacing/Closing concealment and typed disconnection.
- Foreign watch/results and same-session stale completion without rollback.
- Actual readiness before one listed opponent reply, with no duplicate request.
- Failure/cancellation retention and explicit retry with the previous receipt kept.
- Ended and active canonical summaries, and exact-context confirmed End routing.
- Real transactional inventory pages, numeric unsigned IDs through `u64::MAX`,
  cursor exhaustion and selected storage revisions.
- Empty inventory, missing collection and a disabled Start Run in the egui view.
- Missing/corrupt Open failures and explicit retry.
- Application Closing recovery for Snapshot, Summary, Retry Save and Interrupt.
- An ordinary in-flight write leaves the board and active countdown state visible
  while the canonical checkpoint permits it.

The production controller and DesktopPuzzleCommands own all puzzle behavior in
these tests. Controlled implementations exist only at the time, wake and storage
completion boundaries. Inventory tests use the real temporary SQLite repository.
The tests contain no alternate puzzle rules, sleep, live data or engine process.
The four additional review cases and the parent's actual red scope are recorded
above. Post-repair behavior remains unverified until the parent reruns them.

## Executed checks and remaining verification

Executed in this worktree only:

- Required instruction/rule/API/source inspection and feature prerequisite check.
- Standalone formatter from the installed pinned toolchain path
  `/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt`, reporting rustfmt 1.10.0.
- Standalone edition-2024 formatting and `--check` of the two owned Rust files.
- Source/patch whitespace and scope checks at export.
- Read-only inspection of the parent's targeted review log, including the three
  assertion failures and exact 23-pass/3-fail result.

The formatter parsed both files and its check succeeded. This does not establish
type correctness, Clippy success or test behavior. An initial PATH-only rustup
invocation was unavailable; the installed pinned standalone formatter was then
located and used without installing tooling.

Still required from the parent: post-repair compiler/Clippy feedback, the complete
26-case panel selection, the canonical final gates and native window/renderer/input
acceptance. Existing root dependencies and the parent-added private test include
must remain in place. The parent also owns workspace selection, immediate gesture
cancellation on workspace/context changes and actual readiness delivery even when
`board()` is absent. No native visual, performance, CPU utilization or end-to-end
acceptance claim is made here. The feature's broader Start/catalog/history/deletion
and release acceptance tasks remain with their existing owners.

The implementation follows the existing component boundaries in
[architecture](../../../.specify/rules/architecture.md) and
[structure](../../../.specify/rules/structure.md), concrete contracts in
[Rust style](../../../.specify/rules/rust-style.md), bounded retained presentation
state under [performance](../../../.specify/rules/performance.md), and controlled
real-owner test boundaries under [testing](../../../.specify/rules/testing.md).
The three-file export and source-only verification limit are recorded against
[code quality](../../../.specify/rules/code-quality.md) and
[verification](../../../.specify/rules/verification.md).

The original export remains
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-workspace-panel.patch`.
The tests-only append remains
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-workspace-panel-regressions-tests.patch`.
The incremental repair target is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/puzzle-workspace-panel-review-repair.patch`.
It contains only the production and evidence deltas against the tests-applied
baseline, with no repeated tests or registration changes. Module roots,
application/host/coordinator/messages/DTO/bootstrap, dependencies and other
worktrees remain excluded. No commit, push, agent or additional worktree was made.
