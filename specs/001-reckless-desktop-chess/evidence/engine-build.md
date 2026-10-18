# Current-host engine build

T025 built the approved Reckless source in a temporary checkout outside the
application repository. The tool verified HEAD against the pinned commit and
the downloaded network against its exact length and SHA-256 before supplying
EVALFILE to Cargo. Source acquisition and network acquisition overlapped under
the shared capacity budget. Compilation reserved the detected full capacity.

Reproduce with `cargo run --locked --bin chess-tools -- engine-build`.
The executable and model cache are ignored build inputs; the committed manifest
records the built artifact. This does not publish or convey an engine package.

| Property | Observed value |
| --- | --- |
| Source | Reckless v0.9.0, 0e92358f5acd66e5ac77b1bf558202e47c515435 |
| Compiler | rustc 1.99.0, b940084d7, Homebrew standalone binary |
| Package target | aarch64-apple-darwin |
| Binary format | Mach-O 64-bit executable arm64 |
| Deployment target | LC_BUILD_VERSION minos 14.0, SDK 27.0 |
| Upstream compiler diagnostic | aarch64-apple-darwin, features neon |
| Binary bytes | 65,383,664 |
| Binary SHA-256 | ab4128cd33149b14713475e049db8ddaff702ee79bfb412ecfbde91655273689 |
| Network bytes | 63,266,880 |
| Network SHA-256 | 5478683cb1bababde29ae8f29468a99846726548fc6a0ed54cac40ab6d38efbf |
| Production feature | Syzygy enabled |

The build explicitly overrides inherited Rust flags with the target's portable
profile and sets the macOS deployment target. It leaves upstream source intact;
the upstream build script generates its own Fathom bindings normally.

Strict Clippy passed. Two hermetic manifest tests passed, including direct
deserialization checks against source/model changes, path traversal, disabled
Syzygy, empty binaries, duplicate packages and noncanonical checksums. One
tool-publication test passed; it was observed failing on an empty copied binary
before correcting the shared file offset after hashing.

Current-host UCI readiness is recorded in [engine startup](engine-startup.md).
Windows/Linux compilation, minimum-hardware execution and the full capability
inventory remain subsequent acceptance work. Network terms
for external distribution remain the release gate recorded in research.
