# Current-host optimized compatibility attempt

At source commit `294ae72ae5da65813b795fb37bdfc2fb3f01dfe8`, the parent ran
`env CARGO_TARGET_DIR=/Users/sadrashameli/Personal/chess/target make native-tests NATIVE_PROFILE=release`.
The isolated compatibility harness uses temporary asset storage. No owner's
profile or application data was written. There were no concurrent Cargo gates,
job caps or test-thread caps.

Both named release test selections compiled successfully. The actual harness
then exited one and Make exited two. Apple M4 Pro/Metal initialized, the original
GLB and dependencies reported Loaded, and the harness reached 580 UI frames.
All 192 captured 2560x1600 BGRA frames contained zero for every channel, with
zero geometry pixels and zero text pixels. The ten-second readback deadline
therefore failed. The final window observation was visible, unfocused and with
no reported occlusion value. The process exited and no matching native or Cargo
process remained at the post-run check.

The log establishes unsuccessful optimized GPU readback, not its cause.
Successful device creation and asset loading do not establish rendered output.
No checker, deadline, classification threshold, renderer, dependency or backend
flag was changed to make the attempt pass. The canonical nonexecuting
[T099 verification](game-identity.md) remains a separate successful result.

The exact log and checksum/command manifest are retained under
`.git/parallel-patches/current-native-compatibility-20261017/`.
T049/T051 and the remaining native/platform acceptance stay open. Earlier debug
and asset-comparison evidence remains historical evidence for its own source and
configuration, rather than a passing result for this attempt.
