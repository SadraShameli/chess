# Played library encoding reservation repair

## Status and observed parent red

This repair was authored on 2026-10-17 under source-only authorization. No Cargo,
tests, Clippy, build, CPU/runtime diagnostics, agents, commits, pushes or extra
worktrees were run or created here. The parent owns the exclusive verification
lane. No compilation, runtime, RSS, performance or codec acceptance is claimed.

The parent observed the actual semantic red before authorizing this repair:

```text
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib storage::library_played::tests:: -- --nocapture
exit 101; 13 passed; 1 failed; 375 filtered out; 0.07s
```

The failing test was
`exhausted_byte_budget_refuses_played_preparation_before_executor_admission`.
Its first poll waited for encoding capacity instead of returning
`StorageLibraryError::Store(StorageDocumentError::Saturated)` with receiving
bytes exhausted. All 13 earlier cases passed. The raw parent log was read at
`/tmp/claude-chess-pgo-diagnosis-20261016/library-played-save-baseline-tests.log`;
its SHA-256 is
`79a59289067bcbee73f5ef31a124fad7c6dd32884ac2bc58f939bfe22e52c661`.
This is baseline evidence from the parent, not execution of the repair.

The parent subsequently supplied exit-101 all-target/all-feature Clippy output at
`/tmp/claude-chess-pgo-diagnosis-20261016/library-lifecycle-import-repaired-clippy.log`.
This round read its sole finding: the existing stamp test's
`request.clone().content_change()` was redundant because the clone was neither
consumed nor mutated. Under explicit parent authorization, that test now
consumes the clone by rebasing it to revision 74, preserves original revision
73, and checks both requests retain the same stamp, game and covered revision.
This authored assertion repair has not been run here. Production scope is unchanged.

## Exact dependency baseline

The checkout began clean at commit
`647147d9ff95cb745ede46637048ef312e3070af`. These patches were SHA-256 checked
and applied sequentially with `git apply --index`:

| Patch | SHA-256 |
| --- | --- |
| `native-library-adoption-recovery.patch` | `61fcfd0f7e03c5c50b077c346dec6bf0e64bfef3f4d8bf4a40d3880a2aacbe61` |
| `engine-premove-checkpoint-parent-fix.patch` | `43b51a3e13d8ed13cde16de064713944cbdecc5ad77585a5bebc14ead7c8d145` |
| `game-lifecycle-preparation.patch` | `e434ee77b9b217ff527b01366a3090e1fadc47e18d66c1d50033171771b4f44a` |
| `library-played-save.patch` | `0c6427f6530025beb30570b823d8b263f9d8b16bd8a64035545a3d25546b86c4` |
| `library-played-preparation-budget-red.patch` | `35c032ddb5c246587802eed4fbbeebfca1724c1cb8682df900d03d757921cf80` |

The resulting exact baseline tree is
`9c02d5bf30479785a738698309d4bd16a7ed4897`, written without a commit. The index
and working tree matched before implementation. The repair's baseline blobs are:

| Path | Baseline blob |
| --- | --- |
| `src/storage/game_document.rs` | `be99791940431011a93c1c30f15b373fb6939b56` |
| `src/storage/library_played.rs` | `984471b9281fba9ed07199b1b7d8d86afa7a326e` |
| `src/storage/library_prepared.rs` | `e90ec1d3147e87efe1a0614f078d0e0bab75a7fc` |
| `src/storage/library_repository.rs` | `65df217299093dfd7e33d27bfb321d40eb7d34e2` |
| `tests/storage/library_played.rs` | `603ac094ebf19381bc5958009656b1b8d2669d5c` |
| `specs/001-reckless-desktop-chess/evidence/library-played-save.md` | `4e36d59527ca7fab70d495fcdd241271986a3471` |

## Allocation and transfer limits

Played preparation first reserves the existing `reservation_bytes(0)` fixed
overhead from the receiving client, before its first executor admission. This
nonzero admission fails immediately when receiving bytes are exhausted.

The injected executor measures the borrowed snapshot without copying a tree or
constructing a DTO. Checked bounds are reserved before `from_snapshot` allocates:

- The played DTO box, the exact move-vector element count and eight bytes per
  UCI string. Inspected shakmaty 0.30.2 emits at most five ASCII UCI bytes;
  inspected Rust 1.99.0 grows an initially empty byte string to eight bytes.
- One temporary UUID-sized string covers sequential ID and decimal-revision
  conversions made by the existing serializers, including while the writer
  grows. The UUID bound is the pinned type's Hyphenated length of 36 bytes.
- Two initial-FEN copies, covering the DTO start and the existing BoardStart
  serializer's temporary clone. Inspected Chess/Castles fields have no heap
  buffers. The DTO stage vector and one temporary source-stage copy from
  `ClockStateSnapshot::persisted()` are included; clock captures are sequential.
- Every root/move note byte and NAG byte. Marking-vector elements use the current
  color, square and optional-square field layout as their allocation bound.
- All retained known metadata text, opening-code text, extension names and
  values. The fallible extension collection reserves a checked power-of-two
  capacity with minimum four elements, plus the previous half-capacity for
  possible reallocation. Each extension owns two strings.

The existing one-MiB field and 100000-move limits are checked during sizing and
remain enforced by the DTO constructors. Sizing checks cancellation at entry,
per move, per extension and at exit. Caller-owned captured snapshot storage and
executor infrastructure are outside this DTO/output charge. These bounds cover
Rust-visible capacities, without measuring allocator bookkeeping or RSS.

The existing writer accepts an injected reservation callback. Before each owned
byte-buffer growth it checks size and arithmetic, then reserves
`dto_bound + old_output_capacity + new_output_capacity`. Both output capacities
cover possible overlapping allocations during reallocation. The writer keeps
its doubling/minimum-eight growth policy, capped at the document limit, and
requests capacity with `try_reserve_exact`. String conversion reuses that buffer.
Legacy encoding uses the same writer with an infallible callback; its repository
remains unindexed.

`StorageLibraryPreparedMemory` retains one OwnedSemaphorePermit. Growth acquires
only the missing delta from that permit's semaphore, then merges without
releasing the original reservation. Pinned Tokio 1.53.3 merge/split source was
inspected. Merge always receives a permit from the same semaphore; total checked
admission remains within the existing 64-MiB shared budget.

The DTO is explicitly dropped before reducing to the unchanged prepared charge:
`4 * actual_string_capacity + projection_size + 2 * transaction_request_size`.
The same permit grows by a delta if needed, then splits/releases only excess
bytes. Body and permit move together through the existing validated decoder,
projection and identity builder. The options owner stays intact on errors so
its body drops before its permit. The temporary decoded record is dropped before
the final prepared owner, preserving payload-before-permit cleanup order.

Cancelled/abandoned encoding keeps bytes in the executor closure until its
buffers are destroyed. Abandoned prepared results use the existing executor
result/cleanup ownership. The prepared write transfers to the existing actor
without another payload reservation, retaining its permit through commit or
rollback even when the caller drops its wait. Receipts still come from actual
committed actor replies. The stamp/rebase API, parent `from_snapshot` visibility
and `PendingResult` serde skip are unchanged.

Admission and DTO growth preserve direct typed Store refusals. Writer growth
preserves Store inside the codec error. Checked overflow, field limits,
cancellation and allocator reservation failure remain distinct typed causes.
No actor/supervisor API, semaphore, pool, decoder, normalizer, transaction,
schema, root or domain change is included.

## Authored coverage and handoff

All original 14 played tests are retained. Only the parent-identified clone
assertion is strengthened as described above; the other 13 bodies are unchanged.
Four additional played tests are authored and unrun:

- Large metadata, escaped Unicode root/move notes, markings and 17 extensions survive
  save/load while one exact shared reservation remains held through a controlled
  pending commit and releases afterward.
- A budget one byte below the measured large DTO bound refuses before encoding
  and fully releases initial admission.
- Encoding growth refusal retains the DTO charge until payload destruction,
  then releases all bytes without creating documents or projections.
- Cancellation and abandonment during controlled encoding growth retain bytes
  until observed worker cleanup.

Two private writer tests, also unrun, check preservation of existing bytes and
capacity after refused growth, and overflow refusal before the reservation
effect. The authored total is 18 played tests plus two writer tests. No repair
pass count or additional production defect repair is claimed.

Standalone `/opt/homebrew/Cellar/rust/1.99.0_1/bin/rustfmt` reports 1.10.0. It
formatted the five edited Rust files with edition 2024 and child traversal
disabled. Final formatter parsing, Git whitespace checks, explicit scope and
source-content inspection, and incremental forward/reverse patch checks are the
only local verification. They do not establish compilation or behavior.

The staged binary export is
`/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-played-encoding-memory-fix.patch`,
relative to the dependency tree recorded above. It contains only these six
repair paths, without prior dependency hunks or local source registration.
The parent must run the original cases, the added memory tests and its required
broader gates. Source and tests are frozen at handoff for parent execution.

## Parent canonical integration

The combined source and all relevant regressions were executed by the parent.
[Verified integration](library-runtime-import-integration.md) records the exact
snapshot, 1,666 passing tests, three clean Clippy selections, both optimized
builds, durable log hashes, known linker warning and remaining acceptance gates.
Earlier unrun statements describe this writer's isolated handoff only.
