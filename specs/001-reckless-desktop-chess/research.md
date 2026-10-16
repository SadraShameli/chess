# Research: Reckless Desktop Chess

**Date**: 2026-10-16

**Status**: Source research plus selected design decisions under owner delegation.
[Final resolution research](#final-resolution-research) and
[resolved decisions](decisions.md) supersede earlier draft alternatives and
separate runtime/release evidence from planning selections.

**Scope**: Phase 0 evidence for [the plan](plan.md). Primary source inspection,
release/API metadata, documentation, and a read-only macOS binary-header check.
No builds, engine execution, application dependency installations, or performance
measurements. Stripe CLI help automatically installed its Directory plugin for
provider discovery; the subsequent dataset search returned no results.

## 1. Desktop framework and presentation

**Confirmed owner stack**: Tauri 2, Vue, Tailwind CSS, shadcn, Bun, ESLint,
and Prettier. Use the Vue-compatible shadcn-vue project rather than React-only
shadcn/ui. Exact selected versions and supporting tooling are recorded in
final resolution research.

**Proposed integration**: Vue 3 SFC presentation, strict TypeScript 6.0.3, Vite 8,
Tailwind 4 through its official Vite plugin, shadcn-vue/Reka UI components,
Bun as package manager and script runtime, ESLint flat configuration with Vue
type-aware rules, and Prettier with Tailwind class ordering.

**Rationale**: This is the owner's selected stack. Rust retains the game,
clock, process, and storage authorities. Vue holds presentation state and typed
interaction controllers, not an independent chess rules engine.

| Alternative evaluated | Tradeoff |
| --- | --- |
| Slint 1.18.1 | Strong native/Rust alternative, different UI language and licensing choice |
| Iced 0.14.0 | Rust-only messages/tasks, experimental status and accessibility concerns |
| egui/eframe 0.37.0 | Strong engine-workbench fit, more bespoke consumer visual work |
| Lit 3.3.3 | Class-based Web Components, superseded by the owner's Vue selection |

No comparative performance superiority is established. Actual system WebViews,
engine saturation, and the polished board must be measured together.

**Verified compatibility facts**:

- Stable Tauri v2 is 2.12.4. GitHub's latest-release flag currently points at a
  v3 alpha; never use that flag alone to select the engine shell version.
- Current Tauri JS API is 2.12.4 and CLI is 2.12.3. Compatible patches need not
  have identical numbers.
- Current registry Vue is 3.5.44; Tailwind and its Vite plugin are 4.3.3;
  shadcn-vue is 2.8.2; ESLint is 10.12.0; Prettier is 3.9.10.
- TypeScript 7 is currently advertised by latest metadata, but current
  typescript-eslint 8.71.2 peers require TypeScript below 6.1. TypeScript 6.0.3 is
  the mutually compatible stable proposal. Do not hide unsupported-version checks.
- Tailwind 4 requires modern CSS support: official minimums include Safari 16.4,
  Chrome 111, and Firefox 128. macOS 14's Safari generation fits. Ubuntu 22.04
  requires an updated WebKitGTK runtime and actual CSS/visual tests.
- A frozen Bun install is not enough if the lockfile is absent: require committed
  text bun.lock and fail before install when missing. Native optional packages
  still need installation on each native target.
- Bun should execute tools intentionally with its documented runtime mode;
  package installation alone does not prove every native-test worker runs on Bun.
- The Tauri WebdriverIO service peers with WDIO 9, while latest WDIO is 10.
  Use a mutually compatible stable family, proposed 9.31.9, not latest blindly.

Sources:

- [Tauri v2.12.4](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.4)
- [Tauri WebViews](https://v2.tauri.app/reference/webview-versions/)
- [Vue TypeScript](https://vuejs.org/guide/typescript/overview)
- [Tailwind Vite integration](https://tailwindcss.com/docs/installation/using-vite)
- [Tailwind compatibility](https://tailwindcss.com/docs/compatibility)
- [shadcn-vue installation](https://www.shadcn-vue.com/docs/installation/vite.html)
- [Vue ESLint TypeScript configuration](https://github.com/vuejs/eslint-config-typescript/blob/v14.9.0/README.md)
- [Bun frozen installs](https://github.com/oven-sh/bun/blob/bun-v1.4.3/docs/pm/cli/install.mdx)
- [Prettier Tailwind plugin](https://github.com/tailwindlabs/prettier-plugin-tailwindcss)
- [Slint](https://github.com/slint-ui/slint/tree/v1.18.1)
- [Iced](https://github.com/iced-rs/iced/tree/0.14.0)
- [egui](https://github.com/emilk/egui/tree/0.37.0)

## 2. Engine selection and complete coverage

**Decision**: Pin Reckless v0.9.0, released 2026-03-01, commit
`0e92358f5acd66e5ac77b1bf558202e47c515435`. Integrate through one owned UCI
subprocess, retaining the exact tagged upstream source separately from app code.

**Rationale**: Its Cargo package is binary-only, non-published, and does not expose
a supported Rust library API. An in-process fork would couple the application to
unsafe internal representations, vendor changes, and search implementation details.

**Alternatives considered**: Embedding private modules, swapping in another
engine, or exposing only `bestmove` would fail compatibility or owner scope.
The [capability matrix](contracts/reckless-capabilities.md) is the sole detailed
coverage inventory; it includes all options, commands, statistics, build features,
unsupported popular options, and suspected broken SPSA scaffolding.

Source inspection overrides inaccurate summaries:

- Threads' advertised upper bound is dynamic, not the README's fixed 512.
- The official macOS asset is thin ARM64, despite release prose calling it universal.
- NUMA is optional Linux/libnuma build support, not enabled in official packages.
- `readyok` during search does not acknowledge completion; most commands are
  discarded until the search is stopped.
- Syzygy path quoting cannot repair its whitespace-splitting parser.
- Terminal positions and malformed inputs must be intercepted before search.
- Static `eval` and search centipawns have different normalization/perspectives.

Sources:

- [Stable release and assets](https://github.com/codedeliveryservice/Reckless/releases/tag/v0.9.0)
- [Cargo metadata](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/Cargo.toml)
- [UCI implementation](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/uci.rs)
- [Search initialization](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/search.rs)
- [FEN parser](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/board/parser.rs)
- [Reporting](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/thread.rs)
- [Score normalization](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/types/score.rs)
- [Time control](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/time.rs)
- [NUMA implementation](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/numa.rs)

## 3. Chess rules, notation, and persistence

**Decision**: Wrap shakmaty 0.30.2 in owning BoardPosition/GameSession types; use
pgn-reader 0.29.1 for bounded streaming PGN import, and an owned exporter for the
supported move-tree/annotation contract. The owner selected SQLite for indexed
data, with PGN/FEN interchange. Select bundled rusqlite and the storage actor/
schema ownership in resolved decisions.

**Rationale**: Legality, FEN/SAN/UCI conversions, castling, and standard/Chess960
support are concrete library facilities. pgn-reader moved into the shakmaty
repository; its old separate repository is not the current release source.
Its 0.29.1 manifest depends on shakmaty 0.30. Both declare Rust 1.97 minimum,
compatible with the project's 1.99 pin.

**Alternatives considered**: The engine's FEN parser is not a rules library.
Implementing every move rule from scratch adds correctness risk. Browser chess
state as another authoritative implementation creates disagreement. Indexed file
storage was considered; the owner selected SQLite for the library and opening
statistics. Transaction, indexing and migration policies follow resolved decisions.

The approved [study tools](contracts/study-tools.md) also require explicit data
source, exact CSV schema, scale, report interpretation, and detailed puzzle
policies. PGN opening games/studies, Polyglot .bin books, and puzzle CSV imports
are approved. Puzzles include filters, progressive source/destination/next-move
hints with assistance tracking, review, saved progress, and timed sessions.
Record failed attempts and permit practice retries on the continuing countdown.
Accept dataset-listed solution lines; label other legal moves unlisted without
claiming they lose or verifying alternatives with engine searches. Reports start automatically after games played in the app complete, covering
the played main line with a configurable 3-second-per-position default and
depth/node alternatives. Imported completed games require explicit report
requests. Solved puzzles show solution review and wait for Next without charging
session time during review; active hints keep the countdown running.
Variations are analyzed on demand; reports pause during
live games and resume afterward. The owner approved move labels and an
app-calculated accuracy percentage, with formulas/thresholds still open. Displayed
search evaluations are White-positive. Timed puzzles offer whole-session and
per-puzzle countdowns, defaulting to configurable 5-minute sessions and 60-second
puzzles, with one countdown mode per timed run. Manual pause and app focus loss
pause puzzles and hide board/solution until resumed. Session timeout ends the run;
per-puzzle timeout records the result and advances. Shuffle filtered puzzles
without repeats until exhausted. Save local per-puzzle attempt history, outcomes,
assistance, elapsed time, and session summaries. Preserve paused runs and remaining
time across sleep/relaunch; always require explicit Resume. The Settings tab includes
common options and advanced engine/resources. Appearance applies immediately;
other settings require Save, with deferred safe engine application where needed.
The approved inventory and application policy are tracked
in [the UI contract](contracts/ui-experience.md#settings-tab). Starter
data plus personal imports are approved within a 2 GB installed starter-data
budget excluding app/engines. The opening book is study-only and never supplies
opponent moves.
No external corpus/book/puzzle provider has been chosen or acquired.

Confirmed live-game behavior keeps clocks running on app focus loss and pauses
timed play on computer sleep pending explicit Resume. Both modes allow takebacks:
previous human turn against Reckless, one move locally. Restore clocks before
undone moves, reversing increments. Local confirmation is configurable per game
and required by default. Clocks continue until local consent is accepted; then
restore snapshots. No request expiry; reuse matching variations in stable order,
with terminal-result cancellation under the selected race policy.
Manual Pause in both game modes freezes clocks/search pending explicit Resume.
Engine faults preserve the game and pause clocks; restart only on explicit Retry
and require Resume after recovery. This resolves the stop-timeout restart conflict.
The adjustable initial new-game preset is 10 minutes plus 5 seconds per move.
Untimed engine play defaults to a configurable 3 seconds per move. Retain removed
takeback moves as alternative variations. Approved clock families include untimed,
sudden death, Fischer increment, staged/cyclic, simple delay, and Bronstein delay.
Reckless has no delay token; the selected native delay/refund accounting and
single-movetime search allocation are in resolved clock decisions and require
controlled-time and pinned-engine tests.
History browsing keeps game/clocks/search running and offers
Return to Live.

The library does not replace application responsibilities: repetition history,
claimable versus automatic draws, authoritative clocks, suspend policy, complete
dead-position/flag-fall rules, and game-tree persistence must be implemented and
verified explicitly. Include difficult dead-position and possible-mate fixtures;
do not equate `is_insufficient_material` alone with full adjudication.

Sources:

- [shakmaty 0.30.2](https://github.com/niklasf/shakmaty/tree/shakmaty-0.30.2)
- [shakmaty capabilities and license](https://github.com/niklasf/shakmaty/blob/shakmaty-0.30.2/README.md)
- [Matching pgn-reader manifest](https://github.com/niklasf/shakmaty/blob/shakmaty-0.30.2/pgn-reader/Cargo.toml)
- [pgn-reader 0.29.1 documentation](https://docs.rs/pgn-reader/0.29.1/pgn_reader/)

## 4. Process lifecycle, transport, and capacity

**Decision**: A Rust engine actor owns stdin/stdout/stderr and the process handle.
Use Tauri's shared async runtime and async child pipes; isolate CPU search in the
sidecar. A CapacityCoordinator partitions heavy work between the primary engine
and workbench. Use ordered typed Tauri channels for streams, with bounded producer
queues and coalesced replaceable analysis snapshots.

**Rationale**: UCI supplies no correlation IDs, and Reckless ignores configuration
commands while searching. Only serialized transitions can attach output to the
right application search. General Tauri events are not the high-throughput
analysis transport.

One engine gets all detected usable capacity by default, constrained by its
advertised maximum. When another heavy task is admitted, stop and rebudget the
engine before launching it, rather than independently creating a full-size pool.
Brief UI/IPC work is not another unbounded CPU executor. Measure its contention
with full search and adapt only from evidence. Do not introduce Rayon merely
because parallel execution is desirable.

**Alternatives considered**: A thread per line, unbounded event queues, a second
async runtime, arbitrary small thread caps, or direct JS shell access conflict
with correctness, safety, or the feature's resource requirements.

Sources:

- [Tauri channels](https://v2.tauri.app/develop/calling-frontend/#channels)
- [Tauri sidecars](https://v2.tauri.app/develop/sidecar/)
- [Engine protocol sequencing](contracts/engine-protocol.md)
- [Concrete capacity model](data-model.md)

## 5. Visual design and accessibility

**Confirmed direction**: Dark-first premium styling, richer effects and animation,
and a full light theme. The owner selected the Chess.com-inspired layout with
collapsible left navigation, a large board/player bars/clocks, and resizable
right-side tabs. Configurable board aids, a command palette, keyboard shortcuts,
and optional move/check/result sounds are approved. Strict TypeScript and Vite
are also owner approved.

**Confirmed appearance defaults**: Graphite surfaces, emerald accents, green/cream
board, and warm light theme. Legal-move dots appear on selection; last-move/check
highlights and captured pieces are initially enabled. Sounds default to enabled
during active play and silent in background. The owner chose sculpted 3D-style
pieces, bundled modern sans-serif UI/notation with tabular clock digits, and
configurable 180 ms piece moves/160 ms panel transitions with subtle elevation,
respecting reduced motion. Real-time 3D board/pieces with camera rotation/zoom,
ivory/ebony satin materials, and a 1280x800 CSS-pixel minimum window are confirmed.
The default camera is centered at 20 degrees from top-down with all squares
visible. Maximum manual zoom-in keeps the full board visible. Reset View restores
default pose/framing/tilt while retaining White/Black orientation. Normal piece dragging,
right-drag drawing, and visible camera rotation/zoom controls are confirmed.
The board is 3D-only, without a selectable or fallback 2D board. Remember the
last camera view across launches with Reset View. The evaluation bar appears
only in analysis/review. Clickable report findings navigate to their position,
show alternatives, and offer an analysis branch. Move-list/explorer/book tabs
share the selected board position. Quality defaults to Automatic
with manual Low/Medium/High overrides. Graphics failure preserves the game,
pauses clocks, and offers Retry; explicit resume is required after recovery.

**Resolved presentation details**: Exact tokens, Three.js 0.186.1, original GLB
meshes/synthesized WAV, Inter 4.1/SVG figurines, panel sizes, camera framing,
quality/adaptation, keyboard/audio and context recovery follow
[presentation decisions](decisions.md#presentation-assets-and-input). Animation
durations and selected hardware baselines remain targets rather than measured
performance. Accessibility and native WebView validation remain release work.

**Rationale**: Appearance must remain coherent during play, analysis, errors,
resizing, promotion, and keyboard navigation. Semantic squares enable keyboard
movement and assistive technology alongside the selected real-time 3D scene.
The owner chose real-time 3D with rotation/zoom over pre-rendered and fixed-camera
options. Direct Three.js inside Vue with a dedicated scene controller and
WebGL2 is owner selected. Pins are selected in final resolution research; scene
rendering and camera transforms cannot remove accessible board controls.

Vue views use shared Tailwind/shadcn-vue theme tokens. Typed interaction
controllers receive application ports from bootstrap; view setup does not create
concrete engine or storage clients. Reka UI primitives support control behavior,
but real WebView keyboard and assistive-technology checks remain necessary.

**Alternatives considered**: A canvas-only board, drag-only interaction, or a
generic dashboard template would need additional accessibility work or weaken
the desired visual identity. See [the UI contract](contracts/ui-experience.md).

Sources:

- [Vue testing](https://vuejs.org/guide/scaling-up/testing.html)
- [Reka UI accessibility](https://reka-ui.com/docs/overview/accessibility)
- [WCAG 2.2](https://www.w3.org/TR/WCAG22/)

## 6. Cross-platform packaging and testing

**Subsequent owner decisions**: Windows NSIS .exe installer and macOS Apple
Silicon .dmg containing the app. Under the no-payment constraint, use unsigned
personal-use Windows installers and locally ad-hoc-signed macOS builds without
Developer ID/notarization. Document Windows warnings and validate manual macOS
approval; OTA signatures remain mandatory. No paid services or setup are
authorized. Linux is x86-64 only, with .deb and AppImage selected. Bundle the offline Evergreen WebView2 installer; Linux dependency
installation may use internet access, with offline-preparation guidance.
Linux .deb updates download and verify the package, then open the system
installer after confirmed pause/save/backup, with OS approval where required.
Validate this integration separately from AppImage OTA. Hosted CI uses included
standard-runner allowance only, with account limits and controls preventing paid
overages verified before workflows are enabled; stop on exhausted allowance.
No account billing controls have been verified during planning. Keep the offline
OTA signing-key backup on passphrase-encrypted removable storage kept disconnected;
its exact procedure requires separate approval. Matching OTA
artifacts and native install behavior still require validation.

**Decision**: Native builders for Windows x86-64, macOS ARM64, and Linux
x86-64 only; explicit CPU baselines and a matching engine package manifest.
Use test-only embedded WebDriver automation for native integration.

**Rationale**: Tauri's system WebView, native graphics, sidecar loading, signing,
and Linux glibc cannot be proven by cross-compilation alone. Current Tauri docs
support embedded WebDriver on macOS as well as Windows/Linux; direct
`tauri-driver` alone still has narrower platform coverage.

**Alternatives considered**: Intel/universal macOS violates owner scope;
shipping host-native compiler flags is unsafe for distribution; relying on an
AppImage does not establish all-Linux compatibility. Linux ARM64 was removed
from first-release scope; no Linux ARM64 build is required.

Sources:

- [Tauri WebDriver guidance](https://v2.tauri.app/develop/tests/webdriver/)
- [WebdriverIO Tauri service](https://webdriver.io/docs/desktop-testing/tauri/)
- [Tauri distribution](https://v2.tauri.app/distribute/)
- [AppImage constraints](https://v2.tauri.app/distribute/appimage/)
- [macOS signing](https://v2.tauri.app/distribute/sign/macos/)
- [Reckless release workflow](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/.github/workflows/release.yml)
- [Compiler SIMD stability](https://doc.rust-lang.org/stable/core/arch/x86/fn._mm512_add_epi16.html)

## 7. Network asset and redistribution

**Confirmed owner use**: Private personal use. The application license is
not changed: private owner rights and existing upstream notices are retained,
with no application license file or distribution to other people planned.

**Proposed build approach**: Build against the exact embedded network and record
source/build provenance. Local private implementation can proceed; licensing
checks for external distribution apply if the owner later requests it.

**Rationale**: Reckless v0.9.0 uses AGPLv3; Fathom uses MIT. shakmaty and pgn-reader
are GPL-3.0-or-later. A UCI subprocess is a technical boundary and does not itself
establish a licensing exemption. The separate network repository lacks an
explicit LICENSE, so its redistribution terms cannot be invented.

Exact network: `v54-5478683c.nnue`, 63,266,880 bytes, SHA-256
`5478683cb1bababde29ae8f29468a99846726548fc6a0ed54cac40ab6d38efbf`.
The build script downloads it unless provided; acquire and verify it as a build
input using `EVALFILE`, then bundle the compiled engine. No runtime model
download or EvalFile control is required.

**Alternatives considered**: Linking only the moving upstream branch, asserting
the model is permissively licensed, or claiming that private development requires
immediate public publication is unsupported. No license file is changed here.

This is an explicit release gate with an evidence owner, not an unresolved GUI
or integration decision. Resolve it by obtaining authoritative model terms and
documenting compatible application licensing and corresponding-source delivery
before any external distribution occurs. Do not contact maintainers in this task.

Sources:

- [Reckless license](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/LICENSE)
- [Fathom license](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/deps/Fathom/LICENSE)
- [Build script](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/build/build.rs)
- [Network embedding](https://github.com/codedeliveryservice/Reckless/blob/v0.9.0/src/nnue.rs)
- [Network repository](https://github.com/codedeliveryservice/RecklessNetworks)
- [Network release](https://github.com/codedeliveryservice/RecklessNetworks/releases/tag/networks)

## Evidence limits and next-phase checks

The resolved choices are recorded in [the decision review](review.md). Remaining verification belongs to
implementation/release acceptance, not an assumed successful result:

| Evidence | Current status | Required disposition |
| --- | --- | --- |
| Engine source/options/commands | Tagged source inspected | Protocol fixtures and real-engine tests for every matrix entry |
| Official macOS architecture | ARM64 header inspected without execution | Rebuild portable ARM64 and execute on minimum Apple Silicon baseline |
| SPSA feature | Static missing-symbol finding | Controlled optional-feature build; classify unavailable unless upstream supplies a stable repair |
| Linux x86-64 NUMA profile | Source configuration inspected | Native compile, compatibility, libnuma, and hardware topology tests |
| UI quality and responsiveness | Design only | Recorded target-system measurements and visual/accessibility review |
| Model distribution rights | No explicit model LICENSE found | Authoritative terms before external distribution |


## Real-time 3D renderer options, reviewed 2026-10-16

**Status**: Owner selected direct Three.js inside Vue with a dedicated scene
controller and WebGL2. Automatic quality adapts to sustained frame-time changes
with safeguards against rapid switching. The owner selected a 60 fps active
target, on-demand idle scene rendering with separate clock updates, and reducing
shadows/effects before internal resolution. Exact dependency pins are unapproved;
Tauri compatibility/performance remains unmeasured. No renderer is installed.

- [Three.js r186](https://github.com/mrdoob/three.js/releases/tag/r186) has a
  [WebGL2 renderer](https://github.com/mrdoob/three.js/blob/r186/src/renderers/WebGLRenderer.js)
  and a [WebGPU renderer with WebGL2 fallback](https://github.com/mrdoob/three.js/blob/r186/src/renderers/webgpu/WebGPURenderer.js).
  Both render the approved 3D board; backend fallback does not introduce a 2D view.
- [TresJS](https://github.com/Tresjs/tres) describes declarative Vue components
  using a Vue custom renderer over Three.js. The release list includes stable
  [core 5.9.3](https://github.com/Tresjs/tres/releases/tag/%40tresjs/core%405.9.3).
  This offers Vue scene composition, adding another lifecycle/integration layer.
  Exact Three.js peer compatibility must be checked before choosing pins.
- [Babylon.js 9.30.0](https://github.com/BabylonJS/Babylon.js/releases/tag/9.30.0)
  is another rendering-engine option with a
  [WebGPU engine entry point](https://github.com/BabylonJS/Babylon.js/blob/9.30.0/packages/dev/core/src/Engines/webgpuEngine.ts).
  Choosing it requires its own scene integration and dependency assessment.

**Owner selection and rationale**: Direct Three.js gives the scene owner
explicit renderer/resource lifecycle control without a Vue scene wrapper. This
is an architectural preference, not evidence that it is faster than alternatives.
The owner selected WebGL2 after asking about native Vulkan/OpenGL/ImGui.
WebGL2 is the WebView-facing OpenGL ES-based API; native Vulkan rendering would
require a different native-surface integration, and Dear ImGui is a separate UI
toolkit rather than a Three.js graphics backend. WebGL2 keeps fewer backend
paths across system WebViews;
WebGPU-preferred with WebGL2 fallback is an alternative requiring both paths to
be tested. Neither API support nor successful source inspection establishes
actual availability on the minimum supported OS/GPU configurations.

Exact dependency pins, licenses, asset pipeline, GPU baseline, device-loss
handling, shader/material compatibility, worker boundaries, quality thresholds,
are selected in presentation decisions; actual frame/input/memory results
remain implementation validation work.


## Bundled font candidates, license review 2026-10-16

**Status**: Owner selected Inter, English-only interface, and letter/figurine
move display with letters initially selected. PGN retains standard letters.
No font files are installed or pinned.

- [Inter](https://github.com/rsms/inter) has a verified
  [SIL Open Font License 1.1](https://github.com/rsms/inter/blob/master/LICENSE.txt).
- [Geist Sans](https://github.com/vercel/geist-font) has a verified
  [SIL Open Font License 1.1](https://github.com/vercel/geist-font/blob/main/OFL.txt).

Inter is selected as the UI/notation family; Geist Sans remains an evaluated
alternative.
Exact release files, license notices, glyph coverage for approved languages,
tabular digit behavior, font loading, and notation-symbol coverage need validation
before bundling. License inspection is not a rendering or performance test.


## Desktop updater research, 2026-10-16

**Owner decision**: Include signed desktop OTA through GitHub Actions with
private binaries, automatic build/publication on version-tag pushes, and
nonblocking startup plus manual Settings checks. Use a repository-scoped
fine-grained Contents-read token in the OS credential store. Automatically
download updates; install only after explicit Install, confirmation, and successful
pause/save of all live games; restore them paused. Credential failures show a
persistent nonblocking Settings notice with token controls. Failed downloads stop
and offer Retry without automatic retries. Relaunch automatically after successful
installation where supported; explain any manual-reopening requirement. This is
an owner-selected policy, not verified cross-platform lifecycle behavior. Stable
releases only use vMAJOR.MINOR.PATCH tags matching the application version.
Before installation require a fresh successful safety backup of user data/settings
at the configured destination, excluding datasets and credentials; failure blocks
installation with Retry. Retain the latest five successful pre-update backups plus
any needed for unresolved recovery, separately from daily/manual retention.
Failed migrations stop startup, preserve data, and offer previewed, confirmed
pre-update backup recovery, potentially requiring compatible-app reinstallation.
Installation failures offer explicit Retry only where safe and manual-recovery
guidance otherwise, without automatic retries. These policies do not establish
tested migration or binary-rollback support. Keep published private releases and
assets until explicit deletion. GitHub Actions encrypted secrets hold the OTA
signing key and passphrase, with an owner-controlled offline backup. Ask before
downloads on OS-detected metered connections; cross-platform detection and
deadline enforcement remain unvalidated. Ask also when connection status is
unknown or unavailable. Approved deadlines are 15 seconds total for each check
attempt across discovery/metadata, excluding binary downloads; downloads stop
after 60 seconds without received bytes or 30 minutes total, whichever occurs
first, with manual Retry. These are selected policy values, not measurements. Cancel stops downloads,
discards partial bytes, and waits for explicit Retry. Completed downloads survive
relaunch and require revalidation before installation. Every metered/unknown
attempt, including Retry, requires approval. There is no Remove Download action;
retain completed downloads until installation/replacement. Keep a downloaded
version when a newer stable release appears and offer explicit Download Newer
Version under the same approval rules. On detected change to metered/unknown,
stop, discard partial bytes, and ask before a fresh attempt. Platform detection
and detailed cache/replacement mechanics remain unvalidated.
Remaining delivery policies
are selected in [resolved decisions](decisions.md).

**Verified upstream facts**: [Tauri updater documentation](https://v2.tauri.app/plugin/updater/)
requires cryptographic update signatures and documents public-key verification,
private signing keys, HTTPS endpoints, static manifests, and dynamic update
servers. [The official action README](https://github.com/tauri-apps/tauri-action)
documents release publication, updater JSON, and signature artifact upload.
Updater signatures do not replace platform code signing/notarization.

**Implementation evidence needed**: Private GitHub release authentication/discovery/download must be
validated end to end against the selected stable 2.14.0 pin. Action commit
pins, per-target artifact/relaunch support, key access controls, network-cost
detection and recovery implementation require the selected verification gates;
no unanswered owner choice blocks task generation. No updates are built, signed, or published by this review.


### Private GitHub update delivery investigation

[GitHub release asset documentation](https://docs.github.com/en/rest/releases/assets#get-a-release-asset)
documents authenticated asset retrieval through the API, including fine-grained
Contents-read tokens, an octet-stream Accept header, and direct or redirected
responses. [Latest-release discovery](https://docs.github.com/en/rest/releases/releases#get-the-latest-release)
is a separate API operation from fetching a release's manifest/installer assets.
The stock action's browser-download links are not proof of private access.

[Inspected native updater source on the v2 branch](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs)
accepts native request headers for checks/downloads, exposes the resolved update
URL/headers, and verifies installer bytes before installation. This inspection
is a feasibility finding, not a verified stable plugin pin or an end-to-end test.

**Owner-approved design requiring source-pinned validation**: A user-provided
fine-grained token limited to this repository's Contents-read permission, stored
in the platform credential store, with native GitHub release/asset discovery.
Resolve authenticated manifest and platform assets through the GitHub API rather
than relying on anonymous private-release browser URLs. Retain update-signature
verification and validate redirects, target/version/asset identity, manifest URL
mapping, expiry/revocation, unavailable credentials, and rate limits. A browser
OAuth sign-in was considered but not selected; it would require GitHub app
registration and separate scope/device-flow assessment. No token is requested or stored during planning.

## Selected starter-data direction

Follow [starter provenance/selection](decisions.md#starter-datasets-and-provenance),
[puzzle interchange](decisions.md#puzzle-interchange-and-lifecycle), and
[book generation](decisions.md#book-generation-and-explorer-semantics). Standard
starters, two selectable CC0 online collections including a titled-master subset,
broad puzzle difficulty, manual updates and the 2 GB installed cap are selected.
Exact file checksums and measured collection sizes are preparation outputs,
not unanswered preferences or already acquired data.

## Final resolution research

The owner delegated remaining decisions on 2026-10-16. Selected policies are in
[the resolution document](decisions.md); facts below were checked during this
planning pass. No application dependencies or datasets were installed.

### Dataset licensing and schema

[The official Lichess database](https://database.lichess.org/) gives CC0 terms for
standard online games and puzzles. Its **broadcast section overrides that with
CC BY-SA 4.0**. Therefore do not generalize the top-level CC0 statement to
broadcasts. Select titled-master online games from standard CC0 exports, label
them truthfully, and exclude broadcast data. December 2025 standard-game exports
are the selected source snapshot; freeze downloaded file hashes during data
preparation. Online PGN supports WhiteTitle/BlackTitle metadata where available;
exclude games missing required titles from the master subset.

The puzzle export advertised update date is 2026-10-05. The documented CSV adds
optional DailyDate after OpeningTags. Its FEN precedes the first opponent move;
apply Moves[0] before presenting the puzzle and start the accepted solution at
Moves[1]. The plan supports both the older header and this extension. No large
export was downloaded; collection counts and installed sizes remain unmeasured.

[Opening-name data](https://github.com/lichess-org/chess-openings) has an explicit
[CC0 dedication](https://github.com/lichess-org/chess-openings/blob/master/COPYING.txt).
Freeze Git revision a6189a30dc273ccb21fc2536a9a2fefd5592a67a (resolved during this
pass) and actual TSV hashes when preparing data. Do not fetch a moving branch
for reproducible releases. Provider discovery via Stripe Directory returned no
matches for free chess PGN/puzzle datasets; official source documentation supplied
these selections. No payment or provider provisioning occurred.

### Selected stable library versions

Versions were checked against npm package manifests and crates.io stable,
non-yanked releases. These are design selections, not an installed compatible
lockfile. Implementation must verify exact dependency resolution and APIs.

| Area | Selected version |
| --- | --- |
| Rust / edition | Existing 1.99.0 / 2024 |
| Tauri Rust / JS API / CLI | 2.12.4 / 2.12.4 / 2.12.3 |
| Updater Rust / JS | 2.14.0 / 2.14.0; native ownership |
| Vue / Vue TypeScript checker | 3.5.44 / 3.3.13 |
| TypeScript / typescript-eslint | 6.0.3 / 8.71.2 |
| Bun / Vite / Vue Vite plugin | 1.4.3 / 8.3.4 / 6.0.10 |
| Tailwind / shadcn-vue | 4.3.3 / 2.8.2 |
| ESLint / Prettier | 10.12.0 / 3.9.10 |
| Three.js | 0.186.1 |
| shakmaty / pgn-reader | 0.30.2 / 0.29.1 |
| rusqlite / schemars / Serde | 0.40.2 / 1.2.3 / 1.0.229 |
| reqwest / keyring / Rayon | 0.13.5 / 4.2.0 / 1.12.0 |
| json-schema-to-ts / Ajv | 3.1.1 / 8.20.0 |
| Vitest / browser-playwright provider | 5.0.5 / 5.0.5 |
| Playwright / Vue Test Utils | 1.64.1 / 2.5.1 |
| WebdriverIO / @wdio/tauri-service | 9.31.9 / 1.5.0 |
| Inter | v4.1, Inter-4.1.zip |

TypeScript 7 is newer but outside typescript-eslint's declared <6.1 peer range;
retain 6.0.3. WebdriverIO 10 is newer but the selected Tauri service declares
webdriverio ^9.0.0; retain 9.31.9. Vite, ESLint and browser-test tools declare
Node engine constraints; execution on Bun requires actual toolchain tests rather
than assuming those fields prove Bun support. Resolve supporting dependencies
at stable compatible pins and commit their lockfiles in implementation.
Sources include [npm registry](https://registry.npmjs.org/),
[crates.io API](https://crates.io/data-access),
[Inter v4.1](https://github.com/rsms/inter/releases/tag/v4.1), and
[Inter OFL](https://github.com/rsms/inter/blob/master/LICENSE.txt).

### Pinned updater and CI feasibility

Inspected src/updater.rs from the published
[tauri-plugin-updater 2.14.0 crate](https://crates.io/crates/tauri-plugin-updater/2.14.0).
It supports native headers, explicit verification of cached bytes, and install
that verifies bytes before invoking the platform installer. Its download API
returns Vec<u8>; use native streamed staging for downloads, then account for
byte-buffer memory before verification/install. It also contains Debian/RPM
installer support, so earlier branch-only claims about package limitations must
not be treated as a current absence of native support. Retain the owner's
system-installer .deb handoff policy and validate it independently. Header
support alone does not prove private GitHub API discovery/redirect safety.

[GitHub's Actions billing documentation](https://docs.github.com/en/billing/managing-billing-for-your-products/managing-billing-for-github-actions/about-billing-for-github-actions)
confirms private-repository quotas and billing beyond included usage. No account
budget, allowance or billing control was read or changed. Gate CI activation on
verified hard-stop controls for compute and storage. A YAML timeout alone does
not guarantee zero cost. No paid runner or overage is selected.

### Rules and exactness risk

[FIDE Laws 5.2.2 and 6.9](https://handbook.fide.com/chapter/E012023) concern
checkmate possibility by any legal sequence, including cooperative play. An
engine's best-play WDL draw, or an ordinary insufficient-material shortcut, is
not equivalent. The selected proof adjudicator must return Unknown rather than
invent a terminal result when its bounded proof is incomplete; practical
coverage/latency remains an explicit release blocker until demonstrated.
This is a selected correctness boundary, not a claim of a solved efficient
algorithm for every arbitrary constructed position.

Polyglot entry-format reference: [python-chess v1.11.2 polyglot.py](https://github.com/niklasf/python-chess/blob/v1.11.2/chess/polyglot.py)
uses the big-endian QHHI record layout: 64-bit key, 16-bit move, 16-bit weight,
32-bit learn field. This confirms the 65535 unsigned weight ceiling; it is a
reference for fixtures, not a selected Python runtime dependency or copied code.
