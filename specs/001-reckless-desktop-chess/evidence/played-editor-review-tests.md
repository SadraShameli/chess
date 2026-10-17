# Played editor blocker regression source handoff

This tests-only handoff adds two unrun regressions. Neither red has been observed
in this lane. The parent must compile and run both against the buggy behavior
before authorizing production repairs in a new round. No production GO or
journey acceptance is established.

The checkout started at local commit
`465837a95c0de9129460171ea57a5edb2d054aac`. The applied and frozen dependency was
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-service-current.patch`,
SHA-256 `2619b38ef072424167f43c25398e83f01dbabfa5ecbc31216c5f5543013a9ee6`.
Its verified index tree is `8af39a32b83db046bd1ab6e035459fe324b8120d`.
The parent's later private CPU request-context repair for `UnwindSafe` is absent
here. These tests use existing APIs and do not mirror that repair.

The incremental binary patch is exported as
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/played-editor-review-tests.patch`.
The adjacent `played-editor-review-tests.manifest.json` records its exact SHA-256,
base and result trees, source hashes, test names, and static verification. The
export contains only an append to `tests/desktop/game_editor_runtime.rs` and this
evidence file. The frozen test file's original 64,434-byte prefix is preserved
exactly. Its twenty-four existing tests are unchanged; two cases are appended.

Mandatory guidance read in this lane includes the global Claude file, root
Claude bridge, constitution, rule catalog, and all seven canonical rule files.
The user-scoped rule frontmatter has no matching Rust or Markdown paths. No
additional applicable nested guidance was found. Engineering policy remains in
[the canonical catalog](../../../.specify/rules/README.md).

## Deferred Open and an already due live deadline

Exact test name:
`desktop::game_supervisor::editor::tests::deferred_editor_open_adjudicates_due_clock_before_returned_publication`.

The real supervisor starts with a timed active local session and its initial
owned save. A lease holds the fixture's injected CPU capacity while an actual
application-admitted Open enters the ordinary queue. One manual scheduler poll
uses Tokio's cooperative-budget override so cooperative yielding cannot hide
ready queue work. The test records that the matching original receipt occupies
the real deferred Open slot, the save remains unresolved, and no result exists.

The injected monotonic clock advances from 50 nanoseconds to 120 seconds for a
60-second clock. The test checks the actual published clock's deadline is due
while Open and the save are still retained. It then releases CPU capacity and
awaits the real save through the supervisor's existing save owner, without
dispatching Open during that drain.

The next scheduler run delivers Open's actual returned publication. The test
copies that publication's context and state before shutdown. Its final semantic
assertion requires `Completed`, flagfall, and a Black victory, together with the
original admitted Open epoch. A later completed supervisor event or a fresh
after-save snapshot cannot satisfy this assertion. Source review predicts the
frozen scheduler can return `Active` because deferred dispatch schedules editor
computation before the deadline select is eligible. This remains an unobserved
prediction until the parent runs the test.

## Accepted publication recovery across shutdown

Exact test name:
`desktop::game_supervisor::editor::tests::shutdown_retries_accepted_publication_after_receiving_memory_is_released`.

The real receiving storage client reserves all but 65,536 bytes of its memory
budget. An actual notes command accepts 131,072 bytes of notes into the real
session. The test retains the original application receipt, original editor
receipt, and an additional editor receipt witness. Setup must yield the typed
`EditorPublicationPending` error caused by receiving-memory `Saturated`, report
`Publishing`, retain the original accepted annotation edit, and have no storage
acknowledgment.

Shutdown is invoked under pressure. The recorded source, revision, accepted
edit, original timestamp, and UTC invocation count must remain intact. After
pressure is released, shutdown is retried. If that attempt still fails, only the
original accepted command receipt is resubmitted, with a bounded result wait,
followed by one final shutdown observation. A successful internal shutdown retry
needs no new editor admission.

Success requires a completed shutdown, the original receipt's actual storage
acknowledgment shared with its witness, matching durable revision and notes from
the indexed played repository, the original modified timestamp, unchanged edit
revision and UTC count, a current authoritative source, and the final Changed
publication with its exact game, played revision, selection, epoch, generation,
and acknowledged storage revision. The wall-time provider fails after original
acceptance, making an attempted edit replay visible.

The failed shutdown results and original-receipt admission/result errors remain
available in the final semantic failure. The frozen source is expected to report
`SaveRequired` again after memory is released and `Closed` for the original
receipt retry. That prediction has not been observed here.

## Cleanup and verification boundary

Both cases complete supervisor shutdown observations before their final
assertions. Teardown records that no computation, save, queued write,
projection, deferred envelope, or factory request remains in flight. On the
expected shutdown red, it drops the quiescent failed actor after recording its
accepted state and failed recovery. That teardown cannot satisfy the success
assertions for durable publication or shutdown.

The tests release all publication-owning receipts, check that the actual client
can reserve its entire receiving-memory budget again, observe storage closure
and executor shutdown, verify full injected CPU-capacity release, and remove the
temporary database directory before evaluating semantic assertions. Watchdog
deadlines bound result and cleanup waits. Scheduling uses injected time and
capacity, with no wall-clock sleeps.

Executed checks are Rust source formatting/parsing of the appended code with
`rustfmt 1.10.0` in edition 2024, whitespace checks, frozen-prefix preservation,
owned-path inspection, patch hashes, reverse applicability, and forward
application through a temporary Git index with exact resulting-tree comparison.
These are source and artifact checks only.

No Cargo command, build, test, Clippy, application runtime, CPU diagnostic, agent,
extra worktree, commit, or push ran. No production source was edited beyond
applying the authorized frozen dependency. The parent owns compilation, both
observed semantic reds, the unchanged original suite, and subsequent repair
authorization.
