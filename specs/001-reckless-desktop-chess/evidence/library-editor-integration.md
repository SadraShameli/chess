# Editor projection, played export and notation persistence integration

The parent integrated both source handoffs and the reserved notation adapter
after checkpoint `e618533aa6f2f74da54b3d745f2b7b4ac19ecbc3`. The final canonical
command exited 0 on 2026-10-17:

```text
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make verify
```

Root Rust remained frozen during execution. Default Cargo parallelism was used,
with sequential gates and no worker caps. Formatting, all three Clippy
selections with `-D warnings`, all tests and both optimized builds passed.

| Executed test selection | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Library unit/private contracts | 445 | 0 | 0 |
| Tools | 44 | 0 | 0 |
| Assets | 5 | 0 | 0 |
| Desktop | 232 | 0 | 0 |
| Domain | 466 | 0 | 0 |
| Engine | 235 | 0 | 0 |
| Presentation | 103 | 0 | 0 |
| Scene | 12 | 0 | 0 |
| Storage | 161 | 0 | 0 |
| Total | 1703 | 0 | 0 |

The executable unit selection and doctests each ran zero tests. The native GPU
harness was compiled and linted separately, not executed. The existing macOS
debug linker warning, `__eh_frame section too large`, remains unsuppressed.
Clippy emitted no warnings. These results do not establish a zero-warning
platform or native GPU acceptance.

The new cases comprise 14 editor projections, eight notation-save contracts,
eight played-export cases and seven clockless study-continuation cases. The
domain tests include all 54 cases in the two exporter/study source handoffs.
The wide-parent fixture actually exercised more than 100 legal children.

Earlier focused executions passed seven notation-save cases, then all 132
notation domain cases. A subsequent interface refinement shared one canonical
`Arc<StorageLibraryRecord>` with the editor, deriving the real ID and notation
together. It adapted the seven save contracts and added a real played-source
refusal case. The first combined compile found one fixture still calling
`tree()` directly on that Arc; correcting the fixture allowed all 445 private
cases to pass. Domain and storage compatibility then passed 466 and 161 cases.
The first canonical attempt found two needless borrows in the new preparation
adapter. The parent removed them and repeated the complete command above.
Neither compilation nor lint corrections were described as semantic reds.

The same canonical writer now exports real played sessions with their selected
study line, actual outcome, metadata and genuine mover post-clocks. The new
snapshot-side accessors read typed balances without cloning the time-control
vector for each exported move or codec clock. Clockless variations use the
canonical tree preparation and preserve the original mainline endpoint.
No played session or clock is fabricated for notation.

Editor projections carry full game, source, source revision, storage revision,
reopen epoch, local generation and selected-node context. Tree and child pages
remain bounded to 100 canonical rows, retaining actual child order and private
validated continuations. The notation adapter reserves receiving bytes before
CPU admission, charges DTO/output copies, transfers its permit to the existing
prepared write and returns the real retained storage mutation. Its source stays
owned on cancellation, conflict or capacity refusal. The inherited decoder's
four-times-body allowance remains unproven as a whole-process peak-memory bound.

Seven raw logs, `SHA256SUMS` and the 19 changed Rust blob identities are retained
in `.git/parallel-patches/library-editor-integration-20261017/`. The final log is
`library-editor-notation-make-verify-2.log`, SHA-256
`4d53a0d6339ef0eb6e09bacd1887282bd94f93e8a7266bbe4a6b1448367e9eb6`.
The adjacent accepted patch and manifest record the complete staged snapshot.

The notation owner, played-supervisor editor forwarding, admitted library
handlers, runtime saved-game opening/closing, correlated Trash prerequisites,
file-handle publication, import warning display and mounted controls remain
subsequent integration work. T097/T100 still require real import backend and
usable-space/scale acceptance. T104/T106/T107/T108/T109/T110 and native/release
gates remain pending. Task markers remain 94 complete and 201 pending of 295.
