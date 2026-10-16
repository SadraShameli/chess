# Platform, Engine Package, and Release Contract

**Status**: Design selected under owner delegation; implementation and release evidence remain unperformed.

Use [the resolved decisions](../decisions.md) for concrete defaults, algorithms
and former draft questions; [the review](../review.md) records owner choices.

See [research](../research.md) for tagged sources and provenance limitations.
This contract defines future build/release acceptance; it publishes nothing.

## Supported target matrix

The owner confirmed these OS/architecture minimums on 2026-10-16. Installer
Windows NSIS .exe and macOS Apple Silicon .dmg formats are approved. The owner
requires no payments for anything for now. Use unsigned personal-use Windows
installers with OS-warning guidance and locally ad-hoc-signed macOS builds
without Developer ID/notarization, with validated manual OS approval guidance.
No paid service, membership, hosting, assets, data, or CI spending is authorized.
Linux is x86-64 only, with both .deb and AppImage selected. Use native hosted
builders and selected native tests after the zero-cost CI activation gate passes. Matching updater
artifacts and native installation behavior remain unvalidated.

| Target | Native builder and artifacts | Acceptance |
| --- | --- | --- |
| Windows 11 x86-64 | MSVC Windows runner; NSIS installer and matching `.exe` sidecar | Clean install, WebView2/offline prerequisite path, play/analysis/save, documented unsigned OS warnings, OTA signature verification |
| macOS 14+ ARM64 | Apple Silicon runner; `.app` and DMG | Minimum supported ARM Mac, ad-hoc-sign nested engine/app, validate manual OS approval, offline first run |
| Ubuntu 22.04+ x86-64 | Oldest declared baseline, WebKitGTK 4.1; `.deb` and AppImage | Actual minimum WebKit/CSS compatibility, X11/Wayland, supported Ubuntu releases |

Do not ship Intel macOS or a universal Mac package. Windows ARM64 is outside the
initial target set. Linux ARM64 is also outside the initial target set. Linux distribution coverage is limited to tested runtime
baselines; AppImage does not remove glibc, graphics, or WebView dependencies.
If Tailwind's current CSS requirements exceed a stock minimum OS WebView,
require an available supported runtime update or raise the documented minimum;
do not silently ship broken styles or downgrade the chosen stack.

## Reproducible inputs

- Root Cargo.lock and existing Rust toolchain pin; UI package manifest plus
  text `bun.lock`, pinned Bun, frozen installs, and compatible stable tagged
  Tauri/Vue/Tailwind/shadcn-vue/lint/formatter/test tool versions.
- Reckless v0.9.0 commit and all required submodule/source inputs, stable
  compiler, explicit target, Cargo features, build flags, and binary digest.
- Exact network identity/hash from research, supplied as a verified build input
  through `EVALFILE`; no opportunistic build-script network fetch.
- Fathom/Clang/libclang prerequisites for default Syzygy builds. All production
  profiles retain Syzygy. No `--no-default-features` shortcut to avoid packaging it.
- Asset licenses, source bundles, dependency notices, and target runtime inventory.

Build the external engine in an isolated build checkout/cache, without changing
vendor source. Override its host-native compiler configuration explicitly.
The application manifest maps resource names to target-specific executable paths;
the frontend never supplies those paths. Verify binary hashes before launch.

## Engine CPU profiles

| Profile | Target and requirements |
| --- | --- |
| Generic | Windows/Linux x86-64 explicit `x86-64` baseline |
| AVX2 | Explicit `x86-64-v3`, including its full required CPU/OS state |
| AVX512 | `x86-64-v4` plus upstream explicit `gfni,avx512bw,avx512vl,avx512vbmi,avx512vbmi2,avx512vnni,avx512bitalg` requirements |
| ARM64 | Explicit portable AArch64 baseline, engine NEON path; no host-native leakage |
| Linux NUMA | Optional Linux build with `numa`, libnuma runtime and compatible CPU profile |

Select the fastest compatible profile using complete feature checks, including
OS SIMD state, never by trying a binary and waiting for an illegal instruction.
Bundle the generic baseline plus optimized profiles required for the declared
platform. Record the resulting installer size; do not duplicate model files at
runtime because each sidecar embeds its network. Assess package compression and
profile inclusion with size/load measurements rather than deleting capabilities.

Official upstream macOS prose is inaccurate: header inspection found a thin
ARM64 asset, and its workflow uses native CPU flags. Controlled baseline builds
are the release source of truth. Linux NUMA needs our own verified builds
because official release assets do not provide that profile.

NUMA selection is conditional on actual library/topology support. Show the active
profile and reason; a standard Linux fallback is explicit, not a claim that NUMA
is active everywhere. Benchmark on genuine multi-node hardware before marking
NUMA acceptance complete. A successful compile alone is insufficient.

## Native runtime and offline behavior

Opening corpora, book data, and puzzle collections are now included application
features. Bundle starter datasets and allow personal-file imports. The installed
starter-data budget is up to 2 GB, excluding application and engines. Exact
sources, selection quotas, formats, package composition and installed-size
measurement follow [dataset decisions](../decisions.md#starter-datasets-and-provenance). Bundle
separate master/tournament and broad online-game collections, selectable alone
or together. Prefer free public-domain or permissively licensed redistributable
data with required attribution/notices; review source terms before approval.
Dataset updates/imports are explicit manual actions only for the first release,
separate from app OTA. Starter opening/puzzle datasets cover Standard chess
only; clearly indicate missing matching Chess960/DFRC data. Starter puzzles
cover a broad available difficulty range with difficulty/theme filters, without
a player-rating system. Generate the study-only Polyglot book from the approved
master/tournament collection, weighted by observed move frequency, through the
first 20 full moves (40 plies) from the standard starting position. Integer
scaling preserves fitting counts, scales overflowing positions proportionally
to max 65535 with nearest rounding and minimum 1, and includes every legal
observed move; generate only from standard-initial-position games. See
[book decisions](../decisions.md#book-generation-and-explorer-semantics).
No automatic first-launch download is approved.

Bundle UI assets, the selected sculpted-piece assets, bundled fonts, engine,
and embedded network. The owner selected real-time 3D; package all required
models/textures and rendering resources for offline use after their formats,
sources, and licenses are approved. Direct Three.js with WebGL2 is selected;
graphics prerequisites still need validation. Validate compatibility and graphics failure handling
in each supported system WebView. The board is 3D-only, without a 2D fallback;
graphics failure during live play preserves game state, pauses clocks, and offers
Retry with explicit resume required after recovery. Initialization, context-loss,
asset/shader failure handling and 10-second retry deadline follow
[presentation decisions](../decisions.md#presentation-assets-and-input). WebView2 or
WebKit system prerequisites must be covered by the installer/documentation and
validated on a clean machine. For offline Windows installation, include an
offline Evergreen WebView2 runtime installer and validate its integration and
redistribution license, rather than relying on a network bootstrapper. Linux
installation may use internet access for missing system dependencies; document
offline preparation. Running chess after installation remains offline. Installation prerequisites and
application offline behavior are tested separately.

Use stable test-only WebdriverIO/Tauri embedded WebDriver tooling for integration
tests on all targets. Current official guidance supports macOS via this path;
direct tauri-driver alone has narrower coverage. Release artifacts must exclude
the WebDriver plugin, listener, permissions, development URL, and debug tooling.

## License and corresponding-source gate

Select private personal use, retain owner rights and upstream notices, and
add no application license file. This section records requirements for possible future distribution;
it does not require choosing an open-source license or publishing the private
repository for the current personal-use plan. No external release is requested.

Reckless v0.9.0 is AGPLv3. Fathom is MIT; shakmaty and pgn-reader are
GPL-3.0-or-later. Determine and document the application's compatible licensing
before conveying binaries. A subprocess boundary does not decide whether a
distribution is aggregation or a combined work.

The model repository has no explicit model LICENSE at research time. Before
external distribution, obtain authoritative terms covering the exact embedded
network and record provenance. Do not infer permission from download availability.

Package notices and license text, and provide compliant access to exact
Corresponding Source, build/install scripts, dependencies, and required model
materials. A moving main-branch link is insufficient. A clearly directed source
server can be used only when compliant access and availability are maintained.
These duties concern conveyance to recipients, including private distribution,
not only public releases. Private local development does not require publishing
the repository by itself.

No owner contact or maintainer message is authorized by this plan. Resolving
model terms may use already available authoritative documentation or a separately
authorized inquiry. Signing credentials and publication are later owner actions.

## Release checklist ownership

Implementation evidence must cover package installation/removal, OS/WebView/CPU
baseline, real offline game/analysis, every applicable capability matrix row,
resource/latency measurements, accessibility/visual review, process cleanup,
atomic-save recovery, license/source gate, and signed artifact verification.
Do not mark the release ready while any required target or capability is untested.


## Desktop OTA and GitHub Actions

The owner included desktop OTA updates built and released through GitHub Actions.
Use the [Tauri updater](https://v2.tauri.app/plugin/updater/) contract: update
signatures are mandatory, the app receives the public verification key, and
release signing uses a protected private key. These signatures are separate from
Windows executable signing and macOS code signing/notarization.

The [official action](https://github.com/tauri-apps/tauri-action) documents release
assets, updater signatures, and updater JSON generation. Exact action/plugin
pins, workflow permissions, per-target jobs, matching updater artifacts, signing-key
rotation/access controls, pin verification and pipeline gates follow
[updater decisions](../decisions.md#updater-completion-and-zero-cost-ci).
Store the OTA signing key and passphrase in GitHub Actions encrypted secrets,
with an offline backup on passphrase-encrypted removable storage kept disconnected
and controlled by the owner. Approve the exact backup procedure separately. This does not select Windows or
macOS code-signing credentials or create any secrets during planning. Windows
installers are unsigned and macOS personal builds are locally ad-hoc-signed,
without Developer ID/notarization. Mandatory OTA signatures remain in scope
without a paid signing service. Linux ships .deb and AppImage on x86-64 only;
validate AppImage OTA. For .deb updates, download and verify the package, then
open the system installer only after confirmed pause/save/backup; request OS
approval where required. Validate this separate integration rather than claiming
.deb native self-updating. Use only included standard GitHub hosted-runner
allowance; verify account limits and controls preventing paid overages before
enabling workflows. Stop builds when the allowance is exhausted. The selected
CI policy is not evidence that account billing controls have been verified.
Use NSIS .exe installers for Windows and Apple Silicon .dmg packages containing
the macOS app; matching updater artifacts require source-pinned validation. Stable releases only
are selected, with vMAJOR.MINOR.PATCH tags matching the application version.
Verify native updater support
for each selected installer format; do not assume every Linux package supports
self-updating or that minimum-platform behavior has been tested.

Keep update binaries private. Version-tag pushes trigger automatic build and
release publication. Check nonblockingly at startup and expose Check for Updates
in Settings. Authenticate with a user-provided fine-grained GitHub token
restricted to this repository with Contents read permission, stored in the OS
credential store. Automatically download available updates, then wait for explicit
Install and confirmation. Ask before downloading when the OS identifies the
connection as metered, and also when metered status is unknown or unavailable.
Cross-platform detection remains unvalidated. Pause and successfully save all live games before
installation; a save failure blocks proceeding. After pausing and saving games,
require a fresh successful safety backup of user data and settings at the configured
backup destination, excluding dataset files and credentials. Backup failure blocks
installation and offers Retry. Restore unfinished games paused.
Missing, expired, or rejected tokens produce a persistent update-status notice
in Settings with add/replace-token controls, without a blocking prompt; offline
chess remains usable. Stop failed downloads and offer visible Retry, without
automatic download retries. Relaunch automatically after successful installation
where supported; explain platforms requiring manual reopening. Games remain
paused. Keep the latest five successful pre-update safety backups, plus any
backup needed for unresolved recovery; this is separate from daily/manual
retention. If a data migration fails, stop normal startup, preserve the data,
and offer recovery from the pre-update backup with preview and explicit
confirmation. Recovery may require reinstalling a compatible app version; it
is not a guaranteed binary rollback. If installation fails, report the failure
and offer explicit Retry only where safe, with manual-recovery instructions
otherwise. Do not retry installation automatically. Migration compatibility,
recovery mechanics and remaining network-failure handling follow
[the selected recovery and updater decisions](../decisions.md).
Each update-check attempt has a 15-second total deadline across release
discovery and metadata, excluding the binary download. Stop downloads after
60 seconds without received bytes or 30 minutes total, whichever occurs first;
show manual Retry. These are owner-approved limits, not measured results. Keep all published private
releases and downloadable assets until the owner explicitly deletes them; no
automatic release cleanup. A private source
repository does not make updater release downloads anonymously accessible.
Private delivery needs a source-verified authentication/discovery/download design;
never solve it by embedding a repository credential in distributed app assets.

Offer a visible Cancel action before installation. Cancellation stops the
download, discards partial bytes, and waits for explicit Retry. Keep completed
update downloads across app relaunch and revalidate them before installation.
Every download attempt on a metered or unknown connection, including manual
Retry, requires approval. Do not provide Remove Download; retain a completed
download until installation or replacement. When a newer stable update appears,
keep the downloaded version and offer explicit Download Newer Version, subject
to the same approval rules. On detected connection change to metered or unknown,
stop the transfer, discard partial bytes, and ask before a fresh attempt. Platform
detection and detailed cache/replacement mechanics still require validation.

OTA network access is separate from offline chess workflows. Planning creates no
workflow, keys, GitHub secrets, release, or hosted service. Future checks must
exercise authentic and invalid signatures, platform/architecture matching,
authentication, offline checks, interrupted downloads/installation, and preservation
of user state according to the owner-approved installation/recovery policy.
