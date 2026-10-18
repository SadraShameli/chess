# Native annotation review regressions

This increment authors two tests for the reported annotation UI defects before
any repair. Zero tests were executed. Neither failure has been observed at
runtime in this lane. The original 23 annotation UI tests remain unrun and their
bodies are unchanged. This is not production acceptance or repair authorization.

## Frozen dependency baseline

- Base commit: `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
- `library-bounded-review-current.patch` SHA256:
  `1e33879828e67a43f090dfb8dbaab1642f5f46e3637572a86b1021d1b6ce3588`.
- First dependency tree: `b1a08663d700e714f34ccb0f8dfce6d1b5137414`.
- `native-annotation-panels.patch` SHA256:
  `595d61c51e092617389b54ab03fa0878c1f595e143cc8a888da95c1fbc703046`.
- Combined frozen tree: `c38631e92f244ec1327db11d6073dfbb8cc2c948`.

Both dependency checksums and the first dependency tree were verified before
authoring. The export is incremental against the combined tree. Its external
manifest records the final tree, patch checksum, paths and static checks.

## Exact incremental paths and registration

- `tests/presentation/analysis/annotation_guard_review.rs`: new guard regression.
- `tests/presentation/analysis/editor_history_review.rs`: new history regression.
- `tests/presentation/analysis/annotations.rs`: append one private path module.
- `tests/presentation/analysis/variation_tree.rs`: append one private path module.
- `specs/001-reckless-desktop-chess/evidence/native-annotation-review-tests.md`:
  this evidence.

The two existing mirrored test roots receive only appended registrations. Both
child modules reuse the existing crate-private canonical fixture from
`presentation::analysis::editor_intent::tests`. No production source changes are
included. The parent still owns registration of `annotations`, `editor_intent`
and `variation_tree` in `src/presentation/analysis/mod.rs`. Those registrations
must exist before the parent can compile or select these tests.

## Guard origin regression

Test name:
`discard_never_rebinds_a_guarded_board_move_to_another_game_with_the_same_fen`.

Two actual notation owners provide permit-backed publications at their roots.
The test checks equal FEN, distinct game identity and source epoch, and a typed
legal board move obtained from game A that is also legal in game B. It dirties
game A's notes, queues `AppendStudyBranch` with an actual admitted request,
synchronizes game B, and selects Discard with another admitted request.

The observation records draft preservation, conflict, guard disposition and
pending state. It never submits the returned move to either owner. If a command
is emitted, it must retain the original request, node, typed move and complete
game A context. Explicit conflict refusal, guard invalidation or cancellation
are accepted. Discard may restore game B's authoritative notes without emitting
the old action against game B.

All owners and the panel are dropped before the storage client reserves the
entire canonical receiving-memory budget. That permit is released and storage
and the injected executor are closed before final assertions, including the
context assertion. The test also records that live publications charged that
budget. It adds no parallel Navigate matrix because the origin loss is shared.

Source-reviewed expected failure: `resolve_guard(Discard)` currently takes the
latest publication context and combines it with the old queued destination, so
the final admitted command carries game B's context instead of game A's.

## Completed request retention regression

Test name:
`one_tree_panel_remains_usable_after_256_completed_canonical_page_requests`.

One actual owner and one native egui test harness retain one variation-tree
panel for 257 sequential Load Moves inputs. Each input receives a distinct
actual application receipt and must emit exactly one canonical `TreePage`
intent carrying that request identity and the full unchanged source context.
The panel receives the actual attached receipt and actual owner result. Each
source page must be nonempty, contain at most the canonical 100-row limit and
match the installed panel page exactly. A foreign admitted receipt must not
consume that result or clear the pending request.

Every successful input is replayed while pending and recently completed.
The previous completed identity is also replayed during the next request.
The harness forces discarded egui passes on emitted effects and runs repeated
frames before and after completion; extra intents or failures are rejected.

An admission failure is retained as an observation instead of propagated before
cleanup. The harness, owner and pages are dropped, then storage and the injected
executor are closed. Final assertions require the first 256 results to complete
and the 257th new input to remain usable. The test defines no new memory limit,
timing budget or wall-time benchmark. The old evidence's lifetime-cap description
does not replace the requirement that normal panel use continue.

Source-reviewed expected failure: 256 requests complete, then input 257 emits
zero intents with `PresentationAnalysisEditorError::InputCapacity` because the
current history never retires completed identities.

## Deferred observation contracts

A generic `DesktopFailure` does not certify that accepted source work stopped.
No test in this increment requires a generic failure notice to clear a pending
draft or release a guarded action. A later regression needs a source-certified
terminal-refusal disposition, distinct from accepted or pending work, before
testing correction of invalid NUL notes or Discard recovery.

The existing service already returns
`DesktopLibraryServiceEnumResult::Saved { snapshot, receipt }`, where the typed
receipt is `Played(GameStorageReceipt)` or
`Notation { receipt: Arc<StorageLibraryContentReceipt>, covered: GameRevision }`.
The current UI expects `Edited`, a fresh Checkpoint and notation-owner save
progress. The parent will design a direct service observation API in a later
round. Its tests should use actual played and notation service completions,
correlate their receipts and coverage, retain newer drafts, and release a guard
only after certified completion. This increment fabricates no edit result,
notation receipt or redundant save, and calls no proposed observation method.

## Verification boundary

Only source reading, standalone formatting and Git integrity checks are allowed
in this lane. The manifest records what was actually performed. No Cargo,
tests, builds, Clippy, application runtime, CPU diagnostics, agents, extra
worktrees, commits, pushes or production repairs were run. Shared target,
runtime ownership and default parallelism remain with the parent. The parent
must observe both semantic failures before authorizing a production repair
round. Compilation, semantic red results, repaired green results, native
mounting and all broader acceptance remain unverified.
