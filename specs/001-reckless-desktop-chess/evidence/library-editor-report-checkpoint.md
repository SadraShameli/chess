# Library, editor domain and study-model checkpoint

The parent completed the unchanged canonical Makefile verification on the
integrated source at HEAD `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68` plus
the current uncommitted implementation. No commit or push was made for this
checkpoint.

Command:
`/tmp/claude-chess-pgo-diagnosis-20261016/with-pinned-rust gmake verify`.
The wrapper selects recovered Rust 1.99.0 without changing the repository pin.
GNU Make preserves its task-local dynamic-library environment. The command
exited successfully. The log is
`/tmp/claude-chess-pgo-diagnosis-20261016/library-report-increment-verify-4.log`,
SHA-256 `b6bab73fcd7601d18f5dea3f457ab01f47bd8362f050b31b0d4d4eca054e4722`.

All three Clippy selections passed with warnings denied, including the
separately selected native harness. Hermetic test results were:

| Selection | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Library units | 670 | 0 | 0 |
| Tools | 44 | 0 | 0 |
| Assets | 5 | 0 | 0 |
| Desktop | 250 | 0 | 0 |
| Domain | 537 | 0 | 0 |
| Engine | 236 | 0 | 0 |
| Presentation | 103 | 0 | 0 |
| Scene adapters | 12 | 0 | 0 |
| Storage | 194 | 0 | 0 |
| Total | 2051 | 0 | 0 |

The main executable unit selection and library doctests each contained zero
tests. The optimized all-targets build and the separate optimized native-harness
build both passed. The existing macOS linker warning
`__eh_frame section too large` appeared during test linking and remains
unsuppressed. This is not a zero-warning linker result.

The frozen source inventory is
`/tmp/claude-chess-library-report-checkpoint-20261017/source.sha256`, with manifest
SHA-256 `b634410e9edb89ad30f71dd934fddfc9aa4cf817de5ffcc72adea4d21a8b43de`.
Every entry was checked after verification before beginning the next source
increment.

## Completed task scope

- T146, T148 and T149: the raw editor domain, structural validation, declared
  variants and FEN, source isolation, selection and local history. Domain tests
  also verify the canonical non-nil UUID gateway.
- T150: schema version 4 and the separate editor repository, including invalid
  drafts, typed durable receipts, stale saves/deletes, tombstones and bounded
  pages. Reopened sessions contain no stored selection or edit history.
- T158 and T169: independent Standard Polyglot keys and record decoding,
  canonical legal continuation preparation, malformed-input refusals, weights,
  shares and study-only domain navigation. The actual board gateway uses
  pseudo-legal en-passant hashing.
- T160 and T161: immutable report lines, actual engine configuration and search
  correlation, progress and cancellation models, findings, and the selected
  expected-score policy. The two newer-score regressions were observed failing
  before their engine/report repair.

The specific source and test evidence is retained in
[editor domain](position-editor-domain.md),
[editor storage](position-editor-storage.md),
[Polyglot](opening-polyglot-book.md),
[report foundation](report-model-scoring.md), and
[report update regressions](report-analysis-update-review.md).

## Additional verified integration and remaining work

The full selection also covers the Library service and native host wiring,
application-level initial Open recovery, populated import budgeting, ten mounted
Library interaction cases and the puzzle-window readiness repair. Their narrower
evidence remains in [native Library mount](library-native-ui-mount.md),
[native routing](library-native-routing-integration.md), and
[import budgeting](populated-import-bound-repair.md).

Full Library handlers and organization/Trash/import/export UI, remaining annotation
widgets and save recovery, editor application and raw-scene mounting, durable
report scheduling/services/UI, and indexed opening services remain outstanding.
No complete journey, native GPU execution, OS accessibility, cross-platform
installation, performance acceptance or release acceptance is established by
this hermetic/build checkpoint.
