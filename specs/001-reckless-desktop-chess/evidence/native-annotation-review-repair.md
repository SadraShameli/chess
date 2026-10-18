# Native annotation review repair

This is a source-only handoff. This worker executed zero Cargo commands, builds,
tests, Clippy checks, application runs, CPU diagnostics, agents, commits, pushes,
or extra worktrees. Compilation and behavior are unverified here. Only the
approved pinned formatter and source/Git checks were used.

## Dependency and observed baseline

The clean checkout started at `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
The supplied absolute dependency patch was SHA-256 verified before applying with
`git apply --index`. Its staged tree matched
`b9bc17560f7812e4d0197bed6c0def843fd858d3` exactly.

The dependency bundle is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/native-annotation-two-red-baseline-20261017`.
Its `source.patch` SHA-256 is
`4d4685950f2739ca0d3a428e2e9cc424e0ba46dbe2e934c3a03f65c4cbd6ccd9`.
The supplied `native-annotation-review-semantic-reds.log` SHA-256 is
`90a3e34c5bc086aa3903da658d92d89e95c0bd7d5ed07e0a292d1c4bda5b183e`.
That log reports 23 passed and two failures: the guard rebind is a semantic
failure, while the first history attempt stopped with fixture `Busy`.
The supplied corrected `native-annotation-history-semantic-red.log` SHA-256 is
`a59223203ad6393d8f6e3b5a915cb072bb472a3de4e1368cbf2252bebd843274`.
It reports zero passed and one failed after 256 completed requests; request 257
emits nothing and reports `InputCapacity`. These are inherited root results,
not test executions by this worker.

## Repairs

The guard retains the entire originating editor context with its destination.
Discard captures the current publication but invalidates a queued action when
that full context changed. It records the invalidated input to suppress a
repeated pass. Equal FEN, node identity, and legal moves cannot transfer a queued
action to another game or publication binding.

The input ledger separates retained pending identifiers from 256 recent completed
identifiers. Completing work unpins it and evicts the oldest completed entry as
needed. Tree replies, completed annotation saves, completed checkpoints, and
replaced checkpoint retries retire their original identifiers. Pending requests
remain pinned independently of recent-history eviction. A deterministic source
test covers 768 completions while another identifier remains pending.

Both panels directly consume `DesktopLibraryServiceEnumResult` through
`observe_service_result(&DesktopApplicationReceipt, DesktopGameEditorContext,
&DesktopLibraryServiceEnumResult)`. The context argument is the original intent's
expected context. The annotation method returns an optional guard effect; the
tree method returns unit. The caller attaches the original actual application
receipt before delivering any result.

`Saved` consumes the actual Played or Notation receipt. Validation covers source
kind, game, epoch, generation, storage revision, and covered content revision.
Older acknowledged revisions may precede a newer publication, but they must
advance beyond the original expected revision. Acknowledging an earlier save
preserves a newer draft and the guard. Exact completion also handles publication
arriving before its result. `Editor(Edited { change: Unchanged, .. })` completes
without requesting storage. Changed editor results and pending results retain
ownership until completion. These entry points create no synthetic editor
results, receipts, checkpoint intents, or additional writes.

Both panels accept service errors through
`observe_service_error(&DesktopApplicationReceipt, DesktopGameEditorContext,
&Arc<DesktopLibraryServiceError>)`. A retained failure keeps the actual Arc and
requires its disposition's request and expected context to match the original
intent. Retry returns the original intent and requires the original attached
application receipt. Certified refusal retires the pending input and keeps the
draft and guard available for correction or Discard. Pending state correlated
through this API is tracked separately from externally synchronized pending state.
Generic `DesktopFailure` never releases pending ownership. Tree error dismissal
only dismisses feedback; its explicit retry also retains the original intent.

The existing low-level annotation owner APIs remain for their existing callers
and baseline checks. The service completion path does not call those checkpoint
APIs.

## Third-defect baseline stage and source tests

The independent tests-only bundle is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/native-annotation-service-refusal-tests-20261017`.
It contains only a test registration and the invalid-NUL service regression,
using the existing `observe_failure` API. Its current patch SHA-256 is
`80ee5edf873f766000cd399e7564bd61b38321f4184d639d2c797b56c011aa76`;
its result tree is `15df0a02949f08701eaff9d81ba9fd43d3420421`.
The prior patch hash `fd512a22fae5555a57065d8bdd905cb7c9fdf539fb32be5e62d60340c717068d`
is superseded by a correction to match the actual typed owner error wrapper.
The root must run
`invalid_notes_refused_by_the_real_service_allow_correction_and_discard`
at this tests-only stage before mounting the third-defect production repair.
There is no observed runtime red for this third defect in this worker's evidence.

The final regression routes the same real validation failure through the typed
service API. Seven additional direct-result tests use the existing real Library
service, registry, SQLite store, domain objects, application receipts, injected
clock, and controlled capacity. They cover both saved-content variants, exact
receipt correlation, publication-before-result order, unchanged markings,
pending snapshots, retained failure retry, tree saves and unchanged edits, older
acknowledgments with newer publications, and stale or foreign storage receipts.
Fixtures clean up before the added tests' behavior assertions. The older receipt
case pairs an actual earlier storage receipt with an actual later publication;
neither is constructed or exposed through a new constructor.

The original 23 checks and the two inherited semantic regression cases are
preserved. Nine additional tests are authored. All 34 focused tests remain for
the root's execution; this worker reports no passing test count.

## Cross-owner prerequisite and verification limits

The exact dependency tree does not contain the accepted
`DesktopLibraryServiceEnumDisposition::{Refused, Retained { request, expected }}`
or `DesktopLibraryServiceError::disposition()` API. The source references that
canonical API directly. The service owner's actual Continuation/disposition
artifact must be mounted before compilation. No mirror, stub, constructor
exposure, service edit, app routing edit, native edit, or root edit is included.

The root's actual `LibraryFailure(Arc<DesktopLibraryServiceError>)` routing must
deliver that Arc to the typed error entry point. Service retry aliases must keep
the original operation and receipt. Those cross-owner integration changes and
their runtime behavior are outside this patch.

Formatting uses only
`/tmp/claude-chess-pgo-diagnosis-20261016/with-pinned-rust rustfmt`
with edition 2024 and child traversal disabled for explicitly owned files.
Source patch applicability, index trees, hashes, explicit owned paths, and
`git diff --check` establish packaging evidence only. They do not establish
compilation, test success, native wiring, or runtime acceptance.
