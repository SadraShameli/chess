# Release blockers

Release binaries are checked with `chess-tools package-verify <platform> <directory>`
(`tools/packaging/verify.rs`): no development URLs, test listeners or permissions, no
engine binary for another platform, executables only for the wanted architecture, the
engine hash matching `resources/engine/manifest.json`, and on Linux no glibc symbol
newer than 2.35. Unperformed checks stay blockers.

| Blocker | State |
| --- | --- |
| `package-verify` run on a real Windows 11 x64 package | Outstanding |
| `package-verify` run on a real macOS arm64 package | Outstanding |
| `package-verify` run on a real Ubuntu 22.04 x64 package | Outstanding |
| Native clean-install and optimized-profile checks listed in evidence/us5.md | Outstanding |
| Review of the NNUE network terms before any external conveyance | Outstanding |
| Production starter datasets and original sound resources revalidated after US13 | Outstanding |
