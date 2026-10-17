use chess::game;

#[path = "game/storage_wall_time_fixture.rs"]
mod game_storage_wall_time_fixture;

#[path = "desktop/native/game_engine.rs"]
mod desktop_native_game_engine;
#[path = "engine/analysis.rs"]
mod engine_analysis;
#[path = "engine/capabilities.rs"]
mod engine_capabilities;
#[path = "engine/diagnostics.rs"]
mod engine_diagnostics;
#[path = "engine/lifecycle.rs"]
mod engine_lifecycle;
#[path = "engine/live_game.rs"]
mod engine_live_game;
#[path = "engine/options.rs"]
mod engine_options;
#[path = "engine/package_manifest.rs"]
mod engine_package_manifest;
#[path = "engine/package_selection.rs"]
mod engine_package_selection;
#[path = "engine/protocol.rs"]
mod engine_protocol;
#[path = "engine/resource_config.rs"]
mod engine_resource_config;
#[path = "engine/retained_shutdown.rs"]
mod engine_retained_shutdown;
#[path = "engine/retained_startup.rs"]
mod engine_retained_startup;
#[path = "engine/startup.rs"]
mod engine_startup;
#[path = "engine/tablebase.rs"]
mod engine_tablebase;

#[path = "desktop/game_supervisor_storage.rs"]
mod desktop_game_supervisor_storage;

#[path = "engine/search_session.rs"]
mod engine_search_session;
#[path = "engine/workbench.rs"]
mod engine_workbench;
#[path = "workbench/executor.rs"]
mod workbench_executor;
#[path = "workbench/job.rs"]
mod workbench_job;

#[path = "desktop/engine_commands.rs"]
mod desktop_engine_commands;
#[path = "workbench/export.rs"]
mod workbench_export;

#[path = "desktop/workbench_commands.rs"]
mod desktop_workbench_commands;
#[path = "engine/coverage.rs"]
mod engine_coverage;
#[path = "engine/report_persistence_codecs.rs"]
mod engine_report_persistence_codecs;

#[path = "engine/configuration.rs"]
mod engine_configuration;
