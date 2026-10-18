# Dynamic engine Threads bounds

Validated 2026-10-16 against base `e283ddb` on
`001-reckless-desktop-chess`, using Rust 1.99.0 and debug checks only.
This is a partial T077 slice. Task markers remain unchanged at 64/295.

## Source and behavior

Pinned Reckless source: `0e92358f5acd66e5ac77b1bf558202e47c515435`.
[uci.rs](https://github.com/codedeliveryservice/Reckless/blob/0e92358f5acd66e5ac77b1bf558202e47c515435/src/uci.rs#L147)
advertises `ThreadPool::available_threads()`;
[threadpool.rs](https://github.com/codedeliveryservice/Reckless/blob/0e92358f5acd66e5ac77b1bf558202e47c515435/src/threadpool.rs#L23)
returns `max(512, 4 * available_parallelism)` and `set_count` accepts `usize`.
The prepared local sources were read and their Git blob hashes verified against
the pinned tree: UCI `1862a7af9a29fd83f7b1c69bf2cd33abf330dc95`,
thread pool `5d0b9ff2c32847f162c3114477aacc9c13633856`.

Discovery retains descriptors keyed by canonical option enums. Threads keeps
default/minimum 1 and accepts canonical, representable maxima at least 512 and
divisible by four, including 516 and 1024. Fixed descriptors remain exact;
duplicate, malformed and incompatible options fail. Unknown advertisements fail
with a typed cause retaining their name and descriptor.

Resource construction accepts positive `u32` thread requests without a fixed
512 ceiling. Each session checks all requested resources against its discovered
descriptors before lifecycle mutation or option I/O. Rejected 513/512 and
1025/1024 requests preserve idle state, prior resources and subsequent reuse.
Hash bounds, overhead bounds and exact whole milliseconds remain enforced.
Desktop allocation assigns the complete lease without a portable thread cap.

## Red-first evidence

Before production edits, `cargo test --locked --test engine` exited 101:

```text
dynamic_threads_descriptor_accepts_the_advertised_high_core_bound: Error: OptionMismatch
resource_options_validate_representation_and_keep_fixed_hash_and_overhead_bounds:
Error: Range { option: Threads, value: 513, minimum: 1, maximum: 512 }
resource_application_uses_each_sessions_advertised_threads_bound: Error: Capabilities(OptionMismatch)
unsupported_threads_preserve_idle_resources_and_reusability_without_io:
Error: Range { option: Threads, value: 513, minimum: 1, maximum: 512 }
test result: FAILED. 88 passed; 4 failed
```

`cargo test --locked --test desktop desktop_native_game_resources::high_core_budget_is_fully_assigned_shared_fairly_and_released`
exited 101 before production edits:

```text
assertion `left == right` failed
  left: 512
 right: 1024
test result: FAILED. 0 passed; 1 failed; 200 filtered out
```

After enabling representation/discovery and before adding the session guard,
`cargo test --locked --test engine engine_resource_config::unsupported_threads_preserve_idle_resources_and_reusability_without_io`
also exited 101: `assert_no_command` found unexpected queued engine I/O.

## Final checks

Executed sequentially after final source edits:

- `cargo fmt --all`: passed.
- `cargo clippy --locked --lib --test engine --test desktop --test native -- -D warnings`:
  passed without warnings.
- `cargo test --locked --test engine --test desktop`: 93 engine and 201 desktop
  tests passed, with zero failures or ignored tests.
- `cargo test --locked --profile dev --test native -- --game-composition`:
  passed once. Both engine colors delivered legal mates; two post-result
  requests were rejected. Production SQLite restoration/relaunch preserved the
  one-move records and frozen clocks. Both bootstraps recovered 14/14 CPU slots,
  acquired a full-worker lease, observed zero direct children and reacquired the
  exclusive profile.

Real engine: macOS ARM64, binary SHA-256
`ab4128cd33149b14713475e049db8ddaff702ee79bfb412ecfbde91655273689`,
embedded `v54-5478683c.nnue`, 63,266,880 bytes, model SHA-256
`5478683cb1bababde29ae8f29468a99846726548fc6a0ed54cac40ab6d38efbf`.

High-core behavior uses transcript processes and semaphore leases. The injected
1024-worker regression assigns all workers, splits two simultaneous searches
512/512, rejects oversubscription, waits for other shared work and recovers all
capacity. It spawns no real 1024-thread engine. Existing fairness rotation,
cancellation, memory pressure and startup-hash admission tests also pass.

The known macOS `__eh_frame section too large (max 16MB)` linker warning remains
in test linking. No release, all-features, all-targets, full-product, GPU or
cross-platform acceptance checks were run. No throughput/scaling claim is made.
Full configuration of all eight options, tablebase path/load, workbench and
presentation work, and T076 diagnostic tails remain pending. No vendor or
dependency changes were made.
