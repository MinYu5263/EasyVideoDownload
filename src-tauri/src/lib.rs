mod database;
mod datetime;
mod required_tools;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Keep the default WebView directory so its legacy locale remains available.
            let database_path = app
                .path()
                .local_data_dir()?
                .join("EasyVideoDownload")
                .join("app.db");
            let previous_database_path = app
                .path()
                .app_local_data_dir()?
                .join("EasyVideoDownload")
                .join("app.db");
            let storage = database::Storage::new_with_previous(
                &database_path,
                &app.path().app_config_dir()?.join("required-tools.json"),
                &previous_database_path,
            );
            app.manage(required_tools::RequiredToolManager::new(storage.clone()));
            app.manage(storage);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            database::get_app_settings,
            database::save_app_settings,
            database::open_app_data_directory,
            required_tools::get_required_tools,
            required_tools::check_required_tool,
            required_tools::select_required_tool_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
