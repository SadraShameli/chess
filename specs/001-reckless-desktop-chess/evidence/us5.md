# User Story 5 evidence

Packaging configuration and tools are written (`packager.toml`, `tools/packaging/`,
`tools/engine/build.rs`). Nothing below has been built, installed or run on a target
system; each row stays outstanding until the exact result is recorded here.

| Check | Environment | State |
| --- | --- | --- |
| Clean unsigned NSIS install, offline, with the SmartScreen warning flow | Windows 11 x64 | Outstanding |
| Clean app and DMG install with manual Open Anyway approval | macOS 14+ Apple Silicon | Outstanding |
| Clean .deb install and AppImage run, X11 | Ubuntu 22.04+ x64 | Outstanding |
| Clean .deb install and AppImage run, Wayland | Ubuntu 22.04+ x64 | Outstanding |
| Oldest supported glibc symbol check on the Linux binary | Ubuntu 22.04 | Outstanding |
| Optimized x86-64-v3 and x86-64-v4 engine builds and selection on suitable CPUs | x86-64 hardware | Outstanding |
| NUMA engine build and selection on a multi-node host with libnuma | Linux NUMA hardware | Outstanding |
| Fixed-fixture offline package launch and engine startup | each platform | Outstanding |
| Maintained stable-tagged AppImage toolchain gate resolved | Linux packager | Outstanding |
| Exact OS, GPU driver, CPU and package hashes | each platform | Outstanding |

Automated coverage that supports these checks: `tests/engine/package.rs`,
`tests/desktop/game_recovery_policy.rs`, `tests/desktop/migration_continuity.rs`,
`tests/native/installation.rs` and the unit tests under `tools/packaging/`.
