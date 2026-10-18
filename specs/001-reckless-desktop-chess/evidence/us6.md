# US6 position editor scenarios

This records focused test runs against HEAD bdd395b in a lane worktree. It does not
claim native window, GPU, screen reader or platform acceptance.

## Executed selections

| Command | Passed | Failed |
| --- | ---: | ---: |
| `cargo test --test presentation editor` | 10 | 0 |
| `cargo test --test domain editor` | 21 | 0 |
| `cargo test --test storage editor` | 13 | 0 |
| `cargo test --test desktop editor` | 23 | 0 |
| `cargo test --lib editor` | 79 | 0 |

## Scenario coverage

| Scenario | Evidence |
| --- | --- |
| 1 Validation before search or play | `tests/board/editor.rs` structural validation; `tests/presentation/editor.rs::invalid_findings_are_visible_and_play_is_disabled_without_legal_board_substitution` |
| 2 Invalid findings shown, nothing sent to the engine | same presentation test; Play stays disabled |
| 3 Annotated branch keeps notes and alternatives through save or export | `evidence/game-annotations.md`, `evidence/pgn-export.md`, `tests/notation/interchange.rs` |
| 4 Invalid draft survives save and reopen, Play and analysis unavailable | `tests/presentation/app_editor.rs::mounted_opening_uses_actual_ticket_and_invalid_draft_survives_save_reopen`; `tests/storage` editor repository cases |
| 5 Play from Position prefills New Game and waits for Start | `tests/presentation/app_editor.rs::play_prefills_new_game_without_creating_or_replacing_the_retained_draft` |
| 6 Save, Discard, Stay protect unsaved edits; imported FEN opens a separate draft | `tests/presentation/editor.rs::unsaved_leave_exposes_save_discard_stay_and_never_mints_start_game`; `tests/presentation/app_editor.rs::fen_transfer_resolves_real_prompt_and_saves_prior_invalid_draft_separately`, `mounted_navigation_stay_retains_draft_and_discard_releases_exact_destination` |
| 7 Undo, Redo, Clear Board, Standard Setup with confirmation | `tests/presentation/editor/keyboard.rs::placement_removal_undo_and_redo_use_the_editor_session_and_reject_stale_palette_payload`; `tests/presentation/editor.rs::native_palette_and_clear_confirmation_emit_one_typed_action_per_activation`, `raw_invalid_projection_and_typed_board_inputs_use_real_editor_history` |

## Not established here

Native pointer input on the 3D board, platform file dialogs and assistive
technology remain external evidence.
