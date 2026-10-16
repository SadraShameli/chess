# Puzzle definition boundary

T181 source implementation and the definition portion of T176 are provided by
`src/puzzle/` and `tests/puzzle/definition.rs`, registered through `tests/domain.rs`.
The task markers remain the parent's responsibility.

`PuzzleDefinition::new` consumes a validated `BoardPosition`, source identity,
typed `BoardMove` lines and validated optional metadata. The source identity
preserves the exact nonempty source, immutable dataset version and source puzzle
identifier without trimming or case folding. Source record deduplication and
conflicts across a collection belong to its future import boundary.

The supplied position is the position at which the solver starts, and its turn
identifies the solver. Under [the CSV decision](../decisions.md#puzzle-interchange-and-lifecycle),
the future importer validates and applies the opponent setup move before calling
this constructor. The definition neither removes nor applies another move and
does not impose an undocumented line-length parity restriction.

The accepted tree retains source line order and each parent's first-listed child
order. Every listed move is replayed through `BoardPosition::play` against its
actual preceding position, including shared prefixes. Empty solutions, empty
lines, duplicate complete lines, and a terminal line that prefixes another line
are rejected. Construction returns no partial definition. Transposed positions
remain distinct source paths. The tree is immutable through its public API and
uses contiguous node storage, typed node/line identities and typed moves.
Node and line identities are deterministic ordinals local to the immutable
definition; consumers must pair them with its source identity.

`solution().continuation(parent, movement)` answers listed membership. It does
not classify a submitted move's legality: T183 must use the node's authoritative
position for that distinction. Ordered `children()`, node moves/positions and
terminal line identities support listed-only progression, source/destination
hints, solution review and injected seeded opponent choice without generating
alternatives. This module imports only board domain types and `thiserror`; it
has no engine, search, clock, randomness, storage or presentation dependency.

Difficulty uses the source's full unsigned 32-bit rating range. Theme tokens are
nonempty UTF-8 tokens without ASCII whitespace, with exact case and source order
retained. Duplicate tokens and an available but empty theme collection are
rejected. Missing difficulty/themes remain `None`. Source preparation may perform
the [explicitly selected theme deduplication](../decisions.md#starter-subset-selection-v1)
before this typed boundary; the definition performs no silent normalization or
metadata inference.

The focused fixtures cover single and branching solutions, shared prefixes and
source order, deterministic identities, duplicates and terminal conflicts in
both orders, illegal moves at each ply, incompatible branch history, king safety,
legal unlisted moves, pre-applied setup positions, en passant/counters, all four
promotions, Chess960 stationary king/rook castling, post-mate moves, counter
overflow, transposed source paths, provenance and optional metadata boundaries.
They use real board objects and no engine or external effects.

The tests were authored before the implementation. Cargo was not run because the
parent reserved the shared verification slot; no red or passing Cargo result is
claimed. Only owned Rust files were formatted with the installed Rust formatter,
whose compiler reports the pinned version 1.99.0. `git diff --check` passed before
handoff. Focused Clippy/domain execution and consolidated repository gates remain
pending the parent's slot grant. No runtime performance measurements are claimed.

Parent integration at `f0f0630` subsequently passed `cargo fmt --all`,
`cargo clippy --locked --lib --test domain -- -D warnings` and
`cargo test --locked --profile dev --test domain`. All 191 domain tests passed,
including these 18 definition cases, with no failures or ignored tests. The
existing macOS `__eh_frame` linker warning remains unsuppressed. T181 is accepted
after these checks; no engine, native-window or performance acceptance is claimed.

Progressive hint resets, practice outcomes, seeded reply selection execution,
sessions, timers, persistence, CSV import and UI are outside this patch. T176
and US8 remain incomplete until their owning lanes provide that behavior.
