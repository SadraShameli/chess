# Toolchain evidence

Recorded 2026-10-16 on macOS 27.0.1 ARM64. This is current-host setup evidence,
not minimum-platform or complete application acceptance.

| Tool | Observed |
| --- | --- |
| rustc | 1.99.0, b940084d7 |
| Cargo | 1.99.0, 5f94df478 |
| Bun | 1.4.3 |
| Node | 26.11.1 |
| Apple Clang | 21.0.0, Command Line Tools |
| Tauri Rust / API / CLI | 2.12.4 / 2.12.4 / 2.12.3 |
| Vue / vue-tsc / TypeScript | 3.5.44 / 3.3.13 / 6.0.3 |
| Vite / Vue plugin | 8.3.4 / 6.0.10 |
| Tailwind / Vite plugin | 4.3.3 / 4.3.3 |
| shadcn-vue / Reka UI | 2.8.2 / 2.11.0 |
| Three.js | 0.186.1 |
| ESLint / typescript-eslint | 10.12.0 / 8.71.2 |
| Prettier / Tailwind plugin | 3.9.10 / 0.8.1 |
| Stylelint / standard config | 17.16.0 / 40.0.0 |
| Vitest / browser-playwright | 5.0.5 / 5.0.5 |
| Playwright / Vue Test Utils | 1.64.1 / 2.5.1 |
| WDIO CLI / webdriverio | 9.31.9 / 9.31.9 |
| WDIO types / reporter | 9.31.2 / 9.31.2 |
| WDIO Tauri service / native plugins | 1.5.0 / 1.5.0 |

The exact native and JavaScript transitive graphs are in Cargo.lock and
ui/bun.lock. The current Cargo graph contains 477 packages. Cargo uses the
standalone compiler matching the repository pin; rustup is unavailable.
No toolchain replacement was installed. Python tooling remains uv-managed.

## Executed checks

The canonical `make verify` passed after correcting Bun argument order. It ran
Rust and frontend formatting, strict vue-tsc, typed ESLint, Stylelint, all-target
all-feature Clippy, Rust tests, browser tests and an optimized production build.
There are currently zero Rust behavior tests and one passing Chromium component
test. This proves the shell loads, not chess correctness.

A separate instrumented native build and WDIO test passed in macOS WebKit
605.1.15: one native application-landmark test. The runner stopped the app with
no remaining task-owned application process. The normal Cargo dependency tree
and production JavaScript contain no WDIO plugin or frontend listener.

The empty-argument run command has no separator; supplied arguments use Tauri's
documented runner/application boundary. See the actual Makefile for commands.

## Compatibility findings

Bun runs Vite, Vitest, ESLint, Stylelint, Prettier and the Tauri CLI successfully.
Vue's compiler interception under Bun failed to resolve SFC imports; Node runs
vue-tsc successfully with the same strict configuration. WDIO's expectation
dependency failed under Bun with an invalid superclass; Node runs the native
suite successfully. Bun still owns installation and all project scripts.

WDIO's ambient declaration graph conflicts with TypeScript 6 DOM definitions,
exact optional properties and unrelated native framework extensions. The local
declaration describes only the consumed, verified browser/element methods.
Runtime TSX uses a separate configuration so declaration mappings cannot replace
the actual package. No vendor source was edited and skipLibCheck is not enabled.
Extend this declaration alongside real native-test API usage.

Native test plugins and capabilities require the explicit native-test feature
and test configuration. Their frontend is built separately from production.
The embedded provider needs no paid driver or downloaded Edge driver.

## Remaining target prerequisites

Full Xcode is unavailable on this host; Command Line Tools can compile and run
this native shell. DMG construction and signing remain separate package tests.
Windows requires the MSVC C/C++ toolchain and WebView2; its offline runtime
installer is a later packaging task. Linux needs a modern compiler, pkg-config,
GTK/WebKitGTK development dependencies and an updated WebKitGTK runtime meeting
Tailwind 4's browser requirements. Ubuntu's initial release runtime alone is not
evidence of compatibility.

Reckless's bundled Fathom support needs an available C compiler and native
archive/link tooling. Clang is available locally; Windows/Linux compiler and
native Syzygy builds remain unverified. No engine, model, dataset, installer,
release workflow, OTA credential or signing key has been provisioned by setup.
