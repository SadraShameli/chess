# Populated import enforced page budget

This is a source-only handoff. Cargo, builds, tests, Clippy, runtime SQL, CPU
diagnostics, agents, additional worktrees, commits and pushes were not run in
this repair lane. The parent owns compilation and behavioral acceptance.

## Frozen input and actual reds

The checkout started at `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68` and applied
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/native-annotation-two-red-baseline-20261017/source.patch`.
Its verified SHA-256 is
`4d4685950f2739ca0d3a428e2e9cc424e0ba46dbe2e934c3a03f65c4cbd6ccd9`;
the resulting staged dependency tree is
`b9bc17560f7812e4d0197bed6c0def843fd858d3`.

The complete supplied `import-populated-admission-semantic-reds.log` was read.
Its SHA-256 is
`9bc58b484b2311c30d255cc7e77fcc6d8b50a987a1d9607c7fdb673d425ebcf5`.
It records zero passed and two failed, after real unlimited controls and cleanup:

- 1000 existing games plus 1000 distinct tiny games used 8,559,497 additional
  bytes in the control. The 16,426,438,720-byte SQLite estimate refused a
  256-MiB allowance before the writer ran.
- A checkpointed unrelated document changed the document count from 63 to 64
  for an incoming batch of 64. The SQLite estimate jumped from 35,065,280 to
  178,768,632 bytes and refused 64 MiB. Both unlimited controls used 714,083
  additional bytes, with unchanged committed library content.

These are imported observed results, not tests executed in this checkout.

## Contract and formula

The exact-count probe still reads only covering primary-key identifiers, at
most K=81C per parent table. It never reads old payloads or database pages.
Reaching K remains `AtLeast(K)`, even when the actual cardinality happens to be
K. Existing probe cancellation checks and partial-index parent reuse remain.

For each imported tree j, D=20, V=81C, B=DC and incoming overflow charge O_j
retain their existing definitions. The operational allocation charge remains
A_j=2B+C+O_j. The structural page bound g_j(r) is 2r+1 for a rowid table and
r+1 for an index. These sparse-tree bounds count structural pages, not overflow
chains. A partial index can use its parent cardinality as an upper bound.

The incoming page budget is:

`E_j = min(A_j, g_j(C) + O_j)` and `E = sum(E_j)`.

E is an enforced budget. It is not a prediction that every valid existing
SQLite layout can import every admitted batch without hitting the ceiling.
No subtraction of g_j(R) from g_j(R+C) is used. Subtracting two structural
upper bounds would not bound the required incremental allocation.

The internal production field is `new_page_budget`; the tree entry point is
`plan_budget`. The existing commit path installs and verifies
`min(previous_max_page_count, N+E, SQLite_max_pages)` before inserting rows.
An unusually sparse valid tree or split cascade can exhaust this budget and
return SQLITE_FULL. The entire import then rolls back through the existing
transaction lifecycle. Ceiling restoration, compound cleanup causes and
connection retirement remain owned by the unchanged machinery.

For every `Exact(R)`, old structural pages are bounded by
`old_j = min(V, g_j(R))`, including R=C and R>C. For `AtLeast(K)`, old_j remains
V. No old cardinality is guessed after the bounded probe stops. This bound
counts every original structural page that could be touched, independently of
how full or sparse the actual tree is.

## Overflow, allocator and WAL argument

The pinned SQLite 3.53.2 amalgamation was inspected read-only. Its SHA-256 is
`0a409f1633283fa31a9126b11fbfd64a1991c5d30defad07e5745d4667f5e23d`.
In `balance_nonroot`, original cells and divider cells are copied with their
overflow pointers intact (81580-81660 and 81950-82020). Rowid table dividers
contain rowids, not copies of old body payloads. The balancing routine has no
`clearCell` or `fillInCell` call. Pointer-map rewriting is conditional on
auto-vacuum, which the layout reader already rejects. Import insertion checks
that the document ID is absent before the unchanged entry UPSERT, so the
production path does not replace existing body or entry overflow chains.
Large old bodies and index keys therefore do not introduce an old-overflow
rewrite charge for these canonical new-row writes.

The existing allocator-operation charge remains A_j+3B per tree, even though
the new-page budget is smaller. It is not reduced to E. The freelist dirty
allowance remains `min(initial_free_pages, 3*allocator_operations)`, with the
existing two header/catalog pages retained. Insertion balancing can retire
and reuse structural pages; their original pages are already included by the
old-tree charge. Newly allocated pages are either within the initial freelist
allowance or above N and constrained by N+E. No unrelated tree is scanned to
derive these charges.

Main-file demand still includes `(N+E)*page_size - actual_main_length`, floored
at zero. An uncheckpointed backlog remains chargeable even if the WAL file has
a retained physical high-water length. WAL demand still adds the allowed dirty
pages to the retained frame high water, plus a commit frame and sector padding.
SHM demand retains the existing block formula and actual-length subtraction.
In the no-savepoint production path, SQLite's WAL write loop at 71579-71597
rewrites an existing uncommitted frame for a repeatedly spilled page rather
than appending another frame, apart from the already charged commit frame.

For the canonical 14 trees at C=R=1000, 4096-byte pages, zero incoming overflow,
zero initial freelist, no main backlog and an existing 32768-byte SHM file,
static arithmetic gives E=16014, old_tree_pages=16014, dirty=32030 and 32047 WAL
frames including commit/padding. Main is 65,593,344 bytes, WAL 132,033,672 and
additional SHM 229,376, totaling 197,856,392 bytes (188.69 MiB). This is a static
example, not a measured post-repair import result.

## Conditional statement-journal omission

The owning production SQL constants and their callers are unchanged:
`INSERT_DOCUMENT_SQL`, `PROJECT_SQL` and `BUMP_CATALOG_SQL`. The migrated relevant
main tables have no triggers or deferred schema FK constraints. The actor
opens FK ON connections; the import owns a top-level Immediate transaction,
with no enclosing or import-created savepoint. The layout reader now also
checks FK ON and defer-FK OFF, and refuses unsupported modes with ImportLayout
before using this proof. Existing WAL/UTF-8/auto-vacuum/reserved-byte and main
trigger validation remains.

The complete parent-supplied
`/tmp/claude-chess-pgo-diagnosis-20261016/bounded-import-compiled-journal-tests-2.log`
was read. Its SHA-256 is
`eb6cc6775cce2208cd48f070203066aca05e2eee9a0fa04abd3dd6b5065c5d06`.
It records three passed and zero failed against these exact owning SQL strings
and the pinned SQLite source. The single-row writes, direct immediate FK Halt
paths and one-pass catalog update support the compiler argument in
[the prior handoff](bounded-import-planning-repair.md#canonical-sql-journal-proof-conditions):
`usesStmtJournal` requires both `isMultiWrite` and `mayAbort`. This lane did not
rerun those tests or observe physical VFS journal allocation.

Without TEMP triggers, the statement-journal charge is zero under those
unchanged SQL/schema/transaction invariants. Any TEMP trigger retains both the
conservative whole-N old-page path and the full logical statement-journal
charge. The original compiled TEMP fault-trigger positive control and actor
rollback controls are unchanged. There is no temp-store mutation, vendor edit,
unsafe code or new dependency.

## Tests first and remaining acceptance

Before production edits, three focused tests and their module declaration were
staged at tree `18a1ceb07b6b4e2769b5385dc02b458b4582449d`. The captured
tests-before-source patch SHA-256 is
`103d003ada6b9973205c2aaf3556e391f950946e9e713d071a08d122b456cbac`.
They cover the incoming-count boundary without an estimate cliff; a real
SQLite page-ceiling refusal after a prior document insert and catalog write,
with preservation, rollback and restoration; and actual FK mode validation.
All three are authored and unrun. The fixture uses JSON text accepted by the
real STRICT documents schema, and recognizes SQLITE_FULL explicitly.

The original old-note regression retains the same IDs and counts while only
payload bytes change. Every original assertion is retained, including its
actual covering-probe VM-step comparison and preservation/cleanup controls.
References to the renamed budget field/API are updated mechanically. Neither
populated admission test nor any canonical SQL compiler test was edited.

Two inherited checks need explicit parent attention:

- `tiny_import_bounds_are_stable_when_a_checkpointed_unrelated_document_is_added`
  changes the document cardinality from 2 to 3 while keeping the entry count
  fixed. Its equal-`old_tree_pages` assertion remains unchanged. Exact structural
  accounting now adds two table pages and one primary-key index page, so this
  assertion is expected to fail by three pages. Equal new-page budget and
  allocator-operation assertions remain valid. No bucketing or hidden fixture
  rewrite was introduced. The parent must execute and resolve this contract
  with its own observed evidence.
- `retained_wal_file_high_water_does_not_hide_checkpoint_backlog` remains
  unchanged, including `allowance < before.wal`. The incoming-only budget and
  conditional journal omission should reduce its allowance, but it was not
  executed here. A failure must be investigated as either accounting behavior
  or an unmet fixture precondition, without silently dropping that assertion.

Parent acceptance still requires the two original populated reds, old-note
VM/preservation regression, all focused planner checks, new transient ceiling
rollback test, canonical no-journal proof, TEMP rollback controls, retained WAL
and uncheckpointed backlog cases, SQLITE_FULL, cancellation, ceiling restoration
and retirement controls. No green result, full native acceptance or physical
allocation proof is claimed. Final export identity and static formatting/diff
checks are recorded in the export manifest.

## Parent integration observations

The first parent semantic run after fixing four test-only SQLite `u64` decode
errors recorded 53 passed and two failed, with no ignored tests and 577 filtered.
The preserved log is `parent-first-semantic-run.log` in the repair artifact
directory, SHA-256
`e9f1f34f2f3f977a3af971566b790cc397ac952d156454e761e6e8c749d64529`.

Both original populated regressions passed. The 1000-plus-1000 SQLite budget was
212,447,912 bytes and admitted the 256-MiB allowance; the measured total control
growth remained 8,559,497 bytes. The cardinality-boundary controls estimated
13,737,352 and 13,749,712 bytes and admitted 64 MiB. Retained WAL, uncheckpointed
backlog, transient page-ceiling refusal, rollback, restoration and retirement
checks also passed in that run. This is focused evidence, not full acceptance.

The two failed inherited assumptions were resolved in their fixtures:

- The internal exact-equality fixture added a row to an affected tree and
  observed the legitimate old-page allowance increase from 46 to 49. It now
  seeds the unrelated row before both observations and grows only its payload,
  keeping identity and cardinality fixed. All three equality assertions remain.
  The separate cardinality-boundary admission tests remain unchanged.
- The actor ballast fixture used the exact no-ballast computed allowance
  (452,026 bytes) as its free-space contract. One additional document required
  462,284 bytes after structural accounting, although both measured controls
  used about 65 KiB. The fixture now uses an independent 512-KiB allowance and
  still verifies that the baseline estimate and both measured controls fit it.
  The ballast is still 2 MiB, every preservation/cleanup assertion remains,
  and a planner that charges the unrelated body would still refuse this budget.

No production arithmetic was adjusted for those fixture assumptions. Their
rerun and the final canonical checks remain pending.
