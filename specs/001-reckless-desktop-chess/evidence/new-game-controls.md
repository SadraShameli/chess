# New Game controls

The domain configuration and native egui form cover the approved presets, all
six custom clock families, Standard and reproducible FRC/DFRC positions, custom
editor positions, player seats, and mode-specific options. Initial selections
remain White, Standard, 10+5 and a separate three-second untimed engine budget.
Random starts resolve in the application rather than repeated egui layout passes.

The earlier focused debug run passed 152 domain, 23 presentation and 26 storage tests.
Eleven of those cases cover configuration and New Game controls, including
explicit Start, repeated internal layout passes and preset accessibility.
Targeted Clippy passed for the library and these three test targets.

The form is mounted by `PresentationApp::show`; explicit Start uses its retained,
revision-checked game creation path and the production native game bridge.
Repeated layout passes do not resolve random starts or duplicate requests.
The full configuration selection and dispatch were revalidated during acceptance
review at `cc9ca3e`, and all 55 presentation checks pass in the subsequent
473-test adjudication checkpoint. T069 is complete.

This completion covers the New Game controls and their authoritative dispatch.
Auto-flip animation, real renderer interaction and the remaining US1 acceptance
cases retain their own task owners. Optimized builds and platform acceptance
remain deferred under the owner's debug-build instruction. The existing macOS
debug linker unwind-table warning remains.
