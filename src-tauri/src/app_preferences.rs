use crate::database::AppSettings;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_notification::NotificationExt;

#[derive(Default)]
pub(crate) struct AppPreferences {
    settings: Mutex<Option<AppSettings>>,
    prompt_open: AtomicBool,
    exiting: AtomicBool,
    tray_menu: Mutex<Option<(MenuItem<tauri::Wry>, MenuItem<tauri::Wry>)>>,
}

pub(crate) fn update(app: &tauri::AppHandle, settings: &AppSettings) {
    let state = app.state::<AppPreferences>();
    if let Ok(mut saved) = state.settings.lock() {
        *saved = Some(settings.clone());
    }
    if let Ok(menu) = state.tray_menu.lock() {
        if let Some((show, exit)) = menu.as_ref() {
            let zh = settings.locale == "zh-CN";
            if let Err(error) = show.set_text(if zh { "显示窗口" } else { "Show window" }) {
                report_error(app, "trayFailed", error);
            }
            if let Err(error) = exit.set_text(if zh { "退出应用" } else { "Quit" }) {
                report_error(app, "trayFailed", error);
            }
        }
    };
}

fn report_error(app: &tauri::AppHandle, code: &str, detail: impl ToString) {
    let detail = detail.to_string();
    eprintln!("{code}: {detail}");
    let _ = app.emit(
        "app-native-error",
        serde_json::json!({"code": code, "detail": detail}),
    );
}

pub(crate) enum DownloadOutcome<'a> {
    Completed {
        path: &'a str,
        already_downloaded: bool,
    },
    Failed {
        code: &'a str,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct Notice {
    title: String,
    body: String,
}

fn download_notice(settings: &AppSettings, outcome: DownloadOutcome<'_>) -> Option<Notice> {
    let zh = settings.locale == "zh-CN";
    let (title, body) = match outcome {
        DownloadOutcome::Completed {
            path,
            already_downloaded: false,
        } if settings.notify_on_completion => (
            if zh {
                "下载完成"
            } else {
                "Download complete"
            },
            path.rsplit(['/', '\\'])
                .next()
                .unwrap_or("EasyVideoDownload")
                .chars()
                .take(180)
                .collect(),
        ),
        DownloadOutcome::Failed { code }
        if settings.notify_on_failure
            && !matches!(
                    code,
                    "downloadCancelled" | "downloadBusy" | "applicationExiting"
                ) =>
            {
                (
                    if zh {
                        "下载失败"
                    } else {
                        "Download failed"
                    },
                    if zh {
                        "请打开 EasyVideoDownload 查看错误详情。"
                    } else {
                        "Open EasyVideoDownload to view the error details."
                    }
                        .into(),
                )
            }
        _ => return None,
    };
    Some(Notice {
        title: title.into(),
        body,
    })
}

pub(crate) fn notify_download(app: &tauri::AppHandle, outcome: DownloadOutcome<'_>) {
    let state = app.state::<AppPreferences>();
    if state.exiting.load(Ordering::SeqCst) {
        return;
    }
    let notice = state.settings.lock().ok().and_then(|settings| {
        settings
            .as_ref()
            .and_then(|settings| download_notice(settings, outcome))
    });
    if let Some(notice) = notice {
        if let Err(error) = app
            .notification()
            .builder()
            .title(notice.title)
            .body(notice.body)
            .show()
        {
            report_error(app, "notificationFailed", error);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CloseChoice {
    Ask,
    Tray,
    Exit,
    Cancel,
}

fn close_choice(settings: Option<&AppSettings>) -> CloseChoice {
    match settings.map(|settings| settings.close_action.as_str()) {
        Some("tray") => CloseChoice::Tray,
        Some("exit") => CloseChoice::Exit,
        _ => CloseChoice::Ask,
    }
}

fn take_close_response(state: &AppPreferences, action: &str) -> Result<CloseChoice, String> {
    let choice = match action {
        "exit" => CloseChoice::Exit,
        "tray" => CloseChoice::Tray,
        "cancel" => CloseChoice::Cancel,
        _ => return Err("Invalid close response".into()),
    };
    if !state.prompt_open.swap(false, Ordering::SeqCst) {
        return Err("No pending close request".into());
    }
    Ok(choice)
}

#[tauri::command]
pub(crate) fn get_close_prompt_state(window: tauri::WebviewWindow) -> bool {
    window.label() == "main"
        && window
        .state::<AppPreferences>()
        .prompt_open
        .load(Ordering::SeqCst)
}

#[tauri::command]
pub(crate) fn respond_to_close_request(
    window: tauri::WebviewWindow,
    action: String,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the main window can respond to close requests".into());
    }
    let state = window.state::<AppPreferences>();
    match take_close_response(&state, &action)? {
        CloseChoice::Exit => request_exit(window.app_handle()),
        CloseChoice::Tray => {
            if let Err(error) = try_hide_to_tray(window.app_handle()) {
                state.prompt_open.store(true, Ordering::SeqCst);
                report_error(window.app_handle(), "trayFailed", &error);
                return Err(error.to_string());
            }
        }
        _ => {}
    }
    Ok(())
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let result = window
            .show()
            .and_then(|_| window.unminimize())
            .and_then(|_| window.set_focus());
        if let Err(error) = result {
            report_error(app, "windowFailed", error);
        }
    }
}

fn ensure_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    if app.tray_by_id("main-tray").is_some() {
        return Ok(());
    }
    let state = app.state::<AppPreferences>();
    let zh = state
        .settings
        .lock()
        .ok()
        .and_then(|settings| settings.clone())
        .is_some_and(|settings| settings.locale == "zh-CN");
    let show = MenuItem::with_id(
        app,
        "show",
        if zh { "显示窗口" } else { "Show window" },
        true,
        None::<&str>,
    )?;
    let exit = MenuItem::with_id(
        app,
        "exit",
        if zh { "退出应用" } else { "Quit" },
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&show, &exit])?;
    let mut tray = TrayIconBuilder::with_id("main-tray")
        .tooltip("EasyVideoDownload")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "exit" => request_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left | MouseButton::Middle,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left | MouseButton::Middle,
                    ..
                }
            ) {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    if let Ok(mut items) = state.tray_menu.lock() {
        *items = Some((show, exit));
    }
    Ok(())
}

fn try_hide_to_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    ensure_tray(app).and_then(|_| {
        app.get_webview_window("main")
            .ok_or(tauri::Error::WindowNotFound)?
            .hide()
    })
}

fn hide_to_tray(app: &tauri::AppHandle) {
    if let Err(error) = try_hide_to_tray(app) {
        report_error(app, "trayFailed", error);
    }
}

pub(crate) fn close_requested(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let app = window.app_handle();
        let state = app.state::<AppPreferences>();
        if state.exiting.load(Ordering::SeqCst) || state.prompt_open.load(Ordering::SeqCst) {
            return;
        }
        let settings = state
            .settings
            .lock()
            .ok()
            .and_then(|settings| settings.clone());
        match close_choice(settings.as_ref()) {
            CloseChoice::Tray => hide_to_tray(app),
            CloseChoice::Exit => request_exit(app),
            CloseChoice::Ask => {
                if state.prompt_open.swap(true, Ordering::SeqCst) {
                    return;
                }
                if let Err(error) = app.emit_to("main", "app-close-requested", ()) {
                    state.prompt_open.store(false, Ordering::SeqCst);
                    report_error(app, "windowFailed", error);
                }
            }
            CloseChoice::Cancel => {}
        }
    }
}

pub(crate) fn request_exit(app: &tauri::AppHandle) {
    if app
        .state::<AppPreferences>()
        .exiting
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let downloads = app.state::<crate::video::download::DownloadManager>();
        let tools = app.state::<crate::required_tools::managed::ConfigureManager>();
        let result = async {
            downloads.begin_exit()?;
            tools.begin_exit()?;
            // Let process trees stop and real terminal records/installation rollback settle.
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                while downloads.is_active()? || tools.is_active()? {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Ok::<_, String>(())
            })
                .await
                .map_err(|_| "Timed out while stopping active tasks".to_string())?
        }
            .await;
        match result {
            Ok(()) => app.exit(0),
            Err(error) => {
                downloads.abort_exit();
                tools.abort_exit();
                app.state::<AppPreferences>()
                    .exiting
                    .store(false, Ordering::SeqCst);
                show_window(&app);
                report_error(&app, "exitFailed", &error);
                app.dialog()
                    .message(error)
                    .title("EasyVideoDownload")
                    .kind(MessageDialogKind::Error)
                    .show(|_| {});
            }
        }
    });
}

pub(crate) fn settle_on_native_exit(app: &tauri::AppHandle) {
    // Cocoa's native Quit/Cmd+Q/Dock Quit can emit Exit directly. The runtime
    // still exists during this callback; drain Rust work before Tauri cleans up.
    let downloads = app.state::<crate::video::download::DownloadManager>();
    let tools = app.state::<crate::required_tools::managed::ConfigureManager>();
    app.state::<AppPreferences>()
        .exiting
        .store(true, Ordering::SeqCst);
    let result = tauri::async_runtime::block_on(async {
        downloads.begin_exit()?;
        tools.begin_exit()?;
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while downloads.is_active()? || tools.is_active()? {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok::<_, String>(())
        })
            .await
            .map_err(|_| "Timed out while draining native termination".to_string())?
    });
    if let Err(error) = result {
        eprintln!("Native exit cleanup failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> AppSettings {
        AppSettings {
            locale: "en".into(),
            theme: "system".into(),
            notify_on_completion: false,
            notify_on_failure: true,
            close_action: "ask".into(),
        }
    }

    #[test]
    fn notices_obey_preferences_and_exclude_cancelled_busy_and_skipped_downloads() {
        let mut preferences = settings();
        assert!(download_notice(
            &preferences,
            DownloadOutcome::Completed {
                path: "C:/videos/test.mp4",
                already_downloaded: false
            }
        )
            .is_none());
        preferences.notify_on_completion = true;
        assert_eq!(
            download_notice(
                &preferences,
                DownloadOutcome::Completed {
                    path: "C:/videos/test.mp4",
                    already_downloaded: false
                }
            ),
            Some(Notice {
                title: "Download complete".into(),
                body: "test.mp4".into()
            })
        );
        assert!(download_notice(
            &preferences,
            DownloadOutcome::Completed {
                path: "C:/videos/test.mp4",
                already_downloaded: true
            }
        )
            .is_none());
        for code in ["downloadCancelled", "downloadBusy", "applicationExiting"] {
            assert!(download_notice(&preferences, DownloadOutcome::Failed { code }).is_none());
        }
        assert_eq!(
            download_notice(
                &preferences,
                DownloadOutcome::Failed {
                    code: "downloadFailed"
                }
            ),
            Some(Notice {
                title: "Download failed".into(),
                body: "Open EasyVideoDownload to view the error details.".into()
            })
        );
        preferences.notify_on_failure = false;
        assert!(download_notice(
            &preferences,
            DownloadOutcome::Failed {
                code: "spawnFailed"
            }
        )
            .is_none());
    }

    #[test]
    fn notices_use_saved_locale_and_do_not_expose_full_paths() {
        let mut preferences = settings();
        preferences.locale = "zh-CN".into();
        preferences.notify_on_completion = true;
        assert_eq!(
            download_notice(
                &preferences,
                DownloadOutcome::Completed {
                    path: r"C:\private\视频.mp4",
                    already_downloaded: false
                }
            ),
            Some(Notice {
                title: "下载完成".into(),
                body: "视频.mp4".into()
            })
        );
        assert_eq!(
            download_notice(
                &preferences,
                DownloadOutcome::Failed {
                    code: "cookieReadFailed"
                }
            )
                .unwrap()
                .title,
            "下载失败"
        );
    }

    #[test]
    fn close_policy_uses_saved_behavior_and_asks_when_settings_are_unavailable() {
        assert_eq!(close_choice(None), CloseChoice::Ask);
        let mut preferences = settings();
        assert_eq!(close_choice(Some(&preferences)), CloseChoice::Ask);
        preferences.close_action = "tray".into();
        assert_eq!(close_choice(Some(&preferences)), CloseChoice::Tray);
        preferences.close_action = "exit".into();
        assert_eq!(close_choice(Some(&preferences)), CloseChoice::Exit);
    }

    #[test]
    fn responses_require_a_pending_prompt_and_consume_it_once() {
        let state = AppPreferences::default();
        assert!(take_close_response(&state, "exit").is_err());
        for (action, expected) in [
            ("cancel", CloseChoice::Cancel),
            ("tray", CloseChoice::Tray),
            ("exit", CloseChoice::Exit),
        ] {
            state.prompt_open.store(true, Ordering::SeqCst);
            assert_eq!(take_close_response(&state, action).unwrap(), expected);
            assert!(!state.prompt_open.load(Ordering::SeqCst));
            assert!(take_close_response(&state, action).is_err());
        }
        state.prompt_open.store(true, Ordering::SeqCst);
        assert!(take_close_response(&state, "unexpected").is_err());
        assert!(state.prompt_open.load(Ordering::SeqCst));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "opens a real native window and sends a diagnostic system notification"]
    fn native_window_tray_theme_notification_and_exit_smoke() {
        use std::{
            os::windows::process::CommandExt,
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let profile = tempfile::Builder::new()
            .prefix("evd-preferences-smoke-")
            .tempdir()
            .unwrap();
        eprintln!(
            "temporary native WebView profile: {}",
            profile.path().display()
        );
        // Wry retains COM resources for the runner's lifetime. Use a dedicated
        // child so its WebView2 profile is released before the parent cleans it.
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "app_preferences::tests::native_preferences_fixture",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("EVD_NATIVE_PREFERENCES_TEST_PROFILE", profile.path())
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(30);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= until {
                child.kill().unwrap();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let output = child.wait_with_output().unwrap();
        eprintln!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let exact_path = profile.path().to_path_buf();
        if profile.close().is_err() {
            let until = Instant::now() + Duration::from_secs(5);
            while exact_path.exists() {
                match std::fs::remove_dir_all(&exact_path) {
                    Ok(()) => break,
                    Err(error) if Instant::now() >= until => {
                        panic!("could not clean {}: {error}", exact_path.display())
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        }
        assert!(
            output.status.success(),
            "native fixture failed: {}",
            output.status
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "native child fixture; run only through the native smoke test"]
    fn native_preferences_fixture() {
        use std::{
            sync::{atomic::AtomicUsize, Arc, Mutex},
            time::{Duration, Instant},
        };
        use tauri::Listener;
        let profile_path = std::path::PathBuf::from(
            std::env::var_os("EVD_NATIVE_PREFERENCES_TEST_PROFILE")
                .expect("native fixture profile must come from its parent"),
        );
        let mut context = tauri::generate_context!();
        context.config_mut().app.windows.clear();
        let database_path = profile_path.join("settings.db");
        let setup_database_path = database_path.clone();
        let close_events = Arc::new(AtomicUsize::new(0));
        let results = Arc::new(Mutex::new(None));
        let output = results.clone();
        let app = tauri::Builder::default()
            .any_thread()
            .plugin(tauri_plugin_notification::init())
            .plugin(tauri_plugin_dialog::init())
            .manage(AppPreferences::default())
            .manage(crate::video::download::DownloadManager::default())
            .manage(crate::required_tools::managed::ConfigureManager::default())
            .on_window_event(close_requested)
            .setup(move |app| {
                tauri::WebviewWindowBuilder::new(
                    app,
                    "main",
                    tauri::WebviewUrl::App("index.html".into()),
                )
                    .title("EasyVideoDownload native interface test")
                    .visible(false)
                    .data_directory(profile_path.clone())
                    .build()?;
                let storage = crate::database::Storage::new(
                    &setup_database_path,
                    &profile_path.join("legacy.json"),
                );
                let preferences = storage.database().unwrap().app_settings("en").unwrap();
                app.manage(storage);
                update(app.handle(), &preferences);
                Ok(())
            })
            .build(context)
            .unwrap();
        let event_count = close_events.clone();
        app.listen_any("app-close-requested", move |_| {
            event_count.fetch_add(1, Ordering::SeqCst);
        });
        let exit_code = app.run_return(move |app, event| {
            if let tauri::RunEvent::Ready = event {
                let handle = app.clone();
                let output = output.clone();
                let close_events = close_events.clone();
                let database_path = database_path.clone();
                std::thread::spawn(move || {
                    let result = (|| -> Result<(), String> {
                        let window = handle.get_webview_window("main").ok_or("no main window")?;
                        window.set_theme(Some(tauri::Theme::Dark)).map_err(|e| e.to_string())?;
                        if window.theme().map_err(|e| e.to_string())? != tauri::Theme::Dark { return Err("native dark theme did not apply".into()); }
                        window.set_theme(Some(tauri::Theme::Light)).map_err(|e| e.to_string())?;
                        if window.theme().map_err(|e| e.to_string())? != tauri::Theme::Light { return Err("native light theme did not apply".into()); }
                        window.set_theme(None).map_err(|e| e.to_string())?;
                        window.show().map_err(|e| e.to_string())?;
                        window.close().map_err(|e| e.to_string())?;
                        let until = Instant::now() + Duration::from_secs(5);
                        while !get_close_prompt_state(window.clone()) {
                            if Instant::now() > until { return Err("native close request did not open the application prompt".into()); }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        if !window.is_visible().map_err(|e| e.to_string())? || close_events.load(Ordering::SeqCst) != 1 {
                            return Err("asking did not keep the real window open and emit one prompt".into());
                        }
                        window.close().map_err(|e| e.to_string())?;
                        if !window.is_visible().map_err(|e| e.to_string())? || close_events.load(Ordering::SeqCst) != 1 {
                            return Err("a repeated native close request duplicated the prompt".into());
                        }
                        respond_to_close_request(window.clone(), "cancel".into())?;
                        if get_close_prompt_state(window.clone()) || !window.is_visible().map_err(|e| e.to_string())? {
                            return Err("cancelling did not keep the real window open".into());
                        }
                        window.close().map_err(|e| e.to_string())?;
                        let until = Instant::now() + Duration::from_secs(5);
                        while !get_close_prompt_state(window.clone()) {
                            if Instant::now() > until { return Err("cancelled prompt could not be reopened".into()); }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        let mut preferences = settings();
                        preferences.close_action = "tray".into();
                        tauri::async_runtime::block_on(crate::database::save_app_settings(
                            handle.clone(), preferences, handle.state::<crate::database::Storage>(),
                        )).map_err(|error| error.detail)?;
                        let restored = crate::database::Database::open(&database_path, &database_path.with_file_name("legacy.json")).map_err(|error| error.detail)?;
                        if restored.app_settings("en").map_err(|error| error.detail)?.close_action != "tray" {
                            return Err("remembered tray choice did not survive reopening SQLite".into());
                        }
                        window.close().map_err(|e| e.to_string())?;
                        if !window.is_visible().map_err(|e| e.to_string())? {
                            return Err("saving a choice bypassed the pending prompt".into());
                        }
                        respond_to_close_request(window.clone(), "tray".into())?;
                        let until = Instant::now() + Duration::from_secs(5);
                        while handle.tray_by_id("main-tray").is_none() || window.is_visible().map_err(|e| e.to_string())? {
                            if Instant::now() > until { return Err("close-to-tray did not hide the live window".into()); }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        show_window(&handle);
                        if !window.is_visible().map_err(|e| e.to_string())? { return Err("tray restore did not show the live window".into()); }
                        window.close().map_err(|e| e.to_string())?;
                        let until = Instant::now() + Duration::from_secs(5);
                        while window.is_visible().map_err(|e| e.to_string())? {
                            if Instant::now() > until { return Err("remembered tray choice asked again instead of hiding".into()); }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        if close_events.load(Ordering::SeqCst) != 2 { return Err("remembered choice emitted another prompt".into()); }
                        show_window(&handle);
                        handle.notification().builder().title("EasyVideoDownload · 通知测试")
                            .body("系统通知原生接口测试。This is a native notification interface test.")
                            .show().map_err(|e| e.to_string())?;
                        update(&handle, &settings());
                        window.close().map_err(|e| e.to_string())?;
                        let until = Instant::now() + Duration::from_secs(5);
                        while !get_close_prompt_state(window.clone()) {
                            if Instant::now() > until { return Err("native quit prompt did not open".into()); }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        eprintln!("real native close requests, cancel, remembered choice, tray, theme and notification dispatch succeeded");
                        Ok(())
                    })();
                    let succeeded = result.is_ok();
                    *output.lock().unwrap() = Some(result);
                    if succeeded {
                        let window = handle.get_webview_window("main").unwrap();
                        if let Err(error) = respond_to_close_request(window, "exit".into()) {
                            *output.lock().unwrap() = Some(Err(error));
                            request_exit(&handle);
                        }
                    } else {
                        request_exit(&handle);
                    }
                });
            }
        });
        assert_eq!(exit_code, 0);
        results
            .lock()
            .unwrap()
            .take()
            .expect("native smoke result")
            .unwrap();
    }
}
