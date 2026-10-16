# New Game controls

The domain configuration and native egui form cover the approved presets, all
six custom clock families, Standard and reproducible FRC/DFRC positions, custom
editor positions, player seats, and mode-specific options. Initial selections
remain White, Standard, 10+5 and a separate three-second untimed engine budget.
Random starts resolve in the application rather than repeated egui layout passes.

The focused debug run passed 152 domain, 23 presentation and 26 storage tests.
Eleven of those cases cover configuration and New Game controls, including
explicit Start, repeated internal layout passes and preset accessibility.
Targeted Clippy passed for the library and these three test targets.

The form still needs production mounting and authoritative application dispatch;
T069 remains pending until that integration is exercised. Optimized builds and
platform acceptance were deliberately deferred under the owner's debug-build
instruction. The existing macOS debug linker unwind-table warning remains.
