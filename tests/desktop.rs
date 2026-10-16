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
#[path = "desktop/startup.rs"]
mod desktop_startup;

#[path = "desktop/file_handle.rs"]
mod desktop_file_handle;

#[path = "desktop/application.rs"]
mod desktop_application;
#[path = "desktop/envelope.rs"]
mod desktop_envelope;
