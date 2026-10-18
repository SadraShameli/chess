# Import space repair source handoff

This is a source-only US3 T097/T100 repair authorized after the parent observed
the checkpointed unrelated-overflow regression. No Cargo, build, test, Clippy,
runtime or CPU diagnostic was run here. No agents, extra worktrees, commits,
pushes, dependency edits, root registrations or task-marker edits were made.
The global/root instructions, constitution, all seven canonical rules, US3
requirements, T097/T100, storage decisions and study/native-message contracts
were read. T097/T100 remain open.

The checkout starts at `465837a95c0de9129460171ea57a5edb2d054aac`. Applying
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-annotation-review-current.patch`
with verified SHA-256
`4cad29d61a745483cf92cff20a08b7e1365c5f378e16201de555c779243c5d27` produced
exact indexed baseline tree `d6aa39759f9be323023c0f4c4e6e404542c2c72d`, 41 paths.
The incremental export is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-import-space-repair.patch`.
Its adjacent JSON manifest records the final source tree, patch SHA-256, all
13 owned paths, baseline/final blobs and content hashes. Unchanged dependency
hunks and the separately owned snapshot-memory regression are excluded.

## Parent semantic red

The parent ran:

```text
cargo test --locked --all-features --lib storage::library_import::tests::space_review::checkpointed_unrelated_overflow_does_not_refuse_a_sufficient_import_allowance -- --exact --nocapture
```

Exit 101: 0 passed, 1 failed, 500 filtered out. The exact supplied log was read
and its SHA-256 independently verified:
`/tmp/claude-chess-pgo-diagnosis-20261016/library-import-checkpointed-space-red.log`,
`3a8973f8a9f2a29cae2349e14e3eac9cd4f514a48704a914c736102c529cefd8`.
The parent owns durable preservation. Setup, classifier/certification,
preservation, metadata, checkpoint and cleanup controls passed before the final
admission assertion. This was an actual semantic red, not a compilation error.

The successful baseline control had main/WAL/SHM lengths
204800/0/32768 before import, committed WAL 61832, and checkpointed main 204800
with WAL 0. Stage high-water 3601 plus source 45 gave 65478 additional bytes.
The successful checkpointed-ballast control had main/WAL/SHM
2301952/0/32768, the same committed WAL 61832, and no checkpointed main growth.
Stage high-water 3589 plus source 45 gave 65466 additional bytes. The frozen
45909042-byte allowance nevertheless failed with typed
`Space { required: 50113532, available: 45909042 }` solely after the unrelated
ballast was already present in the main file.

The original three-fixture regression remains intact: a real control under the
baseline allowance; the real document-client/adoption-classifier/certification
path followed by a truncating checkpoint and unrestricted ballast control; then
the same checkpointed ballast with constrained admission. It still requires a
successful final import, immutable retained bodies/revisions/annotations and
observed stage cleanup. No growth/refusal assertion was relaxed. Its four
wildcard error projections were only renamed to explicit `_poisoned` bindings.
Its filesystem metadata helper is now shared with the added spill fixture.

The original uncheckpointed
`puzzle_growth_without_catalog_change_invalidates_the_space_allowance` remains
byte-for-byte unchanged. It must still refuse: logical N*S minus actual main
length retains that uncheckpointed backlog even when the library catalog is
unchanged.

## Production API and proof

The public repository port is unchanged. Private space owners now capture actual
main/WAL/SHM lengths and layout, inventory affected trees through compiled-in
dbstat, derive schema-specific payload/overflow and growth bounds, and retain a
page ceiling. `StorageLibraryImportSpace::plan` returns the owning plan with
`bytes()` and `page_ceiling()` accessors.

`StorageLibraryImportPageLimit::install` records the previous main
`max_page_count`, installs the minimum of that value and planned N+E, and verifies
SQLite's returned value. Its scope lives in the existing transaction dispatcher.
`finish` restores only after observed COMMIT/rollback and verifies the restored
value. Primary, rollback and restoration errors remain typed and retained.
A committed restoration failure says that commit occurred. The actor retires on
unresolved transaction/ceiling cleanup. There is no new production connection,
runtime, global PRAGMA change, unsafe code or dependency/schema alteration.
Metadata request/response arms and certification transitions remain unchanged.

[The atomic-import space derivation](library-atomic-import.md#space-repair-after-the-parent-semantic-red)
contains exact formulas, payload constants, SQLite source locations, distinct-ID
and freelist bounds, WAL high-water treatment, subjournal accounting and limits.
The pinned amalgamation hash was independently checked. The chosen structural
page budget is enforced during transient writes; arithmetic alone does not prove
that every transient SQLite shape succeeds.

## Authored, entirely unrun counterchecks

The original private suite retains 28 cases, including its populated-index,
Unicode/long-line/2048-ply growth test and uncheckpointed refusal. The review
module now has 12 cases: the original semantic red and 11 additional cases.
Thus this frozen source declares 40 private import tests. The two existing
native import cases are unchanged dependency paths. None of these counts is an
execution result for the repair.

The 11 added cases cover:

1. A real 1000-record import admitted with 256 MiB free, including staging/source,
   pre/post-commit/post-checkpoint file lengths, retained annotations and an
   actor trigger requiring a lowered ceiling during inserts. It verifies
   restoration afterward. The original estimator's multi-GiB requirement cannot
   meet this practical admission assertion.
2. A retained multi-MiB WAL after PASSIVE checkpoint, with actual file-growth
   comparison and an allowance smaller than that already allocated WAL file.
3. A real final-catalog SQL abort after inserts, preserving the exact primary
   diagnostic, all missing imported IDs and the restored ceiling.
4. An existing stricter actor ceiling, actual SQLITE_FULL on large Unicode
   content, complete rollback and unchanged prior limit.
5. A lower-level real SQLite insert/commit where the requested scope is looser
   than the existing limit, requiring the stricter actual and restored value.
6. A lower-level actual SQLITE_FULL while a four-page ceiling is active, with
   observed page_count <=4 and restoration after SQLite's automatic rollback.
7. Actual SQLite errors for rollback and a forced restoration mismatch after
   controlled out-of-scope test growth. The resulting compound error must retain
   primary, rollback and ceiling causes and require connection retirement.
8. A deliberately unresolved real transaction, retaining its primary cause and
   active scope while requiring retirement. The test itself closes the
   transaction it owns afterward.
9. SQLite clamping a requested limit below current page_count, requiring typed
   requested/actual values and restoration after rollback.
10. A small import after 1000 existing library records are checkpointed, admitted
    under 16 MiB, with actual main/WAL/SHM growth under the allowance.
11. A fresh actor with four-page cache and spill enabled, large Unicode player
    and comment content, and a controlled block before reading the second frame.
    Cancellation is requested and observed before filesystem sampling. Nonzero
    retained WAL proves pre-commit spill: the starting WAL is truncated, no
    commit occurred, and pinned WAL undo does not truncate file high-water.
    Imported IDs remain absent, checkpoint growth fits the bound, and the
    previous ceiling is restored. Sampling after cleanup avoids requiring spare
    CPU capacity while the actor is deliberately blocked.

The first eight counterchecks were authored before production changes; three
further source counterchecks were added during review. No new semantic red or
green is claimed. Five direct SQLite component tests own their isolated
in-memory test connections; they exercise the production page-limit owner.
Application/import-path counterchecks use the existing real actor and provider.
There is no alternate production commit or synthetic page-count override.

Existing late-read cancellation now also asserts restoration after observed
cleanup. Two SQL fault fixtures are installed before freeze, so conservative
trigger accounting is part of the confirmed budget. Their assertions now require
the intended `catalog refused` and `UNIQUE` causes, rather than accepting an
earlier unrelated allowance refusal. No failure is skipped or weakened.

The parent additionally supplied its frozen-baseline all-target/all-feature
Clippy log at
`/tmp/claude-chess-pgo-diagnosis-20261016/library-annotation-clippy-before-repairs.log`.
The 9 owned production and 21 owned test `map_err_ignore` findings were corrected.
Meaningful SQLite value, import-response, UTF-8, byte-array and integer conversion
causes are preserved. Mutex-guard poison and retained-Arc fixture projections use
explicitly named ignored bindings. The parent's separately owned finding in
`library_prepared/import.rs` was not changed. No Clippy rerun occurred here.

## Parent execution and unresolved acceptance

Required first executions are the exact former red, all 40 private import tests,
the unchanged native import cases, useful-scale counterchecks, and all-target
all-feature Clippy. Parent alone owns shared-target default-parallel Cargo and
canonical gates. The supplied pre-repair results of 28 private plus 2 native,
20 metadata, and full 473 private/234 desktop/181 storage tests are context;
they do not validate this frozen repair. The independently owned annotation
memory repair and played-editor/domain work are excluded from this export.

The estimate includes a `U*(S+4)` anonymous statement-journal allowance, but the
current resource port cannot separately observe SQLite's potentially different
temp filesystem. Exact temp-volume admission and anonymous-file high-water are
physical-space acceptance blockers. Logical file lengths do not prove allocated
blocks, physical copy-on-write storage, RSS, million-game throughput or target
VFS behavior. dbstat traversal cost and unusual transient-ceiling refusals need
parent measurement/review. T097/T100 must not be marked complete from this source.

Only standalone rustfmt and Git/source-scope checks were used. These establish
format/applicability evidence, not compilation or behavior. Source is frozen at
export for parent compilation and execution.
