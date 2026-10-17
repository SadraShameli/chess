# US3 library handlers and controls, partial status

This is a partial record. T106, T107, T108, T109 and T111 are not complete and no
task marker was changed for them.

## Implemented and executed (HEAD bdd395b plus staged edits)

- Library service actions Organize, PreviewTrash, ApplyTrash, Restore and
  RemoveFolder. Organize and Restore are stamped with the service wall clock, so
  the presentation layer never supplies a timestamp. RemoveFolder applies a
  previewed plan with no closed-game evidence and refuses any other plan kind.
- Library panel controls: Add to or Remove from Favorites, Show Trash and Show
  Library games, Restore to Library on Trash rows, Preview folder removal with an
  explicit Confirm folder removal or Keep folder step that shows the folder and
  game counts.
- Results of Organize, Trash apply, Restore and folder removal mark the loaded page
  for reload and clear cached folder metadata.

Executed once before the owner stopped further builds: `cargo check --lib --tests`
clean; `cargo test --lib library` 172 passed, 0 failed; `cargo test --test desktop
library` 15 passed, 0 failed. One earlier run of
`recovery::shutdown_control_returns_the_actor_after_deadline_and_can_resume_cleanup`
failed under load and again with `Error: Unavailable`; it passed when run alone twice.
That timing sensitivity was not diagnosed.

## Not done

- Import and export handlers (T106), the import and export previews with
  confirmation and unsupported-syntax reporting (T108). The storage session
  (`StorageImportSession` prepare, page, detail, choose, freeze, commit) and
  notation export exist, but there is no desktop action, native file handle flow or
  presentation component for them.
- Tag and folder assignment and creation controls (T107).
- Permanent delete, game Move to Trash and the open-game Trash confirmation (T109).
  They need closed-game evidence from the game registry close certificate.
- Story scenarios 1 through 9 and exact round-trip, duplicate and Trash results
  (T111). The new presentation flows have no egui test for folder removal or Trash
  restore beyond the service and controller tests.
- T097 and T100 remaining items are physical staging, space and measurement
  acceptance: external evidence.
