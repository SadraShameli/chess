# Typed engine analysis parsing

T078 parsing and score normalization live together in `src/engine/analysis.rs`.
`EngineSession` validates info responses against its active search position before
returning typed telemetry. Live-game adapters retain literal informational text
without making the game domain depend on the engine's analysis types.

The regression was observed failing before implementation: an illegal
`info depth 1 score cp 0 pv e2e5` response was accepted as ordinary telemetry.
Malformed numeric scores and illegal later PV plies now fail the session and
use its existing terminate/reap path, without producing a game move.

Typed reports preserve optional depth, selective depth, rank, score, nodes,
elapsed duration, NPS, per-mille hash occupancy, tablebase hits and continuation.
Missing values stay absent; explicitly reported zero stays present. Counters use
u64 and retain values above JavaScript's exact-integer range. Every PV ply is
validated against the preceding authoritative board, including Chess960's
king-to-rook castling. Rank cannot exceed the legal root-move count.

Scores retain cp/mate identity, reported side and exact/lower/upper bound.
White-perspective conversion negates Black values and reverses bounds. Widening
to i64 before negation safely handles i32::MIN. Raw static NNUE evaluation has a
separate White-perspective integer type, matching the pinned `eval` response.

The pinned upstream `thread.rs::print_uci_info`, `PrincipalVariationTable` and
`types/mod.rs` were inspected at commit
`0e92358f5acd66e5ac77b1bf558202e47c515435`. PV parsing permits at most 130 plies:
the root move plus the table's MAX_PLY + 1 capacity (MAX_PLY is 128). This bounds
legality work as well as the existing 64 KiB line bound. Unknown informational
text remains literal diagnostic text; invalid structured fields, duplicate
statistics and contradictory bounds are rejected.

Checks for this increment:

```sh
cargo fmt --all
cargo clippy --locked --lib --test engine --test native -- -D warnings
cargo test --locked --profile dev --test engine
cargo test --locked --profile dev --test native -- --game-composition
```

All 81 engine tests passed, including the nine new parser cases and expanded
actor regression. Named Clippy passed. Both real-engine production mate games,
durable restoration, relaunch and cleanup checks passed with the new parser.
Analysis orchestration/coalescing, configuration, workbench UI and complete
capability acceptance remain assigned to their own tasks. This is no claim of
full US2 acceptance, optimized performance, installed packages or cross-platform
results. The existing macOS debug linker warning remains recorded in
[US1 evidence](us1.md).
