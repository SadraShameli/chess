# Storage ORM implementation evidence

The owner requires an actual ORM, accepts the internal refactor and requires solo
execution through the matching Spec Kit stages. The selected adapter is Diesel
2.3.14 with SQLite. The [plan](../plan.md#storage-orm-migration),
[compatibility contract](../contracts/storage-orm.md) and T296 through T303 in
[tasks](../tasks.md#phase-17-storage-orm-migration) define this increment.
This evidence does not claim completion of the full native chess feature.

## Baseline, T296

Before modifying the registered production backend or dependency manifest,
the pinned wrapper
`/tmp/claude-chess-pgo-diagnosis-20261016/with-pinned-rust` executed:

| Command arguments after the wrapper | Actual result |
| --- | --- |
| `cargo test --locked --test storage` | 181 passed, 0 failed, 0 ignored |
| `cargo test --locked --lib storage::library_transaction::tests` | 24 passed, 0 failed, 527 filtered; this subset is also included in the next run |
| `cargo test --locked --lib storage::` | 166 passed, 0 failed, 0 ignored, 385 filtered |
| `cargo test --locked --test desktop desktop_game_supervisor::` | 10 passed, 0 failed, 0 ignored, 224 filtered |

The public storage and private storage/import runs cover actual SQLite
migrations/rollback, canonical bodies/revisions, retained memory ownership,
heterogeneous keysets, metadata/folder/Trash operations, duplicate/import staging,
physical space/page ceilings and puzzle journals/recovery. Desktop cases cover
durable game start/move/quit, save failure/retry, retained shutdown and stale
queued mutations. These are observed baseline results, not ORM migration results.

The build emitted the existing unsuppressed macOS linker warning:
`ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected`.
That warning is not a zero-warning final gate or an ORM performance measurement.

Logs are under `/tmp/claude-chess-storage-orm-20261017/`:

| File | SHA-256 |
| --- | --- |
| `storage-baseline.log` | `46d35529c46f77bf28b16af4db4eafd913160571c9caf7ffaa458b5a77e3cd67` |
| `transaction-baseline.log` | `45bc00a5297b5323596cf3a3b2ec5ca42e6f06e879453128258714b299bcb16d` |
| `storage-private-baseline.log` | `b04f98a8b2e4d4167109293989fb3e4e68340de02a7a3ab8151b3ba1e7a58db7` |
| `desktop-storage-baseline.log` | `ccaf6ced2bef0fd7d4b5662be66dc381df26433b93a3181d5f84550655d63f0f` |

Source inventory identifies the actor/migration, library transaction and query,
import-space, and puzzle repository owners. The private transaction fixture is
also driver-bound and must migrate with the actual connection signature.
Independent raw-SQLite test setup remains useful as a reference. No production
driver caller is accepted as migrated merely because the ORM dependency exists.

## Implemented adapter, T297 through T302

The production storage actor now owns one Diesel `SqliteConnection`.
`database.rs` configures WAL, FULL synchronization, foreign keys and the existing
five-second busy timeout. `schema.rs` maps the existing document, library and
puzzle tables. There is no pool, additional runtime or driver compatibility API.
The actor's clients, capacity admission and retained shutdown owners are unchanged.

Document writes and migrations, library content/projection and organization
transactions, keyset/duplicate queries, import space accounting and puzzle
collection/run/journal/checkpoint operations use Diesel. Ordinary CRUD uses
typed schema expressions and row mappings. Scoped SQLite expressions retain
recursive folder traversal, PRAGMA and physical dbstat inspection. Transaction
manager state and page-ceiling restoration govern connection retirement.

All three existing numbered SQL migrations are unchanged. Canonical stored body
bytes, text revisions and IDs, foreign keys, journal digests and receipt-after-
commit semantics retain their existing formats. No database version bump is
needed for the adapter replacement. Production source has no rusqlite caller;
the dev dependency independently constructs and inspects real SQLite fixtures.
Private transaction fixtures now use both the canonical ORM connection and an
independent reference connection to the same isolated database.

The migration passed `cargo check --locked --lib` and all 166 existing private
storage/import tests. The latter exercise actual SQLITE_FULL rollback, scoped
page ceilings/restoration, retained memory reservations, populated indexes/WAL
growth and deep folder operations. The initial public run passed 182 cases and
failed one newly authored non-UTF-8 filename fixture on macOS. Native file
creation and an independent SQLite probe both rejected that filename; the
success case is correctly Linux-specific. It is not accepted as a macOS result.
The Unicode/spaces/literal-URI-character case passed through actor save/read and
independent read-only inspection.

## Native path regression

URI conversion exposed a real compatibility defect: an embedded NUL was encoded
and SQLite opened the existing filename prefix. The actual actor regression
first failed with the prefix database accepted, while the raw reference driver
rejected the same path. The canonical connection owner now rejects embedded NUL
bytes before URI conversion and returns `StorageDatabaseError::NullByte`.
The regression passed after this guard. It also checks that the existing
database body/revision is unchanged, observes actor cleanup and removes the
temporary directory before its final assertions.

| Evidence | Result or SHA-256 |
| --- | --- |
| `native-null-path-red.log` | 0 passed, 1 failed; `46f6c969c9fb8fcfb8c8d5e66dde89c6b951c5d19ca43120e7fa802c4777755b` |
| `database-before-null-guard.rs` | `36b26324f636b8e53fd408454796926b411b9148fe989b26159e1fcd0a8153fe` |
| `native-null-path-green.log` | 1 passed, 0 failed; `e0e1aca80f8c78c305b712e5c25674d05de1f585d059e9c3b94dc718eb38a708` |
| `storage-private-orm-tests.log` | 166 passed, 0 failed, 0 ignored; `1943423f343a270a991e4f4ef565aa773deab9407d38bc5d49e767cf9cc5b844` |
| `orm-library-check.log` | Passed; `91a73b97834cbd94109b1fffb4fef029ea7f5b832f4be18b15cc9ab9567c1076` |

## Ordered verification, T303

The ORM-only canonical `make verify` run passed
all 1,835 hermetic tests with no failures or ignores: 551 library, 44 tools,
5 assets, 234 desktop, 468 domain, 235 engine, 103 presentation, 12 scene and
183 public storage. The executable and doctest selections contain no tests.
The desktop and library selections include the real puzzle repository/native
service fixtures. Formatting and all three strict Clippy selections passed.
It reached the optimized all-target build, which was interrupted. The supervisor
recorded exit status 2. The separately selected optimized native-harness build
was not reached. This is not a completed canonical gate.

`make-verify-final.log` has SHA-256
`c91621f3cff7f9f26f9652eeb7cd4059b4c51aa20333e1b6e251fca147ba7d2c`.

A direct invocation through the
pinned wrapper failed before formatting because macOS Make lost the dynamic
library environment and Cargo could not resolve `_sqlite3_bind_blob64`. A scratch
Cargo launcher invokes the same pinned wrapper and exact Cargo binary for each
unchanged Make recipe. The Makefile and checks are unchanged.

Two earlier foreground attempts were also interrupted before the gate completed.
The supervisor's separate final status distinguishes interruption from success.
Completed portions of those attempts passed formatting, all
three strict Clippy selections and the library, tools, assets, desktop, domain,
engine, presentation and scene test suites. They are partial gate evidence.

Spec Kit analysis retained task coverage for all 26 functional requirements and
14 success criteria. The append-only convergence pass added T304 through T322
for confirmed missing/partial work and the unresolved zero-warning gate. Native
accessibility is the following increment; its current combined canonical gate
uses `native-accessibility-make-verify.log`. See
[its scoped evidence](native-accessibility.md) for those additional changes.

The existing unsuppressed compact-unwind linker warning remains a zero-warning
acceptance discrepancy. Native GPU/accessibility, Windows/Linux, optimized
performance and complete product acceptance remain separate outstanding gates.
The Linux-only raw-byte filename case has not run on this macOS host.

No other agent, additional worktree, commit, push, release or credential action
is involved. The previously exported source-only report regression patch remains
separate and its tests have not been registered or executed by this migration.
