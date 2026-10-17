#[path = "board/position.rs"]
mod board_position;

#[path = "clock/persistence.rs"]
mod clock_persistence;
#[path = "clock/state.rs"]
mod clock_state;
#[path = "clock/time.rs"]
mod clock_time;

#[path = "game/configuration.rs"]
mod game_configuration;
#[path = "game/mate_proof.rs"]
mod game_mate_proof;
#[path = "game/pause.rs"]
mod game_pause;
#[path = "game/premove.rs"]
mod game_premove;
#[path = "game/restoration.rs"]
mod game_restoration;
#[path = "game/result.rs"]
mod game_result;
#[path = "game/rules.rs"]
mod game_rules;
#[path = "game/save.rs"]
mod game_save;
#[path = "game/session.rs"]
mod game_session;
#[path = "game/study.rs"]
mod game_study;
#[path = "game/takeback.rs"]
mod game_takeback;
#[path = "game/transition_fixture.rs"]
mod game_transition_fixture;
#[path = "game/tree.rs"]
mod game_tree;

#[path = "notation/game_identity.rs"]
mod notation_game_identity;

#[path = "puzzle/checkpoint.rs"]
mod puzzle_checkpoint;
#[path = "puzzle/csv_import.rs"]
mod puzzle_csv_import;
#[path = "puzzle/definition.rs"]
mod puzzle_definition;
#[path = "puzzle/fixture.rs"]
mod puzzle_fixture;
#[path = "puzzle/persistence.rs"]
mod puzzle_persistence;
#[path = "puzzle/run.rs"]
mod puzzle_run;
#[path = "puzzle/selection.rs"]
mod puzzle_selection;
#[path = "puzzle/solution.rs"]
mod puzzle_solution;
#[path = "puzzle/summary.rs"]
mod puzzle_summary;
#[path = "puzzle/timer.rs"]
mod puzzle_timer;

#[path = "capacity/budget.rs"]
mod capacity_budget;
#[path = "capacity/native_partition.rs"]
mod capacity_native_partition;
#[path = "capacity/retained_shutdown.rs"]
mod capacity_retained_shutdown;
#[path = "capacity/waiting.rs"]
mod capacity_waiting;
