# Accepted engine checkpoint and premove boundary

The parent observed an accepted-prefix failure on 2026-10-17 against supervisor
blob `faece4d27a6b33c07d5ea5d78a8a93990c7a1391`, unchanged from checkpoint
`647147d9ff95cb745ede46637048ef312e3070af`.

The real session accepts Reckless `e2e4` with a legal queued human `e7e5`.
An injected first monotonic read succeeds and the next read fails. Previously,
the single computation accepted the engine move and consumed the premove before
the second read, then returned `Time(TimeOrder)` without returning the engine
save request. The actor could not enqueue that accepted checkpoint normally.

The exact command was:

```text
env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target cargo test --locked --all-features --lib accepted_engine_checkpoint_survives_later_premove_time_failure -- --nocapture
```

The valid baseline exited 101: zero passed, one failed and 321 filtered. The
assertion confirmed an accepted engine node and a lost computation result.
Earlier fixture compilation and missing premove-enable attempts establish no
semantic red; the authoritative log is `engine-premove-checkpoint-red-3.log`.

The supervisor now returns and enqueues the engine save before scheduling a
separate premove computation. Its retained typed continuation binds the exact
post-engine context and checkpoint instant. Queued urgent controls are observed
before admitting that continuation. A changed context discards the old automatic
continuation instead of rebasing it silently.

The premove step validates its context before sampling time or consuming the
intent. Its time failure leaves the accepted engine request and queued intent
owned. A continuation error publishes a typed contextual notice without
immediately retrying the same failing continuation. This does not promise
continued play when the underlying monotonic provider remains unavailable;
ordinary lifecycle error and retained-cleanup policies still apply.

Legal premoves sample fresh time and retain their own subsequent save. An empty
or illegal intent preserves the engine checkpoint's observation instant and
emits no extra content save. This maintains the existing durable clock and
illegal-notice contract. An intermediate run passed six of seven focused cases;
the illegal-notice clock comparison exposed a later observation instant without
a corresponding checkpoint. Retaining the original instant corrected that
integration failure before the final focused execution.

The final focused premove selection passed seven tests with zero failures or
ignored tests and 315 filtered. Desktop game/supervisor compatibility passed
32 tests with zero failures or ignored tests and 187 filtered. Tests cover
ordered accepted checkpoints, storage failure/retry, cancellation, exact deadline,
actual elapsed clock charges, durable illegal notices and native local-game
recovery. The regression also verifies that failure in the later computation
does not consume the still-owned queued intent or change the accepted revision.

The tests-only baseline patch, frozen production blob, exact command and red
logs are preserved under
`.git/parallel-patches/engine-premove-checkpoint-baseline-20261017/`.
Focused repaired and compatibility logs remain in the shared integration scratch
pending the next combined canonical checkpoint. No new canonical, GPU, platform,
or performance result is established by these focused executions.

## Parent canonical integration

The combined source and all relevant regressions were executed by the parent.
[Verified integration](library-runtime-import-integration.md) records the exact
snapshot, 1,666 passing tests, three clean Clippy selections, both optimized
builds, durable log hashes, known linker warning and remaining acceptance gates.
Earlier unrun statements describe this writer's isolated handoff only.
