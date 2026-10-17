# Saved puzzle workspace integration

This increment connects saved-run inventory and recovery to application admission,
the workspace panel and the native window host. The composition starts one puzzle
service on the existing runtime and injects the existing executor, clock, wake,
repository and scheduler. The service retains the selected command owner and its
storage work. Bootstrap retains puzzle cleanup across dropped waits and observes
it before closing SQLite. T187 and T190 remain open for their full acceptance.

The contracts and focused evidence are owned by
[application admission](puzzle-application-admission.md),
[saved-run recovery](puzzle-recovery-service.md),
[workspace presentation](puzzle-workspace-panel.md) and
[window lifecycle](puzzle-window-lifecycle.md). Applicable policy remains in the
[canonical rule catalog](../../../.specify/rules/README.md).

## Native composition

The host binds the actual primary window once. It observes window and service
state before updating the scene, including quit and graphics-recovery paths.
Focus, visibility, occlusion, suspension and render-failure interruptions retain
the session selected at their original event. One receipt and one latest queued
interruption remain owned until observed. Final AppExit waits for that ownership
to settle, along with the existing shutdown barriers.

The post-presentation readiness report uses the currently applied board and the
root input gates. End Run confirmation withholds Ready without dismissing its
context-bound modal. Pending viewport or camera work withholds Ready even when
capacity contention delays preparation. Paused boardless runs can become ready
for explicit Resume. Promotion remains an interactive canonical move choice;
withholding readiness from its move handler would disable that choice. Ordinary
pending saves retain input while the checkpoint owner permits advancement.

A captured interruption conceals content immediately, including a loss/refocus
pair consumed in one event batch. Native receipt completion alone does not expose
an active board. Authoritative paused state and explicit Resume retain their
existing roles. A replacement session does not inherit an earlier session's
concealment latch or queued interruption identity.

## Reviewed regression evidence

All listed behavior repairs followed observed failures on their prior source.
Fixture construction and compiler failures are distinguished from semantic reds.

| Boundary | Observed prior result | Repair |
| --- | --- | --- |
| Application control admission | 10,000 distinct Game Stops admitted; one failing regression | Separate finite idle and tracked-mutation control budgets with independent Quit |
| Application review | Three failing cases | Preserve mutation safety admission; Quit observes late recovery; terminal admission seals |
| Recovery service review | Two failures among 17 cases | Release failed shutdown reservation before replying; cancelled preparation cannot admit a write |
| Coordinator completion | One failing retained-Pause case | Revisit ready responses after completion progress before parking |
| Workspace selection | Three failures among 26 cases | Merge newer matching watch state after Opened; terminal Open releases opening state; unsafe active replacement refuses |
| Native watch progress | Two failures among five selected Busy cases | Observe receipts before semantic watch progress and exclude failure-only publications |
| Same-context owner publication | One failing case | Retain the actual snapshot Arc as progress evidence |
| Native concealment | One failing focus/refocus case | Retain a session-scoped concealment latch until authoritative pause or terminal state |
| Root End confirmation | One failing case | The root input predicate includes the contextual End modal |
| Root geometry | Three failing cases | Readiness excludes pending viewport and camera work |
| Service reply ordering | One failing case returning Busy after completion | Observe ready retained replies before admitting another interruption |
| Native admission signal | One failing case with an unchanged snapshot Arc | The adapter observes explicit native capacity progress |

Snapshot freshness alone does not prove service reply capacity was released.
The runtime publishes its snapshot before forwarding its receipt, so a Busy
retry can consume that publication while the service still retains an earlier
native reply. The service now owns an opaque native-admission progress identity.
It replaces that identity when a native reply is physically removed, and when
native ingress is consumed while that reply lane is available. A refusal while
the lane remains occupied preserves the identity. All service publications,
including wake-error publication, carry it. The adapter retains the previous
identity and retries when it changes. Both owners hold the Arc allocation while
comparing identity, preventing address reuse; no wrapping counter or timer retry
is involved. Current and previous identities are bounded retained state.

The real service ordering fixture observes the original runtime receipt, confirms
that a still-retained lane refuses without changing progress, then drives the
service to release it and admit the next original-session interruption. It closes
the service, database and executor before assertions. The adapter signal fixture
keeps the exact owner snapshot Arc unchanged and distinguishes a capacity-release
publication from an unchanged publication.

The initial adapter selection had 23 passing cases and one fixture error caused
by constructing invalid Bevy entity bits. The corrected fixture compares two
real entities. Two later mounted cases initially rejected an active initial
checkpoint; their setup now commits the inactive initial snapshot and subsequent
active state through the real storage driver. A compile correction extracts the
nonzero session identifier's integer with its typed getter. These setup failures
are not semantic regression evidence. Host geometry fixtures also explicitly
clear unused egui texture deltas before dropping their CPU-only output.

## Targeted execution

The repaired workspace panel passed 26 cases. The root readiness selection passed
eight cases, including the retained bootstrap cleanup check, real promotion
choices, ordinary-save input, End confirmation and all three geometry scenarios.
The mounted native adapter selection passed 30 cases before the final admission
signal was added. The final combined all-feature library, desktop and domain
selection then passed 242, 215 and 298 cases respectively: 755 passed, zero
failures, ignored cases or filtered cases. This includes all 31 adapter cases,
18 recovery-service cases, 26 workspace-panel cases and the eight root readiness
and retained-cleanup checks. A fresh read-only review found no remaining concrete
blocker in the native capacity signal or concealment paths.

The additional checkpoint crash simulation passed for both countdown modes. It
commits active time at 250 ms, withholds the next 500 ms snapshot, freezes at
1,250 ms and restores only the committed bytes. SQLite load agrees with the
decoded committed snapshot. Lost time is exactly 1,000 ms; recovery hides content,
charges no downtime and requires explicit Resume with the preserved budget. This
completes the previously separate bound and recovery assertions for T177. T177
is accepted with the final gate below; full native/durability tasks remain open.

All Cargo commands use the root shared target directory and detected default
parallelism. Gates run sequentially. The existing macOS debug linker
`__eh_frame` size warning remains visible and unsuppressed. Clippy initially
identified four oversized host-fixture futures; those awaits now use owned
pinned futures, without a suppression or checker change.

## Canonical gate

Final `make verify` exited successfully. All three Clippy selections passed,
followed by 1,205 tests with zero failures, ignored cases or filtered cases.
The all-target optimized build passed in 4 minutes 46 seconds. The separately
selected optimized native harness passed in 48.77 seconds. These durations are
verification timings, not application performance measurements. The native
harness was compiled and linted; GPU/OS acceptance was not executed.

The initial final-gate attempt stopped at two non-exhaustive command matches in
the native composition harness. Both helpers now delegate Puzzle expected
revision to the canonical service method. The entire canonical gate was rerun
after that compiler correction. No dependency, toolchain, manifest, checker,
suppression or vendor change was needed.

Exact commands, historical scopes, raw logs and verified hashes remain under
`.git/parallel-patches/puzzle-saved-workspace-20261017/`.

| Log | SHA-256 |
| --- | --- |
| `puzzle-native-workspace-combined-tests.log` | `cc52bc3c12ac235facb5f4a3d65b0f1f42f9801f60d342954e82ef29b35df74b` |
| `puzzle-saved-workspace-make-verify-initial.log` | `7dc8c61e4c9cfd7de0eedf1bb2edbf544a78d03f945cfdb70a04800f96bb763d` |
| `puzzle-saved-workspace-make-verify.log` | `b17362a4228a0584d412421880c5808336d583057109daab534dc6f055ff373a` |

## Remaining acceptance

The CPU-only Bevy and semantic egui fixtures exercise production owners and root
methods. They do not execute a native GPU window or prove desktop OS sleep-event
delivery. Windows, X11, Wayland, actual macOS lifecycle, accessibility and renderer
acceptance retain their own evidence requirements.

Installed collection discovery and a production Start factory remain unavailable.
The shipping panel truthfully disables Start. The approved
[collection-scale design](puzzle-collection-scale-design.md) still requires
immutable catalogs, pinned run references, bounded checkpoints and historical
continuity before production-scale preparation. History deletion and the full
T187/T190/T191/T192/T193 acceptance remain open. All thirteen journeys remain
first-release scope.
