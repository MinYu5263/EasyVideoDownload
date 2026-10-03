mod cookies;
mod database;
mod datetime;
mod desktop;
mod required_tools;
mod video;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // Keep the default WebView directory so its legacy locale remains available.
            let data_directory = app.path().local_data_dir()?.join("EasyVideoDownload");
            let database_path = data_directory.join("app.db");
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
            app.manage(cookies::CookieStore::new(&data_directory));
            app.manage(video::download::DownloadManager::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            database::get_app_settings,
            database::save_app_settings,
            database::open_app_data_directory,
            cookies::get_cookie_contents,
            cookies::save_cookie_contents,
            desktop::import_cookie_file,
            desktop::select_download_directory,
            video::parse_video,
            video::get_video_parse_command,
            video::get_video_download_command,
            video::download::get_default_download_directories,
            video::download::download_video,
            video::download::cancel_video_download,
            required_tools::get_required_tools,
            required_tools::check_required_tool,
            required_tools::select_required_tool_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
