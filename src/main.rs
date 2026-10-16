use chess::desktop::bootstrap::DesktopBootstrap;
use tauri::Manager;

fn main() -> Result<(), tauri::Error> {
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    #[cfg(feature = "native-test")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    let app = builder
        .setup(|app| {
            let bootstrap =
                tauri::async_runtime::block_on(DesktopBootstrap::start(app.handle().clone()))?;
            app.manage(bootstrap);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            chess::desktop::bootstrap::desktop_command
        ])
        .build(tauri::generate_context!())?;
    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event
            && let Err(error) =
                tauri::async_runtime::block_on(app.state::<DesktopBootstrap>().shutdown())
        {
            eprintln!("Application shutdown failed: {error}");
        }
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
            let code =
                match tauri::async_runtime::block_on(app.state::<DesktopBootstrap>().shutdown()) {
                    Ok(()) => 0,
                    Err(error) => {
                        eprintln!("Application shutdown failed: {error}");
                        1
                    }
                };
            app.exit(code);
        }
    });
    Ok(())
}
