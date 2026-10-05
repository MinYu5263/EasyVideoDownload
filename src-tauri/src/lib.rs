mod app_preferences;
mod cookies;
mod database;
mod datetime;
mod desktop;
mod douyin;
mod proxy;
mod required_tools;
mod thumbnails;
mod video;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .manage(app_preferences::AppPreferences::default())
        .on_window_event(app_preferences::close_requested)
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
            app.manage(required_tools::managed::ConfigureManager::default());
            app.manage(storage);
            app.manage(cookies::CookieStore::new(&data_directory));
            app.manage(thumbnails::ThumbnailStore::new(&data_directory));
            app.manage(video::download::DownloadManager::default());
            app.manage(douyin::lab::LabManager::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            douyin::lab::douyin_lab_get_state,
            douyin::lab::douyin_lab_parse,
            douyin::lab::douyin_lab_download,
            douyin::lab::douyin_lab_cancel,
            app_preferences::get_close_prompt_state,
            app_preferences::respond_to_close_request,
            database::get_app_settings,
            thumbnails::cache_video_thumbnail,
            thumbnails::get_cached_thumbnail,
            database::ui_preferences::get_ui_preferences,
            database::ui_preferences::save_ui_preferences,
            database::platform_settings::get_platform_settings,
            database::platform_settings::save_platform_settings,
            database::page_states::get_download_page_states,
            database::page_states::save_download_page_state,
            database::save_app_settings,
            proxy::get_proxy_settings,
            proxy::save_proxy_settings,
            proxy::test_proxy_connection,
            database::open_app_data_directory,
            cookies::get_cookie_contents,
            cookies::save_cookie_contents,
            desktop::import_cookie_file,
            desktop::select_download_directory,
            video::parse_video,
            video::get_video_parse_command,
            video::get_video_download_command,
            video::download::get_default_download_directories,
            video::download::tasks::commands::enqueue_video_download,
            video::download::tasks::commands::redownload_record,
            video::download::tasks::commands::list_download_tasks,
            video::download::tasks::commands::find_download_record,
            video::download::create_download_request_id,
            video::download::history::list_download_records,
            video::download::history::actions::delete_download_record,
            video::download::history::actions::restore_download_record,
            video::download::history::actions::purge_download_record,
            video::download::history::actions::empty_download_record_trash,
            video::download::history::actions::delete_download_record_and_file,
            video::download::history::actions::open_download_record_file,
            video::download::history::actions::open_download_record_folder,
            video::download::history::actions::open_download_record_source,
            video::download::tasks::commands::cancel_video_download,
            required_tools::get_required_tools,
            required_tools::check_required_tool,
            required_tools::managed::configure_required_tool,
            required_tools::managed::cancel_tool_configuration,
            required_tools::select_required_tool_path
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } => {
                api.prevent_exit();
                app_preferences::request_exit(app);
            }
            tauri::RunEvent::Exit => app_preferences::settle_on_native_exit(app),
            _ => {}
        });
}
