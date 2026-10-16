# Native foundation evidence

Recorded 2026-10-16. Foundation tasks remain independently tracked in
[the task list](../tasks.md). Component-level results do not establish
production-host or cross-platform acceptance.

## T040: Tagged Reckless search lifecycle

The engine session now serializes one exclusive infinite, depth, node, move-time
or clock limit. Clock limits include both players' time and increment plus
optional moves-to-go. Bounds follow Reckless 0.9's signed 32-bit numeric parser
and depth loop. Initial position and the complete legal UCI move history travel
together, including Chess960 castling encoding.

Search events retain game/revision, search and process-generation identities.
An active or stopping search blocks position/configuration changes until its
final bestmove. readyok remains a readiness event. Stop deadlines originate at
application receipt and remain absolute through queue delays and cancellation;
an event becoming ready at the deadline cannot succeed. An interrupted partial
stop write permanently prevents further protocol writes and forces observed
termination rather than reusing ambiguous state.

The isolated engine worktree passed all 23 engine integration cases, including
13 new lifecycle cases, and strict all-target/all-feature Clippy without
warnings. Tests exposed the absent lifecycle API, a simultaneous
bestmove/deadline defect and three canceled partial-stop-write defects before
their fixes. The reviewed test/source binary patches were then applied to the
feature branch with three-way patch integration and explicit module registration.

These deterministic fixtures use controlled process ports and paused Tokio time.
They do not claim complete engine-option UI, production scheduling or live-game
integration, which remain later tasks.

## Profile, owner-record and activation cores

The profile core retains one un-cloned permanent OS lock handle and validates
canonical directory identity, ownership, permissions, supported local filesystem
and private control objects. Existing profile IDs survive a new launch proposal;
aliases contend on the same anchor. macOS extended ACL checks reject unsafe
grants even when Unix mode bits remain private. These checks use safe exacl
path queries bracketed by held-file identity checks because that library exposes
no public descriptor-based ACL query. This does not prove resistance to every
same-user pathname race.

Owner metadata is bounded/versioned and published only with a bound-endpoint
proof and the original lease. Its phases and canceled-shutdown restoration keep
that lease. Control-record writes synchronize bytes, atomically replace the
record and synchronize the held control directory on Unix. Installation-intent
inspection permits ordinary startup only when no intent exists; even a recorded
successful installation still needs explicit installed-package/schema validation.
The shared validated PackageDigest now owns package checksums for engines and
installation records without compatibility aliases.

Activation framing accepts only bounded versioned Activate/response/receipt
messages with canonical profile, owner-generation and request identities. It
authenticates before reading trusted bytes and retains absolute deadlines. A
deterministic regression failed when a response became ready exactly at expiry;
deadline-priority selection plus checks between exchange steps corrected it.

Current-host debug verification passed 11 library tests, 49 desktop tests and
23 engine tests. The final integrated desktop run passed all 49 cases in
0.24 seconds after a 2.14-second incremental debug build; targeted library and
desktop Clippy passed without warnings. The first-launch race test initially failed: concurrent
nonexclusive create/open returned ENOENT on this macOS/APFS host. An isolated
probe using only pinned rustix reproduced it. Exclusive first creation followed
only on EEXIST by opening the existing anchor fixed the regression, without
retrying admission, truncating/unlinking the anchor or taking over ownership.

These are component-level results. Production startup still uses the retained
legacy shell, so T032 through T038 remain unchecked until the native root,
real endpoint transports, secondary-process admission and installation barriers
are integrated. Windows and Linux filesystem/security execution is outstanding.

### Activation receipt delivery and bounded server

Source review found that closing a Windows pipe immediately after writing its
receipt could discard unread receipt bytes when required drop-linger suppression
was active. A regression observed the old client report success without owner
acknowledgment. The corrected version-2 protocol follows the
[activation contract](../contracts/desktop-ipc.md#local-activation-interface);
the same original deadline covers the acknowledgment and version 1 is rejected
before window activation. No transport flush or half-close is required.

The server reserves handler capacity before polling accept, retains at most four
handlers, observes all completions and drains them on shutdown. Malformed clients
remain individual refusals. Seventeen integrated activation tests passed,
including missing/wrong/late acknowledgments, excess bytes, capacity, continued
service after a refused peer and shutdown draining. Targeted strict Clippy for
the library and affected desktop/domain/presentation/scene suites passed.

## T034: Application admission and capacity fixtures

Sixteen application tests cover bounded ordinary admission, reserved coalesced
controls, retained action identities, absolute stop deadlines, cancellation
before commit, ambiguous-commit reconciliation and native wake failures. Eighteen
capacity tests cover the native minimum, preparation/steady/interrupted states,
draining and stale generations, shared maximum capacity, and refusal of raw
budget/executor bypass. All passed in the integrated debug run, alongside the
existing domain and desktop cases. Targeted strict Clippy passed.

These plain Rust owners expose actual typed ports for native presentation.
Production actor effects, Bevy pool wiring and measured CPU chunk limits remain
open under T038/T039; these tests do not establish those later integrations.

## T041/T042: Native scene and semantic input adapters

The adapter resolves the original twelve named GLB piece templates and their
authored descendant transforms. It instances board pieces individually, renders
to a reusable image, fits the approved camera, maps clipped logical coordinates
to physical rays and invalidates pending picks after geometry/device generations
change. A real headless MeshRayCast regression verifies that the nearest piece
occludes the square beneath it. Cached frames stop that board camera until a
change explicitly requests another frame.

All eleven scene tests and thirteen presentation tests passed. The latter cover
64 projected visible semantic squares, actual AccessKit click/focus ingress,
keyboard orientation, bounded stale-generation input, repeated passes, retained
application request identities and confirmed quit. Targeted strict Clippy passed.
The UI reads real bootstrap DTOs and shows engine identity/failure state; square
selection inspects authoritative legal destinations without inventing game play.

Inter 4.1 and its OFL/provenance moved from test fixtures to resources/fonts.
Production and compatibility code share one embedded font owner. The font SHA-256
remains 4989b125924991b90d05b2d16e0e388c48f7d5bb8b30539bbf9c755278d0ccaf.
The cold-layout test now waits for the pending font update before creating its
matching offscreen viewport; production rejects input until both geometries match.

The complete native host, graphics recovery, clock repaint/idle measurement and
OS accessibility acceptance remain later gates. Headless tests do not establish
native frame-time, complete-process idle CPU or real assistive-technology behavior.

## Native piece generator candidate

The optional asset-tools feature uses verified stable gltf-json 1.4.1
(MIT OR Apache-2.0), earcutr 0.5.0 (ISC) and boolmesh 0.2.1 (MPL-2.0).
These libraries remain authoring dependencies rather than application runtime
requirements. The adapted Three.js bevel algorithm retains its MIT notice under
resources/licenses. Generator work and serialization use the injected CPU
executor; admitted asynchronous writes start after CPU leases are released.

Five debug asset tests passed in 0.89 seconds, and targeted tool/asset Clippy
passed without warnings. CLI generation and a separate check invocation passed.
Tests validate named hierarchy/materials, original-compatible bounds within
0.00001 square units, normal directions, the actual bishop slit, exact
one-worker/maximum-worker output and file preservation after detected drift.

The generated candidate is 1,499,440 bytes with SHA-256
f841223d2270426b26c8da913a175be6a4ee6f674e45fa63737973041efb969d.
Original bytes/provenance remain unchanged at 1,376,224 bytes and SHA-256
ee0c9bf8cd98f1239046b0566748030c4c320f049fd6ab3232f1f7e9691a020e.
The CSG topology and exact deduplication differ; native loader and visual
comparison remain required before adoption and final T047 acceptance.

## Native activation transports and host services

Integrated real Unix sockets retain private directory/socket identities, check
reciprocal effective-user credentials, enforce the original absolute deadline
and explicitly clean up only the owned socket and empty directory. Unknown
contents and replaced paths survive cleanup. On this macOS host, the OS-reported
local socket address truncates the final pathname byte; exact filesystem
identity and real full-path connection tests provide the binding evidence.
The transport does not use that reported pathname as authority.

The Windows named-pipe adapter is registered for Windows only. Its explicit
per-user/SYSTEM permissions, reciprocal token checks, nonblocking connection and
drop behavior have source coverage but have not been compiled or run on Windows.
Linux socket/security behavior likewise still requires Linux execution.

The production resource resolver checks the original GLB size/digest and package
containment while treating an absent engine as a separate service failure.
Native wakes coalesce, and recovery choices require an acknowledged matching
generation and preserve a dismissed prompt. Presentation exposes typed quit
readiness only after its retained confirmed-quit receipt actually completes.

The integrated debug run passed 12 library tests and 94 desktop tests in
0.01 and 0.42 seconds respectively, after a 2.39-second incremental compilation.
Five focused presentation application tests passed after the quit-readiness
addition. Targeted strict library/desktop/presentation Clippy passed.
These adapters do not yet replace the retained executable bootstrap.

## Native orchestration and retained service shutdown

The in-process coordinator services bootstrap requests while a native selection
is pending, retains selection identity through cancellation and preserves
revision validation. Its subscription/resynchronization events remain typed
and bounded. Failed shutdown keeps the coordinator and the pending Quit receipt
alive; an explicit Retry quit is required before another cleanup attempt.
The regression first failed when the old coordinator returned its cleanup error
and exited. The fixed coordinator passed all four focused cases.

Native dialogs retain their real parent, future and shared-capacity reservation
until actual completion. Cancellation before opening prevents creation; active
rfd cancellation remains deferred until native close because the pinned library
does not expose an active close operation. Fifteen adapter cases and two
pre-window proxy cases passed. This limitation still leaves T038's bounded
active-dialog cancellation acceptance unresolved.

Fourteen synchronous startup cases passed, including one-owner first-launch
races, aliases, stored-profile binding and installation barriers that release
the lease before displaying blocked status. The resource/startup/native-wake
selection passed 21 cases. These tests do not yet launch the complete executable
or prove secondary-process service exclusion in production.

Engine and database cleanup now borrows retained owners across an absolute
deadline. Timeouts and canceled waits preserve child, pipe-reader and database
worker handles for later observed cleanup. Completion failures retain their
causes and completed handles are not polled twice. Targeted debug verification
passed three process-reader tests, 30 engine cases and 14 storage cases. Strict
library/engine/storage Clippy passed. A real Reckless child shutdown remains
part of the native launch gate.

Two native-host contention cases passed: transient capacity/mailbox contention
rearms the reactive event loop, and nontransient failures remain visible.
Six focused presentation cases passed, including the accessible explicit Retry
quit action and replacement of a stale pending notice with a later cleanup
failure. All development checks used incremental debug builds; native GPU,
cross-platform, optimized performance and final whole-project gates remain open.

## Native executable integration in debug mode

The executable composition now performs synchronous profile/install admission,
starts one owned runtime only for the profile owner, serves bounded activation
while initializing storage and Reckless, and runs Bevy on the OS main thread.
The original OS lease handle remains outside Bevy through service cleanup.
Secondary launches only create the bounded activation-client runtime.

An isolated macOS Apple Silicon debug launch created the native Chess window,
started the packaged Reckless child and published the Ready phase. A second
process against that profile returned ActivationRequested without starting
another engine. Required font and named original GLB assets passed the actual
render-resource/pipeline readiness barrier. This is runtime readiness evidence,
not visual, interaction, accessibility or optimized performance acceptance.
OS screen capture could not produce a display image, and System Events refused
assistive access, so neither was used as visual evidence.

The first smoke process later finished its service cleanup without leaving the
Reckless child, but returned HostEvents during teardown because the host event
sender closed before the authoritative host-done flag arrived. The watcher now
waits for that flag; a later complete native shutdown still needs validation.

Help and invalid-argument paths passed three focused bootstrap tests before
service creation. `make run` now directly starts the native debug executable;
its dry run is `cargo run --locked` with no empty separator, and arguments are
forwarded only when present.

Follow-up checks passed 109 affected debug cases: three process-reader, five
host, 35 domain, 38 engine, 14 presentation and 14 storage cases. Eight engine
cases cover retained startup through failed/canceled handshakes. The executor
result-destruction regression failed before the fix and passed afterward.
Strict focused Clippy passed. macOS test/binary linking still emits the Apple
linker warning that the DWARF unwind section exceeds the compact-unwind offset
range; it is not suppressed and the final zero-warning gate remains open.

GPU generation reset now retires extracted swapchain state and its old surface
before reconstruction. Typed permanent shader errors, recursive asset failures
and ten-second readiness boundaries have deterministic fixtures. Actual device
loss/Retry, native menu/close confirmation, asset visual comparison, installed
resources and the complete cross-platform foundation gates remain outstanding.

## Native quit lifecycle and shared ports

The full production bootstrap now passes a current-host debug lifecycle harness
using its normal profile admission, actual Reckless process, SQLite actor,
capacity partition, native host and retained shutdown owners. The harness waits
for real graphics readiness, sends a native window-close request, clicks the
actual Keep open control through egui AccessKit input, checks that bootstrap
requests still work, and confirms a second close through the real Confirm quit
control. It never submits Quit directly or writes AppExit.

Successful return is followed by independent checks of exclusive profile
reacquisition, the ShuttingDown record, removed Unix activation endpoint and
durable SQLite document reopening. This run completed successfully on macOS
Apple Silicon. Its test driver schedules continuous frames, so it supplies no
idle-performance evidence. Bevy logged a skipped Destroyed event during window
teardown; the bootstrap returned success and left its owned services closed.

The native macOS menu replaces the framework's terminating Quit menu with a
custom Quit Chess action routed through confirmation. A native graphics-recovery
item remains usable while graphics are interrupted or retrying. Two deterministic
menu cases cover coalescing and preserved typed wake failures. Actual OS menu
interaction remains separate from the programmatically driven window-close run.

The shared action registry now owns the three implemented shell intents. The
first Settings section changes the real dark/light appearance immediately and
explicitly identifies its current session-only scope. Persistent Settings remain
later work. Dataset descriptors expose format, expected checksum, canonical local
file location and integrity state; verification uses admitted executor work and
does not claim chess-format semantic validation or installed starter datasets.

One batched debug validation passed 195 cases: 24 library, five asset-generator,
132 desktop, 16 presentation and 18 storage cases. Strict focused Clippy passed.
The real GPU asset replacement evidence is recorded in
[native-assets.md](native-assets.md). The worker-exit regression was observed
failing before correction; six focused shutdown cases and two private-boundary
cases then passed, including retained joins after cancellation or deadlines.

Optimized/performance and final whole-project gates are deferred while development
uses the owner's requested incremental debug checks. Windows/Linux execution,
actual graphics-loss recovery, native power integration, active native-dialog
cancellation and complete product stories remain open. The macOS debug-linker
unwind-table warning remains visible and unresolved.
