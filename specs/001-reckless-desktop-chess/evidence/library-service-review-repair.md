# Library service four-red repair handoff

This is a source-only implementation handoff. Tests executed in this lane: **0**.
Cargo, builds, Clippy, application execution, runtime diagnostics and CPU
diagnostics executed in this lane: **0**. Pinned rustfmt and Git artifact checks
are source-integrity evidence only. Compilation and semantic green observations
remain parent-owned. No commit, push, agent or additional worktree was created.

The global guidance, applicable project bridge, constitution, catalog and all
seven canonical rules were read. The governing policy remains the
[rule catalog](../../../.specify/rules/README.md).

## Exact dependency and preserved observations

- Root commit: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
- Dependency patch: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-red-baseline-20261017/source.patch`.
- Verified dependency SHA-256: `80194a403857ca571fc3063cdeb5f108e3288c50601f942199fd4fe2e8aed18f`.
- Verified staged dependency tree: `67da2aed144a86d9cdaa7822ae42e71eb6c3854c`.
- Preserved log: `library-service-review-semantic-reds.log` in that dependency bundle.
- Verified log SHA-256: `a0eadc5ee3a6d2b87b8b04a2ddacc17768dff6ef76dce04c7d77558e273a3297`.

That historical log reports 312 passes and five failures. Four semantic failures
are the repair baseline: fresh played RetrySave returned StaleContext; cancelling
an already accepted original returned Cancelled; constructor failure retained
9,936 source-reservation bytes; and 67,108,865 note bytes entered the service
transport. The valid-size played publication-pressure control passed in that
log. These are preserved parent observations, not executions by this lane.

The fifth log failure is the saved-open deadline fixture. The task reports its
independent passing run and async-lease correction. The exact dependency already
contains the async lease. This repair does not change that fixture or claim a
new deadline-test result.

## Source changes

The played continuation now owns its original service reply, exact operation,
original lower receipt and bounded retry replies. A fresh RetrySave must match
the complete original editor context before it can join. Resumption uses the
original lower application receipt, whose publication continuation prevents
edit or UTC replay. The original lower response watcher is retained. Successful
recovery publishes the same actual acknowledgement to the original and all
joined retry service receipts. Each service receipt keeps its own admitted
application correlation. Cancellation of an unaccepted fresh retry refuses that
retry; cancellation of the retained original does not erase accepted work.

Saved-open construction now owns its loaded memory reservation in the Creating
phase. Only observed constructor success transfers it to installed saved_sources.
Observed constructor failure drops that phase before publishing its response.
The failure path does not remove or overwrite an installed owner's reservation.
Installation and deadline continuation retain their existing owners.

Operation payload validation checks String and annotation-vector allocation
capacities with checked arithmetic before service queueing or payload cloning.
The charge includes the operation and space for its retained and working copies.
Structural excess returns Library(Store(DocumentTooLarge)). A separate bounded
64 MiB annotation-input transport pool charges retained requests and their
working-copy allowance. Temporary exhaustion returns Busy. This pool does not
consult the receiving publication-memory pool, so valid small edits remain
admissible under the historical publication-pressure scenario. This calculation
is not an allocator-overhead, total-process-memory or RSS measurement; query
cursor and private metadata allocation accounting are outside this repair.

The public application-boundary API is:

```rust
pub fn validate_payload(&self) -> Result<(), DesktopLibraryServiceError>
```

It is an inherent method on DesktopLibraryServiceOperation. It delegates to the
same size calculation as service admission. Root must call it before application
admission and dispatcher cloning. No application, dispatcher or UI root is edited
here. The lower source's separate text limit remains authoritative.

The agreed UI seam exports DesktopLibraryServiceEnumDisposition with Refused and
Retained { request, expected }, through DesktopLibraryServiceError::disposition().
Typed Continuation errors preserve the actual original application request and
source context. Owned pending work and uncertain lower completions are wrapped;
proven preacceptance refusals remain Refused. Cleanup deadlines carry their
actual in-flight reply. Notation dirty-save failures retain their original
replies until a real covering save acknowledgement, while a new unaccepted
draft cannot inherit an older dirty edit's disposition. Retained replies cannot
be reclaimed as ordinary completed cache entries. Played retry aliases and
notation dirty-save replies are explicitly bounded by the actual 256-reply
service capacity and reconcile on successful completion.

## Authored counterchecks and parent verification

Six new tests were authored, with zero executions:

1. Foreign played game, epoch, generation, storage and source-revision bindings,
   plus a cancelled fresh retry, leave the original continuation recoverable.
2. Repeated failed fresh played RetrySave receipts reconcile with the original
   real save acknowledgement after receiving memory returns.
3. Individually valid queued notes share a finite transport byte budget, without
   consuming receiving memory or accepting cancelled edits.
4. Cancelled unaccepted played and notation requests report Refused and preserve
   their context, storage revision and content UTC.
5. A real notation SQL refusal and cleanup refusal report the original retained
   disposition, a later wall-time-refused draft stays Refused, and a real RetrySave
   reconciles the original receipt without accepting that draft.
6. A failed saved constructor followed by same-game success and an existing-owner
   reopen retains the successful owner and releases all memory after cleanup.

The historical oversized-notes test also checks the public preflight API,
multibyte UTF-8 bytes and excess spare String capacity. Historical played
recovery observations now assert the original source-certified disposition.
Assertions preserve actual domain/SQLite/channel fixtures and observed cleanup.
No sleep, ignored test, mock replacement or fabricated acknowledgement was added.

Root must run the four preserved semantic regressions, the positive pressure
control, all six new tests, existing library-service/annotation/played-editor and
saved-open contracts, then the required shared gates in its authorized lane.
Source review and rustfmt do not establish any of those outcomes.

## Explicit initial-open limit

This patch's initial played Open publication failure still reports a retained
operation with expected None. Fresh RetrySave cannot join that Open and returns
ForeignReceipt. Re-submitting the original application request does not redispatch
the service because the application outcome is cached in OnceLock. Root is
authoring a separate end-to-end regression before extending this behavior.

The proposed existing-API extension certifies the original lower Publishing
context in the error, accepts a fresh Editor { expected, action: RetrySave }
bound to that exact context, and wakes the original lower application receipt
through a bound editor RetrySave. That guards replacement epochs before mutation
and resumes publication without issuing a fresh unbound Open. Both receipts
would reconcile to Opened with the actual snapshot. This extension is not
implemented or verified by this four-red patch.

## Export

The bundle is `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-review-repair-20261017/`.
Its source.patch is an explicitly staged full-index binary diff from the exact
dependency tree, excluding dependency changes. manifest.json records the final
tree, exact paths, modes, blobs, per-file SHA-256 values and zero executed test
counts. counterchecks.patch is the test-only review subset of the final patch;
it depends on the accompanying service API and is not an independent baseline
regression artifact. SHA256SUMS covers the exported artifacts.
