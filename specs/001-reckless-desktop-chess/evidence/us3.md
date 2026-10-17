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

## Written under the no-cargo rule (uncompiled and unrun)

Everything in this section was written without compiling or running anything, so it
has no executed evidence and no task marker was changed for it.

- Library service: ConfirmGameTrash applies a Move to Trash or permanent delete after
  the game registry close certificate for a game that is not open, then reports the
  commit or cancels the close. Import actions (Prepare, Page, Detail, Choose, Freeze,
  Commit, Cancel) run one staged session through the new
  `DesktopLibraryServiceImports` and a backend trait that
  `DesktopLibraryServiceImportStorage` implements over `StorageImportSession`. Export
  writes full or mainline PGN of a saved notation game to a native file target through
  an atomic stage, refusing a stale revision, Trash records and saved played games.
- Native file handles: `DesktopLibraryServiceFile` carries a handle id and a one-use
  slot. The library coordinator consumes the registry handle with the workspace owner
  and the correct purpose and fills the slot before the service runs.
- Library panel: tag and folder creation, per-game folder and tag assignment, Move to
  Trash and permanent delete previews with explicit confirmation, import preview
  (counts, rows, exclusion reasons with line and column, duplicate Skip or Keep copy
  with Skip as the default, differences, freeze, commit, cancel) and full or mainline
  export buttons. File requests surface as `SelectFile` effects.
- Tests written: `tests/desktop/library_service/import_export.rs` (fake import
  backend state machine, export of full and mainline PGN, stale refusal, no backend),
  game trash and folder tests in `pagination.rs`, and panel and controller tests.

## Not done

- Production composition of the import backend. `DesktopLibraryServiceOptions::imports`
  is `None` in `src/desktop/native/bootstrap.rs` because the stage directory file,
  library anchor file, space provider, destination and identity provider are not
  constructed anywhere in production yet. Import therefore reports that it is
  unavailable at runtime.
- The app side of `SelectFile`: dispatching `DesktopEnumCommand::FileSelect` with the
  library workspace as owner, and calling `PresentationLibraryPanel::file_selected`
  with the returned handle. `src/presentation/app/library/view.rs` currently shows an
  explicit failure message instead.
- Closing and Trash confirmation for a game that is open in the editor: the action
  refuses an open game (the registry reports a close conflict). Passing the live game
  revision to `try_close_target` is not wired.
- Exporting saved played games.
- Story scenarios 1 through 9 and exact round-trip, duplicate and Trash results.
- T097 and T100 remaining items are physical staging, space and measurement
  acceptance: external evidence.
