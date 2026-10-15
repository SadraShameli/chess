# Performance and concurrency

## Highest engineering priority

- Performance is the highest engineering priority when choosing a correct
  implementation. Establish the required throughput, latency distribution, memory
  budget, and scaling behavior in each relevant feature plan. Never trade away
  specified correctness, type safety, or a project invariant for an unmeasured
  speed claim.
- Exploit all available cores and hardware threads for parallelizable work by
  default. Do not impose an arbitrary fixed worker cap, a single-thread default,
  or a small-machine assumption. Sequential execution requires a dependency,
  determinism requirement, or measured evidence that overhead outweighs benefit.
- Detect usable capacity with `std::thread::available_parallelism` or an equivalent
  runtime facility respecting affinity, container quotas, and the host allocation.
  If detection fails, return a typed error or require an explicit budget; never
  silently fall back to one worker. Capacity is hardware-thread availability, not
  merely the number of physical cores.

## One capacity budget

- The composition root owns a typed concurrency budget and injects it. Give one
  canonical owner responsibility for partitioning runnable CPU work across CPU
  pools, async blocking pools, subprocesses, and third-party native thread pools.
  Never let each subsystem independently create a full-capacity CPU pool.
- Reuse long-lived workers and coarse enough chunks to amortize scheduling.
  Favor work stealing or dynamic distribution for uneven workloads. Avoid a
  thread per item, nested parallel loops with independent pools, or unbounded
  `spawn_blocking` calls. Preserve ordering only where the contract needs it.
- Saturate the shared CPU budget while independent work is available. Measure
  scaling at one worker and the detected maximum; investigate imbalance,
  contention, or memory-bandwidth limits when extra workers do not help.
- Cargo compilation and the Rust test harness use detected CPU parallelism by
  default. Do not check in a machine-specific `jobs` count or test-thread cap.
  Verification gates run in the order defined by [verification](verification.md);
  each gate retains its internal parallelism.

## Async I/O and task lifecycle

- Use async APIs for concurrent network, filesystem, timer, and channel work when
  independent waiting can overlap and the chosen library supports it. A platform
  API implemented with blocking offload remains charged to the shared budget.
  Do not mark pure CPU calculations async merely to return a future.
- Run CPU-bound work on the bounded CPU executor, outside async scheduler
  workers. Never perform blocking I/O, blocking lock acquisition, thread sleeps,
  or long compute loops on an async runtime worker. Async is not CPU parallelism.
- Await independent operations concurrently with bounded fan-out rather than
  sequentially. Bound in-flight tasks, queues, buffers, and outbound requests;
  use backpressure and load shedding with explicit typed outcomes. A semaphore
  must limit task creation as well as work, rather than leaving millions of
  waiting tasks allocated.
- Every task has an owner and an observed completion result. Use structured
  concurrency, explicit cancellation, and bounded graceful shutdown. Do not
  detach critical work. Cancellation must leave shared state valid and release
  resources. Avoid multiple nested async runtimes.
- External operations have explicit timeout policies and bounded retries only
  when idempotency permits them. Preserve causes and distinguish timeout,
  cancellation, unavailable capacity, and application refusal.
- Never hold a lock or `RefCell` borrow across `await`, blocking work, or I/O.
  Minimize critical sections. Prefer owned messages, worker-local state, or
  sharded data over a globally contended mutex. Use channels and atomics only
  with an explicit ownership and memory-ordering argument.
- Introduce an async runtime or parallelism library only with the feature that
  needs it. Reuse one established runtime and CPU executor. If using Tokio and
  Rayon, wire both through the same concurrency policy rather than separate
  unlimited defaults.

## Hot paths and memory

- Choose appropriate algorithms and data structures before micro-optimizing.
  Reuse derived results at their canonical owner; cache keys encode every input
  affecting a value, with bounded capacity and a clear invalidation policy.
- Favor compact validated values, contiguous storage, preallocated or reusable
  buffers, iterators, and explicit borrowing. Avoid per-node or per-item heap
  allocation, repeated cloning, string formatting, serialization, and logging in
  measured hot loops. Do not use linked structures or a heap box without need.
- Keep hot-path polymorphism statically dispatched or a compact enum when the
  contract permits it. Confine trait-object dispatch to actual runtime boundaries.
  Avoid false sharing and unnecessary synchronization; measure layout and cache
  behavior before introducing padding or a specialized representation.
- Use compiler auto-vectorization and stable safe facilities before adding SIMD
  dependencies. Do not use unsafe code as a performance shortcut. Hardware-specific
  tuning is opt-in and benchmarked, with a portable baseline; never check in
  unconditional `target-cpu=native` flags for distributable binaries.

## Measurement and acceptance

- Benchmark optimized builds with representative fixed inputs, warm-up, repeated
  samples, and the same compiler, feature set, CPU budget, and machine conditions.
  Record throughput, latency percentiles, allocations or peak memory as relevant,
  and one-worker versus maximum-worker scaling. Do not benchmark debug builds or
  infer a speedup from a single elapsed-time measurement.
- Keep benchmark fixtures outside instruction directories. Use the profiling
  build for CPU and allocation investigation. Choose profiling tools only when
  needed; do not install speculative dependencies or invent benchmark results.
- A performance change must preserve behavior through a reference comparison or
  invariant check and demonstrate the intended workload improvement. Include
  scheduling, synchronization, I/O, and memory costs. Assess thin versus fat LTO,
  codegen units, PGO, and platform tuning with measurements when justified.
- Record before/after evidence and known limits in the feature's artifacts.
  Define reproducible budgets where the environment permits; timing-sensitive
  benchmarks do not replace deterministic correctness tests.
