# Annotation recovery through the actual application

The parent observed the three source-only recovery contracts through the actual
application, coordinator, Library service, source owners and temporary SQLite.
The baseline was one passed case and two failed cases in
`/tmp/claude-chess-pgo-diagnosis-20261016/notes-fresh-application-lib-baseline-3.log`,
SHA-256 `3e6350919767f61a157b42a26b201aa05a4cc5dbdc857a17262f6eab2be51111`.
The played publication-pressure control passed. The notation retry was refused
as stale, and the notes panel returned the original request identity.

The panel now retains both the original content intent and a fresh actual
application request for `RetrySave`. Original content validation remains bound
to the accepted edit. Receipt matching recognizes the actual retry receipt;
typed continuation errors remain bound to the original certified operation.
Completed and superseded retry input IDs leave the pending history.

The notation source maps an exact certified retained context to its actual
original reply and application receipt. It resumes saving the retained source
through the canonical owner. The fresh request does not resubmit notes, advance
the edit revision or acquire another edit timestamp. The original application
request continues to return its cached terminal failure; the service's actual
retained reply reconciles with the fresh request's durable result.

The corrected selection was:
`cargo test --locked --all-features --lib desktop::library_service::tests::notes_application_recovery_review`.
All three cases passed, with zero failed or ignored tests. The repaired log is
`/tmp/claude-chess-pgo-diagnosis-20261016/notes-fresh-application-repaired-tests.log`,
SHA-256 `036ac8e306a8bd3ce436c948f3afbf8251ee69ae9311ac5595190755f6050a1c`.
An earlier desktop-binary selection ran zero matching tests and is not behavioral
evidence. Compilation fixes for source-included test visibility and exhaustive
Editor command matching preceded the executable baseline.

The cases require one accepted edit, one wall-time read, original modified UTC,
exact source/storage revisions, real durable receipts, unchanged cached original
application failures and complete memory/capacity release. Cleanup runs before
behavioral assertions. See the [authored contract details](annotation-fresh-application-recovery-review.md).

Surrounding widget contracts, variation recovery, mounted UI and complete US3
acceptance require their own integration verification. These results do not
establish native GPU, accessibility, platform or release acceptance.
