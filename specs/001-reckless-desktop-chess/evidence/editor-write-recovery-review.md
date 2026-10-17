# Editor command write recovery review

Status: source-only test handoff. Five tests authored, zero tests executed.
Compilation, semantic red, Clippy, build, runtime and CPU evidence remain with
the parent. No production repair or ORM change is included.

## Exact baseline

- Worktree: `/Users/sadrashameli/.t3/worktrees/chess/t3-0e2ce3b1`.
- Commit: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
- Dependency index tree: `64ee68b9d7111d04c8d34656bf5612c0f2b47d1c`.
- Patch directory: `/Users/sadrashameli/Personal/chess/.git/parallel-patches`.
- Applied in order: `position-editor-domain.patch`, SHA-256
  `a645f2ad378016ebf0b957796d9247eb4ab2480c002a03127b069fbacb3f59f8`;
  `position-editor-storage.patch`, SHA-256
  `3e8302c57aab0db3c1d4c2e0c72e840df8717e5d3e8a85f4dfe302bdc34f4f59`;
  `position-editor-commands-ui.patch`, SHA-256
  `5598c41c94f14183e0204eb98471f30bad5325a895a1f80ea4ea82d593ff8b17`.

The incremental binary patch is `editor-write-recovery-review.patch` in that
directory. It adds only this evidence file and the two new test files below.
Dependency files are baseline content and are excluded from the incremental
patch. Existing source, tests and shared registration roots are unchanged.

## Included files and registration

- [Recovery cases](../../../tests/desktop/editor_commands/write_recovery_review.rs).
- [Focused fixture](../../../tests/desktop/editor_commands/write_recovery_fixture.rs).

The parent supplies the source module registration and typed application
gateways required by the three dependencies. Include the recovery module once
from `tests/desktop.rs`:

```rust
#[path = "desktop/editor_commands/write_recovery_review.rs"]
mod desktop_editor_commands_write_recovery_review;
```

The recovery module includes its own focused fixture. It uses public `chess::`
imports and does not depend on registering the existing producer fixture.

## Source-confirmed dead end

[`DesktopEditorCommandsWrite::wait`](../../../src/desktop/editor_commands/write.rs)
lines 91 through 109 caches every repository wait error. Its next call returns
the cached error without consulting storage.
[`finish_write`](../../../src/desktop/editor_commands.rs) lines 337 through 350 returns
on that error before taking the pending write. `require_no_write`, lines 390
through 395, rejects the retained write; Save calls it through `begin_save`,
Delete calls it directly, and Resolve Save and Resolve Discard call it too.
Resolve Stay only clears the destination; it cannot reconcile the retained write.

The first recovery test removes the actual SQLite trigger, calls the existing
Save and `finish_write` methods, and requires successful acknowledgement of the
original submission. On the supplied source, Save returns `WritePending` and
`finish_write` returns its cached `WriteFailed`. The failing assertion includes
the additional actual Delete, Resolve Save and Resolve Discard results. These
diagnostic probes run only while the recovery attempt remains failed.

This is an expected semantic red derived from source inspection. It has not
been observed by execution in this worktree. The parent must run the tests
against the supplied source before repairing production behavior.

## Authored behavior checks

1. `explicit_save_after_definite_rollback_recovers_original_submission_and_keeps_later_edits`
   seeds a real saved draft, changes its counters, requests Leave, and submits
   Resolve Save. An `AFTER UPDATE` trigger uses SQLite `RAISE(ROLLBACK, ...)`.
   Later local edits deliberately contain invalid raw counters. The test checks
   `SQLITE_CONSTRAINT_TRIGGER`, the unchanged durable baseline, the exact
   retained submitted draft and application receipt, and the original storage
   revision before requiring recovery. A successful recovery must save only the
   original submission, advance storage once, keep the later edits dirty with
   undo history, and preserve the Leave prompt. This is the primary expected red.
2. `rollback_retry_keeps_original_expected_storage_revision_and_refuses_external_update`
   repeats the controlled rollback, removes the trigger, then commits a different
   real domain draft through the repository. Explicit recovery must report a
   conflict using the original expected revision and preserve the external row,
   submitted pending draft and later local edits. The supplied source returns the
   cached trigger failure instead of reaching that check. This is the second
   expected red.
3. `stale_storage_revision_refuses_save_and_keeps_external_and_local_drafts_separate`
   externally advances a row before the editor submits Save. Both initial and
   repeated observation must retain the exact conflict. The local draft and its
   storage base remain intact, and the external row is never overwritten.
4. `abandoned_acknowledgement_wait_preserves_one_write_and_recovers_its_actual_receipt`
   holds the injected capacity budget, queues Save, polls its acknowledgement
   future once to establish Pending, then drops only that future and releases the
   budget. A FIFO repository read establishes that the write has committed before
   the owner observes its acknowledgement. Another Save must remain blocked, the
   original application receipt and draft remain owned, and `finish_write` must
   return the actual single-commit receipt. A second finish returns `NoWrite`.
5. `consumed_storage_acknowledgement_error_preserves_commit_and_refuses_stale_command_save`
   observes a successful real repository receipt and then the same handle's
   `CompletionConsumed` error. The row remains committed at its advanced
   revision. A subsequent command Save using the original storage base must
   refuse with an exact conflict. A wait error alone cannot establish rollback.

The last three cases are source-reviewed counterchecks; no passing outcome has
been measured here.

## Fixture and coverage limits

The focused fixture follows the producer's real SQLite store, real domain
objects and real `DesktopApplication` admission, commit boundary and receipt
sharing. Like the existing fixture, it completes application work with the
command acceptance result and observes the storage receipt separately. This is
command-owner contract coverage, not native service or presentation acceptance.

Only the new fixture uses the canonical draft ID string parser. The existing
producer fixture calls `BoardEditorDraftId::try_from(Uuid)`, which the supplied
domain source does not implement. No existing fixture was edited.

All asynchronous waits have a five-second failure bound. The uncertain-wait
case uses a retained capacity lease and one explicit future poll, with no sleeps
or scheduling guesses. SQL setup and temporary-directory deletion use joined
blocking tasks. Each case collects observations, closes the storage worker and
temporary directory, and then evaluates assertions. Setup errors also pass
through that cleanup path.

There is no UTC field, UTC provider or timestamp parameter on this editor draft,
Save operation or storage receipt. Tests cannot prove UTC preservation through
this API, and no timestamp machinery was invented.

The public command constructor accepts a concrete repository and does not expose
the underlying write or reply sender. Actual channel closure, unexpected-reply
and consumed-completion errors cannot be injected into a command-owned pending
write through the current public API. The tests cover abandoned acknowledgement
observation and an actual consumed repository acknowledgement. They do not claim
command-owned Channel or UnexpectedReply fault-injection coverage. A repair must
retain such uncertain outcomes for reconciliation rather than classify all wait
errors as definite rollback.

## Local verification

The dependency SHA-256 values were checked before application. Standalone
`rustfmt --edition 2024` formatted only the two new Rust files. Incremental diff
whitespace, changed-path ownership and patch applicability were inspected.
No Cargo command, test, build, Clippy invocation, runtime or CPU diagnostic ran.
No agent, commit or push was created.
