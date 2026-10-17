# Install, uninstall and local builds

These steps describe how each package is meant to install. They were written
before any package was built or run on the target systems; the actual results are
recorded in `specs/001-reckless-desktop-chess/evidence/us5.md` and every step stays
unverified until that file says otherwise.

## Windows 11 x86-64 (unsigned NSIS installer)

1. Run the installer. Windows SmartScreen reports an unrecognized app because the
   installer is not signed. Choose More info, then Run anyway.
2. Pick per-user or per-machine installation. No internet connection, WebView2
   runtime or extra Visual C++ download is needed.
3. Uninstall from Settings, Apps, Installed apps, Chess. Your profile folder is not
   removed; delete it yourself to remove games and settings.

## macOS 14+ on Apple Silicon (app in a DMG)

1. Open the DMG and drag Chess to Applications.
2. The app is ad-hoc signed without a Developer ID and is not notarized. The first
   launch is blocked. Open System Settings, Privacy and Security, and choose Open
   Anyway next to Chess, then confirm.
3. Uninstall by moving Chess to the Trash. Your profile folder is not removed.

## Ubuntu 22.04+ on x86-64 (.deb or AppImage)

1. Prerequisites: working Vulkan graphics drivers (`libvulkan1`), X11 or Wayland,
   `libxkbcommon0`, and `xdg-desktop-portal` for native dialogs. On an offline
   machine download these packages and their dependencies from the same Ubuntu
   release on a connected machine, copy them over and install with `dpkg -i`.
2. Install the .deb with `sudo apt install ./chess_*.deb`, or make the AppImage
   executable and run it.
3. Uninstall the .deb with `sudo apt remove chess`; delete the AppImage file.

## Local native builds

- Install the pinned Rust toolchain, then run `make run` for the optimized build or
  `make debug` for a debuggable one. Use `--profile-directory <path>` for an
  isolated profile.
- Build the engine with `cargo run --locked --bin chess-tools -- engine-build`. Add
  `--profile x86-64-v3`, `x86-64-v4` or `numa` for the optimized x86 builds, or `apple-neon` on Apple Silicon; the
  application picks the best profile your processor and operating system support
  and falls back to the portable baseline with a stated reason.
- Assemble a package on its own operating system with
  `chess-tools package-run <windows|macos|linux>`; print the steps first with
  `package-plan`. Linux packaging needs `CHESS_APPIMAGE_TOOL_TAG` set to a stable
  release tag of the AppImage toolchain; rolling and prerelease tags are refused.
- Check a staged package with `chess-tools package-verify <platform> <directory>` and
  the notices with `chess-tools notices-check`.

## Current implementation state


This describes what works in the current source tree. Packaged installers, the
signed update channel and the production starter data package are not available
yet, so installation today means building from source.

## Build and run

- `make run` builds and starts the optimized application.
- `make debug` starts a debuggable build.
- `make run ARGS="--profile-directory <path>"` keeps all data in an isolated
  directory, which is the safest way to try the application.
- `make verify` runs the complete verification sequence.

## Study data

The application never downloads or updates datasets by itself. Opening
statistics, the reference book and puzzles read only data you install.

- Settings, Data lists the packaged starter sources and every installed dataset
  with its source, version, license and record count.
- Import Games (PGN), Import Opening Book (Polyglot) and Import Puzzles (CSV)
  each show a preview with accepted and rejected counts and the available disk
  space. Nothing is installed until you confirm.
- Replacing a dataset installs the new version and keeps the old one until the
  runs, queries and history that use it finish.
- Deleting a dataset asks for confirmation and is refused while active work uses
  it. Saved history keeps its game and puzzle snapshots.
- If installed starter data fails its integrity check, only the affected feature
  (explorer, reference book or puzzles) is unavailable. Your library, history
  and unaffected play continue, and no partial statistics are shown.

Reference books are Standard chess only. In Chess960 and DFRC the book shows an
unsupported state instead of guessing a hash convention.

## Puzzle history

Saved puzzle runs are listed under Puzzles. Delete Run and Delete All History ask
for confirmation. A run that is in progress cannot be deleted. Puzzle datasets
are kept when history is deleted.

## Preparing starter data (developers)

The starter data is prepared offline with `chess-tools`:

- `data-verify <sources.json> <cache-directory>` checks the frozen CC0 sources.
  It refuses until each source's SHA-256 is recorded in
  `resources/datasets/sources.json` and the cached file matches.
- `data-select-games <pgn> <output-directory>` and
  `data-select-puzzles <csv> <output-directory> <source> <version>` apply the
  subset selection policy to uncompressed input. Decompress `.zst` downloads
  first.
- `data-build <selection-directory> <output-directory>` applies the installed
  byte caps and records every reduction.
- `benchmark-evaluate <measurements.json>` checks collected latency, frame,
  startup and idle measurements against the acceptance thresholds.

## Not available yet

Packaged installers for Windows, macOS and Linux, the private update channel,
backup and restore of datasets, and measured performance figures. These are
tracked in the specification evidence and are not claimed here.
