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
| Windows 11 x86-64 | MSVC native builder; NSIS installer, engine/helper `.exe` | Native GPU driver and CRT, offline play/save, unsigned OS warnings, signed OTA |
| macOS 14+ ARM64 | Apple Silicon runner; `.app` and DMG | Minimum supported ARM Mac, ad-hoc-sign nested engine/app, validate manual OS approval, offline first run |
| Ubuntu 22.04+ x86-64 | Oldest declared glibc baseline; `.deb` and AppImage | Native GPU driver, X11/Wayland, portals/dialogs, package/system dependencies |

Do not ship Intel macOS or a universal Mac package. Windows ARM64 is outside the
initial target set. Linux ARM64 is also outside the initial target set. Linux distribution coverage is limited to tested runtime
baselines; AppImage does not remove glibc, native GPU driver, window-system,
portal dependencies or secure-service needs for persistent credentials. Credential
storage is optional for offline chess; FR-026 permits explicit session-only
update credentials when secure storage is unavailable. Mac ARM64 uses native Metal through the selected engine/backend; no forced
Vulkan portability loader/library is required. The final application does not require WebView2 or WebKitGTK.

## Reproducible inputs

- Root Cargo.lock, exact Rust toolchain, Bevy 0.20.0, bevy_egui 0.43.1 and its
  matching egui/render graph, build-time cargo-packager 0.11.8 and installer tools.
- Selected native graphics dependency inventory and resource paths, with ad-hoc
  signing of any nested native libraries in the macOS package.
- Reckless v0.9.0 commit, source/submodules, compiler, target, features, flags and
  binary digest; verified NNUE EVALFILE input, without a build-time network fetch.
- Fathom/Clang/libclang for production Syzygy builds; no feature-removal shortcut.
- Original GLB/fonts/sounds, starter datasets, source bundles and license notices.
- Asset generation migrated from the obsolete Bun tool to reproducible native
  tooling, preserving asset hashes or explicitly validating any replacement.

Use framework-independent cargo-packager for NSIS, app/DMG, deb and prospective
AppImage assembly. Its AppImage backend fetches rolling tools without checked
hashes. A maintained stable-tagged AppImage toolchain satisfying project rules
has not yet been verified: this remains a release blocker, not permission to
silently fetch continuous/alpha tools or drop AppImage. Hashes alone do not make
a prerelease stable. Resolve this tooling gate before accepting that package.

Build the external engine in its isolated checkout/cache without vendor changes.
Override host-native compiler flags. Package manifests own executable paths and
hashes; presentation cannot select an arbitrary executable.

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

Bundle local original meshes, fonts, shaders, audio, engines and NNUE. The native
scene remains 3D-only; packaged GPU/resource failures preserve game state, pause
clocks and offer Retry, then require explicit Resume. Shader/assets and a hardware
graphics adapter must be validated on every minimum target. Test the actual native Metal backend on Apple Silicon. Do not claim backend
support from a successful build alone.

Windows installation no longer bundles WebView2. Validate the native CRT and
native GPU runtime/driver requirements on a clean supported machine, with offline
preparation guidance. Linux may install system prerequisites online; document
how to prepare them offline. Running chess after installation remains offline.
Resource resolution must work from installed app locations, including spaces
and non-ASCII paths, without depending on a developer checkout. Set Bevy
AssetPlugin file_path from the typed package-resource resolver; the default
executable-parent lookup does not find macOS Contents/Resources. Do not mutate
process environment to configure it. Release packages disable Bevy dynamic
linking, asset watchers/processors and development paths. Package native audio,
window-system and portal dependencies only for selected enabled features;
the approved PCM WAV sounds require a verified WAV decoder feature.
Disabling a feature cannot silently remove a product requirement.

Use native egui semantic/input tests plus the real production native GPU
smoke executable and screen-reader checks. Release artifacts exclude test-only
orchestration, listeners and development assets. Native dialogs, sleep events,
credentials, GPU drivers and package installation need actual platform tests.

### Dependency availability and failure scope

Derive the final per-package native library/runtime inventory and preparation
instructions from the actual locked build and clean-install validation. Naming
a platform baseline does not prove every library or service is present. Record
missing-component failures as part of that validation before claiming support.

| Dependency | Required behavior when unavailable or invalid |
| --- | --- |
| Supported OS/architecture, native window/graphics runtime and required package libraries | Actionable setup failure; preserve data and do not start or resume games. If the renderer cannot draw recovery UI, use OS-native recovery controls under the UI contract. |
| Packaged engine, embedded model or presentation assets | Refuse use of the invalid resource and explain repair; affected games remain preserved/paused. Never substitute an unverified binary, empty model or another board mode. |
| OS credential store | Follow FR-026's setup guidance and explicit session-only option. No plaintext fallback and no offline-chess dependency. |
| File dialog/desktop portal | Refuse the affected file-selection operation with setup guidance; preserve data and keep unrelated offline chess available. Do not infer that missing optional dialog services make the profile corrupt. |
| Audio device/service | Show sound-unavailable status and retain visual feedback and normal play; do not turn device failure into a silent preference change. Revalidate the selected sound behavior on supported systems. |
| Bundled opening/book/puzzle collection | Treat missing/corrupt data as an integrity failure: refuse affected queries or puzzle starts, identify the collection and offer repair guidance. Preserve user library/history and unaffected offline play; do not show fabricated zero counts or misleading partial statistics. |
| Personal import or referenced external dataset | Explain missing/invalid source; stage no partial acceptance without the selected preview/confirmation and retain existing accepted data. |
| Optional Syzygy files or Linux NUMA runtime/topology | Show the specific unavailable facility and retain compatible ordinary engine play under the capability contract; acquire no table files automatically. |

All shipped starter collections remain required package contents. Feature-local
failure handling preserves working user data; it does not excuse an incomplete
release or permit dropping bundled datasets, audio or another approved feature.

### Installed profile identity

All native packages resolve the existing `nl.sadra.chess` user data explicitly;
do not silently create a different directory through framework naming defaults.
The [profile ownership protocol](../decisions.md#local-profile-ownership-and-activation)
is applied before migrations, restore-journal recovery or startup update work.
Its stable control directory is not replaced by user-data restore and is not a
backup payload. Clean-install/upgrade tests cover installed paths with aliases,
spaces and Unicode, simultaneous launches and supported local filesystem locks.

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

Implementation evidence must cover package installation/removal, OS/GPU-driver/CPU
baseline, real offline game/analysis, every applicable capability matrix row,
resource/latency measurements, accessibility/visual review, process cleanup,
atomic-save recovery, license/source gate, and signed artifact verification.
Do not mark the release ready while any required target or capability is untested.


## Desktop OTA and GitHub Actions

The owner included desktop OTA updates built and released through GitHub Actions.
Use an application-owned native updater state machine with framework-independent
installer artifacts. cargo-packager 0.11.8 builds packages; reqwest 0.13.5 owns
async HTTPS streaming; keyring 4.2.0 supplies OS credential storage; minisign-verify
0.3.0 verifies signed update manifests. Future release tooling uses minisign
0.10.0. These signatures are separate from OS code signing. Do not retain Tauri
or assume cargo-packager-updater meets the selected safety/lifecycle policy.

### Signed manifest and private transport

A release contains a bounded versioned JSON manifest, its detached Minisign
signature and target-specific packages. Sign the exact UTF-8 bytes. Verify them
before parsing with the embedded trusted public key. The validated manifest
binds schema, stable version, allowed channel, target triple, artifact kind,
GitHub asset identity/name, exact length, SHA-256 and compatible data schema.
Use a 1 MiB manifest and 16 KiB signature-file bound. Reject duplicate/unknown
critical fields, unsupported schema/target, version downgrade, unexpected assets
and invalid signatures before downloading/executing. Length/digest bind payloads
to the signed manifest; check both again immediately before handoff. No unsigned
GitHub metadata can authorize installer bytes. Key rotation requires an update
trusted by an existing key; loss of all signing keys requires manual recovery.

Resolve this repository's release/asset IDs through authenticated GitHub API
calls. Use HTTPS-only, explicit manual redirect validation and no automatic
transport retry: reqwest ClientBuilder::retry(reqwest::retry::never()) disables
its default protocol-NACK retries. Stream Response::chunk into bounded private
staging. Never forward Authorization to a different origin. Approved asset
redirects follow without the GitHub credential, never arbitrary manifest URLs.
Preserve all owner-approved deadlines, cancellation and approval rules below.

The client safety cap remains 6 GiB, but [GitHub's release limit](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases#storage-and-bandwidth-quotas)
requires each actual asset under 2 GiB. Measure compressed installer sizes.
If a full package exceeds that limit, publication remains blocked until the
package/data layout is revised and validated within the approved offline scope.
Do not silently omit starter features or rely on paid hosting.

### Native installation adapters

An owned helper and typed handoff receipt bind verified package bytes, destination,
version, backup identity and parent process. Revalidate data at handoff, prevent
path traversal/symlink substitution, wait for app exit and check package outcome.
Never execute a shell command assembled from user or manifest text.

| Package | Installation/update path |
| --- | --- |
| Windows NSIS | Launch verified installer after save/backup/exit; respect OS approval, wait for outcome and relaunch only on confirmed success where supported |
| macOS ARM64 | DMG for initial install; signed-manifest `.app.tar.gz` payload for updates. Helper stages a validated app beside destination, ad-hoc-signs/validates required nested resources during packaging, waits for exit and replaces with recovery-aware rename; protected destinations may require user action |
| Linux AppImage | Validate outer APPIMAGE path, not current_exe inside its mounted image; stage replacement on same filesystem, preserve executable permissions and use recovery-aware replacement after exit |
| Linux .deb | Verify package, then open the supported system installer after confirmed pause/save/backup; OS handles administrator approval. Do not claim generic in-place self-update or guaranteed automatic relaunch |

Mac packaging uses an app-then-DMG step with explicit nested ad-hoc signing
without timestamp/notarization and a validated final DMG. Restore prior files
only if safe before successful commit; installation failure does not imply
universal binary rollback. Persist a handoff receipt for next-start diagnostics.
Helper termination, full disk, permission denial, interruption and package-manager
failure require explicit tested recovery. Temporary ownership/process helpers
are integration work, not functionality supplied by egui.

### Profile ownership during installation

The application retains its profile lease while resolving confirmations,
pause/save, fresh safety backup and package revalidation. While still holding
it, fsync a durable Prepared install intent binding the destination profile, expected
owner/helper generation, target package digest/version, backup and schema.
The helper acknowledges readiness over an owned private channel; arbitrary
activation requests cannot create or complete this intent. After readiness and
all prerequisites, fsync the same intent as Committed before final shutdown.
Only a matching Committed intent authorizes helper installation; Prepared is
never sufficient, including if the parent exits unexpectedly. Before commit,
cancellation/failure confirms helper shutdown and revokes Prepared under the
held guard. Lost readiness acknowledgment or unconfirmed helper shutdown keeps
recovery status and the app/lease available, without installation or silent
intent clearance. No late helper may act on a revoked generation.

After orderly app shutdown, the helper observes actual parent-process exit,
acquires the same exclusive profile lease and revalidates the Committed intent and bytes
before replacing files or starting a system installer. Every ordinary launch
that acquires the guard during this gap reads the intent before database open,
shows installation/busy guidance, releases the guard and exits. It cannot run
migration, restore, backups, updates or games. No timeout clears the intent or
proves parent/helper exit. The helper does not inherit the parent's lock handle.

Keep the lease while an owned installer is running; record its confirmed outcome
durably before release and relaunch. A .deb adapter without observable system-
installer completion retains unresolved status, never guesses success from the
launcher exiting. A helper crash or interrupted replacement leaves recovery
state for the next lock holder, with explicit safe Retry/manual guidance and
no normal startup or automatic install replay. Before Retry or intent clearance,
confirm the quiescence/outcome of every launched helper and external installer;
release of the helper's file lock does not prove its installer has exited. If
completion cannot be established, retain the barrier and give manual recovery
guidance. Ordinary status launches release the lease before displaying status;
an explicit recovery controller reacquires it and confirms this quiescence
before changing the intent or launching any retry. A confirmed-success receipt lets
the next owner validate installed identity and run supported migration; clear
the intent only after successful startup validation. Failed migration remains
recovery-only and preserves the pre-update backup/data.

Actual parent-exit observation, helper crash recovery, native installer outcome
and focus behavior are target-platform acceptance gates. This is a coordinated
protocol, not a promise that egui or a package manager supplies these safeguards.

Store signing key/passphrase in GitHub Actions encrypted secrets with the
owner-controlled encrypted removable offline backup. No credential is created
here. Windows remains unsigned; macOS remains ad-hoc without paid membership.
Version tags vMAJOR.MINOR.PATCH must match Cargo/package/manifest versions.
Only after verified zero-cost controls may native Actions jobs build/test/package,
sign manifests and publish one complete private stable release; use immutable
verified action revisions and retain releases/assets until owner deletion.

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
