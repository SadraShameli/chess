# Reserved notation save preparation

The parent authored seven real-domain and temporary-SQLite contracts before
implementing the new notation preparation API. The tests-first source blob is
`2c75ec99a1907369a8841435342d162feb1f753f`, retained as
`.git/parallel-patches/library-notation-tests-first.rs`. These are new API
contracts; missing APIs were not represented as semantic regression failures.

`StorageLibraryNotationRepository` uses the existing storage client and capacity
executor. `prepare(expected, Arc<StorageLibraryRecord>, change)` derives the
actual stable ID and notation from the same immutable record used by the editor.
It refuses played content, reserves receiving-client bytes
before CPU admission, charges canonical DTO and encoded-output allocations, and
transfers that reservation into the existing prepared write. `begin_save`
returns the canonical retained `StorageLibraryMutation`. It creates no runtime,
connection, alternate codec, content format or mutation transaction.

The codec measures notation nodes, annotations, metadata, raw time control,
clocks and warning text before making their DTO copies. Shared start, move,
metadata and annotation accounting belongs to the same codec memory owner as
played capture. Canonical JSON writing retains its checked growth callback and
existing cancellation/error causes. Source annotations remain available after
admission, encoding or transaction refusal. Mutation of the sole source requires
its outstanding immutable preparation borrow to be observed and released.

The focused command was:

```text
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib storage::library_notation::tests:: -- --nocapture
```

It exited 0: seven passed, zero failed or ignored, 423 filtered. The cases verify
canonical annotations/raw metadata/clock-warning preservation, unchanged source,
checkpoint timestamp preservation, conflict plus newest-state retry, exhausted
bytes before executor admission, cancelled unadmitted preparation, retained
prepared-byte ownership, foreign receiving-client refusal, new-checkpoint
refusal and DTO-capacity refusal followed by a successful large-note retry.
No sleeps, network or owner-data writes were used.

This is preparation and atomic persistence support. The authoritative desktop
notation owner, accepted generation/UTC watermark, source-bound commands,
Save/Discard/Stay UI and quit/close barriers are subsequent integration work.
The existing transport budget is not a whole-process memory or throughput proof.
Canonical final verification and mounted/native acceptance remain pending for
this increment.

After the first seven passing cases, source review aligned preparation with the
editor's `Arc<StorageLibraryRecord>` source instead of separate ID/game arguments.
The same seven contracts were adapted and an eighth real played-source refusal
case was added. The public `notation_mut()` accessor preserves the record's
stable ID and content kind. This refinement and the eighth case await the next
focused run; the result above describes the earlier seven-case interface.

## Parent canonical integration

[Verified editor integration](library-editor-integration.md) supersedes the
isolated unrun status for the integrated snapshot. It records 1,703 passing
tests, all three Clippy selections, both optimized builds, the final shared
record/clock-side refinements, durable logs and the remaining mounted/native
acceptance boundaries.
