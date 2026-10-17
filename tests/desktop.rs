use chess::game;

#[path = "game/storage_wall_time_fixture.rs"]
mod game_storage_wall_time_fixture;

#[path = "storage/document_write_fixture.rs"]
mod storage_document_write_fixture;

#[path = "desktop/activation.rs"]
mod desktop_activation;
#[cfg(unix)]
#[path = "desktop/activation_unix.rs"]
mod desktop_activation_unix;
#[cfg(windows)]
#[path = "desktop/activation_windows.rs"]
mod desktop_activation_windows;
#[path = "desktop/coordinator.rs"]
mod desktop_coordinator;
#[path = "desktop/dialogs.rs"]
mod desktop_dialogs;
#[path = "desktop/dialogs_proxy.rs"]
mod desktop_dialogs_proxy;
#[path = "desktop/identity.rs"]
mod desktop_identity;
#[path = "desktop/installation.rs"]
mod desktop_installation;
#[path = "desktop/library_routing.rs"]
mod desktop_library_routing;
#[path = "desktop/native_bootstrap.rs"]
mod desktop_native_bootstrap;
#[path = "desktop/native_host.rs"]
mod desktop_native_host;
#[path = "desktop/resources.rs"]
mod desktop_native_resources;
#[path = "desktop/native_startup.rs"]
mod desktop_native_startup;
#[path = "desktop/owner_record.rs"]
mod desktop_owner_record;
#[path = "desktop/profile_lease.rs"]
mod desktop_profile_lease;
#[path = "desktop/recovery.rs"]
mod desktop_recovery;
#[path = "desktop/startup.rs"]
mod desktop_startup;

#[path = "desktop/file_handle.rs"]
mod desktop_file_handle;

#[path = "desktop/application.rs"]
mod desktop_application;
#[path = "desktop/engine_selection.rs"]
mod desktop_engine_selection;
#[path = "desktop/game_registry.rs"]
mod desktop_game_registry;
#[path = "desktop/game_registry_fixture.rs"]
mod desktop_game_registry_fixture;
#[path = "desktop/game_snapshot.rs"]
mod desktop_game_snapshot;
#[path = "desktop/game_supervisor.rs"]
mod desktop_game_supervisor;
#[path = "desktop/game_transition.rs"]
mod desktop_game_transition;
#[path = "desktop/graphics_lifecycle.rs"]
mod desktop_graphics_lifecycle;
#[path = "desktop/native/analysis_resources.rs"]
mod desktop_native_analysis_resources;
#[path = "desktop/native/game_resources.rs"]
mod desktop_native_game_resources;
#[path = "desktop/native/game_runtime.rs"]
mod desktop_native_game_runtime;
#[path = "desktop/native/game_service.rs"]
mod desktop_native_game_service;
#[path = "desktop/request_cache.rs"]
mod desktop_request_cache;
#[path = "desktop/stream.rs"]
mod desktop_stream;
#[path = "game/config_persistence.rs"]
mod game_config_persistence;
#[path = "game/transition_fixture.rs"]
mod game_transition_fixture;

#[path = "desktop/game_supervisor_storage.rs"]
mod desktop_game_supervisor_storage;
#[path = "desktop/local_play.rs"]
mod desktop_local_play;

#[path = "desktop/application_action.rs"]
mod desktop_application_action;
#[path = "desktop/coordinator_editor.rs"]
mod desktop_coordinator_editor;
#[path = "desktop/editor_commands/write_recovery_review.rs"]
mod desktop_editor_commands_write_recovery_review;

#[path = "desktop/preferences.rs"]
mod desktop_preferences;
