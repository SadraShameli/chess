# T089 same-revision selected-view receipts

This lane owns `src/presentation/engine_panels.rs`,
`tests/presentation/engine_panels.rs`, and this evidence file. The parent's
`native-ui-current-source.patch` was applied solely as dependency setup and is
excluded from the handoff delta. No Cargo commands, commits, pushes, additional
worktrees, or subagents were used.

## Failure and fix

Navigating between study nodes changes immutable position history without
changing the game revision. The panel correctly recreated its controllers, but
the workbench context contained only game/revision, engine generation, and
configuration revision. An admitted receipt from the previous controller could
therefore pass context equality and then fail its request check with
`StaleContext`. Start lost its job cancellation path, while configuration and
tablebase preparation failed before installing the authoritative snapshot.

The panel now records the request belonging to its current workbench controller.
Selection-driven controller replacement and engine-context replacement revoke
that ownership. Workbench acknowledgments, rejection, picker continuations, and
export continuations require both the current context and the exact owned
controller request. Controller request IDs remain distinct from native request
IDs; delayed Start matching still uses both IDs and the exact backend job ID.

A late Start retains its mapping until the matching job update is accepted or
an exact-job Cancel has a tracked reserved invalidation request. An unrelated
invalidation receipt is never overwritten by that cancellation. Unowned active
job updates use the same cancellation path; terminal updates require no further
cleanup. The separate urgent control slot remains available under the existing
policy. Configuration and prepared-tablebase snapshots still refresh the
authoritative engine projection when their original UI acknowledgment is stale.
Old failures leave the new controller's pending state and failure unchanged.

The parent's five reported lint findings are addressed in the owned source:
both nested admission/retry conditions are collapsed, and all three watch
receive errors are retained as typed error sources. Terminal-channel
disconnection likewise retains its typed original source.

## Regression coverage

The tests-only delta was produced before the implementation changed. Four new
mounted cases use the real native engine actor, application receipt owner,
capacity budget, resource adapter, and existing in-memory process boundary.
A bounded two-party barrier holds a real actor result before application
receipt completion, lets the mounted UI select the root at the same game
revision, then delivers the late receipt. There are no sleeps or network calls.

- `same_revision_pending_diagnostic_start_cancels_exact_job_and_releases_its_lease`
  starts an isolated perft that cannot complete through the fixture, verifies
  its held CPU lease, navigates before Start delivery, requires a reserved
  cleanup request, and verifies the same job/native request becomes Cancelled
  with all capacity released before runtime shutdown.
- `same_revision_pending_clear_hash_refreshes_revision_before_the_next_request`
  verifies the root selection receives configuration revision 1, then submits
  another real Clear Hash and requires revision 2 with exactly two engine
  commands. This would fail if the next request retained revision 0.
- `same_revision_pending_tablebase_preparation_refreshes_authoritative_status`
  uses a local preparation fixture and requires the actual prepared directory
  and Configured status on the new controller, without claiming engine loading.
- `same_revision_pending_tablebase_failure_does_not_reject_the_new_controller`
  uses a real empty-directory preparation failure and requires no stale error
  propagation or failure acknowledgment on the replacement controller.

The tablebase preparation cases are Unix-specific because the existing native
tablebase policy explicitly refuses Windows preparation in this build.

## Verification and handoff

The standalone formatter from the installed Rust 1.99.0 distribution was used
only on the two owned Rust files. Explicit-file formatter checks, whitespace
checks against the dependency baseline, and reverse-apply validation of the
generated patches passed. The final binary patch names only the three owned
paths and excludes dependency setup.

Cargo remains parent-owned. The four new cases have not been compiled or run
in this lane, and no observed red/green result, Clippy result, complete product
verification, native platform acceptance, or performance measurement is claimed.
The parent must run the tests-only delta against its unfixed source to establish
red evidence, then apply the source delta and run the relevant presentation
tests and all required gates. Coordinator Quit and destination wiring remain
outside this lane.

## Parent integration checks

The four original same-revision regressions were executed against the previous
panel implementation: all four failed. After integration all four passed.
The test process boundary now emits Reckless's actual Clear Hash acknowledgment,
and tablebase expectations compare canonical selected paths on macOS.

A fifth regression navigates while an export is admitted, then verifies the
published bytes still equal the reviewed preview and the replacement controller
receives no stale acknowledgment. The old implementation failed this case.
The pending apply action now retains its expected preview ID independently of
the mounted preview. Terminal diagnostic events also update any retained
progress projection before delivery, preventing an older progress snapshot from
being replayed after the terminal result. The mounted diagnostic/export case
failed before this correction and passed afterward.

The parent used the real canonical live resource owner to reproduce admission
before analysis cleanup: a live allocation received its lease while the analysis
lease remained held. Canonical admission now waits for active analysis claims to
release at observed cleanup, then revalidates before granting the live lease.
The mounted preemption case passed after this correction.

The mounted fixture retains control completion until the affected application
receipt is observed, matching the production coordinator. It also services the
post-cancellation snapshot request before asserting that all work has settled.
These corrections preserve the original capacity and cleanup assertions.
