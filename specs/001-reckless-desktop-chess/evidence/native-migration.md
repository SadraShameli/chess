# Native migration evidence

Recorded 2026-10-16 on the current macOS ARM64 development host. This register
separates the migration inventory from subsequent executed compatibility checks.
The implementation sequence is owned by [the plan](../plan.md) and
[tasks](../tasks.md).

## T029: Preserved migration baseline

The starting branch is `001-reckless-desktop-chess`, at commit
`1f243d139844a72f4d9b8b0d404c365456fcc516`.

Git reported 43 changed or untracked paths: 26 staged paths, 17 tracked paths
with unstaged changes and two untracked paths. Two paths have both staged and
unstaged changes, so these category counts overlap.

| Baseline snapshot | SHA-256 |
| --- | --- |
| Staged binary diff | `960811aea845934742ecd8c5d13e8880700471e1de84221a68132fa17515fd55` |
| Unstaged binary diff | `0fd635b0eac2e3d99251d2f79103e4f16cfeb3eb10c9cee9331b0851b0205688` |

Separate staged, unstaged and combined binary patches were saved outside the
repository before implementation. No stash, reset or owner-profile access was
used to create the baseline.

The staged paths are `resources/desktop/validators.mjs`,
`src/capacity/budget.rs`, `src/desktop/{bootstrap,dto,error,mod,schema}.rs`,
`src/main.rs`, `src/storage/document_store.rs`, `tauri.conf.json`,
`tauri.test.conf.json`, `tests/capacity/budget.rs`,
`tests/storage/{document_store,schema}.rs`, `ui/src/{App.vue,main.ts}`,
`ui/src/desktop/application.ts`,
`ui/src/desktop/generated/{contracts.ts,fixture.json,schema.json}`,
`ui/src/shell/PlayerBar.vue`, `ui/tests/app.test.ts`,
`ui/tests/desktop/{application.test.ts,application_fixture.ts}`,
`ui/tests/native/bootstrap.e2e.ts` and `ui/types/wdio-globals.d.ts`.

Unstaged tooling changes select an optimized, locked former native-test build
in `Makefile` and `ui/wdio.conf.ts`. Additional unstaged screenshot support in
`ui/tests/native/bootstrap.e2e.ts` and `ui/types/wdio-globals.d.ts` must be retained
alongside those files' staged bootstrap assertions/declarations. The other 13
unstaged tracked paths are feature planning artifacts. The untracked paths are
`checklists/native-desktop.md` and `checklists/native-desktop-review.md`.

### Former T029/T030 reuse boundaries

| Existing owner | Reusable behavior | Remaining migration |
| --- | --- | --- |
| `src/desktop/bootstrap.rs` | Capacity/executor construction, SQLite actor ownership, verified engine readiness, bounded queues, snapshots/streams and observed shutdown results | Embedded Tauri handles/runtime/window ownership; no profile lease, activation or Bevy host |
| Desktop DTOs, schemas and generated fixtures | Validated payload semantics and existing external encodings | Replace browser transport with typed in-process messages while retaining persisted compatibility |
| `src/capacity/budget.rs` | Shared async leases and cancellation behavior | Native preparation/steady/recovery admission |
| `src/storage/document_store.rs` | Serialized SQLite actor, bounded admission, transactional receipts and execution-scoped capacity lease | Complete game schemas and native lifecycle ownership |
| `src/main.rs` | Historical composition entry point | Replace setup/exit `block_on` with asynchronous native pause/save/reap orchestration |
| Vue shell and PlayerBar | Board snapshot, engine identity/unavailable/error and stream presentation contracts | Preserve until native parity, then remove with final callers |
| WDIO preparation | Ready-engine, 64-square and visible-canvas assertions, temporary test profile and screenshot preparation | These establish no Bevy renderer or native accessibility acceptance |

The staged bootstrap is 892 lines and 33,519 bytes, SHA-256
`d3857979376d0ac7fba4043bab901fde7c0004e7de4ce145c9357c6efec45d1f`.
Its operations are bootstrap, stream subscribe/acknowledge/resynchronize and file
selection. It does not implement a live chess game.

### Retained assets and fixtures

| Path | Bytes | SHA-256 |
| --- | ---: | --- |
| `resources/pieces/chess.glb` | 1,376,224 | `ee0c9bf8cd98f1239046b0566748030c4c320f049fd6ab3232f1f7e9691a020e` |
| `resources/pieces/provenance.json` | 5,046 | `7ece728b19dd2717020cdfe90119354cacd1663d4aad18ec7ada49c6312852f2` |
| `ui/tools/generate-pieces.ts` | 17,746 | `78910901be4d567f0dbd891a7188ddb112ccb4b59634d570d9170113ed4e5f81` |
| `resources/engine/manifest.json` | 624 | `1ba2ee79a3020c26b25d1d495ca4891f9aed0936600606217f68f78c526db1f9` |
| `resources/engine/bin/reckless-aarch64-apple-darwin` | 65,383,664 | `ab4128cd33149b14713475e049db8ddaff702ee79bfb412ecfbde91655273689` |
| `src/storage/migrations/0001_core.sql` | 2,727 | `f17119fc8c888c166e988c1bdb0e127f7e116d7fbff1c559d892d144e68b1e61` |
| `tests/fixtures/positions.json` | 1,579 | `0b7ebf1545acadb7741b00ded7b3eb8ca58dc59c0f5994a48ebc3190505e64e6` |
| `ui/src/desktop/generated/fixture.json` | 16,242 | `67d64ab10faf543efdf826ebc9f624459f6b7004f2480dcb1d88e5e8d39d5d48` |
| `ui/src/desktop/generated/schema.json` | 59,018 | `584eb03069c6e8587898de7754bba9a7b7e9e9f3608fb660da5ebe23e65641ff` |

The GLB header declares glTF binary version 2 and its actual byte length. Its
JSON contains twelve named color/kind parents with twelve sculpture descendants,
twelve meshes and `IvorySatin`/`EbonySatin` materials. There are no external buffer
or image URIs. Library nodes intentionally overlap and require named-parent
instantiation with authored transforms.

The ignored local engine exists and matches its manifest hash. This inventory
did not execute it. The production application identity is `nl.sadra.chess`;
the former instrumented configuration uses `nl.sadra.chess.native-test` and a
temporary bootstrap data directory. Existing owner data was not opened.

### External-format continuity

The existing desktop protocol is version 1. Identifiers use non-nil canonical
lowercase UUID strings; checked u64 revisions use canonical unsigned decimal
strings. The retained fixture includes `18446744073709551615`.

SQLite migration version 1 is applied transactionally through `PRAGMA
user_version`, with future schema versions rejected before migration. It owns
`documents`, `games`, `move_nodes`, `clock_snapshots` and `preferences`. Preserve
UUID/revision text and JSON bodies, full position identities beside eight-byte
hashes, immutable-parent behavior and composite game-ownership foreign keys.
Seventeen FEN boundary fixtures and existing rollback/future-schema/revision/
collision/cross-game tests were inspected, not rerun for this inventory.

### Evidence boundary

The prior toolchain, contracts, engine build and startup reports remain historical
evidence. Their test counts and target results were not refreshed by T029.
`toolchain.md` records 477 packages; the starting lockfile contains 481.

At this baseline, Cargo still describes the former Tauri graph. There are no
direct Bevy, bevy_egui, egui, rfd, directories, interprocess, winsafe or widestring
dependencies. Transitive rfd presence proves no native integration. The planned
`src/{presentation,scene}`, `src/desktop/{native,profile}`,
`tests/{native,presentation,scene}.rs` and `packager.toml` are absent.

T029 records preservation and reuse boundaries only. Native compatibility,
production-host integration, data migration execution, frame/CPU measurements
and Windows/macOS/X11/Wayland acceptance remain subsequent work.
