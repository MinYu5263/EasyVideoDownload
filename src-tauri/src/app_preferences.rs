use crate::database::AppSettings;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
#[cfg(not(target_os = "macos"))]
use tauri::{
    menu::Menu,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri::{menu::MenuItem, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) fn install_native_quit_handler(app: &tauri::AppHandle) -> Result<(), String> {
    macos::install(app)
}

#[derive(Default)]
pub(crate) struct AppPreferences {
    settings: Mutex<Option<AppSettings>>,
    close_prompt: Mutex<ClosePromptState>,
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
    log::error!("{code}: {detail}");
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
        title: &'a str,
        detail: &'a str,
    },
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct Notice {
    title: String,
    body: String,
    kind: &'static str,
    detail: Option<String>,
}

fn download_notice(settings: Option<&AppSettings>, outcome: DownloadOutcome<'_>) -> Option<Notice> {
    let zh = settings.is_some_and(|settings| settings.locale == "zh-CN");
    let (title, body, kind, detail) = match outcome {
        DownloadOutcome::Completed {
            path,
            already_downloaded: false,
        } if settings.is_none_or(|settings| settings.notify_on_completion) => (
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
            "success",
            None,
        ),
        DownloadOutcome::Failed {
            code,
            title,
            detail,
        } if !matches!(
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
                    title.chars().take(180).collect(),
                    "error",
                    Some(detail.to_string()),
                )
            }
        _ => return None,
    };
    Some(Notice {
        title: title.into(),
        body,
        kind,
        detail,
    })
}

pub(crate) fn notify_download(
    app: &tauri::AppHandle,
    request_id: &str,
    outcome: DownloadOutcome<'_>,
) {
    let state = app.state::<AppPreferences>();
    if state.exiting.load(Ordering::SeqCst) {
        return;
    }
    let settings = state
        .settings
        .lock()
        .ok()
        .and_then(|settings| settings.clone());
    let notice = download_notice(settings.as_ref(), outcome);
    if let Some(notice) = notice {
        if let Err(error) = app.emit(
            "app-download-notice",
            serde_json::json!({"requestId": request_id, "notice": notice}),
        ) {
            report_error(app, "notificationFailed", error);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CloseChoice {
    Ask,
    Background,
    Exit,
    Cancel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClosePromptState {
    revision: u64,
    pub open: bool,
    allow_background: bool,
    has_active_tasks: bool,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CloseBackgroundMode {
    Tray,
    Window,
}

pub(crate) fn close_background_mode() -> CloseBackgroundMode {
    if cfg!(target_os = "macos") {
        CloseBackgroundMode::Window
    } else {
        CloseBackgroundMode::Tray
    }
}

fn background_error_code() -> &'static str {
    match close_background_mode() {
        CloseBackgroundMode::Window => "windowFailed",
        CloseBackgroundMode::Tray => "trayFailed",
    }
}

fn close_choice(settings: Option<&AppSettings>) -> CloseChoice {
    match settings.map(|settings| settings.close_action.as_str()) {
        // Keep the persisted value compatible with existing settings on both platforms.
        Some("tray") => CloseChoice::Background,
        Some("exit") => CloseChoice::Exit,
        _ => CloseChoice::Ask,
    }
}

fn take_close_response(
    state: &AppPreferences,
    action: &str,
    has_active_tasks: bool,
    revision: u64,
) -> Result<CloseChoice, String> {
    let choice = match action {
        "exit" => CloseChoice::Exit,
        "tray" => CloseChoice::Background,
        "cancel" => CloseChoice::Cancel,
        _ => return Err("Invalid close response".into()),
    };
    let mut prompt = state.close_prompt.lock().map_err(|e| e.to_string())?;
    if revision != prompt.revision {
        // A delayed reply must not acknowledge a newer warning or consume its request.
        return Ok(CloseChoice::Ask);
    }
    if !prompt.open {
        return Err("No pending close request".into());
    }
    if choice == CloseChoice::Background && !prompt.allow_background {
        return Err("This request only confirms quitting the application".into());
    }
    if choice == CloseChoice::Exit && has_active_tasks && !prompt.has_active_tasks {
        // New work started after the dialog opened. Require its warning to be acknowledged.
        prompt.allow_background = false;
        prompt.has_active_tasks = true;
        prompt.revision += 1;
        return Ok(CloseChoice::Ask);
    }
    prompt.open = false;
    Ok(choice)
}

#[tauri::command]
pub(crate) fn get_close_prompt_state(
    window: tauri::WebviewWindow,
) -> Result<ClosePromptState, String> {
    if window.label() != "main" {
        return Ok(ClosePromptState::default());
    }
    let state = window.state::<AppPreferences>();
    let prompt = *state.close_prompt.lock().map_err(|e| e.to_string())?;
    Ok(prompt)
}

fn has_active_tasks(app: &tauri::AppHandle) -> Result<bool, String> {
    let downloads = app
        .state::<crate::video::download::DownloadManager>()
        .is_active()?;
    let tools = app
        .state::<crate::required_tools::managed::ConfigureManager>()
        .is_active()?;
    let audio = app.try_state::<crate::audio::AudioExtractionManager>()
        .map(|state| state.is_active()).transpose()?.unwrap_or(false);
    Ok(downloads || tools || audio)
}

fn emit_close_prompt(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppPreferences>();
    let prompt = *state.close_prompt.lock().map_err(|e| e.to_string())?;
    app.emit_to("main", "app-close-requested", prompt)
        .map_err(|e| e.to_string())
}

fn open_close_prompt(
    app: &tauri::AppHandle,
    allow_background: bool,
    has_active_tasks: bool,
) -> Result<(), String> {
    let state = app.state::<AppPreferences>();
    {
        let mut prompt = state.close_prompt.lock().map_err(|e| e.to_string())?;
        if prompt.open && (allow_background || !prompt.allow_background) {
            return Ok(());
        }
        *prompt = ClosePromptState {
            revision: prompt.revision + 1,
            open: true,
            allow_background,
            has_active_tasks,
        };
        log::info!(
            "closePromptOpened: revision={} allowBackground={} hasActiveTasks={}",
            prompt.revision,
            prompt.allow_background,
            prompt.has_active_tasks
        );
    }
    // An explicit Quit may originate from the tray or the macOS application menu.
    show_window(app);
    if let Err(error) = emit_close_prompt(app) {
        state.close_prompt.lock().map_err(|e| e.to_string())?.open = false;
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn respond_to_close_request(
    window: tauri::WebviewWindow,
    action: String,
    revision: u64,
) -> Result<ClosePromptState, String> {
    if window.label() != "main" {
        return Err("Only the main window can respond to close requests".into());
    }
    let state = window.state::<AppPreferences>();
    let active = action == "exit" && has_active_tasks(window.app_handle())?;
    let choice = take_close_response(&state, &action, active, revision)?;
    log::info!(
        "closePromptResponded: revision={revision} action={action} result={choice:?} exitTaskRecheck={:?}",
        (action == "exit").then_some(active)
    );
    match choice {
        CloseChoice::Exit => exit_after_confirmation(window.app_handle()),
        CloseChoice::Background => {
            if let Err(error) = try_hide_in_background(window.app_handle()) {
                state.close_prompt.lock().map_err(|e| e.to_string())?.open = true;
                report_error(window.app_handle(), background_error_code(), &error);
                return Err(error.to_string());
            }
        }
        CloseChoice::Ask => {}
        _ => {}
    }
    get_close_prompt_state(window)
}

pub(crate) fn show_window(app: &tauri::AppHandle) {
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

#[cfg(not(target_os = "macos"))]
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

fn try_hide_in_background(app: &tauri::AppHandle) -> tauri::Result<()> {
    #[cfg(not(target_os = "macos"))]
    ensure_tray(app)?;
    app.get_webview_window("main")
        .ok_or(tauri::Error::WindowNotFound)?
        .hide()?;
    log::info!("windowHidden: mode={:?}", close_background_mode());
    Ok(())
}

fn hide_in_background(app: &tauri::AppHandle) {
    if let Err(error) = try_hide_in_background(app) {
        report_error(app, background_error_code(), error);
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
        if state.exiting.load(Ordering::SeqCst) {
            return;
        }
        match state.close_prompt.lock() {
            Ok(prompt) if prompt.open => return,
            Err(error) => {
                report_error(app, "windowFailed", error);
                return;
            }
            _ => {}
        }
        let settings = state
            .settings
            .lock()
            .ok()
            .and_then(|settings| settings.clone());
        match close_choice(settings.as_ref()) {
            CloseChoice::Background => hide_in_background(app),
            CloseChoice::Exit => request_exit(app),
            CloseChoice::Ask => {
                if let Err(error) =
                    has_active_tasks(app).and_then(|active| open_close_prompt(app, true, active))
                {
                    report_error(app, "windowFailed", error);
                }
            }
            CloseChoice::Cancel => {}
        }
    }
}

pub(crate) fn request_exit(app: &tauri::AppHandle) {
    if app.state::<AppPreferences>().exiting.load(Ordering::SeqCst) {
        return;
    }
    match has_active_tasks(app) {
        Ok(true) => {
            if let Err(error) = open_close_prompt(app, false, true) {
                report_error(app, "windowFailed", error);
            }
        }
        Ok(false) => exit_after_confirmation(app),
        Err(error) => report_error(app, "exitFailed", error),
    }
}

fn exit_after_confirmation(app: &tauri::AppHandle) {
    if app
        .state::<AppPreferences>()
        .exiting
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    log::info!("applicationExitStarted: stopping active tasks");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let downloads = app.state::<crate::video::download::DownloadManager>();
        let tools = app.state::<crate::required_tools::managed::ConfigureManager>();
        let audio = app.try_state::<crate::audio::AudioExtractionManager>();
        let result = async {
            downloads.begin_exit()?;
            tools.begin_exit()?;
            if let Some(audio) = &audio { audio.begin_exit()?; }
            // Let process trees stop and real terminal records/installation rollback settle.
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                while downloads.is_active()? || tools.is_active()? || audio.as_ref().map(|a| a.is_active()).transpose()?.unwrap_or(false) {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Ok::<_, String>(())
            })
                .await
                .map_err(|_| "Timed out while stopping active tasks".to_string())?
        }
            .await;
        match result {
            Ok(()) => {
                log::info!("applicationExitReady: task cleanup completed");
                app.exit(0);
            }
            Err(error) => {
                downloads.abort_exit();
                tools.abort_exit();
                if let Some(audio) = &audio { audio.abort_exit(); }
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
    let audio = app.try_state::<crate::audio::AudioExtractionManager>();
    app.state::<AppPreferences>()
        .exiting
        .store(true, Ordering::SeqCst);
    let result = tauri::async_runtime::block_on(async {
        downloads.begin_exit()?;
        tools.begin_exit()?;
        if let Some(audio) = &audio { audio.begin_exit()?; }
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while downloads.is_active()? || tools.is_active()? || audio.as_ref().map(|a| a.is_active()).transpose()?.unwrap_or(false) {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok::<_, String>(())
        })
            .await
            .map_err(|_| "Timed out while draining native termination".to_string())?
    });
    if let Err(error) = result {
        log::error!("Native exit cleanup failed: {error}");
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
            close_action: "ask".into(),
            max_concurrent_downloads: crate::database::DEFAULT_DOWNLOAD_LIMIT,
        }
    }

    #[test]
    fn notices_obey_preferences_and_exclude_cancelled_busy_and_skipped_downloads() {
        let mut preferences = settings();
        assert!(download_notice(
            Some(&preferences),
            DownloadOutcome::Completed {
                path: "C:/videos/test.mp4",
                already_downloaded: false
            }
        )
            .is_none());
        preferences.notify_on_completion = true;
        assert_eq!(
            download_notice(
                Some(&preferences),
                DownloadOutcome::Completed {
                    path: "C:/videos/test.mp4",
                    already_downloaded: false
                }
            ),
            Some(Notice {
                title: "Download complete".into(),
                body: "test.mp4".into(),
                kind: "success",
                detail: None,
            })
        );
        assert!(download_notice(
            Some(&preferences),
            DownloadOutcome::Completed {
                path: "C:/videos/test.mp4",
                already_downloaded: true
            }
        )
            .is_none());
        for code in ["downloadCancelled", "downloadBusy", "applicationExiting"] {
            assert!(download_notice(
                Some(&preferences),
                DownloadOutcome::Failed {
                    code,
                    title: "Video",
                    detail: "cancelled",
                }
            )
                .is_none());
        }
        assert_eq!(
            download_notice(
                Some(&preferences),
                DownloadOutcome::Failed {
                    code: "downloadFailed",
                    title: "Video",
                    detail: "Network disconnected",
                }
            ),
            Some(Notice {
                title: "Download failed".into(),
                body: "Video".into(),
                kind: "error",
                detail: Some("Network disconnected".into()),
            })
        );
        preferences.notify_on_completion = false;
        assert!(download_notice(
            Some(&preferences),
            DownloadOutcome::Failed {
                code: "spawnFailed",
                title: "Video",
                detail: "Tool failed to start",
            }
        )
            .is_some());
        assert!(download_notice(
            None,
            DownloadOutcome::Failed {
                code: "spawnFailed",
                title: "Video",
                detail: "Tool failed to start",
            }
        )
            .is_some());
        assert!(download_notice(
            None,
            DownloadOutcome::Completed {
                path: "C:/videos/test.mp4",
                already_downloaded: false,
            }
        )
            .is_some());
    }

    #[test]
    fn notices_use_saved_locale_and_do_not_expose_full_paths() {
        let mut preferences = settings();
        preferences.locale = "zh-CN".into();
        preferences.notify_on_completion = true;
        assert_eq!(
            download_notice(
                Some(&preferences),
                DownloadOutcome::Completed {
                    path: r"C:\private\视频.mp4",
                    already_downloaded: false
                }
            ),
            Some(Notice {
                title: "下载完成".into(),
                body: "视频.mp4".into(),
                kind: "success",
                detail: None,
            })
        );
        assert_eq!(
            download_notice(
                Some(&preferences),
                DownloadOutcome::Failed {
                    code: "cookieReadFailed",
                    title: "视频",
                    detail: "Cookie file missing",
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
        assert_eq!(close_choice(Some(&preferences)), CloseChoice::Background);
        preferences.close_action = "exit".into();
        assert_eq!(close_choice(Some(&preferences)), CloseChoice::Exit);
    }

    #[test]
    fn responses_require_a_pending_prompt_and_consume_it_once() {
        let state = AppPreferences::default();
        assert!(take_close_response(&state, "exit", false, 0).is_err());
        for (action, expected) in [
            ("cancel", CloseChoice::Cancel),
            ("tray", CloseChoice::Background),
            ("exit", CloseChoice::Exit),
        ] {
            *state.close_prompt.lock().unwrap() = ClosePromptState {
                revision: 0,
                open: true,
                allow_background: true,
                has_active_tasks: false,
            };
            assert_eq!(
                take_close_response(&state, action, false, 0).unwrap(),
                expected
            );
            assert!(!state.close_prompt.lock().unwrap().open);
            assert!(take_close_response(&state, action, false, 0).is_err());
        }
        *state.close_prompt.lock().unwrap() = ClosePromptState {
            revision: 0,
            open: true,
            allow_background: true,
            has_active_tasks: false,
        };
        assert!(take_close_response(&state, "unexpected", false, 0).is_err());
        assert!(state.close_prompt.lock().unwrap().open);
    }

    #[test]
    fn newly_started_tasks_require_a_warning_but_acknowledged_warnings_do_not_repeat() {
        let state = AppPreferences::default();
        *state.close_prompt.lock().unwrap() = ClosePromptState {
            revision: 0,
            open: true,
            allow_background: true,
            has_active_tasks: false,
        };
        assert_eq!(
            take_close_response(&state, "exit", true, 0).unwrap(),
            CloseChoice::Ask
        );
        assert_eq!(
            *state.close_prompt.lock().unwrap(),
            ClosePromptState {
                revision: 1,
                open: true,
                allow_background: false,
                has_active_tasks: true
            }
        );
        assert_eq!(
            take_close_response(&state, "exit", true, 1).unwrap(),
            CloseChoice::Exit
        );
        assert!(!state.close_prompt.lock().unwrap().open);
    }

    #[test]
    fn exit_only_prompts_reject_background_responses_without_consuming_the_request() {
        let state = AppPreferences::default();
        *state.close_prompt.lock().unwrap() = ClosePromptState {
            revision: 0,
            open: true,
            allow_background: false,
            has_active_tasks: true,
        };
        assert!(take_close_response(&state, "tray", true, 0).is_err());
        assert!(state.close_prompt.lock().unwrap().open);
        assert_eq!(
            take_close_response(&state, "cancel", true, 0).unwrap(),
            CloseChoice::Cancel
        );
        assert!(!state.close_prompt.lock().unwrap().open);
    }

    #[test]
    fn runtime_close_metadata_does_not_change_persisted_preferences() {
        let mut saved = settings();
        saved.close_action = "tray".into();
        let response =
            serde_json::to_value(crate::database::AppSettingsResponse::from(saved.clone()))
                .unwrap();
        assert_eq!(response["closeAction"], "tray");
        assert_eq!(
            response["closeBackgroundMode"],
            if cfg!(target_os = "macos") {
                "window"
            } else {
                "tray"
            }
        );
        assert!(serde_json::to_value(saved)
            .unwrap()
            .get("closeBackgroundMode")
            .is_none());
    }

    #[test]
    fn delayed_replies_cannot_acknowledge_a_newer_task_warning() {
        let state = AppPreferences::default();
        let request = ClosePromptState {
            revision: 2,
            open: true,
            allow_background: false,
            has_active_tasks: true,
        };
        *state.close_prompt.lock().unwrap() = request;
        for action in ["exit", "tray", "cancel"] {
            assert_eq!(
                take_close_response(&state, action, true, 1).unwrap(),
                CloseChoice::Ask
            );
            assert_eq!(*state.close_prompt.lock().unwrap(), request);
        }
        assert_eq!(
            take_close_response(&state, "exit", true, 2).unwrap(),
            CloseChoice::Exit
        );
        assert!(!state.close_prompt.lock().unwrap().open);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "opens a real native window and dispatches an in-app notification event"]
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
        // The Rust test runner does not embed the application binary's Windows icon resource.
        context.set_default_window_icon(Some(tauri::image::Image::new_owned(
            vec![255; 32 * 32 * 4],
            32,
            32,
        )));
        let database_path = profile_path.join("settings.db");
        let setup_database_path = database_path.clone();
        let close_events = Arc::new(AtomicUsize::new(0));
        let results = Arc::new(Mutex::new(None));
        let output = results.clone();
        let app = tauri::Builder::default()
            .any_thread()
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
                        while !get_close_prompt_state(window.clone())?.open {
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
                        respond_to_close_request(window.clone(), "cancel".into(), get_close_prompt_state(window.clone())?.revision)?;
                        if get_close_prompt_state(window.clone())?.open || !window.is_visible().map_err(|e| e.to_string())? {
                            return Err("cancelling did not keep the real window open".into());
                        }
                        window.close().map_err(|e| e.to_string())?;
                        let until = Instant::now() + Duration::from_secs(5);
                        while !get_close_prompt_state(window.clone())?.open {
                            if Instant::now() > until { return Err("cancelled prompt could not be reopened".into()); }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        let mut preferences = settings();
                        preferences.close_action = "tray".into();
                        tauri::async_runtime::block_on(crate::database::save_app_settings(
                            handle.clone(), preferences, handle.state::<crate::database::Storage>(),
                            handle.state::<crate::video::download::DownloadManager>(),
                        )).map_err(|error| error.detail)?;
                        let restored = crate::database::Database::open(&database_path, &database_path.with_file_name("legacy.json")).map_err(|error| error.detail)?;
                        if restored.app_settings("en").map_err(|error| error.detail)?.close_action != "tray" {
                            return Err("remembered tray choice did not survive reopening SQLite".into());
                        }
                        window.close().map_err(|e| e.to_string())?;
                        if !window.is_visible().map_err(|e| e.to_string())? {
                            return Err("saving a choice bypassed the pending prompt".into());
                        }
                        respond_to_close_request(window.clone(), "tray".into(), get_close_prompt_state(window.clone())?.revision)?;
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
                        let notice_received = Arc::new(AtomicBool::new(false));
                        let received = notice_received.clone();
                        let listener = handle.listen_any("app-download-notice", move |event| {
                            let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
                            assert_eq!(payload["requestId"], "native-notice-test");
                            assert_eq!(payload["notice"]["kind"], "error");
                            received.store(true, Ordering::SeqCst);
                        });
                        notify_download(&handle, "native-notice-test", DownloadOutcome::Failed {
                            code: "downloadFailed",
                            title: "Test video",
                            detail: "Native event dispatch test",
                        });
                        handle.unlisten(listener);
                        if !notice_received.load(Ordering::SeqCst) {
                            return Err("in-app notification event was not delivered".into());
                        }
                        update(&handle, &settings());
                        window.close().map_err(|e| e.to_string())?;
                        let until = Instant::now() + Duration::from_secs(5);
                        while !get_close_prompt_state(window.clone())?.open {
                            if Instant::now() > until { return Err("native quit prompt did not open".into()); }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        eprintln!("real native close requests, cancel, remembered choice, tray, theme and in-app notification dispatch succeeded");
                        Ok(())
                    })();
                    let succeeded = result.is_ok();
                    *output.lock().unwrap() = Some(result);
                    if succeeded {
                        let window = handle.get_webview_window("main").unwrap();
                        if let Err(error) = respond_to_close_request(window.clone(), "exit".into(), get_close_prompt_state(window).unwrap().revision) {
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
