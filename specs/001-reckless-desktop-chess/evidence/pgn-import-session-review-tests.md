# PGN import session commit proof repair handoff

## Current incremental round

The current repair depends on the exact r1 tree
`018a80805d1323d015c96dcb0342d2aff492c491`. Both supplied patches were
SHA-256 verified and applied with their indexes, in order: the full parent
lifecycle baseline first, then `pgn-import-session-review-repair.patch`.
The r1 metadata and dependency details are preserved below. The new export is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/pgn-import-session-commit-proof-repair.patch`.
It contains only this round's delta against r1. Its sibling JSON records the
result tree, patch hash, paths, static checks and verification limits.

The parent executed the r1 source with
`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib storage::import_session:: -- --nocapture`.
The raw log is
`/tmp/claude-chess-pgo-diagnosis-20261016/pgn-import-session-review-repaired-tests.log`,
SHA-256 `16294ae4bdbe15bcc6286db6eb46a4919ce13df5b1fb7ada18e84aa100cdd73e`.
It reports exit 101, 66 passed, 6 failed, 0 ignored, 281 filtered, 0.84 seconds.
This writer read that log and did not run the command. The original 11 anchored
annotation regressions pass in that parent run. Clippy has not been rerun by the
parent since r1, so the earlier seven source lint repairs remain unverified.

Five observed behavioral failures return `Repository(Effect(Proof))`:

- `cancelled_commit_wait_resumes_once_without_a_second_insert`
- `committed_cleanup_failure_is_retained_and_cannot_insert_again`
- `exact_duplicates_require_explicit_keep_copy`
- `existing_annotations_are_presented_as_differences_and_never_overwritten`
- `review_complete_classes_preserve_per_record_counts_copies_and_annotations`

The parent reports the first four passed before r1. The file ownership trace
finds a shared production defect: `take_prepared` still popped the last spool,
but r1 appended complete-index, class-index and class-member files after Commit.
It transferred ClassMembers to the prepared consumer while retaining the actual
Commit file. An unchanged, correctly frozen commit therefore failed its stream
proof. The disk owner now transfers the file identified by its typed Commit
index after checking the complete spool inventory. Remaining handles stay owned
by the session for terminal cleanup. Spool creation order and the existing
commit-corruption fixture's handle remain compatible. No class/index rebuilding,
candidate-query port, identity policy or annotation implementation changes.

The sixth failure, `review_matching_annotations_remain_anchored_to_the_initial_position`,
stops at its initial-position `assert_ne!`. Its two FENs had identical pieces,
turn, castling rights and en passant state; only the fullmove counter differed.
The installed shakmaty 0.30.2 `Chess::eq` ignores move counters, as confirmed in
the dependency source. This is a fixture error, not observed production semantic
evidence. The existing canonical SetUp/FEN convention is retained and the right
fixture moves the black h-pawn from h7 to h6. `Nf3` remains legal from both
starts. The initial-position, tree and notes assertions are unchanged. No parser
or identity source is edited.

One additional test was authored before the production repair and remains unrun:
`selected_commit_frames_preserve_record_identity_choices_and_annotations_after_refreeze`.
It starts with a complete duplicate class defaulting to Skip, freezes once,
skips the original representative, chooses explicit Keep Copy for the duplicate,
and refreezes. It refuses the old confirmation and consumes the replacement
through the unchanged prepared-frame verifier and verified EOF. It checks selected
ordinals 2 and 3, original per-record game IDs, actual recovered body IDs, projection
IDs, Keep Copy versus Import choices and distinct per-record notes. Freeze still
verifies the immutable source, directory chain and structural/identity roots.
Static caller inspection finds class files are not modified by Choose or Freeze;
neither mutable choices nor default Skip alter class assignment or immutable
index roots. The original five red cases and all corruption/lifecycle assertions
remain intact. The new test is supplemental source coverage, not an observed red
or green result. Expected discovery is 73 cases with the parent's registration.

`prepared.rs` is byte-identical to r1. Its actual-frame local SHA checks, ordinal
ordering, selected choice requirement, reserved size bounds, exact frame count,
stream length, physical EOF and retained aggregate digest are unchanged. The
producer still verifies all original metadata/body/projection spans before
writing the selected stream and binds both preview decisions and stream digest
into confirmation. Existing body/projection plus checksum tampering, later-frame
truncation, trailing bytes and frame-order tests are preserved for root rerun.

The parent's separate identical-complete operation budget passes for r1:

| Games | Reads | Read bytes |
| --- | --- | --- |
| 8 | 188 | 49,461 |
| 16 | 364 | 97,341 |
| 32 | 716 | 193,101 |

These are measured narrow fixtures from the parent log. This round changes no
class/scaling algorithm. Arbitrary Potential relationships, collision workloads,
full corpus throughput, RSS, disk high-water, one/max-worker, platform and release
acceptance remain open. The new repair has no child runtime or performance result.

The permitted final checks are pinned standalone Rust 1.99.0 rustfmt, whitespace,
scope/protected-blob inspection, added-source policy inspection and forward/reverse
binary patch applicability. Cargo, compilation, tests, Clippy, builds and CPU or
runtime diagnostics are reserved for root and were not run here. No agents,
commits, pushes, extra worktrees, root registrations, protected lifecycle tests,
filesystem owner, library, schema, actor, native owner, codec or dependencies were
changed. The source and staged snapshot are frozen at export.

## Historical r1 handoff

This repair is source-ready and unrun by this writer. The parent owns Cargo,
Clippy, builds, tests, runtime diagnostics and final integration. No checker
suppression, dependency, schema, actor, native owner or task marker is changed.

## Exact dependency and incremental export

The checkout starts at `647147d9ff95cb745ede46637048ef312e3070af`.
The complete supplied dependency was applied with its index and verified as Git
tree `d688d1ade0970a6ccd3e94d5229815d4d1f46197`:

- Patch: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/pgn-import-session-parent-lifecycle-baseline.patch`
- Metadata: `/Users/sadrashameli/Personal/chess/.git/parallel-patches/pgn-import-session-parent-lifecycle-baseline.json`
- Patch SHA-256: `11df57d5d18c3b7876531d2efca2aafb771da456bf18febfdf493d2c82e98d79`

The export is `/Users/sadrashameli/Personal/chess/.git/parallel-patches/pgn-import-session-review-repair.patch`, relative to that exact tree. It excludes the dependency's unchanged foundation, original 41 cases, original 22 review cases, parent compiler corrections, root registration and three parent lifecycle repairs. The completed-cache conditional is collapsed only to address the parent's subsequent Clippy finding; its retention condition stays identical.

## Root execution evidence read for this round

All results below are parent executions read from their existing raw logs.
They are not child executions or outcomes for the new repair.

| Raw log under `/tmp/claude-chess-pgo-diagnosis-20261016/` | SHA-256 | Root result |
| --- | --- | --- |
| `pgn-import-session-review-red-2.log` | `3a4cc64cdef585f0d5a1b1ff4109248ce09f5a90cac65df3ce0766606fa6ee4f` | 43 passed, 20 failed, 0 ignored, 281 filtered, 0.66 seconds |
| `pgn-import-session-parent-lifecycle-repair.log` | `312f07f1902ed18799415d2f23c2921f9c42ee9b89fa969baae1784829c1e228` | 52 passed, 11 failed, 0 ignored, 281 filtered, 0.57 seconds |
| `library-recovery-import-integration-clippy.log` | `6aa5e014094e56cfb447da9bbcb32bc746102d774c395b76b7fddafd5093b60a` | Exit 101; seven import-owned source lints, no semantic test run |

The parent reports the original 41 foundation cases passed. Its full dependency
already fixes four primary-cause retention cases, four read-only cancellation
cases and the replaced-orphan deletion case. The first log's 20 reds and second
log's remaining 11 reds establish the annotation repair authorization. The
cancellation, retention and filesystem test files remain byte-identical to the
supplied dependency. Commit settlement retention is preserved.

The seven Clippy repairs use a typed notation projection, stable array chunks,
typed parser options and a collapsed completed-cache condition. No lint settings
or suppressions change. These corrections have not been rerun here.

## Annotation correspondence

`StorageImportSessionDifferences` retains independent ordered-tree and selected
mainline comparisons. Annotation comparison traverses paired corresponding
parents iteratively and matches children by the existing variant-correct UCI
string. Exact string keys in ordered maps and a duplicate-child refusal protect
correspondence without a path hash or copied full paths. The initial position
is compared before roots are paired. Distinct parent paths stay distinct even
when descendant UCI moves or final positions coincide.

Unmatched subtrees compare against empty annotations. An unannotated new branch
therefore changes only the tree. Reordered siblings with their annotations moved
to different moves change the affected category; annotation-preserving reorder
keeps those categories equal. NAGs, markings and node directives retain their
within-node order. Unknown directives are indexed once by node, and source
clocks use the canonical `NotationGame::clock` index. Rating warnings retain
their separate ordered comparison. No second notation or move parser is added.

The original 11 root-red assertions are unchanged. Five additional authored,
unrun counterchecks cover deep path correspondence, an unannotated branch's full
category result, initial-position anchoring, within-node annotation/directive
order, and preservation of all anchored categories across a variation reorder.
These checks do not constitute additional observed reds or greens.

## Identical complete identity classes

Preparation builds temporary disk indexes for complete identity fingerprints,
exact class representatives and per-record membership/count state. Fingerprints
consume the existing canonical `NotationGameIdentity::hash`; they are local
accelerators, not a second identity serialization contract. Every class assignment
requires full `NotationGameIdentity` equality, and class membership is limited to
identities whose canonical self-comparison is Exact. Digest collisions scan
separate representatives on disk, comparing full identities.

Complete classes retain their first ordinal and a checked count of previously
seen records. Staged comparison weights canonical Exact/Potential comparisons by
that record count. Partial identities remain individual representatives and are
never unioned. Thus an unknown player can bridge conflicting known players
without merging them. Choice, game ID, metadata, body, projection and annotation
relationships remain per record. Details decode requested records lazily; a staged
detail page reuses its target record and identity across that bounded page.

The identical-complete bottleneck changes from reconstructing every pair to a
bounded number of per-record reconstructions plus representative work. There is
no map retaining all identities. Three added spool files require at most 96 bytes
per source record, bounded by the existing million-game directory limit. The
existing 40-byte-entry sort scratch and 128 MiB batch owner are reused. Appends
retain stage-space checks, and all new files participate in total-length accounting
and existing cleanup ownership. Record bodies, parser/text limits and the 64 MiB
codec budget remain unchanged; these limits do not establish whole-process RSS.

A structural bucket with N records and C distinct complete classes or individual
partial records still requires up to O(N*C) canonical comparisons. Complete-key
fingerprint collisions can require pairwise collision counterchecks. Existing
library candidates are still checked per imported record. Arbitrary Potential
relationships and requested per-record relationships can themselves be quadratic.
Full corpus scale, throughput, temporary-disk high-water measurement, RSS and
one/max-worker acceptance remain open.

## Recorded scale baseline and new budget

The existing scaling review case is green observational coverage. It was never
an observed semantic red or an existing linear acceptance gate.

| Games | Identical reads | Identical read bytes | Distinct reads | Distinct read bytes |
| --- | --- | --- | --- | --- |
| 8 | 272 | 74,124 | 151 | 35,770 |
| 16 | 824 | 236,376 | 320 | 72,580 |
| 32 | 2,728 | 822,704 | 689 | 147,456 |

Those counts appear in both root logs. After recording that baseline, this repair
authors a separate deterministic test for identical complete batches of 8, 16 and
32: at most `48*N + 64` filesystem reads and `16,384*N + 8,192` read bytes.
These are fixture-specific operation budgets, not timing or environment caps.
The recorded 32-game baseline exceeds both new bounds (1,600 reads and 532,480
bytes). The new assertion has not been executed on either baseline or repair, so
no test red or improved measured count is claimed. Root must execute it and assess
its operation output before accepting the optimized bound.

A second new scaling case checks every record's Exact/Potential counts across
complete Alice and Bob classes and two unknown-player bridges, individual IDs,
annotation relationships and an explicit five-record Keep Copy commit. A new
foundation-file countercheck forces complete fingerprint collisions across both
conflicting known players and distinct structural identities, requiring class
representatives `[1, 2, 1, 4]`. Both are authored and unrun.

## Supported backend lookup API

`StorageImportSessionCandidateQuery` is Clone, not Copy. It holds an
`Arc<NotationGameIdentity>` shared with the current staged identity and exposes:

```rust
pub fn structural_key(&self) -> &NotationGameIdentityStructuralKey;
pub fn structural_digest_for(
    key: &NotationGameIdentityStructuralKey,
) -> Result<[u8; 32], StorageImportSessionError>;
pub const fn catalog_stamp(&self) -> StorageImportSessionCatalogStamp;
pub const fn structural_digest(&self) -> [u8; 32];
pub const fn cursor(&self) -> u64;
pub const fn limit(&self) -> usize;
```

The canonical key already provides borrowed `initial_fen()` and `mainline_uci()`
and typed `variant()`. Its validated identity owner remains authoritative.
`structural_digest_for` owns the existing compact JSON tuple SHA-256 policy;
stage and query construction consume it, and the backend can consume it alongside
its canonical FEN/UCI columns. Candidate matches still require full canonical
identity comparison. No schema, structural deserializer, FEN parser, ICU path or
identity DTO registry is introduced. A new authored, unrun contract case verifies
shared key ownership and the borrowed usable fields.

## Verification and exact scope

Nine additional cases are authored, giving an expected 72 discovered cases when
the parent's Unix registration is present. Discovery and execution are unverified.
Permitted verification is standalone pinned Rust 1.99.0 rustfmt, static diff,
no-comment/suppression/scope checks and incremental patch applicability checks.
Cargo, compilation, tests, Clippy, builds and runtime diagnostics were not run.
No agents, commits, pushes or additional worktrees were created.

Incremental source paths are `src/storage/import_session.rs`, its
`classes.rs`, `difference.rs`, `disk.rs`, `index.rs`, `model.rs` and `stage.rs`.
Test deltas are confined to `tests/storage/import_session.rs` and review
`differences.rs` and `scaling.rs`. Evidence deltas are this note and
`pgn-import-session.md`. The filesystem, prepared-commit and lifecycle test
owners remain unchanged. Root module registration is excluded.

## Parent canonical integration

The combined source and all relevant regressions were executed by the parent.
[Verified integration](library-runtime-import-integration.md) records the exact
snapshot, 1,666 passing tests, three clean Clippy selections, both optimized
builds, durable log hashes, known linker warning and remaining acceptance gates.
Earlier unrun statements describe this writer's isolated handoff only.
