# T105 annotation and variation domain foundation

Recorded 2026-10-17. This is a source-only isolated-writer handoff. T105 story
acceptance, parent integration, compilation, tests, Clippy, optimized builds,
storage receipts, native handlers and UI acceptance are not established here.
No task or checklist markers were changed.

## Dependency baseline and incremental export

The local commit is `39994327f2742cb51c80a02d54d9d72943be3269`.
The exact supplied dependency is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-codec-integration-current.patch`,
SHA256 `28f2ab7bacb4e36f965d510d312be863d7c090f0fabbe026b1a5ea6b4493e0a2`.
It applied with index consistency before authored source changes.
Its complete applied tree is `7504d66f84ca13a22c7c3a9667f67177ed79893d`.

The shared-file baseline blobs in that tree are:

- `src/game/session.rs`: `5da54caf91762a97c23986970dfcb508fe262be3`.
- `src/game/tree.rs`: `c1c8fa1194d0ccbd244bc48834f3af90a700af9c`.
- `src/notation/game.rs`: `372472a0207f6a8e277064c0350be2e2e66988e3`.

The four other owned files are absent from the baseline. Export uses an explicit
temporary index initialized with that dependency tree and stages only the seven
owned paths. The binary/full-index diff is incremental against that tree, so
codec-only storage, registration, test and evidence changes are excluded.
The parent owns three-way/index-consistent integration. The dependency artifact
is left unchanged.

## Frozen public contracts

`GameSession` gains these methods:

```rust
save_notes_and_nags(&mut self, expected: GameRevision, node: GameMoveId, notes: String, nags: Vec<GameMoveNodeNag>) -> Result<GameRevision, GameSessionError>
save_markings(&mut self, expected: GameRevision, node: GameMoveId, markings: Vec<GameMoveNodeMarking>) -> Result<GameRevision, GameSessionError>
preview_variation(&self, expected: GameRevision, request: GameAnnotationsEnumVariationRequest) -> Result<GameAnnotationsSessionPreview, GameSessionError>
apply_variation(&mut self, confirmation: GameAnnotationsSessionConfirmation) -> Result<GameRevision, GameSessionError>
```

`NotationGame` gains the clockless equivalents:

```rust
save_notes_and_nags(&mut self, node: GameMoveId, notes: String, nags: Vec<GameMoveNodeNag>) -> Result<GameAnnotationsEnumChange, NotationGameError>
save_markings(&mut self, node: GameMoveId, markings: Vec<GameMoveNodeMarking>) -> Result<GameAnnotationsEnumChange, NotationGameError>
preview_variation(&self, request: GameAnnotationsEnumVariationRequest) -> Result<GameAnnotationsVariationPreview, NotationGameError>
apply_variation(&mut self, confirmation: GameAnnotationsConfirmedVariation, current_view: GameMoveId) -> Result<GameAnnotationsVariationApplied, NotationGameError>
```

`GameAnnotationsEnumChange` has `Unchanged` and `Changed` cases.
`GameAnnotationsEnumVariationRequest` has `Promote { tip }` and `Delete { root }`.
Shared previews expose `request`, `previous_tip`, `previous_path`, `selected_path`,
`deleted_nodes`, `parent` and `resulting_tip` through immutable getters. Promotion
also privately binds the ordered children of every selected path node. Deletion
binds deterministic preorder scope, owning parent and its sibling order, previous
tip/path and replacement tip. Consuming `confirm()` is the only constructor of
each confirmed type. Session previews additionally bind actual `GameId` and
`GameRevision`, exposed by `game()`, `revision()` and `variation()`.
The notation applied receipt exposes `change()`, `mainline_tip()` and `view()`.

## Accepted policy and source behavior

The owner's accepted minimal-slice policy applies the existing
`Completed | Analysis` study gate to notes/NAGs, markings, promotion and deletion.
Setup, active and interrupted sessions remain unavailable. The existing append
path shares that gate. This scope boundary avoids introducing actor/revision
coordination behavior ahead of T106; it is not an observed defect.

Split explicit saves preserve the counterpart annotations, validate typed node
content, and detect equality before advancing a session revision. They do not
read clocks or change live history, cursor, outcome or play configuration.

Played-session promotion requires a leaf tip. Notation promotion permits an
explicit interior tip, preserving its existing aggregate contract. Full export's
typed interior-tip refusal is unchanged; Mainline remains available. Every
selected path child moves to index zero while other siblings retain order and
the previous line remains in the tree.

Deletion rejects the immutable root. Session policy always protects a scope
containing `live()`, including completed games, retaining outcome origin and
played identity. If the selected tip is removed, choose the first surviving
sibling's ordered leaf, or the owning parent when no sibling survives. Repair
only a removed view to that parent. A parent becoming a leaf is validated against
the exact pending deletion rather than rejected for its pre-deletion children.
Notation validates the actual caller cursor and removes only deleted-node clocks
and `UnknownDirective` warnings, rebuilding its clock lookup. Surviving source
metadata, annotations, warnings and result declarations stay attached.

Tree preparation rebuilds and compares the full preview proof, validates the
cursor and computes mutation indices before commit. Session policy and checked
revision advancement also precede commit. No whole-tree rollback clone or
fallible domain operation follows commit.

Notation gains no session identity, clock owner, logical revision or new storage
dependency. The T106 parent must bind the actual durable game ID/storage revision,
fresh editor epoch, checked local generation and immutable preview ID/digest.
Only matching real storage receipts advance its durable base. This slice does
not implement or claim those desktop guarantees.

## Tests authored and verification limits

The first test draft preceded implementation: session file SHA256
`33b0e321a85f8ea89d5f96ac67e313f33f4ed5520d764c249258dcdf67009fb6`;
notation file SHA256
`4eba8d1150fe944357637db6b29d111e2f078baa7d9fd4b2b6c71d808797a9dd`.
Subsequent source review corrected fixture API usage and added boundary coverage.
No semantic-red observation is claimed for this missing new functionality.

There are nine session and seven notation deterministic contract tests. They use
real game sessions, completed Fool's Mate, existing injected proof fixtures,
explicit clock instants, validated move trees, typed annotations and the real PGN
codec. Coverage includes split/no-op saves, invalid nodes/text, stale/exhausted
revisions, unavailable phases/save gates, actual game binding, promotion order,
interior notation tips, preorder deletion, live-path protection, tip/view repair,
restoration, reference cleanup, stale proofs and supported interchange.

All 16 tests are authored and unrun. The parent must add `pub mod annotations;`
in `src/game/mod.rs` and include `tests/game/annotations.rs` and
`tests/notation/game_annotations.rs` from `tests/domain.rs`, retaining its existing
`game_transition_fixture` registration. Those roots are deliberately untouched.

Permitted validation consists of standalone rustfmt from the installed
`/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt`, whitespace/scope/source scans,
and cached-index forward/reverse application checks. These establish source
format and patch consistency only. No Cargo, compiler/build, test, Clippy,
runtime, CPU diagnostic, commit, push, agent or extra-worktree action was run.

## Parent-observed stale-content regression and source-only repair

This follow-up supersedes the initial handoff's verification status only where
explicitly recorded here. The original `game-annotations.patch` is unchanged at
SHA256 `4a631dd7e7fb3e8c345883ef92bfa65465e96447462dc35527d0c72eb6f81d80`.
Its complete local delivered tree was
`4646386fc6aa5be5e517c72eca68a2a979e645a2`.

Before production repair, the parent supplied tests-only delta
`game-annotations-stale-content-tests.patch`, SHA256
`3f441b913881f2ae00becfb255d53e424450e5888da62096e60ac08a4bcd7948`.
It applied with index consistency to the exact delivered snapshot. The frozen
tests-applied tree is `18230f8a0b27f8612cd4112427cc683eacafe87b`.
Original production/evidence blobs in both trees are:

- `src/game/annotations.rs`: `6736481894782e4cd15bd5eabd6513ee8d824878`.
- `src/game/tree.rs`: `a680a584020be5f9edc1d60f25bc8dd4b65c5248`.
- `src/game/session.rs`: `ceb1380fafc3b5af9ae9a671a51a3c384f90c29b`.
- `src/notation/game.rs`: `2bc7d52cf37eb79f042dc6f58adc6a43840c6683`.
- This evidence: `0dea3c3876b37398c76acf1a407d200c387ccecb`.

The original notation test blob was
`956adc4e28226314cc89eb260cd09b6d88a27357`; the frozen parent-tests-applied
notation test blob is `ee5c77adcf785a43936c4a1449793fcb721db5a8`.
The session test blob remains `f271f755f1127540b5fafc0ec63f2a6ad10c9e59`.

The parent reports the original 16 tests passed before adding its two regressions.
The supplied observed-red command was
`cargo test --locked --all-features --test domain notation_delete_confirmation_refuses_ -- --nocapture`.
It exited 101 with 0 passed, 2 failed, 0 ignored and 445 filtered. Both
`notation_delete_confirmation_refuses_notes_saved_after_preview` and
`notation_delete_confirmation_refuses_markings_saved_after_preview` failed the
expected `StaleVariation` assertion: deletion incorrectly accepted the old
confirmation after a same-instance explicit save. The raw log is
`/tmp/claude-chess-pgo-diagnosis-20261016/game-annotations-stale-content-red.log`.
The log also contains the existing macOS `__eh_frame section too large` linker
warning, which is separate from the semantic test failures. This writer read
the parent evidence and did not rerun Cargo.

The repair adds a private 32-byte SHA-256 content fingerprint to each existing
shared proof. Deletion fingerprints every node in the deterministic removed
subtree. Promotion fingerprints the previous and selected root-to-tip paths as
two separately framed sequences. Each borrowed node record contains its UUID,
optional parent UUID, ordered child UUIDs, optional SAN, UTF-8 note bytes,
ordered NAG bytes, and ordered markings. Markings encode exhaustive color,
origin, and an explicit destination-present marker plus destination, preserving
circle/arrow distinctions. Counts and variable byte lengths use checked u64
big-endian framing, optional values use explicit presence bytes, UUIDs use their
fixed byte representation, and the hash begins with its versioned domain tag.
Length conversion failure is a typed `GameTreeError::ContentLengthOverflow`.

The existing exact ID/order/parent/tip proof and public method signatures remain
intact. Apply regenerates the fingerprint during existing proof validation
before any tree, selected-tip, clock-reference or warning mutation. Fingerprints
stream borrowed annotations into fixed-size hash state; they do not clone
annotation payloads or whole trees. No-op saves preserve the fingerprint.
Surviving-node annotation saves outside deletion scope preserve confirmation,
because they cannot discard content through that action. Promotion compares the
content of both explicitly displayed paths. Other mutable session state remains
protected by its real game/revision/policy checks; notation exposes no mutation
of clock/warning/result metadata through these save methods. No persisted or
synthetic notation revision, desktop identity or storage wrapper was introduced.

The production/evidence repair export is incremental against the frozen
parent-tests-applied tree and contains only `src/game/annotations.rs`,
`src/game/tree.rs`, and this evidence. It excludes the parent regressions, the
original seven-path patch, dependency-only files and registration roots.
A separate additional-tests delta contains four new deterministic contract
tests for descendant NAG edits and atomicity, no-op/outside-scope preservation,
marking color/shape/endpoints, and promotion selected/previous path notes.
Those additional tests were authored before this repair's implementation and
are unrun. The parent's original regression text is preserved exactly, including
its one inherited long assertion; new test bodies use pinned rustfmt formatting.

Only standalone source formatting, whitespace/scope/source scans and cached
index application checks are authorized in this follow-up. The parent retains
the compile/test/Clippy/build lane and registration ownership. There is no
post-fix green, runtime, performance or storage/UI acceptance claim here.

## Parent integration and canonical verification

The parent integrated the original source, both observed stale-content
regressions, the scoped fingerprint repair and four additional contract cases.
The final focused annotation command passed 22 tests, with zero failures or
ignored tests. A separate read-only review of the repaired proof and callers
found no remaining blocker in this domain slice.

The combined library/annotation snapshot passed
`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify`
with exit 0 on 2026-10-17. All three canonical Clippy selections passed. The
default all-feature suite passed 1,507 tests with zero failures, ignored tests
or filtered tests. The optimized all-target build passed in 5m 01s and the
separately named optimized native harness build passed in 49.90s. This is one
combined execution, rather than independent executions for each component.

Raw focused/red/canonical logs and their SHA-256 manifest are preserved under
`.git/parallel-patches/game-annotations-integration-20261017/`. The macOS debug
linker emitted its existing `__eh_frame section too large` warning; it remains
unsuppressed and separate from clean Clippy results. Native GPU execution was
not repeated, and neither native rendering nor other-platform acceptance is
established by compiling that harness.

T105's domain operations are implemented and verified. Durable editor context,
receipt correlation, mounted Save/Discard/Stay controls, native handlers and
the complete T106/T110 flows remain separate work. The confirmation adapter
added in the same checkpoint concerns Trash lifecycle evidence, not annotation
editor orchestration.
