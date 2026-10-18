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

## Linux glibc compatibility check (written, not compiled or run)

`EnginePackagePlatform` now carries the detected glibc version (from
`/usr/bin/getconf GNU_LIBC_VERSION` on Linux) and `EnginePackageSelector::incompatibilities`
rejects every profile with the reason "glibc X is older than the required 2.35" when a
detected version is below the packaged baseline. An undetectable version does not block
selection because the host version check already requires Ubuntu 22.04 or later. The
libnuma check keeps its `libnuma.so.1` soname. The oldest-glibc symbol check on the Linux
binary and the Ubuntu 22.04 run stay outstanding. Test:
`glibc_older_than_the_package_baseline_blocks_every_profile_with_a_reason` in
`tests/engine/package.rs`.
