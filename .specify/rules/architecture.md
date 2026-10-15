# Rust architecture

- Behavior belongs to the struct or enum owning its data, in inherent `impl`
  methods. One component has one responsibility and a small public API. Use
  composition for shared state and behavior; do not imitate class inheritance.
- Define traits first at actual variability and external-effect boundaries. The
  consuming lower-level component owns the port; higher-level adapters implement
  it. Required operations are required trait methods, never defaults that panic
  or return an unimplemented-operation error.
- Use generic bounds and static dispatch when the implementation is known. Use
  `dyn Trait` only for required runtime selection or heterogeneous storage. Do
  not create a trait for every value object. Abstract-first design means explicit
  contracts, not a speculative framework.
- One composition root in `main.rs`, or a delegated bootstrap module, constructs
  the object graph and injects dependencies. Deep components never construct
  their own global config, clients, loggers, repositories, or service locators.
- Maintain a functional core with deterministic calculations and typed results.
  Pure operations still belong to their owning type. Standalone functions are
  limited to entry points, test functions, and genuinely shared cross-component
  helpers. Iterators, typed closures, and pattern matching are welcome.
- Keep I/O, clocks, randomness, persistence, and logging at explicit application
  and adapter boundaries. Inject clocks and seeded randomness. Own state
  transitions on the state type; return new values where practical or use narrow
  `&mut self` transitions for intrinsically mutable state.
- Interchangeable evaluators, handlers, adapters, and other plugin families share
  a trait and one typed registry. Selection and contract coverage derive from the
  registry, never separate discovery lists. A closed family may use exhaustive
  enum dispatch. Register explicitly at the composition root, without import
  side effects or uncontrolled global registration.
- Prefer the standard library and existing project facilities to a new wrapper.
  An abstraction must protect a real invariant, boundary, or shared behavior.
  See [structure](structure.md) for dependency direction and
  [performance](performance.md) for hot-path dispatch and concurrency decisions.
