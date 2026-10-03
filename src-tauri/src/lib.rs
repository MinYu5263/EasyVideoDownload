mod required_tools;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(required_tools::RequiredToolManager::new(
                app.path().app_config_dir()?.join("required-tools.json"),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            required_tools::get_required_tools,
            required_tools::check_required_tool,
            required_tools::select_required_tool_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
