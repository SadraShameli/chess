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
