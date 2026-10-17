# Editor application and write recovery

The parent integrated the actual application action tickets, editor command owner,
presentation adapter, typed DTO and coordinator route. The existing single
coordinator retains the command owner and admitted application work through
write completion. The route does not publish `WritePending` as a terminal value.
No commit, push or ORM migration was performed.

## Observed regressions and repairs

All logs below are under
`/tmp/claude-chess-pgo-diagnosis-20261016/`. Commands used the recovered pinned
Rust 1.99.0 wrapper without changing the repository toolchain.

| Baseline log | Observed result | Repair |
| --- | --- | --- |
| `editor-routing-recovery-baseline-2.log` | 14 passed, 3 failed | Retire the exact controller request on actual typed `EditorFailure`; retry an original write only after confirmed pre-commit rollback |
| `editor-quit-rollback-baseline.log` | 0 passed, 1 failed | Retry Quit observes a source-owned continuation without completing the original terminal application work again |
| `editor-retry-destination-baseline.log` | 10 passed, 2 failed | Distinguish the original write receipt, the fresh retry's navigation authority and shutdown recovery |
| `editor-history-bound-baseline-3.log` | 0 passed, 1 failed | Bound local history to 256 retained edits and disclose that bound in the editor |

The original two payload-admission regressions and their logs remain recorded in
the [application routing evidence](editor-application-routing.md). Admission now
checks retained FEN capacity against `BoardPosition::MAX_FEN_BYTES` before
observing time, stamping a receipt or retaining application work. The direct
command handler and production presentation adapter reuse that check.

The storage transaction owner returns a private rollback-confirmed outcome only
after a pre-commit error followed by observed automatic rollback or a successful
explicit rollback. Commit errors, failed rollback, channel failure and consumed
completion do not grant retry. The original write retains its immutable intent,
expected storage revision, actor identity and memory reservation. Retry accepts
that exact mutable write handle, never a replacement draft or rebased revision.
Failed retry admission preserves its rollback authorization.

The command owner retains the original submitted draft and actual application
receipt while later edits remain local. A fresh admitted retry contributes its
actual receipt only as navigation authority. Canceling either applicable receipt
prevents the pending destination from executing. Shutdown recovery has no
navigation authority. Successful persistence still acknowledges the original
submitted draft and exact next storage revision.

Retry Quit uses the existing coordinator's retained editor future. It preserves
the original terminal application failure, retains any subsequent source failure,
and waits for actual storage observation before lifecycle shutdown. It neither
fabricates an application receipt nor replays an accepted domain edit.

## Verified selections

`cargo test --locked --all-features --test desktop editor` passed all 20 selected
cases after the destination-authority repair: zero failures, zero ignored.
The log is `editor-retry-destination-repaired-tests.log`, SHA-256
`ce082e3573dace0b9c6f307f17aa73fda282ffb6be61a7216d59383e33928059`.
Its preceding 18-case checkpoint is
`editor-quit-rollback-repaired-tests.log`, SHA-256
`f9307fd976db6ffd0f5119a7dc21905dfcd2e0e7c9a504f1443d82a70475783e`.

The cancellation regressions use actual coordinator admission, Stop and Quit,
temporary SQLite triggers, explicit transaction gates and SQL completion
barriers. They close the coordinator or drain the known storage work and close
the actor before evaluating behavioral assertions. No sleeps or fake outcomes
are used.

The broader integration tests and final canonical gates remain separate evidence.
The existing macOS `__eh_frame section too large` linker warning remains
unsuppressed. Native editor mounting, GPU/accessibility behavior, platform
acceptance and complete US6 acceptance are not established by this selection.
