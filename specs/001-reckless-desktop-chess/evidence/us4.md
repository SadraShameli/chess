# User Story 4 evidence

Status of the native acceptance evidence required by T126 and T128. Every item
below is outstanding until it is recorded on the named platform; none has been
claimed from automated egui or unit tests.

| Check | Platform | State |
| --- | --- | --- |
| Keyboard-only play, markings, promotion and dialogs | macOS, Windows, Linux | Outstanding |
| VoiceOver: semantic squares, dialog focus, status announcements | macOS | Outstanding |
| NVDA: semantic squares, dialog focus, status announcements | Windows | Outstanding |
| Orca: semantic squares, dialog focus, status announcements | Linux X11 | Outstanding |
| Orca: semantic squares, dialog focus, status announcements | Linux Wayland | Outstanding |
| Dark and light screenshots at 1280x800, 1440x900 and a large window | all | Outstanding |
| 100%, 150% and 200% display scale layout and 24-point targets | all | Outstanding |
| WCAG AA contrast of the approved tokens | all | Outstanding |
| Piece readability at every permitted camera pose | all | Outstanding |
| Reduced-motion immediate final states | all | Outstanding |
| Graphics recovery Retry and 10-second deadline on real device loss | all | Outstanding |
| Automatic quality adaptation on integrated Intel UHD 620-class and Apple M1-class hardware | Windows/Linux, macOS | Outstanding |

Automated coverage that supports, but does not replace, these checks lives in
`tests/presentation/board/accessibility.rs`, `tests/presentation/board/input.rs`,
`tests/presentation/board/markings.rs` and `tests/scene/quality.rs`.
