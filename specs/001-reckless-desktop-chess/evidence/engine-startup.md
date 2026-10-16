# Engine selection and startup

T026 adds the injected process boundary and the current-host readiness command:

```sh
cargo run --locked --bin chess-tools -- engine-check
```

The command selected the app-owned ARM64 package, verified its length and SHA-256,
started both bounded pipe readers, completed the UCI handshake, discovered all
eight expected options, completed a separate readiness handshake, and gracefully
quit and reaped the engine. No network acquisition was performed.

| Observation | Result |
| --- | --- |
| Engine | Reckless 0.9.0 |
| Author | Arseniy Surkov, Shahin M. Shahin, and Styx |
| Target | aarch64-apple-darwin |
| Binary SHA-256 | ab4128cd33149b14713475e049db8ddaff702ee79bfb412ecfbde91655273689 |
| Advertised options | Hash, Threads, MoveOverhead, Minimal, Clear Hash, UCI_Chess960, MultiPV, SyzygyPath |
| MultiPV | Default 1, minimum 1, maximum 256 |
| Strict all-target/all-feature Clippy | Passed |
| Engine integration tests | 10 passed |
| Bounded pipe-reader unit test | 1 passed |

The real engine's MultiPV maximum is 256. The earlier inventory value of 218 was
corrected from the binary's descriptor. Application search requests still need
their legal-root-move cap when the analysis layer is implemented.

Hermetic tests cover host minima/unknown versions/CPU refusal, unsupported package
targets, changed bytes, nonexecutable files, symlink escape, oversized manifests,
complete/duplicate/changed capabilities, wrong identities, independent five-second
UCI/readiness deadlines and cleanup. Paused Tokio time drives deadline checks.
The pipe-reader test covers oversized unterminated output, truncated lines,
invalid UTF-8 and CRLF handling. Commands reject embedded line breaks; native
shutdown shares a three-second budget for graceful exit, forced termination,
reaping and pipe-task observation.

Host observations use sysinfo 0.39.6, MIT licensed, with only its system feature.
Windows/Linux observations are hermetic fixtures; those binaries and native
systems have not been exercised here. Search lifecycle, option application,
telemetry, stress testing and full platform acceptance remain their later tasks.
