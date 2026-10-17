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
  `--profile x86-64-v3`, `x86-64-v4` or `numa` for the optimized x86 builds; the
  application picks the best profile your processor and operating system support
  and falls back to the portable baseline with a stated reason.
- Assemble a package on its own operating system with
  `chess-tools package-run <windows|macos|linux>`; print the steps first with
  `package-plan`. Linux packaging needs `CHESS_APPIMAGE_TOOL_TAG` set to a stable
  release tag of the AppImage toolchain; rolling and prerelease tags are refused.
- Check a staged package with `chess-tools package-verify <platform> <directory>` and
  the notices with `chess-tools notices-check`.
