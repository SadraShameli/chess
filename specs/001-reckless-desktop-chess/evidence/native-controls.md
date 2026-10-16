# Native game controls

This slice starts at `7d0a441` and completes the presentation obligations of T055
and T070. It adds eleven hermetic acceptance cases. It does not complete US1, T071,
T119 or T126, or establish production GPU/native accessibility acceptance.

## Controls and ownership

Mounted `PresentationApp` checks exercise the visible Cancel Premove button and
Escape, immediate local takeback with consent disabled, and confirmation and
cancellation of resignation for each local player. Requests carry the original
game/revision and are submitted once despite forced repeated egui passes. Before
authoritative completion, the mounted snapshot and real session retain their
position, clock and queued intent. Local takeback acceptance uses the existing
domain transition without opening opponent approval.

Existing panel/controller cases retain engine takeback without a dialog, required
local consent, engine resignation confirmation, accepted/rejected move feedback,
explicit current/intended draw claims, ordinary premove queue/replacement and
Return to Live. A new mounted-player-bar case checks tenths, the textual Low time
cue and the exact remaining nanoseconds without mutating the authoritative clock.
It advances deterministic frames because the low-time display schedules recurring
repaints.

The board input now carries an optional typed `GamePremoveIntent`, separately from
normal selection. Source and destination use the existing square projection and
viewport clipping, current theme selection/warning colors, outlined squares and
visible Premove from/Premove to captions. Semantic square labels identify queued
premove source/destination independently of color. Mounted checks cover visible
paint output and semantic roles, replacement, history suppression, return to live,
explicit cancellation, an engine-fault pause and rejected stale snapshots. The
board remains semantically available while move actions are disabled; those
actions are consumed without reaching the controller.

Promotion uses the actual pinned egui 0.36.2 `Modal`, with explicit AccessKit Dialog
and modal semantics. It starts on Queen, retains all four choices, traps Tab and
Shift+Tab, accepts Q/R/B/N or accessible activation, and supports visible cancel
and Escape. Closing returns focus to the origin's existing semantic square once
that square is mounted, including while native acceptance is pending. Initial
focus waits for egui's visible pass after Area sizing. A view-frame readiness
result and current square projection defer focus return while the renderer still
has an obsolete viewport. Containment applies only
when this modal can receive focus, preserving a higher modal's input.

The mounted application disables underlying controls and board actions for the
entire frame that displays promotion, including repeated passes after closure.
Escape dismisses a premove promotion without canceling an already queued move.
Keyboard and AccessKit selection preserve the original board action identity,
revision and all four promotion kinds. A simultaneous board action cannot leak
through. A changed native revision dismisses the prompt and rejects its stale
accessible choice without mutation. Promotion selection precedes premove queuing.

Backend ordered checkpoint saves, persistent illegal-premove notices, exact
immutable cancellation retry requests and adjudication retry semantics are
unchanged. No native harness, bootstrap/factory, dependency or checker was edited.

## Red and green evidence

Before implementation,
`cargo test --locked --test presentation mounted_ -- --nocapture` ran eight cases:
five passed and three failed. Two failures reproduced the product gaps:
`g1: White knight` lacked queued-premove identification and promotion had no
Dialog semantics. The third was a new fixture missing `begin_commit`; the fixture
was corrected before accepting green evidence. After modal integration, the focus
case also failed with `modal lost focus` when Tab wrapped beyond the last choice.
Explicit modal-owned traversal resolved that failure without weakening assertions.

The final review added
`cargo test --locked --test presentation mounted_stale_promotion_returns_focus_only_after_the_current_board_is_mounted -- --nocapture`.
It failed with zero passed and one failed: `focus must not target an unmounted
square`. The new view-frame readiness result prevents focus from being restored
through egui's retained response for a square absent from the current viewport.

After the final related source/test edits, these commands ran sequentially:

```sh
cargo fmt --all
cargo test --locked --lib --test domain --test desktop --test presentation --test storage --test engine -- --format terse
cargo clippy --locked --tests -- -D warnings
```

The debug test command passes 590 tests: 50 library, 201 desktop, 173 domain,
72 engine, 66 presentation and 28 storage. Presentation increased from 55 to 66.
There are no failed or ignored tests in these selections. Test-target Clippy
passes with warnings denied. The existing macOS oversized-unwind linker warning
still appears during debug linking; it is not a source or Clippy diagnostic.

## Verification limits

These are deterministic egui paint/AccessKit/application-port checks with real
domain sessions and immutable snapshots, plus the listed existing suites. They
use the current host's Rust 1.99.0 compiler, matching the project pin. No branches,
worktrees or agents were created. No files were staged, committed or pushed.

These checks do not run the production native window/GPU harness, the prepared real-engine
`--game-composition` scenario, installed-package play, actual VoiceOver/NVDA/Orca,
other platforms, optimized performance or release/all-target/all-feature gates.
The full `make verify` gate was outside the authorized debug-only slice. The
remaining US4 animation, drawing, quality, Auto-flip and complete modal/native
accessibility work stays open.
