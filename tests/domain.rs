#[path = "board/position.rs"]
mod board_position;

#[path = "clock/persistence.rs"]
mod clock_persistence;
#[path = "clock/state.rs"]
mod clock_state;
#[path = "clock/time.rs"]
mod clock_time;

#[path = "game/mate_proof.rs"]
mod game_mate_proof;
#[path = "game/pause.rs"]
mod game_pause;
#[path = "game/premove.rs"]
mod game_premove;
#[path = "game/rules.rs"]
mod game_rules;
#[path = "game/session.rs"]
mod game_session;
#[path = "game/takeback.rs"]
mod game_takeback;
#[path = "game/transition_fixture.rs"]
mod game_transition_fixture;
#[path = "game/tree.rs"]
mod game_tree;

#[path = "capacity/budget.rs"]
mod capacity_budget;
#[path = "capacity/native_partition.rs"]
mod capacity_native_partition;
#[path = "capacity/retained_shutdown.rs"]
mod capacity_retained_shutdown;
