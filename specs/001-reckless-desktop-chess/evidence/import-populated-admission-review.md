# Populated-library import admission regressions

Status: two authored regressions, zero executed. Compilation, actual red results,
runtime cleanup and measured control values remain parent-owned. No GO is given.

## Frozen input and owned scope

The checkout starts at commit
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`. The supplied baseline is the shared
absolute patch
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-bounded-review-current.patch`,
with SHA-256
`1e33879828e67a43f090dfb8dbaab1642f5f46e3637572a86b1021d1b6ce3588`.
Applying that patch to the index produced exactly tree
`b1a08663d700e714f34ccb0f8dfce6d1b5137414`. That tree is the frozen parent input,
not a new commit. The incremental export contains only this evidence file and
`tests/storage/library_import_populated_review.rs`.

The global guidance, project bridge, constitution, rule catalog and all seven
[canonical rules](../.specify/rules/README.md) were read. Existing helpers,
registration roots, thresholds and production source are unchanged. The baseline
dependencies are deliberately absent from this export.

The parent supplied a bounded-planner result of 49 passes and one failure:
`retained_wal_file_high_water_does_not_hide_checkpoint_backlog` fails its setup
assertion `allowance < before.wal` after the new estimate. This lane neither
diagnoses nor changes that failure. The supplied successful actual VM-count
repair does not establish useful admission for populated bulk appends.

## Parent registration

The new file must be a child of
`tests/storage/library_import_space_review.rs`. Parent adds this declaration to
that existing file:

```rust
#[path = "library_import_populated_review.rs"]
mod populated_review;
```

This placement lets the descendant reuse the actual private review fixture and
its filesystem measurements. All additional helpers remain private in the new
child. No public test API or second production writer is introduced.

Expected full test names after registration:

- `storage::library_import::tests::space_review::populated_review::a_thousand_existing_games_admit_a_thousand_distinct_tiny_games_with_measured_headroom`
- `storage::library_import::tests::space_review::populated_review::one_checkpointed_unrelated_document_at_the_import_cardinality_boundary_keeps_useful_admission`

## Common bulk append

The first regression creates exactly 1000 existing games: the fixture's retained
annotated game plus 999 distinct annotated PGNs committed through the real
session, repository and SQLite actor. It then imports 1000 further distinct tiny
PGNs. Existing and incoming games use different opening moves and unique White
headers, a fixed Black header and date, deterministic identity-provider seeds and
the fixture's fixed modification timestamp. Every staged record must be valid,
selected and nonduplicate.

The first complete trial uses unrestricted injected availability. It commits and
checkpoints the additional 1000 games, checks all 2000 bindings and reopens the
same database to verify durability. Only after storage and executor cleanup does
it require the measured additional file demand to be below 64 MiB, one quarter
of the 256 MiB admission allowance. Its one-line diagnostic records the actual
before, committed and checkpointed DB/WAL/SHM lengths, their combined additional
high-water, the spool high-water, original source length, computed SQLite
allowance and commit-port invocation count.

A fresh matching fixture repeats the same population and incoming source with
256 MiB of injected availability. Initial bindings and checkpointed file lengths
must match the control. Successful admission must preserve the complete control
image of all 2000 bindings. A refusal must leave all incoming IDs absent and all
1000 original bindings unchanged, including annotations, organization, location,
content revision and modification timestamp. Both branches verify persistence
after closing and reopening the canonical store.

The intended old-source red is the final admission assertion: a typed `Space`
failure during freeze, with required bytes above the useful allowance and no
incoming commit-port invocation. Cancellation/drain, spool release, retained
content checks, SQLite closure and executor shutdown precede that assertion.
There is no early assertion that the computed estimate itself must be small.

## Cardinality boundary with one unrelated document

The second regression fixes incoming count C at 64. Initially both affected
tables contain R = 63 real indexed games. Adding one small checkpointed unrelated
document makes documents contain 64 rows while library entries remain at 63.
The exact counts are asserted through the actor's existing real SQLite SQL
fixture. The ballast is a valid unrelated JSON document, not a game or an
overflow-sized allocation.

Two unrestricted real commit/checkpoint controls measure actual additional file
demand, one without ballast and one with ballast. Each must fit below 16 MiB,
one quarter of the fixed 64 MiB useful allowance. Their complete game bindings
must agree; their computed bounds and physical layouts are allowed to differ.
A third fresh fixture first freezes the 64-game import with the useful allowance
before ballast, cancels and drains it, then adds ballast and rebuilds the same
import with the same allowance and identities. The final rebuilt import must be
admitted after cleanup. This checks useful admission on both sides of the
boundary without requiring equal computed bounds for different cardinalities.

The public canonical document save cannot preserve the catalog stamp here.
`StorageDocumentActor::save` bumps the library catalog and clears adoption
certification. The test explicitly asserts both effects and an unchanged import
destination. It asks the real adoption API to classify the new document as
`Unrelated`, recertifies the new catalog, asserts that recertification preserves
that new stamp, checkpoints, and creates a fresh import session. The old frozen
confirmation is never reused across the catalog mutation. No direct SQL insert,
catalog rollback, hidden certification bypass, synthetic cardinality or foreign
writer is used. This is a legitimate rebuilt production-admission regression,
not a claim that a stale confirmation remains valid after an unrelated save.

The baseline's structural cap currently applies only to exact R < C. The
independent review's approximately 15.28 GiB estimate for a 1000 + 1000 workload
is static arithmetic. It is neither an actual red nor a performance measurement.
Only the parent's runs can establish either new failure and its actual required
bytes and control demand.

## Verification and limits

Executed checks are standalone Rust formatting and Git/source-scope checks.
They do not establish compilation or runtime behavior. No Cargo, tests, builds,
Clippy, application runtime, CPU diagnostics, agents, extra worktrees, commits,
pushes or production repairs ran in this lane. The tests retain the parent's
default test parallelism and use five fixture lifecycles across the two cases,
with only one fixture open at a time within each case. Failures identify one
changed binding without dumping the entire library.

The measured control is conservative additional named-file length: growth of
`import.sqlite`, `import.sqlite-wal` and `import.sqlite-shm`, plus all observed
spool lengths at their maximum and the original source length. WAL
autocheckpointing is disabled and measurements bracket actual COMMIT and TRUNCATE
checkpoint. This does not measure allocated filesystem blocks, anonymous SQLite
statement journals, copy-on-write amplification, RSS, latency or throughput.
No elapsed-time threshold or SQL mock is present. The existing whole-database
clamp regression and the retained-WAL setup assertion are untouched.

Parent must register and compile these two tests, execute them against the exact
frozen input and inspect the controls before treating either as an actual red.
If setup, compilation, adoption, scoped ceiling or cleanup fails first, that is
not the intended admission red. A repair requires parent reruns of these cases
and its existing acceptance checks; this source-only handoff accepts no scale
claim and changes no task completion marker.
