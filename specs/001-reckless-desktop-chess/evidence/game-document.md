# Durable game documents

Typed game documents persist the complete ordered move tree, annotations,
metadata, clock snapshots, warning flags, selected position and interruption or
result together. Loading validates the schema envelope before decoding its
payload, then reconstructs legal positions and clock transitions. Recovery
charges no downtime and requires explicit Resume. Ephemeral premoves and local
takeback requests do not survive relaunch.

Database compare-and-swap revisions are separate from game revisions. Repository
conversion runs on the shared bounded CPU executor. Cloneable database clients
hold weak senders, so retained clients and response waits cannot keep database
shutdown alive; admitted writes still drain through the one SQLite worker.

Regression tests reproduced and then fixed continuation beyond an automatic
result and retention of a stale premove after an accepted manual move. Exact
duplicate restored nodes are rejected. Local result consent is bound to the
game, revision, live position and chosen result. Voluntary results preserve
flagged-clock evidence rather than claiming an unresolved proof succeeded.

The focused debug run passed 152 domain, 23 presentation and 26 storage tests;
targeted Clippy passed for those targets and the library. Native storage callers
were updated to the client API. Production autosave dispatch, failure/retry
coordination and native relaunch acceptance remain pending in T065/T068. No
optimized build or cross-platform acceptance is claimed. The existing macOS
debug linker unwind-table warning remains.
